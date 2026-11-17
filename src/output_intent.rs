//! Output intents (`/OutputIntents` of the document catalog, ISO 32000-1, 14.11.5): the
//! characteristics of the device that the document is made for, which makes its device colours
//! well defined. PDF/A needs one for documents that use `DeviceRGB`, `DeviceGray` or `DeviceCMYK`.

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::object::{IndirectReference, TextString};
use crate::stream::Stream;

/// The kind of output intent: the standard that it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PdfFormat)]
pub enum OutputIntentSubtype {
    /// PDF/A (all parts).
    #[rename("GTS_PDFA1")]
    PdfA,
    /// PDF/X.
    #[rename("GTS_PDFX")]
    PdfX,
    /// PDF/E.
    #[rename("ISO_PDFE1")]
    PdfE,
}

/// The dictionary of the stream of an ICC profile (ISO 32000-1, 8.6.5.5).
#[derive(Debug, Clone, PartialEq, Eq, PdfFormat)]
#[omit_type(true)]
pub struct IccProfileDict {
    /// The number of colour components of the profile: 1, 3 or 4.
    pub n: i64,
}

/// An output intent as it is written to the file.
#[derive(Debug, Clone, PdfFormat)]
#[rename("OutputIntent")]
pub struct OutputIntent {
    /// The standard that the intent is for.
    #[rename("S")]
    pub subtype: OutputIntentSubtype,
    /// The name of the output condition, for example `sRGB IEC61966-2.1`.
    pub output_condition_identifier: TextString,
    /// The registry that the identifier is from; none if the profile is embedded.
    pub registry_name: Option<TextString>,
    /// A description for people.
    pub info: Option<TextString>,
    /// The ICC profile of the output condition.
    pub dest_output_profile: Option<IndirectReference<Stream<IccProfileDict>>>,
}

/// What `DocumentContext::add_output_intent` writes an output intent from.
#[derive(Debug, Clone)]
pub struct OutputIntentDesc {
    pub subtype: OutputIntentSubtype,
    pub output_condition_identifier: String,
    pub info: Option<String>,
    /// The ICC profile (the file) and its number of components.
    pub profile: Vec<u8>,
    pub components: i64,
}
