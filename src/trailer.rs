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
    pub(crate) fn write(&mut self, context: &mut DocumentContext) -> Result<()> {
        writeln!(context.output, "trailer")?;
        let digest = get_digest(context.current_offset());

        let mut formatter = Formatter {
            writer: &mut context.output,
        };
        let mut formatter = formatter
            .format_dictionary()
            .key_value(&"Size", &context.crossref.len())
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
