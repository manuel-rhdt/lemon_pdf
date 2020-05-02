use lemon_pdf_derive::PdfFormat;

use super::FontUnit;
use crate::object::{PdfFormat, IndirectReference};
use bitflags::bitflags;

use crate::stream::Stream;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PdfFormat)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

bitflags! {
    pub struct FontFlags: u32 {
        const FIXED_PITCH = 1 << 0;
        const SERIF = 1 << 1;
        const SYMBOLIC = 1 << 2;
        const SCRIPT = 1 << 3;
        // here an entry is missing from the PDF spec
        const NONSYMBOLIC = 1 << 5;
        const ITALIC = 1 << 6;

        const ALL_CAP = 1 << 16;
        const SMALL_CAP = 1 << 17;
        const FORCE_BOLD = 1 << 18;
    }
}

impl Default for FontFlags {
    fn default() -> Self {
        FontFlags::NONSYMBOLIC
    }
}

impl PdfFormat for FontFlags {
    fn write(&self, output: &mut crate::object::Formatter) -> std::io::Result<()> {
        PdfFormat::write(&self.bits(), output)
    }
}

fn is_default<T: Default + PartialEq>(val: &T) -> bool {
    val == &T::default()
}

#[derive(Debug, Clone, Default, PartialEq, PdfFormat)]
pub struct FontDescriptor {
    pub font_name: String,
    #[skip_if("Vec::is_empty")]
    pub font_family: Vec<u8>,
    #[skip_if("Option::is_none")]
    pub font_stretch: Option<FontStretch>,
    #[skip_if("Option::is_none")]
    pub font_weight: Option<u32>,
    pub flags: FontFlags,
    pub font_b_box: [FontUnit; 4],
    pub italic_angle: f64,
    pub ascent: FontUnit,
    pub descent: FontUnit,
    #[skip_if("is_default")]
    pub leading: FontUnit,
    #[skip_if("is_default")]
    pub cap_height: FontUnit,
    #[skip_if("is_default")]
    pub x_height: FontUnit,
    #[skip_if("is_default")]
    pub stem_h: FontUnit,
    pub stem_v: FontUnit,
    #[skip_if("is_default")]
    pub avg_width: FontUnit,
    #[skip_if("is_default")]
    pub max_width: FontUnit,
    #[skip_if("is_default")]
    pub missing_width: FontUnit,
    #[skip_if("Option::is_none")]
    pub font_file: Option<IndirectReference<Stream>>,
    #[skip_if("Option::is_none")]
    pub font_file2: Option<IndirectReference<Stream>>,
    #[skip_if("Option::is_none")]
    pub font_file3: Option<IndirectReference<Stream>>,
    #[skip_if("Vec::is_empty")]
    pub char_set: Vec<u8>,
}
