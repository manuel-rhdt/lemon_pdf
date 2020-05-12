use super::{descriptor::FontDescriptor, FontType, FontUnit};

use crate::object::{IndirectReference, PdfFormat, Object};
use lemon_pdf_derive::PdfFormat;
use crate as lemon_pdf;

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
#[rename("Font")]
pub struct CompositeFont {
    pub subtype: FontType,
    pub base_font: String,
    pub encoding: String,
    pub descendant_fonts: [IndirectReference<CIDFont>; 1],
    #[skip_if("Option::is_none")]
    pub to_unicode: Option<String>,
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

#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum MetricsEntry {
    Range {
        from: u32,
        to: u32,
        advance: FontUnit,
    },
    Consecutive {
        start: u32,
        advances: Vec<FontUnit>,
    },
}

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
#[rename("Font")]
pub struct CIDFont {
    pub subtype: CIDFontType,
    pub base_font: String,
    #[rename("CIDSystemInfo")]
    pub cid_system_info: Object<CIDSystemInfo>,
    pub font_descriptor: IndirectReference<FontDescriptor>,
    #[rename("DW")]
    #[skip_if("Option::is_none")]
    pub dw: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub w: Option<Object<Vec<MetricsEntry>>>,
    #[rename("DW2")]
    #[skip_if("Option::is_none")]
    pub dw2: Option<FontUnit>,
    #[skip_if("Option::is_none")]
    pub w2: Option<Object<Vec<MetricsEntry>>>,
    #[rename("CIDToGIDMap")]
    #[skip_if("String::is_empty")]
    pub cid_to_gid_map: String,
}

#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct CIDSystemInfo {
    pub registry: Vec<u8>,
    pub ordering: Vec<u8>,
    pub supplement: u32,
}

impl Default for CIDSystemInfo {
    fn default() -> Self {
        CIDSystemInfo {
            registry: b"Adobe".to_vec(),
            ordering: b"Identity".to_vec(),
            supplement: 0,
        }
    }
}
