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

//! Image XObjects (ISO 32000-1, 8.9) with an optional alpha channel (soft mask, ISO 32000-1,
//! 11.6.5.3).
//!
//! An `Image` is written with `DocumentContext::write_image`, added to a page with
//! `PageContext::add_xobject` and painted with `PageContext::draw_image`.
//!
//! Raw pixel data is compressed with Flate. JPEG files are embedded as they are (`DCTDecode`),
//! without decoding them. Other formats like PNG have to be decoded by the caller (for example
//! with the `image` crate) and passed as raw pixels.

use std::fmt;
use std::io::{Result, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate as lemon_pdf;
use crate::object::{Formatter, IndirectReference, PdfFormat};
use crate::DocumentContext;
use lemon_pdf_derive::PdfFormat;

/// The color spaces images can use.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PdfFormat)]
pub enum ColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
}

impl ColorSpace {
    /// The number of color components of a pixel.
    pub fn components(self) -> u32 {
        match self {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRGB => 3,
            ColorSpace::DeviceCMYK => 4,
        }
    }
}

/// Why an image could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// The width or the height is zero.
    Empty,
    /// The bits per component are not 1, 2, 4, 8 or 16.
    InvalidBitsPerComponent(u32),
    /// The pixel data (or the alpha channel) does not have the length the dimensions require.
    DataLength { expected: usize, actual: usize },
    /// The data is not a JPEG file, or it is truncated.
    InvalidJpeg(&'static str),
    /// A valid JPEG file that PDF readers cannot display (for example a lossless or
    /// arithmetic coded one).
    UnsupportedJpeg(&'static str),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Empty => write!(f, "the image has no pixels"),
            ImageError::InvalidBitsPerComponent(bits) => {
                write!(f, "{} bits per component are not supported", bits)
            }
            ImageError::DataLength { expected, actual } => write!(
                f,
                "the image data has {} bytes, expected {}",
                actual, expected
            ),
            ImageError::InvalidJpeg(reason) => write!(f, "invalid JPEG file: {}", reason),
            ImageError::UnsupportedJpeg(reason) => write!(f, "unsupported JPEG file: {}", reason),
        }
    }
}

impl std::error::Error for ImageError {}

#[derive(Debug, Clone, PartialEq)]
enum ImageData {
    /// Uncompressed samples, rows padded to whole bytes. Compressed with Flate when written.
    Raw(Vec<u8>),
    /// A complete JPEG file, written unchanged with the `DCTDecode` filter.
    Jpeg(Vec<u8>),
}

/// An image that can be embedded into a document with `DocumentContext::write_image`.
#[derive(Clone, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    color_space: ColorSpace,
    bits_per_component: u32,
    data: ImageData,
    /// The opacity of every pixel, 8 bits each, written as a soft mask.
    alpha: Option<Vec<u8>>,
    /// Inverts the color components (Adobe CMYK JPEG files store inverted values).
    inverted: bool,
    /// Asks the reader to smooth the image when it is scaled up.
    pub interpolate: bool,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("color_space", &self.color_space)
            .field("bits_per_component", &self.bits_per_component)
            .field("jpeg", &matches!(self.data, ImageData::Jpeg(_)))
            .field("alpha", &self.alpha.is_some())
            .field("interpolate", &self.interpolate)
            .finish()
    }
}

/// The number of bytes of uncompressed samples (every row padded to whole bytes).
fn data_length(width: u32, height: u32, components: u32, bits_per_component: u32) -> usize {
    let row_bits = width as usize * components as usize * bits_per_component as usize;
    (row_bits + 7) / 8 * height as usize
}

fn check_length(expected: usize, actual: usize) -> std::result::Result<(), ImageError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ImageError::DataLength { expected, actual })
    }
}

impl Image {
    /// Creates an image from uncompressed samples.
    ///
    /// The samples are stored row by row from the top left, the components of a pixel next to
    /// each other. With fewer than 8 bits per component, every row starts at a new byte;
    /// 16 bit samples are big-endian.
    pub fn new(
        width: u32,
        height: u32,
        color_space: ColorSpace,
        bits_per_component: u32,
        samples: Vec<u8>,
    ) -> std::result::Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        if ![1, 2, 4, 8, 16].contains(&bits_per_component) {
            return Err(ImageError::InvalidBitsPerComponent(bits_per_component));
        }
        let expected = data_length(width, height, color_space.components(), bits_per_component);
        check_length(expected, samples.len())?;
        Ok(Image {
            width,
            height,
            color_space,
            bits_per_component,
            data: ImageData::Raw(samples),
            alpha: None,
            inverted: false,
            interpolate: false,
        })
    }

    /// Creates an image from 8 bit RGB pixels.
    pub fn from_rgb8(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> std::result::Result<Self, ImageError> {
        Image::new(width, height, ColorSpace::DeviceRGB, 8, pixels)
    }

    /// Creates an image from 8 bit RGBA pixels (not premultiplied).
    ///
    /// The alpha channel becomes a soft mask; it is left out if every pixel is opaque.
    pub fn from_rgba8(
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> std::result::Result<Self, ImageError> {
        Image::split_alpha(width, height, ColorSpace::DeviceRGB, pixels)
    }

    /// Creates an image from 8 bit gray pixels.
    pub fn from_gray8(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> std::result::Result<Self, ImageError> {
        Image::new(width, height, ColorSpace::DeviceGray, 8, pixels)
    }

    /// Creates an image from 8 bit gray and alpha pixels (not premultiplied).
    ///
    /// The alpha channel becomes a soft mask; it is left out if every pixel is opaque.
    pub fn from_gray_alpha8(
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> std::result::Result<Self, ImageError> {
        Image::split_alpha(width, height, ColorSpace::DeviceGray, pixels)
    }

    fn split_alpha(
        width: u32,
        height: u32,
        color_space: ColorSpace,
        pixels: &[u8],
    ) -> std::result::Result<Self, ImageError> {
        let channels = color_space.components() as usize + 1;
        let pixel_count = width as usize * height as usize;
        check_length(pixel_count * channels, pixels.len())?;
        let mut color = Vec::with_capacity(pixel_count * (channels - 1));
        let mut alpha = Vec::with_capacity(pixel_count);
        for pixel in pixels.chunks_exact(channels) {
            color.extend_from_slice(&pixel[..channels - 1]);
            alpha.push(pixel[channels - 1]);
        }
        let image = Image::new(width, height, color_space, 8, color)?;
        if alpha.iter().all(|&a| a == 255) {
            Ok(image)
        } else {
            image.with_alpha(alpha)
        }
    }

    /// Embeds a JPEG file without decoding it. Gray, RGB (YCbCr) and CMYK JPEG files with
    /// baseline or progressive encoding are supported.
    pub fn from_jpeg(jpeg: Vec<u8>) -> std::result::Result<Self, ImageError> {
        let info = parse_jpeg(&jpeg)?;
        let color_space = match info.components {
            1 => ColorSpace::DeviceGray,
            3 => ColorSpace::DeviceRGB,
            4 => ColorSpace::DeviceCMYK,
            _ => {
                return Err(ImageError::UnsupportedJpeg(
                    "unsupported number of components",
                ))
            }
        };
        Ok(Image {
            width: info.width,
            height: info.height,
            color_space,
            bits_per_component: 8,
            data: ImageData::Jpeg(jpeg),
            alpha: None,
            // Adobe applications write CMYK JPEG files with inverted components, and mark them
            // with an APP14 segment.
            inverted: info.components == 4 && info.adobe,
            interpolate: false,
        })
    }

    /// Adds an alpha channel: the opacity of every pixel from 0 (transparent) to 255 (opaque),
    /// one byte per pixel, row by row from the top left.
    pub fn with_alpha(mut self, alpha: Vec<u8>) -> std::result::Result<Self, ImageError> {
        check_length(self.width as usize * self.height as usize, alpha.len())?;
        self.alpha = Some(alpha);
        Ok(self)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    pub fn has_alpha(&self) -> bool {
        self.alpha.is_some()
    }
}

/// An external object (ISO 32000-1, 8.8): a self-contained graphic that pages paint with the
/// `Do` operator.
#[derive(Debug)]
pub enum XObject {
    Image(ImageXObject),
}

impl PdfFormat for XObject {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        match self {
            XObject::Image(image) => image.write(f),
        }
    }

    fn is_stream(&self) -> bool {
        true
    }
}

/// The encoded stream of an image XObject.
pub struct ImageXObject {
    width: u32,
    height: u32,
    color_space: ColorSpace,
    bits_per_component: u32,
    filter: &'static str,
    decode: Option<Vec<f64>>,
    interpolate: bool,
    soft_mask: Option<IndirectReference<XObject>>,
    bytes: Vec<u8>,
}

impl fmt::Debug for ImageXObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageXObject")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("color_space", &self.color_space)
            .field("bits_per_component", &self.bits_per_component)
            .field("filter", &self.filter)
            .field("soft_mask", &self.soft_mask)
            .field("length", &self.bytes.len())
            .finish()
    }
}

impl PdfFormat for ImageXObject {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let mut dict = f
            .format_dictionary()
            .key_value(&"Type", &"XObject")
            .key_value(&"Subtype", &"Image")
            .key_value(&"Width", &self.width)
            .key_value(&"Height", &self.height)
            .key_value(&"ColorSpace", &self.color_space)
            .key_value(&"BitsPerComponent", &self.bits_per_component)
            .key_value(&"Filter", &self.filter)
            .key_value(&"Length", &self.bytes.len());
        if let Some(decode) = &self.decode {
            dict = dict.key_value(&"Decode", decode);
        }
        if self.interpolate {
            dict = dict.key_value(&"Interpolate", &true);
        }
        if let Some(soft_mask) = &self.soft_mask {
            dict = dict.key_value(&"SMask", soft_mask);
        }
        dict.finish()?;
        write!(f, "\nstream\n")?;
        f.write_all(&self.bytes)?;
        write!(f, "\nendstream")
    }

    fn is_stream(&self) -> bool {
        true
    }
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    encoder.finish()
}

impl<'a> DocumentContext<'a> {
    /// Writes an image (and its alpha channel as a soft mask) to the document. The returned
    /// reference can be added to any number of pages with `PageContext::add_xobject`.
    pub fn write_image(&mut self, image: Image) -> Result<IndirectReference<XObject>> {
        let soft_mask = match &image.alpha {
            Some(alpha) => Some(self.write_object(XObject::Image(ImageXObject {
                width: image.width,
                height: image.height,
                color_space: ColorSpace::DeviceGray,
                bits_per_component: 8,
                filter: "FlateDecode",
                decode: None,
                interpolate: image.interpolate,
                soft_mask: None,
                bytes: deflate(alpha)?,
            }))?),
            None => None,
        };
        let (filter, bytes) = match image.data {
            ImageData::Raw(samples) => ("FlateDecode", deflate(&samples)?),
            ImageData::Jpeg(jpeg) => ("DCTDecode", jpeg),
        };
        let decode = if image.inverted {
            Some(
                (0..image.color_space.components())
                    .flat_map(|_| vec![1.0, 0.0])
                    .collect(),
            )
        } else {
            None
        };
        self.write_object(XObject::Image(ImageXObject {
            width: image.width,
            height: image.height,
            color_space: image.color_space,
            bits_per_component: image.bits_per_component,
            filter,
            decode,
            interpolate: image.interpolate,
            soft_mask,
            bytes,
        }))
    }
}

#[derive(Debug, PartialEq)]
struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    /// Whether the file has an Adobe APP14 segment.
    adobe: bool,
}

/// Reads the dimensions and the number of components from the frame header of a JPEG file.
fn parse_jpeg(bytes: &[u8]) -> std::result::Result<JpegInfo, ImageError> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err(ImageError::InvalidJpeg("missing start of image marker"));
    }
    let truncated = ImageError::InvalidJpeg("unexpected end of file");
    let mut adobe = false;
    let mut pos = 2;
    loop {
        if *bytes.get(pos).ok_or_else(|| truncated.clone())? != 0xFF {
            return Err(ImageError::InvalidJpeg("expected a marker"));
        }
        // a marker can be preceded by any number of fill bytes (0xFF)
        while bytes.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *bytes.get(pos).ok_or_else(|| truncated.clone())?;
        pos += 1;
        match marker {
            // markers without a segment
            0x01 | 0xD0..=0xD8 => continue,
            0xD9 | 0xDA => return Err(ImageError::InvalidJpeg("no frame header")),
            _ => {}
        }
        let length = match bytes.get(pos..pos + 2) {
            Some(length) => usize::from(u16::from_be_bytes([length[0], length[1]])),
            None => return Err(truncated),
        };
        if length < 2 {
            return Err(ImageError::InvalidJpeg("invalid segment length"));
        }
        let segment = bytes
            .get(pos + 2..pos + length)
            .ok_or_else(|| truncated.clone())?;
        match marker {
            // baseline, extended sequential and progressive Huffman coding
            0xC0 | 0xC1 | 0xC2 => {
                if segment.len() < 6 {
                    return Err(ImageError::InvalidJpeg("frame header too short"));
                }
                if segment[0] != 8 {
                    return Err(ImageError::UnsupportedJpeg(
                        "only 8 bit samples are supported",
                    ));
                }
                let height = u32::from(u16::from_be_bytes([segment[1], segment[2]]));
                let width = u32::from(u16::from_be_bytes([segment[3], segment[4]]));
                if height == 0 {
                    return Err(ImageError::UnsupportedJpeg(
                        "the height is defined after the scan",
                    ));
                }
                if width == 0 {
                    return Err(ImageError::InvalidJpeg("the width is zero"));
                }
                return Ok(JpegInfo {
                    width,
                    height,
                    components: segment[5],
                    adobe,
                });
            }
            // lossless, hierarchical and arithmetic coding (0xC4, 0xC8 and 0xCC are other
            // segments)
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(ImageError::UnsupportedJpeg(
                    "lossless, hierarchical and arithmetic coding are not supported",
                ))
            }
            0xEE if segment.starts_with(b"Adobe") => adobe = true,
            _ => {}
        }
        pos += length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of a JPEG file: SOI, optionally an Adobe APP14 segment, and a frame header.
    fn jpeg_header(frame_marker: u8, components: u8, adobe: bool) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8];
        // an APP0 segment that has to be skipped
        jpeg.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x07, b'J', b'F', b'I', b'F', 0x00]);
        if adobe {
            jpeg.extend_from_slice(&[0xFF, 0xEE, 0x00, 0x0E]);
            jpeg.extend_from_slice(b"Adobe");
            jpeg.extend_from_slice(&[0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x02]);
        }
        let length = 8 + 3 * u16::from(components);
        jpeg.extend_from_slice(&[0xFF, 0xFF, frame_marker]);
        jpeg.extend_from_slice(&length.to_be_bytes());
        jpeg.extend_from_slice(&[8, 0x01, 0x2C, 0x02, 0x58, components]);
        for id in 0..components {
            jpeg.extend_from_slice(&[id + 1, 0x11, 0]);
        }
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn jpeg_dimensions_are_read() {
        let info = parse_jpeg(&jpeg_header(0xC0, 3, false)).unwrap();
        assert_eq!(
            info,
            JpegInfo {
                width: 600,
                height: 300,
                components: 3,
                adobe: false
            }
        );
        let info = parse_jpeg(&jpeg_header(0xC2, 4, true)).unwrap();
        assert_eq!((info.components, info.adobe), (4, true));
    }

    #[test]
    fn adobe_cmyk_jpeg_is_inverted() {
        let image = Image::from_jpeg(jpeg_header(0xC0, 4, true)).unwrap();
        assert_eq!(image.color_space(), ColorSpace::DeviceCMYK);
        assert!(image.inverted);
        let image = Image::from_jpeg(jpeg_header(0xC0, 3, true)).unwrap();
        assert!(!image.inverted);
    }

    #[test]
    fn invalid_jpeg_files_are_rejected() {
        assert_eq!(
            parse_jpeg(b"\x89PNG"),
            Err(ImageError::InvalidJpeg("missing start of image marker"))
        );
        let header = jpeg_header(0xC0, 3, false);
        assert!(matches!(
            parse_jpeg(&header[..header.len() - 8]),
            Err(ImageError::InvalidJpeg(_))
        ));
        assert!(matches!(
            parse_jpeg(&jpeg_header(0xC3, 3, false)),
            Err(ImageError::UnsupportedJpeg(_))
        ));
        assert!(matches!(
            Image::from_jpeg(jpeg_header(0xC0, 2, false)),
            Err(ImageError::UnsupportedJpeg(_))
        ));
    }

    #[test]
    fn data_length_is_checked() {
        assert!(Image::from_rgb8(2, 2, vec![0; 12]).is_ok());
        assert_eq!(
            Image::from_rgb8(2, 2, vec![0; 11]),
            Err(ImageError::DataLength {
                expected: 12,
                actual: 11
            })
        );
        // rows of 1 bit images are padded to whole bytes
        assert!(Image::new(9, 3, ColorSpace::DeviceGray, 1, vec![0; 6]).is_ok());
        assert!(Image::new(2, 1, ColorSpace::DeviceCMYK, 16, vec![0; 16]).is_ok());
        assert_eq!(
            Image::new(1, 1, ColorSpace::DeviceGray, 3, vec![0]),
            Err(ImageError::InvalidBitsPerComponent(3))
        );
        assert_eq!(Image::from_gray8(0, 5, vec![]), Err(ImageError::Empty));
        assert!(Image::from_gray8(1, 2, vec![0; 2])
            .unwrap()
            .with_alpha(vec![0; 3])
            .is_err());
    }

    #[test]
    fn alpha_is_split_from_rgba() {
        let image = Image::from_rgba8(2, 1, &[1, 2, 3, 128, 4, 5, 6, 255]).unwrap();
        assert_eq!(image.data, ImageData::Raw(vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(image.alpha, Some(vec![128, 255]));

        let image = Image::from_gray_alpha8(2, 1, &[7, 0, 8, 10]).unwrap();
        assert_eq!(image.data, ImageData::Raw(vec![7, 8]));
        assert_eq!(image.alpha, Some(vec![0, 10]));
    }

    #[test]
    fn opaque_alpha_is_dropped() {
        let image = Image::from_rgba8(1, 1, &[1, 2, 3, 255]).unwrap();
        assert!(!image.has_alpha());
    }
}
