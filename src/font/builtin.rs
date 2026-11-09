//! Contains the builtin PDF fonts

use std::io::Result;

use lemon_pdf_derive::PdfFormat;

use super::{
    descriptor::{FontDescriptor, FontFlags},
    encoding, FontType, FontUnit,
};
use crate::document::DocumentContext;
use crate::object::{IndirectReference};

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

use crate as lemon_pdf;

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
    pub fn font(self, context: &mut DocumentContext) -> Result<SimpleFont> {
        let metrics = self.metrics();

        let flags = match metrics.family_name {
            "Helvetica" => FontFlags::NONSYMBOLIC,
            "Times" => FontFlags::NONSYMBOLIC | FontFlags::SERIF,
            "Courier" => FontFlags::NONSYMBOLIC | FontFlags::FIXED_PITCH | FontFlags::SERIF,
            "Symbol" => FontFlags::SYMBOLIC,
            "ZapfDingbats" => FontFlags::SYMBOLIC,
            _ => FontFlags::default(),
        };
        let flags = if metrics.italic_angle != 0.0 {
            flags | FontFlags::ITALIC
        } else {
            flags
        };

        let font_descriptor = FontDescriptor {
            font_name: metrics.font_name.to_string(),
            font_family: metrics.family_name.as_bytes().to_owned(),
            font_b_box: metrics.font_bbox,
            cap_height: metrics.cap_height,
            italic_angle: metrics.italic_angle,
            x_height: metrics.x_height,
            stem_h: metrics.stem_h,
            stem_v: metrics.stem_v,
            ascent: metrics.ascender,
            descent: metrics.descender,
            flags,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn units(values: [f64; 4]) -> [FontUnit; 4] {
        [FontUnit(values[0]), FontUnit(values[1]), FontUnit(values[2]), FontUnit(values[3])]
    }

    #[test]
    fn helvetica_metrics_match_the_afm_file() {
        let metrics = BuiltInFont::Helvetica.metrics();
        assert_eq!(metrics.font_name, "Helvetica");
        assert_eq!(metrics.family_name, "Helvetica");
        // `FontBBox -166 -225 1000 931` is llx lly urx ury
        assert_eq!(metrics.font_bbox, units([-166.0, -225.0, 1000.0, 931.0]));
        assert_eq!(metrics.cap_height, FontUnit(718.0));
        assert_eq!(metrics.descender, FontUnit(-207.0));

        // `C 65 ; WX 667 ; N A ; B 14 0 654 718 ;`
        let index = (metrics.unicode_mapping)('A').unwrap();
        let a = metrics.char_metrics[index];
        assert_eq!(a.name, "A");
        assert_eq!(a.character_code, 65);
        assert_eq!(a.advance_width, FontUnit(667.0));
        assert_eq!(a.bbox, units([14.0, 0.0, 654.0, 718.0]));
    }

    #[test]
    fn all_bounding_boxes_are_ordered() {
        let fonts = [
            BuiltInFont::Courier,
            BuiltInFont::CourierBold,
            BuiltInFont::CourierBoldOblique,
            BuiltInFont::CourierOblique,
            BuiltInFont::Helvetica,
            BuiltInFont::HelveticaBold,
            BuiltInFont::HelveticaBoldOblique,
            BuiltInFont::HelveticaOblique,
            BuiltInFont::Symbol,
            BuiltInFont::TimesBold,
            BuiltInFont::TimesBoldItalic,
            BuiltInFont::TimesItalic,
            BuiltInFont::TimesRoman,
            BuiltInFont::ZapfDingbats,
        ];
        for font in &fonts {
            let metrics = font.metrics();
            let ordered = |[llx, lly, urx, ury]: [FontUnit; 4]| llx.0 <= urx.0 && lly.0 <= ury.0;
            assert!(ordered(metrics.font_bbox), "{:?}: {:?}", font, metrics.font_bbox);
            for c in metrics.char_metrics {
                assert!(ordered(c.bbox), "{:?} {}: {:?}", font, c.name, c.bbox);
            }
        }
    }
}
