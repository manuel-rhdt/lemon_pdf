use lemon_pdf_derive::{PdfDictionary, PdfFormat};
use crate as lemon_pdf;

use super::FontUnit;
use crate::object::{ByteString, IndirectReference, Name, PdfFormat};
use bitflags::bitflags;

use crate::stream::Stream;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PdfFormat)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct FontFlags: u32 {
        const FIXED_PITCH = 1 << 0;
        const SERIF = 1 << 1;
        const SYMBOLIC = 1 << 2;
        const SCRIPT = 1 << 3;
        // here an entry is missing from the PDF spec
        const NONSYMBOLIC = 1 << 5;
        const ITALIC = 1 << 6;

        const ALL_CAP = 1 << 16;
        const SMALL_CAP = 1 << 17;
        const FORCE_BOLD = 1 << 18;
    }
}

impl Default for FontFlags {
    fn default() -> Self {
        FontFlags::NONSYMBOLIC
    }
}

impl PdfFormat for FontFlags {
    fn write(&self, output: &mut crate::object::Formatter) -> std::io::Result<()> {
        PdfFormat::write(&self.bits(), output)
    }
}

fn is_default<T: Default + PartialEq>(val: &T) -> bool {
    val == &T::default()
}

#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub struct FontDescriptor {
    pub font_name: Name,
    pub font_family: Option<ByteString>,
    pub font_stretch: Option<FontStretch>,
    pub font_weight: Option<u32>,
    pub flags: FontFlags,
    pub font_b_box: [FontUnit; 4],
    pub italic_angle: f64,
    pub ascent: FontUnit,
    pub descent: FontUnit,
    #[skip_if("is_default")]
    pub leading: FontUnit,
    #[skip_if("is_default")]
    pub cap_height: FontUnit,
    #[skip_if("is_default")]
    pub x_height: FontUnit,
    #[skip_if("is_default")]
    pub stem_h: FontUnit,
    pub stem_v: FontUnit,
    #[skip_if("is_default")]
    pub avg_width: FontUnit,
    #[skip_if("is_default")]
    pub max_width: FontUnit,
    #[skip_if("is_default")]
    pub missing_width: FontUnit,
    pub font_file: Option<IndirectReference<Type1FontFile>>,
    pub font_file2: Option<IndirectReference<TrueTypeFontFile>>,
    pub font_file3: Option<IndirectReference<FontFile3>>,
    pub char_set: Option<ByteString>,
}

impl FontDescriptor {
    /// A descriptor with the given name and the default (zero) metrics.
    pub fn new(font_name: Name) -> Self {
        FontDescriptor {
            font_name,
            font_family: None,
            font_stretch: None,
            font_weight: None,
            flags: Default::default(),
            font_b_box: Default::default(),
            italic_angle: 0.0,
            ascent: Default::default(),
            descent: Default::default(),
            leading: Default::default(),
            cap_height: Default::default(),
            x_height: Default::default(),
            stem_h: Default::default(),
            stem_v: Default::default(),
            avg_width: Default::default(),
            max_width: Default::default(),
            missing_width: Default::default(),
            font_file: None,
            font_file2: None,
            font_file3: None,
            char_set: None,
        }
    }
}

/// The dictionary of an embedded Type 1 font program (`FontFile`, ISO 32000-1, 9.9).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct Type1FontFileDict {
    /// The length of the clear-text portion of the font program.
    pub length1: usize,
    /// The length of the encrypted portion of the font program.
    pub length2: usize,
    /// The length of the fixed-content portion of the font program.
    pub length3: usize,
}

pub type Type1FontFile = Stream<Type1FontFileDict>;

/// The dictionary of an embedded TrueType font program (`FontFile2`).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct TrueTypeFontFileDict {
    /// The length of the decoded font program.
    pub length1: usize,
}

pub type TrueTypeFontFile = Stream<TrueTypeFontFileDict>;

/// The format of a font program embedded as `FontFile3`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PdfFormat)]
pub enum FontFile3Subtype {
    /// A Type 1 compact font (CFF).
    Type1C,
    /// A CIDFont compact font (CFF).
    CIDFontType0C,
    /// An OpenType font.
    OpenType,
}

/// The dictionary of an embedded font program in a format given by its subtype (`FontFile3`).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct FontFile3Dict {
    pub subtype: FontFile3Subtype,
}

pub type FontFile3 = Stream<FontFile3Dict>;
