//! CIE-based colour spaces (ISO 32000-1, 8.6.5) that are named in the resources of a page: colours
//! that mean the same on every device, whatever the gamut of the device is.

use std::io::Result;

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat as DerivePdfFormat;

use crate::object::{Formatter, IndirectReference, Name, PdfFormat, Precise};

fn precise(values: &[f64]) -> Vec<Precise> {
    values.iter().map(|value| Precise(*value)).collect()
}
use crate::output_intent::IccProfileDict;
use crate::stream::Stream;

/// The D50 white point of CIELAB (the one that CSS uses for `lab()` and `lch()`).
pub const D50_WHITE_POINT: [f64; 3] = [0.964_295_676_429_567_7, 1.0, 0.825_104_602_510_460_2];

/// A CIELAB colour space (`[/Lab << /WhitePoint … /Range … >>]`). Its colours have three
/// components: L* from 0 to 100, and a* and b* in `range`. Every colour that can be seen has a
/// value in it, so nothing is lost to a gamut.
#[derive(Debug, Clone, PartialEq)]
pub struct LabColorSpace {
    /// The tristimulus value of the white of the space (CIE XYZ, with Y of 1).
    pub white_point: [f64; 3],
    /// The range of a* and b*: `[a_min, a_max, b_min, b_max]`. Colours outside of it are clamped
    /// by readers.
    pub range: [f64; 4],
}

impl LabColorSpace {
    /// CIELAB relative to the D50 white, with a range that holds the colours of every gamut that
    /// CSS has (Rec. 2020 and ProPhoto RGB reach about ±200) and then some.
    pub fn d50() -> Self {
        LabColorSpace {
            white_point: D50_WHITE_POINT,
            range: [-256.0, 256.0, -256.0, 256.0],
        }
    }
}

impl PdfFormat for LabColorSpace {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        #[derive(Debug)]
        struct Parameters<'a>(&'a LabColorSpace);

        impl PdfFormat for Parameters<'_> {
            fn write(&self, f: &mut Formatter) -> Result<()> {
                f.format_dictionary()
                    .key_value("WhitePoint", &precise(&self.0.white_point))
                    .key_value("Range", &precise(&self.0.range))
                    .finish()
            }
        }

        f.format_array()
            .value(&Name::from("Lab"))
            .value(&Parameters(self))
            .finish()
    }
}

/// A calibrated RGB colour space (`[/CalRGB << /WhitePoint … /Gamma … /Matrix … >>]`): CIE XYZ
/// colours are made from the three components by a power function for each channel and a matrix.
/// It can only say a curve that is a power function, so a space with another curve (sRGB, which
/// has a linear segment) is approximated.
#[derive(Debug, Clone, PartialEq)]
pub struct CalRgbColorSpace {
    /// The tristimulus value of the white of the space (CIE XYZ, with Y of 1).
    pub white_point: [f64; 3],
    /// The exponent of every channel.
    pub gamma: [f64; 3],
    /// The XYZ of the red, green and blue colorants, one after another (`XA YA ZA XB YB ZB XC YC ZC`).
    pub matrix: [f64; 9],
}

impl PdfFormat for CalRgbColorSpace {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        #[derive(Debug)]
        struct Parameters<'a>(&'a CalRgbColorSpace);

        impl PdfFormat for Parameters<'_> {
            fn write(&self, f: &mut Formatter) -> Result<()> {
                f.format_dictionary()
                    .key_value("WhitePoint", &precise(&self.0.white_point))
                    .key_value("Gamma", &precise(&self.0.gamma))
                    .key_value("Matrix", &precise(&self.0.matrix))
                    .finish()
            }
        }

        f.format_array()
            .value(&Name::from("CalRGB"))
            .value(&Parameters(self))
            .finish()
    }
}

/// A colour space that is defined by an ICC profile (`[/ICCBased stream]`), from
/// `DocumentContext::write_icc_profile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IccBasedColorSpace(pub IndirectReference<Stream<IccProfileDict>>);

impl PdfFormat for IccBasedColorSpace {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        f.format_array()
            .value(&Name::from("ICCBased"))
            .value(&self.0)
            .finish()
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
