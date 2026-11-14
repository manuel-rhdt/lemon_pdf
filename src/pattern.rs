//! Patterns (ISO 32000-1, 8.7): the color of fills and strokes that is a shading or a cell that
//! repeats.
//!
//! A tiling pattern (8.7.3) is a content stream that is painted again and again, at a
//! distance of `x_step` and `y_step`. `DocumentContext::write_tiling_pattern` writes one, the
//! content is drawn like a page. `PageContext::add_tiling_pattern` makes it a resource and
//! `PageContext::set_fill_pattern` paints with it.

use crate as lemon_pdf;
use crate::content::{Matrix, Pt};
use crate::object::IndirectReference;
use crate::pagetree::ResourceDictionary;
use crate::shading::ShadingPattern;
use crate::stream::Stream;
use derive_more::From;
use lemon_pdf_derive::{PdfDictionary, PdfFormat};

/// The dictionary of a tiling pattern (ISO 32000-1, 8.7.3.2). The pattern paints with its own
/// colors (`PaintType` 1), and cells are not moved to avoid distortion (`TilingType` 1).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[rename("Pattern")]
#[constant(PatternType = 1)]
#[constant(PaintType = 1)]
#[constant(TilingType = 1)]
pub struct TilingPatternDict {
    /// The rectangle in pattern space that one cell is clipped to.
    #[rename("BBox")]
    pub bbox: [Pt; 4],
    #[rename("XStep")]
    pub x_step: Pt,
    #[rename("YStep")]
    pub y_step: Pt,
    pub resources: ResourceDictionary,
    /// Maps pattern space to the default coordinate system of the page (or of the form that
    /// the pattern is used in), whatever transformation is current when it is used.
    pub matrix: Option<Matrix>,
}

/// A tiling pattern.
pub type TilingPattern = Stream<TilingPatternDict>;

/// An entry of the pattern resources.
#[derive(Debug, Clone, PartialEq, From, PdfFormat)]
pub enum Pattern {
    Shading(ShadingPattern),
    /// A tiling pattern, which is a stream of its own.
    Tiling(IndirectReference<TilingPattern>),
}
