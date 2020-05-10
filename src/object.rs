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
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use lemon_pdf_derive::PdfFormat;

use crate::array::Array;
use crate::dictionary::Dictionary;
use crate::stream::Stream;
use serde::Serialize;

#[allow(missing_debug_implementations)]
pub struct Formatter<'a> {
    pub(crate) writer: &'a mut dyn Write,
}

impl<'a> Formatter<'a> {
    pub fn format_dictionary<'b>(&'b mut self) -> DictionaryFormatter<'a, 'b>
    where
        'a: 'b,
    {
        DictionaryFormatter::new(self)
    }

    pub fn format_array<'b>(&'b mut self) -> ArrayFormatter<'a, 'b>
    where
        'a: 'b,
    {
        ArrayFormatter::new(self)
    }
}

impl<'a> Write for Formatter<'a> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        self.writer.write(bytes)
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()
    }
}

#[allow(missing_debug_implementations)]
#[must_use]
pub struct DictionaryFormatter<'a, 'b> {
    formatter: &'b mut Formatter<'a>,
    result: Result<()>,
}

impl<'a, 'b> DictionaryFormatter<'a, 'b> {
    fn new(formatter: &'b mut Formatter<'a>) -> Self {
        let mut result = Ok(());
        result = result.and_then(|_| write!(formatter, "<< "));
        DictionaryFormatter { formatter, result }
    }

    pub fn key_value(mut self, key: &dyn PdfFormat, value: &dyn PdfFormat) -> Self {
        let formatter = &mut self.formatter;
        self.result = self.result.and_then(|()| {
            key.write(formatter)?;
            write!(formatter, " ")?;
            value.write(formatter)?;
            writeln!(formatter)?;
            Ok(())
        });
        self
    }

    pub fn finish(mut self) -> Result<()> {
        let formatter = &mut self.formatter;
        self.result = self.result.and_then(|()| write!(formatter, ">>"));
        self.result
    }
}

#[allow(missing_debug_implementations)]
#[must_use]
pub struct ArrayFormatter<'a, 'b> {
    formatter: &'b mut Formatter<'a>,
    result: Result<()>,
}

impl<'a, 'b> ArrayFormatter<'a, 'b> {
    fn new(formatter: &'b mut Formatter<'a>) -> Self {
        let mut result = Ok(());
        result = result.and_then(|_| write!(formatter, "[ "));
        ArrayFormatter { formatter, result }
    }

    pub fn value(mut self, value: &dyn PdfFormat) -> Self {
        let formatter = &mut self.formatter;
        self.result = self.result.and_then(|()| {
            value.write(formatter)?;
            write!(formatter, " ")?;
            Ok(())
        });
        self
    }

    pub fn finish(mut self) -> Result<()> {
        let formatter = &mut self.formatter;
        self.result = self.result.and_then(|()| write!(formatter, "]"));
        self.result
    }
}

pub trait PdfFormat: std::fmt::Debug {
    fn write(&self, output: &mut Formatter) -> Result<()>;
}

impl<'a> PdfFormat for &'a str {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        // TODO: proper escaping
        write!(output, "/{}", self)
    }
}

impl<'a, T> PdfFormat for &'a T
where
    T: PdfFormat,
{
    fn write(&self, output: &mut Formatter) -> Result<()> {
        (*self).write(output)
    }
}

impl PdfFormat for bool {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        match self {
            true => write!(output, "true"),
            false => write!(output, "false"),
        }
    }
}

impl PdfFormat for String {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        <Self as AsRef<str>>::as_ref(self).write(output)
    }
}

impl<'a> PdfFormat for &'a [u8] {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "(")?;
        for &byte in self.iter() {
            let escape = match byte {
                b'\\' | b'(' | b')' => true,
                _ => false,
            };
            if escape {
                write!(output, "\\")?;
            }
            output.write_all(&[byte])?;
        }
        write!(output, ")")
    }
}

impl PdfFormat for Vec<u8> {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        <Self as AsRef<[u8]>>::as_ref(self).write(output)
    }
}

impl PdfFormat for f32 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{:.2}", self)
    }
}

impl PdfFormat for f64 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{:.2}", self)
    }
}

impl PdfFormat for u32 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{}", self)
    }
}

impl PdfFormat for usize {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{}", self)
    }
}

impl PdfFormat for i64 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{}", self)
    }
}

#[derive(Debug, From, PdfFormat)]
pub enum Value {
    Null,
    Boolean(bool),
    Integer(i64),
    Real(f64),
    String(Vec<u8>),
    Name(String),
    Array(Array),
    Dictionary(Dictionary),
    Stream(Stream),
}

impl<'a> From<&'a str> for Value {
    fn from(from: &'a str) -> Value {
        Value::Name(from.to_string())
    }
}

impl From<f32> for Value {
    fn from(from: f32) -> Value {
        Value::Real(f64::from(from))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename = "R")]
pub struct RawIndirectReference(pub i64, pub i64);

impl<T> From<IndirectReference<T>> for RawIndirectReference {
    fn from(other: IndirectReference<T>) -> Self {
        RawIndirectReference(other.number(), other.generation())
    }
}

/// A reference to an indirect object.
///
/// Indirect references to objects can be obtained using `write_object` or
/// `write_object_fn` on the `Context`.
#[derive(Serialize)]
#[serde(into = "RawIndirectReference")]
pub struct IndirectReference<T> {
    raw: RawIndirectReference,
    _marker: PhantomData<T>,
}

impl<T> PartialEq for IndirectReference<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw() == other.raw()
    }
}

impl<T> Eq for IndirectReference<T> {}

impl<T> Hash for IndirectReference<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state)
    }
}

impl<T> Debug for IndirectReference<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.raw().fmt(f)
    }
}

impl<T> Default for IndirectReference<T> {
    fn default() -> Self {
        IndirectReference::new(0, 0)
    }
}

impl<T> Copy for IndirectReference<T> {}

impl<T> Clone for IndirectReference<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> IndirectReference<T> {
    pub(crate) fn new(number: i64, generation: i64) -> Self {
        IndirectReference {
            raw: RawIndirectReference(number, generation),
            _marker: PhantomData,
        }
    }

    pub fn number(&self) -> i64 {
        self.raw.0
    }

    pub fn generation(&self) -> i64 {
        self.raw.1
    }

    /// Converts an indirect reference to point to an object of type `U`.
    ///
    /// This function is to be used with care, else invalid PDF documents may be created.
    pub fn convert<U>(self) -> IndirectReference<U> {
        IndirectReference::new(self.number(), self.generation())
    }

    pub fn raw(self) -> RawIndirectReference {
        self.raw
    }
}

impl<T> PdfFormat for IndirectReference<T> {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "{} {} R", self.number(), self.generation())
    }
}

impl<T: PdfFormat> PdfFormat for Option<T> {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        match self {
            Some(val) => val.write(f),
            None => write!(f, "null"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Object<T> {
    Direct(T),
    Indirect(IndirectReference<T>),
}

impl<T> From<IndirectReference<T>> for Object<T> {
    fn from(reference: IndirectReference<T>) -> Self {
        Object::Indirect(reference)
    }
}
