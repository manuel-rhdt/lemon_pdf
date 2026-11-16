//    Copyright 2018 Manuel Reinhardt
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.

use std::io::{Result, Write};

use crate as lemon_pdf;
use crate::document::{DocumentCatalog, DocumentContext, DocumentInfo};
use crate::object::{Formatter, IndirectObject, IndirectReference, PdfFormat};
use crate::stream::Stream;
use lemon_pdf_derive::{PdfDictionary, PdfFormat};

#[derive(Debug, Clone)]
struct HexString(String);

impl PdfFormat for HexString {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "<{}>", self.0)
    }
}

/// The start value of the 128-bit FNV-1a hash (http://www.isthe.com/chongo/tech/comp/fnv/).
pub(crate) const FNV_OFFSET_BASIS: u128 = 0x6c62272e_07bb0142_62b82175_6295c58d;

/// Continues the 128-bit FNV-1a hash `hash` with `bytes`.
///
/// Not a cryptographic hash: it is only used to make file identifiers unlikely to collide.
pub(crate) fn fnv1a_128_extend(hash: u128, bytes: &[u8]) -> u128 {
    const PRIME: u128 = 0x00000000_01000000_00000000_0000013b;
    bytes
        .iter()
        .fold(hash, |hash, &byte| (hash ^ u128::from(byte)).wrapping_mul(PRIME))
}

/// The 128-bit FNV-1a hash of `bytes`.
#[cfg(test)]
fn fnv1a_128(bytes: &[u8]) -> u128 {
    fnv1a_128_extend(FNV_OFFSET_BASIS, bytes)
}

/// A file identifier (the `/ID` entry of the trailer, ISO 32000-1, 14.4): the hash of the bytes
/// of the file in front of the trailer. The same document is written to the same bytes, and two
/// different documents have different identifiers unless the hash collides. (It used to be a
/// hash of the time and the process, which made every file different.)
fn file_id(hash: u128) -> HexString {
    HexString(format!("{:032x}", hash))
}

/// The trailer dictionary (ISO 32000-1, 7.5.5).
#[derive(Debug, PdfFormat)]
#[omit_type(true)]
struct TrailerDictionary {
    /// One more than the highest object number.
    size: usize,
    root: IndirectReference<DocumentCatalog>,
    info: Option<IndirectReference<DocumentInfo>>,
    #[rename("ID")]
    id: [HexString; 2],
}

/// The dictionary of a cross-reference stream (ISO 32000-1, 7.5.8): the entries of the trailer
/// dictionary and the layout of the cross-reference data.
#[derive(Debug, PdfDictionary)]
#[rename("XRef")]
struct XRefStreamDict {
    #[flatten]
    trailer: TrailerDictionary,
    /// The widths of the fields of a cross-reference entry in bytes.
    w: [usize; 3],
}

pub struct Trailer {
    pub crossref_offset: u32,
    pub(crate) document_catalog: IndirectReference<DocumentCatalog>,
    pub(crate) document_info: Option<IndirectReference<DocumentInfo>>,
}

impl Trailer {
    fn dictionary(&self, size: usize, hash: u128) -> TrailerDictionary {
        let digest = file_id(hash);
        TrailerDictionary {
            size,
            root: self.document_catalog,
            info: self.document_info,
            id: [digest.clone(), digest],
        }
    }

    /// Writes the cross-reference information as a cross-reference stream (PDF 1.5).
    pub(crate) fn write_xref_stream(&mut self, context: &mut DocumentContext) -> Result<()> {
        let offset = context.current_offset();
        let hash = context.output.hash();
        // the xref stream is an object itself
        let num = context.crossref.add_entry(offset, 0);
        let (data, width) = context.crossref.stream_data();
        let dict = XRefStreamDict {
            trailer: self.dictionary(num as usize + 1, hash),
            w: [1, width, 2],
        };
        let stream = Stream::deflated(dict, &data)?;

        writeln!(context.output, "{} 0 obj", num)?;
        let mut formatter = Formatter {
            writer: &mut context.output,
        };
        stream.write_indirect(&mut formatter)?;
        write!(context.output, "\nendobj\n")?;
        write!(context.output, "startxref\n{}\n%%EOF\n", offset)
    }

    pub(crate) fn write(&mut self, context: &mut DocumentContext) -> Result<()> {
        writeln!(context.output, "trailer")?;
        let size = context.crossref.len() as usize + 1;
        let dictionary = self.dictionary(size, context.output.hash());
        dictionary.write(&mut Formatter {
            writer: &mut context.output,
        })?;
        write!(context.output, "\nstartxref\n{}\n", self.crossref_offset)?;
        writeln!(context.output, "%%EOF")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_128_matches_reference_values() {
        assert_eq!(fnv1a_128(b""), 0x6c62272e07bb014262b821756295c58d);
        assert_eq!(fnv1a_128(b"a"), 0xd228cb696f1a8caf78912b704e4a8964);
        assert_eq!(fnv1a_128(b"foobar"), 0x343e1662793c64bf6f0d3597ba446f18);
    }

    #[test]
    fn file_id_is_32_hex_digits() {
        let HexString(id) = file_id(fnv1a_128(b"abc"));
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()), "{}", id);
    }

    #[test]
    fn extending_the_hash_is_hashing_the_concatenation() {
        let (a, b) = (b"%PDF-1.7\n1 0 obj", b" << >> endobj");
        let whole: Vec<u8> = a.iter().chain(b).copied().collect();
        assert_eq!(fnv1a_128_extend(fnv1a_128_extend(FNV_OFFSET_BASIS, a), b), fnv1a_128(&whole));
    }
}
