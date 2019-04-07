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

use crate::object::{Formatter, PdfFormat, Value, Object};

/// An Array of `PdfObject`s.
pub type Array = Vec<Object<Value>>;

impl<T: PdfFormat> PdfFormat for Vec<T> {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut array_formatter = f.format_array();
        for obj in self.iter() {
            array_formatter = array_formatter.value(obj);
        }
        array_formatter.finish()
    }
}

impl<T: PdfFormat> PdfFormat for &[T] {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut array_formatter = f.format_array();
        for obj in self.iter() {
            array_formatter = array_formatter.value(obj);
        }
        array_formatter.finish()
    }
}

macro_rules! impl_array {
    ($num:expr) => {
        impl<T: PdfFormat> PdfFormat for [T; $num] {
            fn write(&self, f: &mut Formatter) -> Result<()> {
                let mut array_formatter = f.format_array();
                for obj in self.iter() {
                    array_formatter = array_formatter.value(obj);
                }
                array_formatter.finish()
            }
        }
    }
}

impl_array!(0);
impl_array!(1);
impl_array!(2);
impl_array!(3);
impl_array!(4);
impl_array!(5);
impl_array!(6);
impl_array!(7);
impl_array!(8);
impl_array!(9);
impl_array!(10);
impl_array!(11);
impl_array!(12);
impl_array!(13);
impl_array!(14);
impl_array!(15);
impl_array!(16);
