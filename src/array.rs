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

use std::io::Result;

use crate::object::{Formatter, Object, PdfFormat, Value};

/// An array whose elements are only known at runtime. Typed arrays are `Vec<T>`, `[T]` or
/// `[T; N]` of a type `T` that implements `PdfFormat`.
pub type Array = Vec<Object<Value>>;

impl<T: PdfFormat> PdfFormat for [T] {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut array_formatter = f.format_array();
        for obj in self.iter() {
            array_formatter = array_formatter.value(obj);
        }
        array_formatter.finish()
    }
}

impl<T: PdfFormat> PdfFormat for Vec<T> {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        self.as_slice().write(f)
    }
}

impl<T: PdfFormat, const N: usize> PdfFormat for [T; N] {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        self[..].write(f)
    }
}
