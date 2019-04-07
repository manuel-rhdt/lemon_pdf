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

use std::collections::HashMap;
use std::hash::Hash;
use std::io::Result;

use crate::object::{Formatter, Object, PdfFormat, Value};

pub type Dictionary = HashMap<String, Object<Value>>;

impl<T: PdfFormat + Eq + Hash, U: PdfFormat, S: std::hash::BuildHasher> PdfFormat
    for HashMap<T, U, S>
{
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut dict_fmt = f.format_dictionary();
        for (key, value) in self.iter() {
            dict_fmt = dict_fmt.key_value(key, value);
        }
        dict_fmt.finish()
    }
}
