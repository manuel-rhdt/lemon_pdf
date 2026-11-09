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

use crate as lemon_pdf;
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

    /// Whether this object is a stream. Streams can never be stored inside object streams.
    fn is_stream(&self) -> bool {
        false
    }
}

/// Writes a name object (ISO 32000-1, 7.3.5). Bytes outside of the printable ASCII range,
/// delimiters and `#` are written as `#` followed by two hexadecimal digits.
impl<'a> PdfFormat for &'a str {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        output.write_all(b"/")?;
        for &byte in self.as_bytes() {
            match byte {
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#' => {
                    write!(output, "#{:02X}", byte)?
                }
                b'!'..=b'~' => output.write_all(&[byte])?,
                _ => write!(output, "#{:02X}", byte)?,
            }
        }
        Ok(())
    }
}

impl<'a, T> PdfFormat for &'a T
where
    T: PdfFormat,
{
    fn write(&self, output: &mut Formatter) -> Result<()> {
        (*self).write(output)
    }

    fn is_stream(&self) -> bool {
        (*self).is_stream()
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

/// A text string (ISO 32000-1, 7.9.2.2): human-readable text such as a title, a language tag
/// or an alternate description.
///
/// Printable ASCII is written as a literal string, anything else as UTF-16BE with a byte order
/// mark (a hexadecimal string).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextString(pub String);

impl PdfFormat for TextString {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        if self.0.bytes().all(|byte| byte == b' ' || byte.is_ascii_graphic()) {
            return self.0.as_bytes().write(output);
        }
        write!(output, "<FEFF")?;
        for unit in self.0.encode_utf16() {
            write!(output, "{:04X}", unit)?;
        }
        write!(output, ">")
    }
}

pub trait WriteEscaped {
    fn write_escaped(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn write_hex_escaped(&mut self, bytes: &[u8]) -> std::io::Result<()>;
}

impl<W: Write> WriteEscaped for W {
    fn write_escaped(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        for &byte in bytes {
            match byte {
                0x0c /* Form Feed */ => self.write_all(b"\\f")?,
                0x08 /* Backspace */ => self.write_all(b"\\b")?,
                b'\t' => self.write_all(b"\\t")?,
                b'\r' => self.write_all(b"\\r")?,
                b'\n' => self.write_all(b"\\n")?,
                b')' => self.write_all(b"\\)")?,
                b'(' => self.write_all(b"\\(")?,
                b'\\' => self.write_all(b"\\\\")?,
                non_graphic if !byte.is_ascii_graphic() && byte != b' ' => write!(self, "\\{:03o}", non_graphic)?,
                other => self.write_all(&[other])?
            }
        }
        Ok(())
    }

    fn write_hex_escaped(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        for &byte in bytes {
            write!(self, "{:02X}", byte)?;
        }
        Ok(())
    }
}

impl<'a> PdfFormat for &'a [u8] {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "(")?;
        output.write_escaped(self)?;
        write!(output, ")")
    }
}

impl PdfFormat for Vec<u8> {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        <Self as AsRef<[u8]>>::as_ref(self).write(output)
    }
}

/// The largest magnitude of a real number that PDF readers are expected to handle
/// (ISO 32000-1, Annex C.2: approximately ±3.403 × 10^38, the range of a 32-bit float).
pub const MAX_REAL: f64 = f32::MAX as f64;

/// Makes a real number representable in PDF, which has no NaN or infinity: NaN becomes 0, and
/// infinite or too large values are clamped to `±MAX_REAL`. Logs a warning when it changes
/// the value, since this usually points to a bug in the caller (like a division by zero).
pub(crate) fn sanitize_real(value: f64) -> f64 {
    if value.is_nan() {
        log::warn!("Cannot write NaN to a PDF, writing 0 instead");
        0.0
    } else if value.abs() > MAX_REAL {
        let clamped = MAX_REAL.copysign(value);
        log::warn!("Cannot write {} to a PDF, writing {} instead", value, clamped);
        clamped
    } else {
        value
    }
}

/// Writes a real number with at most two decimals, without trailing zeros
/// (`20.00` becomes `20`, `-41.50` becomes `-41.5`) to keep content streams small.
fn write_trimmed_real(output: &mut Formatter, value: f64) -> Result<()> {
    let formatted = format!("{:.2}", sanitize_real(value));
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    match trimmed {
        "" | "-" | "-0" => write!(output, "0"),
        other => write!(output, "{}", other),
    }
}

impl PdfFormat for f32 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write_trimmed_real(output, *self as f64)
    }
}

impl PdfFormat for f64 {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write_trimmed_real(output, *self)
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

impl<T: Default> Default for Object<T> {
    fn default() -> Self {
        Object::Direct(Default::default())
    }
}

impl<T> From<IndirectReference<T>> for Object<T> {
    fn from(reference: IndirectReference<T>) -> Self {
        Object::Indirect(reference)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hex_escape() {
        let mut outp = vec![];
        outp.write_hex_escaped(b"\nabc").unwrap();
        assert_eq!("0A616263", std::str::from_utf8(&outp).unwrap());
    }

    fn written(object: &dyn PdfFormat) -> String {
        let mut buffer = Vec::new();
        object
            .write(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn reals_are_trimmed() {
        assert_eq!(written(&20.0f64), "20");
        assert_eq!(written(&-41.5f64), "-41.5");
        assert_eq!(written(&0.126f64), "0.13");
        assert_eq!(written(&1.5f32), "1.5");
        assert_eq!(written(&0.0f64), "0");
        assert_eq!(written(&-0.0f64), "0");
        assert_eq!(written(&-0.001f64), "0");
        assert_eq!(written(&1e10f64), "10000000000");
    }

    #[test]
    fn nan_is_written_as_zero() {
        assert_eq!(written(&f64::NAN), "0");
        assert_eq!(written(&f32::NAN), "0");
        assert_eq!(written(&-f64::NAN), "0");
    }

    #[test]
    fn infinite_and_huge_reals_are_clamped() {
        let max = format!("{:.0}", MAX_REAL);
        assert_eq!(max, "340282346638528859811704183484516925440");
        assert_eq!(written(&f64::INFINITY), max);
        assert_eq!(written(&f64::NEG_INFINITY), format!("-{}", max));
        assert_eq!(written(&f32::INFINITY), max);
        assert_eq!(written(&1e300f64), max);
        assert_eq!(written(&-1e300f64), format!("-{}", max));
        // the largest f32 itself is in range
        assert_eq!(written(&f32::MAX), max);
    }

    #[test]
    fn plain_names_are_written_unchanged() {
        assert_eq!(written(&"Type"), "/Type");
        assert_eq!(written(&"F0"), "/F0");
        assert_eq!(written(&"A;B-C_D.E*F!G~"), "/A;B-C_D.E*F!G~");
        assert_eq!(written(&""), "/");
    }

    #[test]
    fn special_characters_in_names_are_escaped() {
        assert_eq!(written(&"My Font"), "/My#20Font");
        assert_eq!(written(&"a#b"), "/a#23b");
        assert_eq!(written(&"a/b"), "/a#2Fb");
        assert_eq!(written(&"()<>[]{}%"), "/#28#29#3C#3E#5B#5D#7B#7D#25");
        assert_eq!(written(&"tab\there"), "/tab#09here");
        assert_eq!(written(&"ü"), "/#C3#BC");
        assert_eq!(written(&"x\x7f"), "/x#7F");
        assert_eq!(written(&String::from("A B")), "/A#20B");
    }

    #[test]
    fn literal_strings_are_escaped() {
        assert_eq!(written(&&b"plain text"[..]), "(plain text)");
        assert_eq!(written(&&b"a(b)c\\d"[..]), "(a\\(b\\)c\\\\d)");
        assert_eq!(written(&&b"\n\r\t\x08\x0c"[..]), "(\\n\\r\\t\\b\\f)");
        assert_eq!(written(&&b"\x00\x7f\xff"[..]), "(\\000\\177\\377)");
        assert_eq!(written(&b"%".to_vec()), "(%)");
    }

    #[test]
    fn ascii_text_strings_are_literal_strings() {
        assert_eq!(written(&TextString("Hello World".to_owned())), "(Hello World)");
        assert_eq!(written(&TextString("a (b)".to_owned())), "(a \\(b\\))");
        assert_eq!(written(&TextString(String::new())), "()");
    }

    #[test]
    fn other_text_strings_are_utf16_with_bom() {
        assert_eq!(written(&TextString("für".to_owned())), "<FEFF006600FC0072>");
        // control characters are not printable, so they force the UTF-16 form
        assert_eq!(written(&TextString("a\nb".to_owned())), "<FEFF0061000A0062>");
        // characters outside the BMP become surrogate pairs
        assert_eq!(written(&TextString("𝄞".to_owned())), "<FEFFD834DD1E>");
    }
}
