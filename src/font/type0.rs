use std::io::Result;

use super::{descriptor::FontDescriptor, FontType, FontUnit, ToUnicodeCMap};

use crate as lemon_pdf;
use crate::object::{ByteString, Formatter, IndirectReference, Name, Object, PdfFormat};
use lemon_pdf_derive::PdfFormat;

/// A Type 0 (composite) font (ISO 32000-1, 9.7.6).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Font")]
pub struct CompositeFont {
    pub subtype: FontType,
    pub base_font: Name,
    /// The name of a predefined CMap, like `Identity-H`.
    pub encoding: Name,
    pub descendant_fonts: [IndirectReference<CIDFont>; 1],
    pub to_unicode: Option<IndirectReference<ToUnicodeCMap>>,
}

#[derive(Debug, Copy, Clone, PartialEq, PdfFormat)]
pub enum CIDFontType {
    CIDFontType0,
    CIDFontType2,
}

/// The horizontal advances of a range of CIDs: one element of a `W` array
/// (ISO 32000-1, 9.7.4.3).
#[derive(Debug, Clone, PartialEq)]
pub enum MetricsEntry {
    /// The CIDs `from` to `to` (inclusive) all have the same advance.
    Range {
        from: u32,
        to: u32,
        advance: FontUnit,
    },
    /// The CIDs starting at `start` have the given advances.
    Consecutive { start: u32, advances: Vec<FontUnit> },
}

/// The `W` array of a CIDFont: the advances of the glyphs.
///
/// Its elements are written into one flat array (`[c [w1 w2 …] cfirst clast w …]`), so a
/// `MetricsEntry` is not an object on its own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Widths(pub Vec<MetricsEntry>);

impl PdfFormat for Widths {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut array = f.format_array();
        for entry in &self.0 {
            array = match entry {
                MetricsEntry::Range { from, to, advance } => {
                    array.value(from).value(to).value(advance)
                }
                MetricsEntry::Consecutive { start, advances } => array.value(start).value(advances),
            };
        }
        array.finish()
    }
}

/// The vertical metrics of a glyph: its vertical advance and the position vector from the
/// horizontal to the vertical origin.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct VerticalMetric {
    pub advance: FontUnit,
    pub origin_x: FontUnit,
    pub origin_y: FontUnit,
}

/// The vertical metrics of a range of CIDs: one element of a `W2` array.
#[derive(Debug, Clone, PartialEq)]
pub enum VerticalMetricsEntry {
    /// The CIDs `from` to `to` (inclusive) all have the same metrics.
    Range {
        from: u32,
        to: u32,
        metric: VerticalMetric,
    },
    /// The CIDs starting at `start` have the given metrics.
    Consecutive {
        start: u32,
        metrics: Vec<VerticalMetric>,
    },
}

/// The `W2` array of a CIDFont: the vertical metrics of the glyphs (for vertical writing).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VerticalWidths(pub Vec<VerticalMetricsEntry>);

impl PdfFormat for VerticalWidths {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut array = f.format_array();
        for entry in &self.0 {
            array = match entry {
                VerticalMetricsEntry::Range { from, to, metric } => array
                    .value(from)
                    .value(to)
                    .value(&metric.advance)
                    .value(&metric.origin_x)
                    .value(&metric.origin_y),
                VerticalMetricsEntry::Consecutive { start, metrics } => {
                    let flat: Vec<FontUnit> = metrics
                        .iter()
                        .flat_map(|m| vec![m.advance, m.origin_x, m.origin_y])
                        .collect();
                    array.value(start).value(&flat)
                }
            };
        }
        array.finish()
    }
}

/// The default vertical metrics of a CIDFont (`DW2`): the vertical origin `y` and the
/// vertical advance.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DefaultVerticalMetrics {
    pub origin_y: FontUnit,
    pub advance: FontUnit,
}

impl PdfFormat for DefaultVerticalMetrics {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        [self.origin_y, self.advance].write(f)
    }
}

/// A CIDFont, the descendant of a Type 0 font (ISO 32000-1, 9.7.4).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Font")]
pub struct CIDFont {
    pub subtype: CIDFontType,
    pub base_font: Name,
    #[rename("CIDSystemInfo")]
    pub cid_system_info: Object<CIDSystemInfo>,
    pub font_descriptor: IndirectReference<FontDescriptor>,
    /// The default advance of the glyphs.
    #[rename("DW")]
    pub dw: Option<FontUnit>,
    pub w: Option<Object<Widths>>,
    #[rename("DW2")]
    pub dw2: Option<DefaultVerticalMetrics>,
    pub w2: Option<Object<VerticalWidths>>,
    /// `Identity` for embedded TrueType fonts whose CIDs are glyph indices.
    #[rename("CIDToGIDMap")]
    pub cid_to_gid_map: Option<Name>,
}

#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct CIDSystemInfo {
    pub registry: ByteString,
    pub ordering: ByteString,
    pub supplement: u32,
}

impl Default for CIDSystemInfo {
    fn default() -> Self {
        CIDSystemInfo {
            registry: ByteString::from("Adobe"),
            ordering: ByteString::from("Identity"),
            supplement: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(object: &dyn PdfFormat) -> String {
        let mut buffer = Vec::new();
        object
            .write(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn widths_are_one_flat_array() {
        let widths = Widths(vec![
            MetricsEntry::Consecutive {
                start: 1,
                advances: vec![FontUnit(500.0), FontUnit(600.0)],
            },
            MetricsEntry::Range {
                from: 10,
                to: 20,
                advance: FontUnit(1000.0),
            },
        ]);
        assert_eq!(written(&widths), "[ 1 [ 500 600 ] 10 20 1000 ]");
    }

    #[test]
    fn vertical_widths_are_one_flat_array() {
        let metric = VerticalMetric {
            advance: FontUnit(-1000.0),
            origin_x: FontUnit(500.0),
            origin_y: FontUnit(880.0),
        };
        let widths = VerticalWidths(vec![
            VerticalMetricsEntry::Consecutive {
                start: 1,
                metrics: vec![metric],
            },
            VerticalMetricsEntry::Range {
                from: 5,
                to: 6,
                metric,
            },
        ]);
        assert_eq!(
            written(&widths),
            "[ 1 [ -1000 500 880 ] 5 6 -1000 500 880 ]"
        );
    }
}
