//! Contains the builtin PDF fonts

use std::io::Result;

use lemon_pdf_derive::PdfFormat;

use super::{
    descriptor::{FontDescriptor, FontFlags},
    encoding, FontType, FontUnit,
};
use crate::document::Context;
use crate::object::{IndirectReference, PdfFormat};

#[derive(Debug, Copy, Clone)]
pub struct BuiltInFontMetrics {
    pub font_name: &'static str,
    pub family_name: &'static str,
    pub ascender: FontUnit,
    pub descender: FontUnit,
    pub cap_height: FontUnit,
    pub stem_h: FontUnit,
    pub stem_v: FontUnit,
    pub font_bbox: [FontUnit; 4],
    pub x_height: FontUnit,
    pub italic_angle: f64,
    pub char_metrics: &'static [CharMetric],
    /// A function that maps from a unicode encoded `char` to the
    /// corresponding index in the `char_metrics` table or None.
    pub unicode_mapping: fn(char) -> Option<usize>,
}

#[derive(Debug, Copy, Clone)]
pub struct CharMetric {
    pub name: &'static str,
    pub bbox: [FontUnit; 4],
    pub character_code: i32,
    pub advance_width: FontUnit,
}

// The following `include!` expands to
//
// #[derive(Debug, Copy, Clone, PartialEq, Eq)]
// pub enum BuiltInFont {
//     CourierBold,
//     CourierBoldOblique,
//     CourierOblique,
//     Courier,
//     HelveticaBold,
//     HelveticaBoldOblique,
//     HelveticaOblique,
//     Helvetica,
//     Symbol,
//     TimesBold,
//     TimesBoldItalic,
//     TimesItalic,
//     TimesRoman,
//     ZapfDingbats,
// }
//
// impl BuiltInFont {
//     pub fn metrics(self) -> BuiltInFontMetrics { ... }
// }
include!(concat!(env!("OUT_DIR"), "/base_14_fonts.rs"));

#[derive(Debug, Clone, PartialEq, Default, PdfFormat)]
#[rename("Font")]
pub struct SimpleFont {
    pub subtype: FontType,
    pub base_font: String,
    #[skip_if("Option::is_none")]
    pub first_char: Option<usize>,
    #[skip_if("Option::is_none")]
    pub last_char: Option<usize>,
    #[skip_if("Vec::is_empty")]
    pub widths: Vec<FontUnit>,
    #[skip_if("Option::is_none")]
    pub font_descriptor: Option<IndirectReference<FontDescriptor>>,
    #[skip_if("Option::is_none")]
    pub encoding: Option<encoding::EncodingEntry>,
}

impl SimpleFont {
    pub fn create_with_base_name(name: String, subtype: FontType) -> Self {
        SimpleFont {
            subtype,
            base_font: name,
            ..Default::default()
        }
    }
}

impl BuiltInFont {
    pub fn font(self, context: &mut Context<impl std::io::Write>) -> Result<SimpleFont> {
        let metrics = self.metrics();

        let font_descriptor = FontDescriptor {
            font_name: metrics.font_name.to_string(),
            font_family: metrics.family_name.as_bytes().to_owned(),
            font_b_box: metrics.font_bbox,
            cap_height: Some(metrics.cap_height),
            italic_angle: metrics.italic_angle,
            x_height: Some(metrics.x_height),
            stem_h: Some(metrics.stem_h),
            stem_v: Some(metrics.stem_v),
            ascent: metrics.ascender,
            descent: metrics.descender,
            flags: match metrics.family_name {
                "Helvetica" => FontFlags::NONSYMBOLIC,
                "Times" => FontFlags::NONSYMBOLIC | FontFlags::SERIF,
                "Courier" => FontFlags::NONSYMBOLIC | FontFlags::FIXED_PITCH | FontFlags::SERIF,
                "Symbol" => FontFlags::SYMBOLIC,
                "ZapfDingbats" => FontFlags::SYMBOLIC,
                _ => FontFlags::default(),
            } | if metrics.italic_angle != 0.0 {
                FontFlags::ITALIC
            } else {
                FontFlags::empty()
            },
            ..Default::default()
        };
        let font_descriptor = context.write_object(font_descriptor)?;

        Ok(SimpleFont {
            subtype: FontType::Type1,
            base_font: metrics.font_name.to_string(),
            font_descriptor: Some(font_descriptor),
            ..Default::default()
        })
    }
}
