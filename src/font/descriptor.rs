use lemon_pdf_derive::PdfFormat;

use super::FontUnit;
use crate::object::PdfFormat;

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
    UltraExpanded
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
    pub flags: u32,
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