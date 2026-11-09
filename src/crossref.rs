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
    /// Objects stored in object streams: object number -> (object stream number, index).
    compressed: std::collections::BTreeMap<u32, (u32, u32)>,
}

impl CrossRef {
    /// Add a `PdfObject` to the `CrossRef` and get its object number.
    pub fn add_entry(&mut self, offset: u64, generation: u32) -> u32 {
        self.entries.push((offset, generation));
        self.len()
    }

    pub fn get_entry_mut(&mut self, num: u32) -> &mut (u64, u32) {
        &mut self.entries[(num-1) as usize]
    }

    pub fn set_compressed(&mut self, num: u32, stream: u32, index: u32) {
        self.compressed.insert(num, (stream, index));
    }

    /// Whether object `num` was reserved but never written (its offset is still the placeholder
    /// 0, which can never be the offset of an object because the file starts with the header).
    fn is_free(&self, num: u32) -> bool {
        num == 0 || (self.entries[(num - 1) as usize].0 == 0 && !self.compressed.contains_key(&num))
    }

    /// The next free object number after `num` in the linked list of free objects, or 0.
    fn next_free(&self, num: u32) -> u32 {
        (num + 1..=self.len()).find(|&n| self.is_free(n)).unwrap_or(0)
    }

    /// Writes a cross-reference stream's data: one `[1, offset_width, 2]` row per object
    /// (including object 0). Returns the data and the width of the second field.
    pub fn stream_data(&self) -> (Vec<u8>, usize) {
        let max = self
            .entries
            .iter()
            .map(|e| e.0)
            .chain(self.compressed.values().map(|v| v.0 as u64))
            .max()
            .unwrap_or(0);
        let width = (1..=8usize).find(|w| max >> (8 * w) == 0).unwrap_or(8);
        let mut data = Vec::with_capacity((self.entries.len() + 1) * (width + 3));
        let mut row = |kind: u8, a: u64, b: u16| {
            data.push(kind);
            data.extend_from_slice(&a.to_be_bytes()[8 - width..]);
            data.extend_from_slice(&b.to_be_bytes());
        };
        row(0, u64::from(self.next_free(0)), 65535);
        for (i, entry) in self.entries.iter().enumerate() {
            let num = i as u32 + 1;
            match self.compressed.get(&num) {
                Some(&(stream, index)) => row(2, stream as u64, index as u16),
                None if self.is_free(num) => row(0, u64::from(self.next_free(num)), 0),
                None => row(1, entry.0, entry.1 as u16),
            }
        }
        (data, width)
    }

    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }
}

impl PdfFormat for CrossRef {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        writeln!(f, "xref")?;
        writeln!(f, "0 {}", self.entries.len()+1)?;
        write!(f, "{:0>10} {:0>5} f\r\n", self.next_free(0), 65535)?;
        for (i, entry) in self.entries.iter().enumerate() {
            let num = i as u32 + 1;
            if self.is_free(num) {
                write!(f, "{:0>10} {:0>5} f\r\n", self.next_free(num), 0)?;
            } else {
                write!(f, "{:0>10} {:0>5} n\r\n", entry.0, entry.1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(crossref: &CrossRef) -> String {
        let mut buffer = Vec::new();
        crossref
            .write(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn table_entries_are_20_bytes() {
        let mut crossref = CrossRef::default();
        crossref.add_entry(15, 0);
        crossref.add_entry(1234567, 0);
        assert_eq!(
            written(&crossref),
            "xref\n0 3\n\
             0000000000 65535 f\r\n\
             0000000015 00000 n\r\n\
             0001234567 00000 n\r\n"
        );
    }

    #[test]
    fn unwritten_objects_are_free_entries() {
        let mut crossref = CrossRef::default();
        crossref.add_entry(15, 0);
        crossref.add_entry(0, 0); // reserved, never written
        crossref.add_entry(40, 0);
        crossref.add_entry(0, 0); // reserved, never written
        assert_eq!(
            written(&crossref),
            "xref\n0 5\n\
             0000000002 65535 f\r\n\
             0000000015 00000 n\r\n\
             0000000004 00000 f\r\n\
             0000000040 00000 n\r\n\
             0000000000 00000 f\r\n"
        );
    }

    #[test]
    fn stream_rows_have_three_fields() {
        let mut crossref = CrossRef::default();
        crossref.add_entry(0, 0); // stored in an object stream
        crossref.add_entry(300, 0); // the object stream
        crossref.add_entry(0, 0); // reserved, never written
        crossref.set_compressed(1, 2, 7);
        let (data, width) = crossref.stream_data();
        assert_eq!(width, 2, "300 needs two bytes");
        assert_eq!(
            data,
            vec![
                0, 0, 3, 0xff, 0xff, // object 0: head of the free list
                2, 0, 2, 0, 7, // object 1: index 7 in object stream 2
                1, 0x01, 0x2c, 0, 0, // object 2: at offset 300
                0, 0, 0, 0, 0, // object 3: free
            ]
        );
    }

    #[test]
    fn stream_offset_width_grows_with_offsets() {
        for &(offset, expected) in &[(0xffu64, 1), (0x100, 2), (0xffff, 2), (0x1_0000, 3)] {
            let mut crossref = CrossRef::default();
            crossref.add_entry(offset, 0);
            assert_eq!(crossref.stream_data().1, expected, "offset {:#x}", offset);
        }
    }
}
