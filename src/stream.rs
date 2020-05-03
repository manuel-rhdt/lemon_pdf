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

use std::io::{Cursor, Result, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::dictionary::Dictionary;
use crate::object::{Formatter, PdfFormat, Value};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StreamFilter {
    Deflate,
}

impl PdfFormat for StreamFilter {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        match self {
            StreamFilter::Deflate => "FlateDecode".write(f),
        }
    }
}

#[derive(Debug)]
enum StreamEncoderType {
    Identity(Cursor<Vec<u8>>),
    Deflate(ZlibEncoder<Cursor<Vec<u8>>>),
}

/// A convenience type to create PDF streams.
#[derive(Debug)]
pub struct StreamEncoder {
    enc_type: StreamEncoderType,
}

impl StreamEncoder {
    pub fn new(filter: Option<StreamFilter>) -> Self {
        let enc_type = match filter {
            None => StreamEncoderType::Identity(Cursor::new(Vec::new())),
            Some(StreamFilter::Deflate) => StreamEncoderType::Deflate(ZlibEncoder::new(
                Cursor::new(Vec::new()),
                Compression::default(),
            )),
        };
        StreamEncoder { enc_type }
    }

    pub fn into_stream(self) -> Stream {
        let (bytes, filter) = match self.enc_type {
            StreamEncoderType::Identity(enc) => (enc.into_inner(), None),
            StreamEncoderType::Deflate(enc) => (
                enc.finish().unwrap().into_inner(),
                Some(StreamFilter::Deflate),
            ),
        };
        Stream::with_bytes(bytes, filter)
    }
}

impl Write for StreamEncoder {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        match self.enc_type {
            StreamEncoderType::Identity(ref mut enc) => enc.write(buf),
            StreamEncoderType::Deflate(ref mut enc) => enc.write(buf),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self.enc_type {
            StreamEncoderType::Identity(ref mut enc) => enc.flush(),
            StreamEncoderType::Deflate(ref mut enc) => enc.flush(),
        }
    }
}

impl From<StreamEncoder> for Value {
    fn from(stream_encoder: StreamEncoder) -> Self {
        Value::Stream(stream_encoder.into_stream())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    // a Vec of bytes that can be decoded using the filter
    bytes: Vec<u8>,
    // A filter that specifies how the bytes are to be decoded. A value of None
    // means that the bytes are not encoded in any way.
    filter: Option<StreamFilter>,
    additional_keys: Dictionary,
}

impl Stream {
    pub(crate) fn with_bytes(bytes: Vec<u8>, filter: Option<StreamFilter>) -> Self {
        Stream {
            bytes,
            filter,
            additional_keys: Default::default(),
        }
    }
}

impl PdfFormat for Stream {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut dict_formatter = f.format_dictionary();
        match self.filter {
            None => {}
            Some(filter) => dict_formatter = dict_formatter.key_value(&"Filter", &filter),
        }
        let mut dict_formatter = dict_formatter.key_value(&"Length", &self.bytes.len());
        for (key, value) in self.additional_keys.iter() {
            dict_formatter = dict_formatter.key_value(key, value);
        }
        dict_formatter.finish()?;

        write!(f, "\nstream\n")?;
        f.write_all(&self.bytes)?;
        write!(f, "\nendstream")
    }
}
