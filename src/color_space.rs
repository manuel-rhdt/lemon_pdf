//! CIE-based colour spaces (ISO 32000-1, 8.6.5) that are named in the resources of a page: colours
//! that mean the same on every device, whatever the gamut of the device is.

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat as DerivePdfFormat;

use crate::object::{IndirectReference, Precise};
use crate::output_intent::IccProfileDict;
use crate::stream::Stream;

/// The D50 white point of CIELAB (the one that CSS uses for `lab()` and `lch()`).
pub const D50_WHITE_POINT: [f64; 3] = [0.964_295_676_429_567_7, 1.0, 0.825_104_602_510_460_2];

/// The parameter dictionary of a CIELAB colour space (ISO 32000-1, Table 64).
#[derive(Debug, Clone, PartialEq, DerivePdfFormat)]
#[omit_type(true)]
pub struct LabDict {
    /// The tristimulus value of the diffuse white point (CIE XYZ, with Y of 1). Required.
    pub white_point: [Precise; 3],
    /// The tristimulus value of the diffuse black point. The default is `[0 0 0]`.
    pub black_point: Option<[Precise; 3]>,
    /// The range of a* and b*: `[a_min, a_max, b_min, b_max]`; colours outside of it are clamped
    /// by readers. The default is `[-100 100 -100 100]`.
    pub range: Option<[Precise; 4]>,
}

/// A CIELAB colour space (`[/Lab dict]`, ISO 32000-1, 8.6.5.4). Its colours have three
/// components: L* from 0 to 100, and a* and b* in the `range` of the dictionary. Every colour
/// that can be seen has a value in it, so nothing is lost to a gamut.
#[derive(Debug, Clone, PartialEq, DerivePdfFormat)]
#[array]
#[rename("Lab")]
pub struct LabColorSpace(pub LabDict);

impl LabColorSpace {
    /// CIELAB relative to the D50 white, with the range of a* and b* of -128 to 128.
    ///
    /// The range matters on iOS. Measured on an iPhone 16 Pro (Preview, Files, 6 ranges), the device
    /// shows a* and b* as the value of the operand times 128 divided by the half width of the range:
    /// ±100 shows them at 1.28 times (ΔC* +17), ±150 at 0.85 (-10.5), ±200 at 0.64 (-25), and ±256 at
    /// 0.5 (the colours are pale). With ±128 the colours are right (CIEDE2000 0.3 for the hues, 0.00
    /// for the greys); -128..127 tints the greys by 1.2. So an operand has to be put in the range
    /// that the space has, and a* and b* beyond ±128 cannot be shown on that device by Lab at all:
    /// use another encoding for them (a `CalRGB` carrier, [`CalRgbColorSpace::xyz_carrier`]).
    pub fn d50() -> Self {
        LabColorSpace(LabDict {
            white_point: D50_WHITE_POINT.map(Precise),
            black_point: None,
            range: Some([-128.0, 128.0, -128.0, 128.0].map(Precise)),
        })
    }
}

/// The parameter dictionary of a calibrated RGB colour space (ISO 32000-1, Table 63).
#[derive(Debug, Clone, PartialEq, DerivePdfFormat)]
#[omit_type(true)]
pub struct CalRgbDict {
    /// The tristimulus value of the diffuse white point (CIE XYZ, with Y of 1). Required.
    pub white_point: [Precise; 3],
    /// The tristimulus value of the diffuse black point. The default is `[0 0 0]`.
    pub black_point: Option<[Precise; 3]>,
    /// The exponent of the power function of every channel. The default is `[1 1 1]`.
    pub gamma: Option<[Precise; 3]>,
    /// The XYZ of the red, green and blue colorants, one after another (`XA YA ZA XB YB ZB XC YC
    /// ZC`). The default is the identity.
    pub matrix: Option<[Precise; 9]>,
}

/// A calibrated RGB colour space (`[/CalRGB dict]`, ISO 32000-1, 8.6.5.3): CIE XYZ colours are
/// made from the three components by a power function for each channel and a matrix. It can only
/// say a curve that is a power function, so a space with another curve (sRGB, which has a linear
/// segment) is approximated.
#[derive(Debug, Clone, PartialEq, DerivePdfFormat)]
#[array]
#[rename("CalRGB")]
pub struct CalRgbColorSpace(pub CalRgbDict);

impl CalRgbColorSpace {
    /// A space with gamma 1 whose three components are the CIE XYZ of the colour divided by the white
    /// point (`X / Xw`, `Y / Yw`, `Z / Zw`), so that the components (1, 1, 1) are the white point.
    ///
    /// That last property is the one that matters: measured on an iPhone 16 Pro, the device scales
    /// the matrix of a `CalRGB` space so that (1, 1, 1) is the white point, whatever the matrix says
    /// (a carrier with a matrix of 1.2 and the white point of D65 or D50 was shown as `white *
    /// components`: the fit is 0.7 and 0.5 CIEDE2000, against 6.8 and 6.7 for the PDF specification).
    /// A matrix whose columns add up to the white point, like the one of sRGB, is the same for the
    /// specification and for the device, and is exact there.
    ///
    /// With the white point of D50 (`D50_WHITE_POINT`, and the colours adapted to it) the carrier is
    /// exact on the device (CIEDE2000 0.00 for greys and hues). With the white point of D65 the greys
    /// are exact and the hues lose about 1.3 of chroma (0.9): the device seems to adapt the colorants
    /// of a D65 space with another transform than Bradford, which a D50 space does not need.
    pub fn xyz_carrier(white_point: [f64; 3]) -> Self {
        let [x, y, z] = white_point;
        CalRgbColorSpace(CalRgbDict {
            white_point: white_point.map(Precise),
            black_point: None,
            gamma: Some([1.0; 3].map(Precise)),
            matrix: Some([x, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, z].map(Precise)),
        })
    }
}

/// A colour space that is defined by an ICC profile (`[/ICCBased stream]`, ISO 32000-1, 8.6.5.5),
/// from `DocumentContext::write_icc_profile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, DerivePdfFormat)]
#[array]
#[rename("ICCBased")]
pub struct IccBasedColorSpace(pub IndirectReference<Stream<IccProfileDict>>);

/// The device colour spaces that a page can redefine (`DefaultGray`, `DefaultRGB`, `DefaultCMYK`,
/// ISO 32000-1, 8.6.5.6): every device colour of the page is then read in the space that was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultColorSpace {
    Gray,
    Rgb,
    Cmyk,
}

impl DefaultColorSpace {
    pub(crate) fn resource_name(self) -> &'static str {
        match self {
            DefaultColorSpace::Gray => "DefaultGray",
            DefaultColorSpace::Rgb => "DefaultRGB",
            DefaultColorSpace::Cmyk => "DefaultCMYK",
        }
    }
}

/// A colour space that is named in the resources of a page.
#[derive(Debug, Clone, PartialEq, DerivePdfFormat)]
pub enum ColorSpaceResource {
    Lab(LabColorSpace),
    CalRgb(CalRgbColorSpace),
    IccBased(IccBasedColorSpace),
}

impl From<LabColorSpace> for ColorSpaceResource {
    fn from(space: LabColorSpace) -> Self {
        ColorSpaceResource::Lab(space)
    }
}

impl From<CalRgbColorSpace> for ColorSpaceResource {
    fn from(space: CalRgbColorSpace) -> Self {
        ColorSpaceResource::CalRgb(space)
    }
}

impl From<IccBasedColorSpace> for ColorSpaceResource {
    fn from(space: IccBasedColorSpace) -> Self {
        ColorSpaceResource::IccBased(space)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Formatter, PdfFormat};

    fn written(value: &impl PdfFormat) -> String {
        let mut buffer = Vec::new();
        value.write(&mut Formatter { writer: &mut buffer }).unwrap();
        String::from_utf8(buffer).unwrap().split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_colour_space_is_an_array_of_its_family_and_its_dictionary() {
        assert_eq!(
            written(&LabColorSpace::d50()),
            "[ /Lab << /WhitePoint [ 0.964296 1 0.825105 ] /Range [ -128 128 -128 128 ] >> ]"
        );
        let cal = CalRgbColorSpace(CalRgbDict {
            white_point: [0.5, 1.0, 0.25].map(Precise),
            black_point: None,
            gamma: None,
            matrix: None,
        });
        assert_eq!(written(&cal), "[ /CalRGB << /WhitePoint [ 0.5 1 0.25 ] >> ]");
        let icc = IccBasedColorSpace(IndirectReference::new(7, 0));
        assert_eq!(written(&icc), "[ /ICCBased 7 0 R ]");
    }
}
