//! Page labels (`/PageLabels` of the document catalog, ISO 32000-1, 12.4.2): the names that a
//! viewer shows for the pages instead of their indices, like "iii" for the third page of the front
//! matter or "A-1" for the first page of an appendix.

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::object::TextString;

/// How the number of a page is written in its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum PageLabelStyle {
    /// 1, 2, 3, ...
    #[rename("D")]
    Decimal,
    /// I, II, III, ...
    #[rename("R")]
    UpperRoman,
    /// i, ii, iii, ...
    #[rename("r")]
    LowerRoman,
    /// A to Z, then AA to ZZ, then AAA to ZZZ, ... (the letter is repeated: not "AB")
    #[rename("A")]
    UpperLetters,
    /// a to z, then aa to zz, ...
    #[rename("a")]
    LowerLetters,
}

/// The label of the pages of a range: from the page where the range starts to the page before the
/// start of the next range.
#[derive(Debug, Clone, Default, PartialEq, Eq, PdfFormat)]
#[omit_type(true)]
pub struct PageLabel {
    /// How the number is written. Without a style the labels are the prefix alone.
    #[rename("S")]
    pub style: Option<PageLabelStyle>,
    /// A text before the number.
    #[rename("P")]
    pub prefix: Option<TextString>,
    /// The number of the first page of the range (1 if it is not given).
    #[rename("St")]
    pub start: Option<u32>,
}

impl PageLabel {
    /// A label with a number in `style`, starting at `start`.
    pub fn numbered(style: PageLabelStyle, start: u32) -> Self {
        PageLabel {
            style: Some(style),
            prefix: None,
            start: Some(start),
        }
    }

    /// The same label with `prefix` before the number.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(TextString(prefix.into()));
        self
    }
}
