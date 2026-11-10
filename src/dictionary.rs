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

use std::collections::BTreeMap;
use std::io::Result;

use crate::object::{DictionaryFormatter, Formatter, PdfDictionary, PdfFormat};

/// A dictionary whose entries are only known at runtime. Typed dictionaries are structs that
/// derive `PdfFormat` instead.
///
/// Maps are `BTreeMap`s so that the entries are written in a deterministic order. The keys are
/// written as names.
pub type Dictionary = BTreeMap<String, Box<dyn PdfFormat>>;

impl<K, V> PdfDictionary for BTreeMap<K, V>
where
    K: AsRef<str> + Ord,
    V: PdfFormat,
{
    fn write_entries<'a, 'b>(
        &self,
        mut f: DictionaryFormatter<'a, 'b>,
    ) -> DictionaryFormatter<'a, 'b> {
        for (key, value) in self {
            f = f.key_value(key.as_ref(), value);
        }
        f
    }
}

impl<K, V> PdfFormat for BTreeMap<K, V>
where
    K: AsRef<str> + Ord + std::fmt::Debug,
    V: PdfFormat,
{
    fn write(&self, f: &mut Formatter) -> Result<()> {
        self.write_entries(f.format_dictionary()).finish()
    }
}
