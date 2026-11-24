//! Colours as CSS Color 4 writes them, in the spaces that a PDF can show them in.
//!
//! A [`ColorValue`] is made in one of three ways, and [`PageContext::set_fill_color`] and
//! [`PageContext::set_stroke_color`] write it in the space that holds it best:
//!
//! - sRGB is a device colour. Every page and form defines `DefaultRGB` as sRGB
//!   ([`PageContext::set_srgb_device_defaults`]), so that it is sRGB in every viewer that honours it;
//! - a colour of a space with a profile ([`RgbSpace`]) is in an ICC space of that profile, which the
//!   document writes once;
//! - every other colour is given as CIE XYZ (D65) and is CIELAB (D50), because a PDF has no Oklab
//!   and the like. A device may show a* and b* only within ±128 (an iPhone scales the range), so a
//!   colour beyond is in the Rec. 2020 ICC space if that holds it, else its chroma is reduced to fit.

use std::io::Result;

use crate::color_space::{DefaultColorSpace, IccBasedColorSpace, LabColorSpace, D50_WHITE_POINT};
use crate::content::PageContext;
use crate::icc::{self, rgb_matrix_profile, ParametricCurve};
use crate::object::Name;
use crate::stream::StreamFilter;

/// An RGB space that has a profile of its own, with the primaries and the transfer function that
/// CSS Color 4 says.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum RgbSpace {
    SrgbLinear,
    DisplayP3,
    DisplayP3Linear,
    A98Rgb,
    ProphotoRgb,
    /// The primaries of Rec. 2020 with a pure gamma of 2.4, as CSS Color 4 says and as an iPhone
    /// decodes every Rec. 2020 profile (see [`icc::rec2020_gamma24_profile`]).
    Rec2020,
}

/// A colour to paint.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ColorValue {
    /// A device colour: the channels from 0 to 1. It is sRGB where the page defines the device
    /// colours so, see [`PageContext::set_srgb_device_defaults`].
    Srgb([f64; 3]),
    /// The encoded components (from 0 to 1) of a colour in an [`RgbSpace`].
    Rgb(RgbSpace, [f64; 3]),
    /// A colour as CIE XYZ (D65, Y of 1 for the white), which holds every colour that there is.
    XyzD65([f64; 3]),
}

/// Linear-light Display P3 to CIE XYZ (D65).
#[rustfmt::skip]
const P3_TO_XYZ: [[f64; 3]; 3] = [
    [0.486_570_948_6, 0.265_667_693_2, 0.198_217_285_2],
    [0.228_974_564_1, 0.691_738_521_8, 0.079_286_914_1],
    [0.0,             0.045_113_381_9, 1.043_944_368_9],
];
/// CIE XYZ (D65) to linear-light Rec. 2020 (CSS Color 4).
#[rustfmt::skip]
const XYZ_TO_REC2020: [[f64; 3]; 3] = [
    [ 1.716_651_187_97, -0.355_670_783_78, -0.253_366_281_37],
    [-0.666_684_351_83,  1.616_481_236_63,  0.015_768_545_81],
    [ 0.017_639_857_45, -0.042_770_613_26,  0.942_103_121_24],
];
/// Bradford adaptation from D65 to D50 (CSS Color 4, "Sample code for color conversions").
#[rustfmt::skip]
const D65_TO_D50: [[f64; 3]; 3] = [
    [ 1.047_929_820_840_548_8,  0.022_946_793_341_019_088, -0.050_192_229_543_135_57],
    [ 0.029_627_815_688_159_344, 0.990_434_484_573_249,    -0.017_073_825_029_385_14],
    [-0.009_243_058_152_591_178, 0.015_055_144_896_577_895,  0.751_874_289_958_000_8],
];
#[rustfmt::skip]
const SRGB_TO_XYZ: [[f64; 3]; 3] = [
    [0.412_390_799_265_959_5, 0.357_584_339_383_878, 0.180_480_788_401_834_3],
    [0.212_639_005_871_510_36, 0.715_168_678_767_756, 0.072_192_315_360_733_7],
    [0.019_330_818_715_591_85, 0.119_194_779_794_626, 0.950_532_152_249_660_7],
];
#[rustfmt::skip]
const A98_TO_XYZ: [[f64; 3]; 3] = [
    [0.576_669_042_910_131_1, 0.185_558_237_906_546_4, 0.188_228_646_234_245_6],
    [0.297_344_975_250_535_9, 0.627_363_566_255_466_3, 0.075_291_458_493_997_8],
    [0.027_031_361_386_412_36, 0.070_688_852_535_827_6, 0.991_337_536_837_639_2],
];
/// The colorants of ProPhoto RGB, which is relative to D50 already.
#[rustfmt::skip]
const PROPHOTO_TO_XYZ_D50: [[f64; 3]; 3] = [
    [0.797_766_6, 0.135_181_3, 0.031_347_73],
    [0.288_074_8, 0.711_835_2, 0.000_089_94],
    [0.0,         0.0,         0.825_104_6],
];
/// Linear-light Rec. 2020 to CIE XYZ (D65).
#[cfg(test)]
#[rustfmt::skip]
const REC2020_TO_XYZ: [[f64; 3]; 3] = [
    [0.636_958_048_301_291, 0.144_616_903_586_208_4, 0.168_880_975_164_172_1],
    [0.262_700_212_011_267_0, 0.677_998_071_518_871_0, 0.059_301_716_469_861_9],
    [0.0,                     0.028_072_693_049_087_5, 1.060_985_057_710_791_3],
];

fn multiply(matrix: &[[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row.iter().zip(vector).map(|(m, v)| m * v).sum())
}

impl RgbSpace {
    /// The key under which the document keeps the profile of the space.
    fn key(self) -> &'static str {
        match self {
            RgbSpace::SrgbLinear => "srgb-linear",
            RgbSpace::DisplayP3 => "display-p3",
            RgbSpace::DisplayP3Linear => "display-p3-linear",
            RgbSpace::A98Rgb => "a98-rgb",
            RgbSpace::ProphotoRgb => "prophoto-rgb",
            RgbSpace::Rec2020 => "rec2020-gamma24",
        }
    }

    /// The ICC profile of the space (version 4, an RGB matrix and one curve).
    pub fn profile(self) -> Vec<u8> {
        const LINEAR: ParametricCurve = ParametricCurve { g: 1.0, a: 1.0, b: 0.0, c: 0.0, d: 0.0 };
        let (name, to_xyz, adapt, curve) = match self {
            RgbSpace::SrgbLinear => ("sRGB linear", SRGB_TO_XYZ, true, LINEAR),
            RgbSpace::DisplayP3 => ("Display P3", P3_TO_XYZ, true, ParametricCurve::SRGB),
            RgbSpace::DisplayP3Linear => ("Display P3 linear", P3_TO_XYZ, true, LINEAR),
            RgbSpace::A98Rgb => (
                "Adobe RGB (1998)",
                A98_TO_XYZ,
                true,
                ParametricCurve { g: 563.0 / 256.0, a: 1.0, b: 0.0, c: 0.0, d: 0.0 },
            ),
            RgbSpace::ProphotoRgb => (
                "ProPhoto RGB",
                PROPHOTO_TO_XYZ_D50,
                false,
                ParametricCurve { g: 1.8, a: 1.0, b: 0.0, c: 1.0 / 16.0, d: 16.0 / 512.0 },
            ),
            // pure gamma 2.4, and the one that an iPhone decodes every Rec. 2020 profile with
            RgbSpace::Rec2020 => return icc::rec2020_gamma24_profile(),
        };
        let [red, green, blue] = [0, 1, 2].map(|column| {
            let xyz = [to_xyz[0][column], to_xyz[1][column], to_xyz[2][column]];
            let [x, y, z] = if adapt { multiply(&D65_TO_D50, xyz) } else { xyz };
            (x, y, z)
        });
        rgb_matrix_profile(name, red, green, blue, curve)
    }
}

/// The coordinates of a colour (CIE XYZ, D65) in [`RgbSpace::Rec2020`], the profile with the gamma 2.4: the linear
/// ones to the power of 1/2.4. A colour that Rec. 2020 cannot hold is clipped, the only clipping here.
pub fn rec2020_gamma24_components(xyz: [f64; 3]) -> [f64; 3] {
    multiply(&XYZ_TO_REC2020, xyz).map(|linear| linear.clamp(0.0, 1.0).powf(1.0 / 2.4))
}

/// CIELAB (D50) of a colour of CIE XYZ (D65): `[L*, a*, b*]`, not limited to any range.
pub fn xyz_d65_to_lab_d50(xyz: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = multiply(&D65_TO_D50, xyz);
    let f = |t: f64| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x / D50_WHITE_POINT[0]), f(y / D50_WHITE_POINT[1]), f(z / D50_WHITE_POINT[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The largest a* and b* of [`LabColorSpace::d50`].
const LAB_LIMIT: f64 = 128.0;

/// Rounds the components that go to the content stream to five decimals.
fn round5(components: [f64; 3]) -> [f64; 3] {
    components.map(|value| (value * 100_000.0).round() / 100_000.0)
}

impl PageContext<'_, '_, '_> {
    /// Defines what a device colour means in the resources of this page or form (`DefaultRGB`):
    /// sRGB. A form has resources of its own, which those of the page do not reach, so every form and
    /// pattern has to call this before it paints a [`ColorValue::Srgb`] (or an image or shading in
    /// `DeviceRGB`) if the colours are to be sRGB.
    pub fn set_srgb_device_defaults(&mut self) -> Result<()> {
        let srgb = self.pdf_context.icc_color_space("srgb", icc::srgb_profile, 3, Some(StreamFilter::Deflate))?;
        self.set_default_color_space(DefaultColorSpace::Rgb, srgb);
        Ok(())
    }

    /// The ICC space of `space`, for what has a colour space of its own (a shading, with
    /// `with_icc_profile`). The profile is written once for the document.
    pub fn rgb_icc_space(&mut self, space: RgbSpace) -> Result<IccBasedColorSpace> {
        self.pdf_context.icc_color_space(space.key(), || space.profile(), 3, Some(StreamFilter::Deflate))
    }

    /// Adds the ICC space of `space` to the resources and returns its resource name.
    pub fn add_rgb_space(&mut self, space: RgbSpace) -> Result<Name> {
        let space = self.rgb_icc_space(space)?;
        Ok(self.add_color_space(space))
    }

    /// Makes the fills use `color`, see the [module](self). Leaves the opacity alone.
    pub fn set_fill_color(&mut self, color: ColorValue) -> Result<()> {
        self.set_color(color, false)
    }

    /// Makes the strokes use `color`, see the [module](self). Leaves the opacity alone.
    pub fn set_stroke_color(&mut self, color: ColorValue) -> Result<()> {
        self.set_color(color, true)
    }

    fn set_color_components(&mut self, space: &Name, components: [f64; 3], stroke: bool) -> Result<()> {
        if stroke {
            self.set_stroke_color_components(space, &components)
        } else {
            self.set_fill_color_components(space, &components)
        }
    }

    fn set_color(&mut self, color: ColorValue, stroke: bool) -> Result<()> {
        let xyz = match color {
            ColorValue::Srgb([r, g, b]) => {
                return if stroke { self.device_rgb_stroke_color(r, g, b) } else { self.device_rgb_fill_color(r, g, b) };
            }
            ColorValue::Rgb(space, components) => {
                let space = self.add_rgb_space(space)?;
                return self.set_color_components(&space, round5(components), stroke);
            }
            ColorValue::XyzD65(xyz) => xyz,
        };
        let round = |value: f64| (value * 10_000.0).round() / 10_000.0;
        let lab = LabColorSpace::d50();
        let limit = LAB_LIMIT;
        let [lightness, mut a, mut b] = xyz_d65_to_lab_d50(xyz);
        if a.abs() > limit || b.abs() > limit {
            // A device may show a* and b* only within ±128, so a colour beyond is in the ICC space
            // of Rec. 2020 if that holds it, and else its chroma is reduced (the lightness and the
            // hue stay).
            let linear = multiply(&XYZ_TO_REC2020, xyz);
            if linear.iter().all(|v| (-0.000_5..=1.000_5).contains(v)) {
                let space = self.add_rgb_space(RgbSpace::Rec2020)?;
                return self.set_color_components(&space, round5(rec2020_gamma24_components(xyz)), stroke);
            }
            let scale = limit / a.abs().max(b.abs());
            a *= scale;
            b *= scale;
        }
        let name = self.add_color_space(lab);
        let lightness = round(lightness.clamp(0.0, 100.0));
        if stroke {
            self.lab_stroke_color(&name, lightness, round(a), round(b))
        } else {
            self.lab_fill_color(&name, lightness, round(a), round(b))
        }
    }
}

impl RgbSpace {
    /// The CIE XYZ (D65) of linear-light components of the space. Not for `ProphotoRgb`, whose
    /// white is D50 (the matrix is relative to it).
    #[cfg(test)]
    fn linear_to_xyz(self, linear: [f64; 3]) -> Option<[f64; 3]> {
        let matrix = match self {
            RgbSpace::SrgbLinear => SRGB_TO_XYZ,
            RgbSpace::DisplayP3 | RgbSpace::DisplayP3Linear => P3_TO_XYZ,
            RgbSpace::A98Rgb => A98_TO_XYZ,
            RgbSpace::Rec2020 => REC2020_TO_XYZ,
            RgbSpace::ProphotoRgb => return None,
        };
        Some(multiply(&matrix, linear))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_is_neutral_in_lab() {
        let [l, a, b] = xyz_d65_to_lab_d50(multiply(&SRGB_TO_XYZ, [1.0, 1.0, 1.0]));
        assert!((l - 100.0).abs() < 1e-3 && a.abs() < 1e-3 && b.abs() < 1e-3, "{l} {a} {b}");
    }

    #[test]
    fn rec2020_matrices_are_inverse() {
        let xyz = RgbSpace::Rec2020.linear_to_xyz([0.2, 0.5, 0.7]).unwrap();
        let back = multiply(&XYZ_TO_REC2020, xyz);
        for (a, b) in back.iter().zip([0.2, 0.5, 0.7]) {
            assert!((a - b).abs() < 1e-6, "{back:?}");
        }
    }

    #[test]
    fn every_space_has_a_profile() {
        for space in [
            RgbSpace::SrgbLinear,
            RgbSpace::DisplayP3,
            RgbSpace::DisplayP3Linear,
            RgbSpace::A98Rgb,
            RgbSpace::ProphotoRgb,
            RgbSpace::Rec2020,
        ] {
            let profile = space.profile();
            assert!(icc::inspect_profile(&profile).is_ok(), "{space:?}");
        }
    }
}
