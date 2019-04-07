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

use crate::context::{Context, DocumentCatalog};
use crate::object::{Formatter, IndirectReference};

pub struct Trailer {
    pub crossref_offset: u32,
    pub(crate) document_catalog: IndirectReference<DocumentCatalog>,
}

impl Trailer {
    pub(crate) fn write(&mut self, context: &mut Context<impl Write>) -> Result<()> {
        writeln!(context.output, "trailer")?;

        let mut formatter = Formatter {
            writer: &mut context.output,
        };
        formatter
            .format_dictionary()
            .key_value(&"Size", &context.crossref.len())
            .key_value(&"Root", &self.document_catalog)
            .finish()?;

        write!(context.output, "\nstartxref\n{}\n", self.crossref_offset)?;

        write!(context.output, "%%EOF")?;
        Ok(())
    }
}
