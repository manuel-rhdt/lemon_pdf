use lemon_pdf_derive::PdfFormat;

use super::FontUnit;
use crate::object::PdfFormat;
use bitflags::bitflags;

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

#[derive(Debug, Default, PartialEq, PdfFormat)]
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
    #[skip_if("Option::is_none")]
    pub leading: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub cap_height: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub x_height: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub stem_h: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub stem_v: Option<FontUnit>,
}
