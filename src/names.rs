//! The name dictionary of the document catalog (ISO 32000-1, 7.7.4) and its name tree of named
//! destinations (12.3.2.3). Viewers jump to a named destination when a URL has it as a
//! fragment: `document.pdf#chapter-2`.

use std::collections::BTreeMap;
use std::io::Result;

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::annotation::Destination;
use crate::object::{ByteString, Formatter, PdfFormat};

/// The name dictionary of the catalog: the name trees of the document.
#[derive(Debug, Clone, PdfFormat)]
#[omit_type(true)]
pub struct Names {
    pub dests: Option<DestinationTree>,
}

/// A name tree with all its entries in a single node (`<< /Names [...] >>`). The names are
/// byte strings in lexical order.
#[derive(Debug, Clone, Default)]
pub struct DestinationTree {
    pub names: BTreeMap<Vec<u8>, Destination>,
}

// `/Names` is a flat array of alternating keys and values.
impl PdfFormat for DestinationTree {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        struct Entries<'a>(&'a BTreeMap<Vec<u8>, Destination>);
        impl<'a> std::fmt::Debug for Entries<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Names")
            }
        }
        impl<'a> PdfFormat for Entries<'a> {
            fn write(&self, f: &mut Formatter) -> Result<()> {
                let mut array = f.format_array();
                for (name, destination) in self.0 {
                    array = array.value(&ByteString(name.clone())).value(destination);
                }
                array.finish()
            }
        }
        f.format_dictionary().key_value("Names", &Entries(&self.names)).finish()
    }
}
