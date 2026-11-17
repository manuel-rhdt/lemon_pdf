//! The viewer preferences of the document catalog (`/ViewerPreferences`, ISO 32000-1, 12.2): hints
//! for how a viewer shows and prints the document. Viewers are free to ignore them.

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

/// The reading direction of the text, which decides the order of the pages in a view with
/// two pages side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum Direction {
    /// Left to right (the default of viewers).
    L2R,
    /// Right to left.
    R2L,
}

/// The page mode that a viewer leaves full-screen mode for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum NonFullScreenPageMode {
    UseNone,
    UseOutlines,
    UseThumbs,
    #[rename("UseOC")]
    UseOc,
}

/// One of the boxes of a page, for the preferences that name a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum PageBox {
    MediaBox,
    CropBox,
    BleedBox,
    TrimBox,
    ArtBox,
}

/// Whether the print dialog of a viewer scales the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum PrintScaling {
    /// The default of the viewer, which usually fits the page to the paper.
    AppDefault,
    /// The pages are printed at their size.
    None,
}

/// How the print dialog of a viewer sets the pages on the paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum Duplex {
    Simplex,
    DuplexFlipShortEdge,
    DuplexFlipLongEdge,
}

/// The entries of `/ViewerPreferences`. An entry that is `None` is not written, and then the
/// viewer uses its own default.
#[derive(Debug, Clone, Default, PartialEq, Eq, PdfFormat)]
#[omit_type(true)]
pub struct ViewerPreferences {
    /// Hide the toolbar of the viewer.
    pub hide_toolbar: Option<bool>,
    /// Hide the menu bar of the viewer.
    pub hide_menubar: Option<bool>,
    /// Hide the controls of the window (scroll bars, navigation panels).
    #[rename("HideWindowUI")]
    pub hide_window_ui: Option<bool>,
    /// Resize the window to the size of the first page.
    pub fit_window: Option<bool>,
    /// Put the window in the middle of the screen.
    pub center_window: Option<bool>,
    /// Show the `Title` of the document information instead of the file name in the title bar.
    pub display_doc_title: Option<bool>,
    pub non_full_screen_page_mode: Option<NonFullScreenPageMode>,
    pub direction: Option<Direction>,
    /// The box of a page that is shown on the screen.
    pub view_area: Option<PageBox>,
    pub view_clip: Option<PageBox>,
    /// The box of a page that is printed.
    pub print_area: Option<PageBox>,
    pub print_clip: Option<PageBox>,
    pub print_scaling: Option<PrintScaling>,
    pub duplex: Option<Duplex>,
    /// Choose the paper tray by the size of the pages.
    #[rename("PickTrayByPDFSize")]
    pub pick_tray_by_pdf_size: Option<bool>,
    /// The ranges of pages that the print dialog suggests, as pairs of the first and the last
    /// page (counted from 1) of every range.
    pub print_page_range: Option<Vec<i64>>,
    /// The number of copies that the print dialog suggests.
    pub num_copies: Option<i64>,
}
