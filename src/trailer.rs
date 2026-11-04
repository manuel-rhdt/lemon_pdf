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
use std::time::SystemTime;

use crate::document::{DocumentContext, DocumentCatalog, DocumentInfo};
use crate::object::{Formatter, IndirectReference, PdfFormat};

use crypto::digest::Digest;
use crypto::md5::Md5;

#[derive(Debug, Clone)]
struct HexString(String);

impl PdfFormat for HexString {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "<{}>", self.0)
    }
}

fn get_digest(file_len: u64) -> HexString {
    let mut md5 = Md5::new();
    let time = SystemTime::now();
    let time = format!("{:?}", time);
    md5.input_str(&time);
    md5.input_str(&format!("{}", file_len));
    HexString(md5.result_str())
}

pub struct Trailer {
    pub crossref_offset: u32,
    pub(crate) document_catalog: IndirectReference<DocumentCatalog>,
    pub(crate) document_info: Option<IndirectReference<DocumentInfo>>,
}

impl Trailer {
    /// Writes the cross-reference information as a cross-reference stream (PDF 1.5).
    pub(crate) fn write_xref_stream(&mut self, context: &mut DocumentContext) -> Result<()> {
        use crate::stream::{Stream, StreamFilter};
        use flate2::{write::ZlibEncoder, Compression};

        let offset = context.current_offset();
        // the xref stream is an object itself
        let num = context.crossref.add_entry(offset, 0);
        let (data, width) = context.crossref.stream_data();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data)?;
        let mut stream = Stream::with_bytes(encoder.finish()?, Some(StreamFilter::Deflate));

        let digest = get_digest(offset);
        stream.add_key_value("Type".to_owned(), Box::new("XRef"));
        stream.add_key_value("Size".to_owned(), Box::new(i64::from(num) + 1));
        stream.add_key_value("W".to_owned(), Box::new([1i64, width as i64, 2]));
        stream.add_key_value("Root".to_owned(), Box::new(self.document_catalog));
        if let Some(info) = self.document_info {
            stream.add_key_value("Info".to_owned(), Box::new(info));
        }
        stream.add_key_value("ID".to_owned(), Box::new([digest.clone(), digest]));

        writeln!(context.output, "{} 0 obj", num)?;
        let mut formatter = Formatter {
            writer: &mut context.output,
        };
        stream.write(&mut formatter)?;
        write!(context.output, "\nendobj\n")?;
        write!(context.output, "startxref\n{}\n%%EOF\n", offset)
    }

    pub(crate) fn write(&mut self, context: &mut DocumentContext) -> Result<()> {
        writeln!(context.output, "trailer")?;
        let digest = get_digest(context.current_offset());

        let mut formatter = Formatter {
            writer: &mut context.output,
        };
        let mut formatter = formatter
            .format_dictionary()
            .key_value(&"Size", &(context.crossref.len() + 1))
            .key_value(&"Root", &self.document_catalog)
            .key_value(&"ID", &[digest.clone(), digest]);

        if let Some(document_info) = self.document_info {
            formatter = formatter.key_value(&"Info", &document_info);
        }
        formatter.finish()?;

        write!(context.output, "\nstartxref\n{}\n", self.crossref_offset)?;

        writeln!(context.output, "%%EOF")?;
        Ok(())
    }
}
