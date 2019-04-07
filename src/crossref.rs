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

use crate::object::{Formatter, PdfFormat};

#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CrossRef {
    entries: Vec<(u64, u32)>,
}

impl CrossRef {
    /// Add a `PdfObject` to the `CrossRef` and get its object number.
    pub fn add_entry(&mut self, offset: u64, generation: u32) -> u32 {
        self.entries.push((offset, generation));
        self.len() - 1
    }

    pub fn get_entry_mut(&mut self, num: usize) -> &mut (u64, u32) {
        &mut self.entries[num]
    }

    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }
}

impl PdfFormat for CrossRef {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        writeln!(f, "xref")?;
        writeln!(f, "0 {}", self.entries.len())?;
        for entry in &self.entries {
            write!(f, "{:0>10} {:0>5} n\r\n", entry.0, entry.1)?;
        }
        Ok(())
    }
}
