//! CIE-based colour spaces (ISO 32000-1, 8.6.5) that are named in the resources of a page: colours
//! that mean the same on every device, whatever the gamut of the device is.

use std::io::Result;

use crate::object::{Formatter, Name, PdfFormat};

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
                    .key_value("WhitePoint", &self.0.white_point.to_vec())
                    .key_value("Range", &self.0.range.to_vec())
                    .finish()
            }
        }

        f.format_array()
            .value(&Name::from("Lab"))
            .value(&Parameters(self))
            .finish()
    }
}
