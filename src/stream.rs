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

//! Streams (ISO 32000-1, 7.3.8): a dictionary followed by a sequence of bytes.
//!
//! `Stream<D>` is a stream whose dictionary has the type `D`, a struct that derives
//! `PdfDictionary` (or `PdfFormat`). `Length` and `Filter` are written by the stream itself, so
//! they always match the data. Streams are indirect objects only: they implement
//! `IndirectObject`, but not `PdfFormat`, and are referred to with `IndirectReference<Stream<D>>`.

use std::fmt;
use std::io::{BufWriter, Cursor, Error, ErrorKind, Result, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate as lemon_pdf;
use crate::object::{Formatter, IndirectObject, PdfDictionary};
use lemon_pdf_derive::PdfFormat;

/// The ways lemon_pdf can encode stream data.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StreamFilter {
    Deflate,
}

/// The filters (ISO 32000-1, 7.4) that decode the data of a stream.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PdfFormat)]
pub enum Filter {
    /// zlib/deflate compressed data.
    FlateDecode,
    /// A JPEG file.
    DCTDecode,
}

impl From<StreamFilter> for Filter {
    fn from(filter: StreamFilter) -> Filter {
        match filter {
            StreamFilter::Deflate => Filter::FlateDecode,
        }
    }
}

#[derive(Debug)]
enum StreamEncoderType {
    Identity(Cursor<Vec<u8>>),
    Deflate(BufWriter<ZlibEncoder<Cursor<Vec<u8>>>>),
}

/// Encodes the data of a stream while it is written. Turn it into a stream with
/// `Stream::from_encoder`.
#[derive(Debug)]
pub struct StreamEncoder {
    enc_type: StreamEncoderType,
}

impl StreamEncoder {
    pub fn new(filter: Option<StreamFilter>) -> Self {
        let enc_type = match filter {
            None => StreamEncoderType::Identity(Cursor::new(Vec::new())),
            Some(StreamFilter::Deflate) => StreamEncoderType::Deflate(BufWriter::with_capacity(
                32 * 1024,
                ZlibEncoder::new(Cursor::new(Vec::new()), Compression::default()),
            )),
        };
        StreamEncoder { enc_type }
    }

    /// The encoded bytes and the filter that decodes them.
    fn finish(self) -> Result<(Vec<u8>, Option<Filter>)> {
        Ok(match self.enc_type {
            StreamEncoderType::Identity(enc) => (enc.into_inner(), None),
            StreamEncoderType::Deflate(enc) => {
                let encoder = enc
                    .into_inner()
                    .map_err(|err| Error::new(ErrorKind::Other, err.to_string()))?;
                (encoder.finish()?.into_inner(), Some(Filter::FlateDecode))
            }
        })
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

/// A stream with a dictionary of type `D`.
pub struct Stream<D> {
    /// The entries of the stream dictionary, except `Length` and `Filter`.
    pub dict: D,
    filter: Option<Filter>,
    bytes: Vec<u8>,
}

impl<D> Stream<D> {
    /// A stream with unencoded data.
    pub fn new(dict: D, bytes: Vec<u8>) -> Self {
        Stream {
            dict,
            filter: None,
            bytes,
        }
    }

    /// A stream with the data compressed with Flate.
    pub fn deflated(dict: D, bytes: &[u8]) -> Result<Self> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        Ok(Stream {
            dict,
            filter: Some(Filter::FlateDecode),
            bytes: encoder.finish()?,
        })
    }

    /// A stream with data that is already encoded with `filter`, like a JPEG file
    /// (`Filter::DCTDecode`).
    pub fn with_encoded_data(dict: D, filter: Filter, bytes: Vec<u8>) -> Self {
        Stream {
            dict,
            filter: Some(filter),
            bytes,
        }
    }

    /// A stream with the data written to `encoder`.
    pub fn from_encoder(dict: D, encoder: StreamEncoder) -> Result<Self> {
        let (bytes, filter) = encoder.finish()?;
        Ok(Stream {
            dict,
            filter,
            bytes,
        })
    }

    /// The (encoded) data of the stream.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn filter(&self) -> Option<Filter> {
        self.filter
    }
}

impl<D: fmt::Debug> fmt::Debug for Stream<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream")
            .field("dict", &self.dict)
            .field("filter", &self.filter)
            .field("length", &self.bytes.len())
            .finish()
    }
}

impl<D: PdfDictionary + fmt::Debug> IndirectObject for Stream<D> {
    const IS_STREAM: bool = true;

    fn write_indirect(&self, f: &mut Formatter) -> Result<()> {
        let mut dict = self.dict.write_entries(f.format_dictionary());
        if let Some(filter) = &self.filter {
            dict = dict.key_value("Filter", filter);
        }
        dict.key_value("Length", &self.bytes.len()).finish()?;
        write!(f, "\nstream\n")?;
        f.write_all(&self.bytes)?;
        write!(f, "\nendstream")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::Dictionary;
    use crate::object::Name;

    fn written(object: &impl IndirectObject) -> Vec<u8> {
        let mut buffer = Vec::new();
        object
            .write_indirect(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        buffer
    }

    #[test]
    fn length_and_filter_are_written() {
        let mut dict = Dictionary::new();
        dict.insert("Type".to_owned(), Box::new(Name::from("Test")));
        let stream = Stream::new(dict, b"abc".to_vec());
        assert_eq!(
            written(&stream),
            b"<< /Type /Test\n/Length 3\n>>\nstream\nabc\nendstream".to_vec()
        );

        let stream = Stream::with_encoded_data(Dictionary::new(), Filter::DCTDecode, vec![1, 2]);
        assert_eq!(
            written(&stream),
            b"<< /Filter /DCTDecode\n/Length 2\n>>\nstream\n\x01\x02\nendstream".to_vec()
        );
    }

    #[test]
    fn encoder_output_round_trips() {
        let mut encoder = StreamEncoder::new(Some(StreamFilter::Deflate));
        encoder.write_all(b"hello hello hello").unwrap();
        let stream = Stream::from_encoder(Dictionary::new(), encoder).unwrap();
        assert_eq!(stream.filter(), Some(Filter::FlateDecode));
        let mut decoded = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(stream.bytes()),
            &mut decoded,
        )
        .unwrap();
        assert_eq!(decoded, b"hello hello hello");
    }
}
