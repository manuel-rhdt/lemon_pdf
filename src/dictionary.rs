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
use std::ops::Deref;

use crate::object::{Formatter, PdfFormat};

pub type Dictionary = HashMap<String, Box<dyn PdfFormat>>;

trait Helper {
    fn as_trait(&self) -> &dyn PdfFormat;
}

impl<T: PdfFormat + Sized> Helper for T {
    fn as_trait(&self) -> &dyn PdfFormat {
        self as &dyn PdfFormat
    }
}

impl Helper for &dyn PdfFormat {
    fn as_trait(&self) -> &dyn PdfFormat {
        *self
    }
}

impl<T, U, S> PdfFormat for HashMap<T, U, S>
where
    T: PdfFormat + Eq + Hash,
    U: PdfFormat,
    S: std::hash::BuildHasher,
{
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut dict_fmt = f.format_dictionary();
        for (key, value) in self.iter() {
            dict_fmt = dict_fmt.key_value(key, value);
        }
        dict_fmt.finish()
    }
}

impl<T, S> PdfFormat for HashMap<T, Box<dyn PdfFormat>, S>
where
    T: PdfFormat + Eq + Hash,
    S: std::hash::BuildHasher,
{
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut dict_fmt = f.format_dictionary();
        for (key, value) in self.iter() {
            dict_fmt = dict_fmt.key_value(key, value.deref());
        }
        dict_fmt.finish()
    }
}
