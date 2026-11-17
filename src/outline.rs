//! The document outline (bookmarks, ISO 32000-1, 12.3.3).
//!
//! The outline is a tree of items below the `/Outlines` entry of the document catalog. Every
//! item has a title and a destination, and links to its parent, its siblings and its first and
//! last child.

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::annotation::Destination;
use crate::object::{IndirectReference, TextString};

/// An item of the document outline, as the caller describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    /// The text shown in the outline panel of the viewer.
    pub title: String,
    /// Where the item jumps to.
    pub destination: Destination,
    /// The items below this one.
    pub children: Vec<OutlineItem>,
    /// Whether the children are shown when the document is opened.
    pub open: bool,
}

impl OutlineItem {
    pub fn new(title: impl Into<String>, destination: Destination) -> Self {
        OutlineItem {
            title: title.into(),
            destination,
            children: Vec::new(),
            open: true,
        }
    }
}

/// The root of the outline, referenced from the document catalog.
#[derive(Debug, Clone, PdfFormat)]
#[rename("Outlines")]
pub struct Outlines {
    pub first: Option<IndirectReference<OutlineEntry>>,
    pub last: Option<IndirectReference<OutlineEntry>>,
    /// The number of visible items at all levels.
    pub count: i64,
}

/// An item of the outline as written to the file.
#[derive(Debug, Clone, PdfFormat)]
#[omit_type(true)]
pub struct OutlineEntry {
    pub title: TextString,
    pub parent: IndirectReference<OutlineEntry>,
    pub prev: Option<IndirectReference<OutlineEntry>>,
    pub next: Option<IndirectReference<OutlineEntry>>,
    pub first: Option<IndirectReference<OutlineEntry>>,
    pub last: Option<IndirectReference<OutlineEntry>>,
    /// The number of visible descendants; negative if the item is closed (its descendants are
    /// hidden). Omitted for items without children.
    pub count: Option<i64>,
    pub dest: Destination,
}
