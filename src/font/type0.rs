use super::{descriptor::FontDescriptor, FontUnit, FontType};

use crate::object::{IndirectReference, Object, PdfFormat};
use lemon_pdf_derive::PdfFormat;

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
#[rename("Font")]
pub struct CompositeFont {
    pub subtype: FontType,
    pub base_font: String,
    pub encoding: String,
    pub descendant_fonts: [CIDFont; 1],
    pub to_unicode: Option<String>
}

#[derive(Debug, Copy, Clone, PartialEq, PdfFormat)]
pub enum CIDFontType {
    CIDFontType0,
    CIDFontType2,
}

impl Default for CIDFontType {
    fn default() -> Self {
        CIDFontType::CIDFontType0
    }
}

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
#[rename("Font")]
pub struct CIDFont {
    pub subtype: CIDFontType,
    pub base_font: String,
    #[rename("CIDSystemInfo")]
    pub cid_system_info: CIDSystemInfo,
    pub font_descriptor: IndirectReference<FontDescriptor>,
    #[rename("DW")]
    #[skip_if("Option::is_none")]
    pub dw: Option<FontUnit>,
    #[skip_if("Vec::is_empty")]
    pub w: Vec<FontUnit>,
    #[rename("DW2")]
    #[skip_if("Option::is_none")]
    pub dw2: Option<FontUnit>,
    #[skip_if("Vec::is_empty")]
    pub w2: Vec<FontUnit>,
    #[rename("CIDToGIDMap")]
    pub cid_to_gid_map: String,
}

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
pub struct CIDSystemInfo {}
