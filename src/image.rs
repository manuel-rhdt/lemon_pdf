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
//! without decoding them. The same goes for PNG files without an alpha channel and without
//! interlacing: their compressed pixel data is a Flate stream with PNG predictors, which PDF
//! readers decode themselves. Other formats, and PNG files with an alpha channel, have to be
//! decoded by the caller (for example with the `image` crate) and passed as raw pixels.

use std::fmt;
use std::io::{Read, Result, Write};

use crate as lemon_pdf;
use crate::object::{Formatter, IndirectReference, Name, PdfFormat};
use crate::stream::{Filter, Stream};
use crate::DocumentContext;
use lemon_pdf_derive::{IndirectObject, PdfDictionary, PdfFormat};

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
    /// The data is not a PNG file, or it is truncated or inconsistent.
    InvalidPng(&'static str),
    /// A valid PNG file that cannot be embedded without decoding it (with an alpha channel,
    /// interlaced, or with partially transparent palette entries).
    UnsupportedPng(&'static str),
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
            ImageError::InvalidPng(reason) => write!(f, "invalid PNG file: {}", reason),
            ImageError::UnsupportedPng(reason) => write!(f, "unsupported PNG file: {}", reason),
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
    /// The concatenated `IDAT` chunks of a PNG file: a zlib stream of filtered rows, written
    /// unchanged with the `FlateDecode` filter and a PNG predictor.
    Png(Vec<u8>),
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
    /// The RGB palette of an indexed image, three bytes per entry. The samples are indices and
    /// `color_space` is the space of the entries.
    palette: Option<Vec<u8>>,
    /// Pixels whose samples are all inside of these (inclusive) ranges are transparent, two
    /// numbers per component.
    color_key: Option<Vec<u32>>,
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
            .field("png", &matches!(self.data, ImageData::Png(_)))
            .field("indexed", &self.palette.is_some())
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
            palette: None,
            color_key: None,
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
            palette: None,
            color_key: None,
            interpolate: false,
        })
    }

    /// Embeds a PNG file without decoding its pixels.
    ///
    /// Gray, RGB and palette images of any bit depth are supported, with a transparent color
    /// (`tRNS` of a gray or RGB image) or a single fully transparent palette entry. PNG files
    /// with an alpha channel or with partially transparent palette entries, and interlaced
    /// ones, are refused with `ImageError::UnsupportedPng`: PDF has no interleaved alpha, so
    /// they have to be decoded and passed with `Image::from_rgba8` and the like.
    ///
    /// The pixel data is inflated once to check its length (without keeping it); the
    /// checksums of the chunks are not verified. Gamma and color profiles are ignored.
    pub fn from_png(png: &[u8]) -> std::result::Result<Self, ImageError> {
        let info = parse_png(png)?;
        Ok(Image {
            width: info.width,
            height: info.height,
            color_space: info.color_space,
            bits_per_component: info.bit_depth,
            data: ImageData::Png(info.idat),
            alpha: None,
            inverted: false,
            palette: info.palette,
            color_key: info.color_key,
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
///
/// References to the variants convert into references to `XObject` (`.into()`).
#[derive(Debug, IndirectObject)]
#[indirect_variants]
pub enum XObject {
    Image(ImageXObject),
    Form(crate::form::FormXObject),
}

/// The color space of an image: a device space, or a palette of RGB colors.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageColorSpace {
    Device(ColorSpace),
    /// `[/Indexed /DeviceRGB hival lookup]`; the lookup table has three bytes per entry.
    Indexed(Vec<u8>),
}

struct HexString<'a>(&'a [u8]);

impl fmt::Debug for HexString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HexString({} bytes)", self.0.len())
    }
}

impl PdfFormat for HexString<'_> {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        write!(output, "<")?;
        for byte in self.0 {
            write!(output, "{:02X}", byte)?;
        }
        write!(output, ">")
    }
}

impl PdfFormat for ImageColorSpace {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        match self {
            ImageColorSpace::Device(space) => space.write(output),
            ImageColorSpace::Indexed(palette) => output
                .format_array()
                .value(&Name::from("Indexed"))
                .value(&Name::from("DeviceRGB"))
                .value(&(palette.len() / 3 - 1))
                .value(&HexString(palette))
                .finish(),
        }
    }
}

/// The `DecodeParms` of a PNG predictor (ISO 32000-1, Table 8): the rows start with a filter
/// type byte, as in a PNG file (`Predictor` 15).
#[derive(Debug, Clone, PartialEq)]
pub struct PngPredictor {
    pub predictor: u32,
    pub colors: u32,
    pub bits_per_component: u32,
    pub columns: u32,
}

impl PdfFormat for PngPredictor {
    fn write(&self, output: &mut Formatter) -> Result<()> {
        output
            .format_dictionary()
            .key_value("Predictor", &self.predictor)
            .key_value("Colors", &self.colors)
            .key_value("BitsPerComponent", &self.bits_per_component)
            .key_value("Columns", &self.columns)
            .finish()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The dictionary of an image XObject (ISO 32000-1, 8.9.5).
///
/// `Image` and `DocumentContext::write_image` create image XObjects whose data matches the
/// dictionary.
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[rename("XObject")]
#[constant(Subtype = "Image")]
pub struct ImageDict {
    pub width: u32,
    pub height: u32,
    pub color_space: ImageColorSpace,
    pub bits_per_component: u32,
    /// How the data of a PNG file is predicted (ISO 32000-1, 7.4.4.4).
    pub decode_parms: Option<PngPredictor>,
    /// A color key mask (ISO 32000-1, 8.9.6.4): the samples that are transparent.
    pub mask: Option<Vec<u32>>,
    /// Maps the samples to the range of the color components, two numbers per component.
    pub decode: Option<Vec<f64>>,
    /// Asks the reader to smooth the image when it is scaled up.
    #[skip_if(is_false)]
    pub interpolate: bool,
    /// The alpha channel.
    #[rename("SMask")]
    pub soft_mask: Option<IndirectReference<SoftMask>>,
}

/// An image XObject.
pub type ImageXObject = Stream<ImageDict>;

/// The dictionary of a soft mask image (ISO 32000-1, 11.6.5.3): a grayscale image whose
/// samples are the opacity of the pixels of the image it belongs to.
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[rename("XObject")]
#[constant(Subtype = "Image", ColorSpace = "DeviceGray")]
pub struct SoftMaskDict {
    pub width: u32,
    pub height: u32,
    pub bits_per_component: u32,
    #[skip_if(is_false)]
    pub interpolate: bool,
}

/// A soft mask image, the alpha channel of an image XObject.
pub type SoftMask = Stream<SoftMaskDict>;

impl<'a> DocumentContext<'a> {
    /// Writes an image (and its alpha channel as a soft mask) to the document. The returned
    /// reference can be added to any number of pages with `PageContext::add_xobject`.
    pub fn write_image(&mut self, image: Image) -> Result<IndirectReference<ImageXObject>> {
        let soft_mask = match &image.alpha {
            Some(alpha) => {
                let dict = SoftMaskDict {
                    width: image.width,
                    height: image.height,
                    bits_per_component: 8,
                    interpolate: image.interpolate,
                };
                Some(self.write_object(Stream::deflated(dict, alpha)?)?)
            }
            None => None,
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
        let color_space = match &image.palette {
            Some(palette) => ImageColorSpace::Indexed(palette.clone()),
            None => ImageColorSpace::Device(image.color_space),
        };
        let decode_parms = match image.data {
            ImageData::Png(_) => Some(PngPredictor {
                predictor: 15,
                colors: if image.palette.is_some() {
                    1
                } else {
                    image.color_space.components()
                },
                bits_per_component: image.bits_per_component,
                columns: image.width,
            }),
            _ => None,
        };
        let dict = ImageDict {
            width: image.width,
            height: image.height,
            color_space,
            bits_per_component: image.bits_per_component,
            decode_parms,
            mask: image.color_key.clone(),
            decode,
            interpolate: image.interpolate,
            soft_mask,
        };
        let stream = match image.data {
            ImageData::Raw(samples) => Stream::deflated(dict, &samples)?,
            ImageData::Jpeg(jpeg) => Stream::with_encoded_data(dict, Filter::DCTDecode, jpeg),
            ImageData::Png(idat) => Stream::with_encoded_data(dict, Filter::FlateDecode, idat),
        };
        self.write_object(stream)
    }
}

#[derive(Debug)]
struct PngInfo {
    width: u32,
    height: u32,
    bit_depth: u32,
    color_space: ColorSpace,
    palette: Option<Vec<u8>>,
    color_key: Option<Vec<u32>>,
    idat: Vec<u8>,
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Reads the chunks of a PNG file that decide how the pixel data is embedded, and collects the
/// pixel data.
fn parse_png(bytes: &[u8]) -> std::result::Result<PngInfo, ImageError> {
    use ImageError::{InvalidPng, UnsupportedPng};

    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(InvalidPng("missing signature"));
    }
    let mut pos = PNG_SIGNATURE.len();
    // (width, height, bit depth, color type)
    let mut header: Option<(u32, u32, u32, u8)> = None;
    let mut palette: Option<Vec<u8>> = None;
    let mut transparency: Option<Vec<u8>> = None;
    let mut idat = Vec::new();
    // 0: before the pixel data, 1: in the IDAT chunks, 2: after them
    let mut data_state = 0;
    let mut ended = false;
    while pos < bytes.len() {
        let head = bytes
            .get(pos..pos + 8)
            .ok_or(InvalidPng("truncated chunk"))?;
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = [head[4], head[5], head[6], head[7]];
        // the chunk is followed by a 4 byte checksum
        let body = pos
            .checked_add(12)
            .and_then(|start| start.checked_add(length))
            .filter(|&end| end <= bytes.len())
            .map(|_| &bytes[pos + 8..pos + 8 + length])
            .ok_or(InvalidPng("truncated chunk"))?;
        pos += 12 + length;
        if header.is_none() && &kind != b"IHDR" {
            return Err(InvalidPng("the first chunk is not IHDR"));
        }
        if &kind == b"IDAT" {
            if data_state == 2 {
                return Err(InvalidPng("the IDAT chunks are not contiguous"));
            }
            data_state = 1;
            idat.extend_from_slice(body);
            continue;
        }
        if data_state == 1 {
            data_state = 2;
        }
        match &kind {
            b"IHDR" => {
                if header.is_some() || body.len() != 13 {
                    return Err(InvalidPng("invalid IHDR chunk"));
                }
                let width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                let height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                if width == 0 || height == 0 {
                    return Err(ImageError::Empty);
                }
                if body[10] != 0 || body[11] != 0 {
                    return Err(InvalidPng("unknown compression or filter method"));
                }
                match body[12] {
                    0 => {}
                    1 => return Err(UnsupportedPng("interlaced images need decoded pixels")),
                    _ => return Err(InvalidPng("unknown interlace method")),
                }
                let (depth, color_type) = (u32::from(body[8]), body[9]);
                let valid = match color_type {
                    0 => [1, 2, 4, 8, 16].contains(&depth),
                    3 => [1, 2, 4, 8].contains(&depth),
                    2 | 4 | 6 => [8, 16].contains(&depth),
                    _ => return Err(InvalidPng("unknown color type")),
                };
                if !valid {
                    return Err(InvalidPng("invalid bit depth for the color type"));
                }
                if color_type == 4 || color_type == 6 {
                    return Err(UnsupportedPng("an alpha channel needs decoded pixels"));
                }
                header = Some((width, height, depth, color_type));
            }
            b"PLTE" => {
                if data_state != 0 || palette.is_some() || body.is_empty() || body.len() % 3 != 0 {
                    return Err(InvalidPng("invalid PLTE chunk"));
                }
                palette = Some(body.to_vec());
            }
            b"tRNS" => transparency = Some(body.to_vec()),
            b"IEND" => {
                ended = true;
                break;
            }
            _ => {}
        }
    }
    let (width, height, depth, color_type) = header.ok_or(InvalidPng("missing IHDR chunk"))?;
    if !ended || idat.is_empty() {
        return Err(InvalidPng("missing pixel data"));
    }

    let (color_space, components) = match color_type {
        0 => (ColorSpace::DeviceGray, 1),
        2 => (ColorSpace::DeviceRGB, 3),
        _ => (ColorSpace::DeviceRGB, 1),
    };
    let (palette, color_key) = if color_type == 3 {
        let palette = palette.ok_or(InvalidPng("a palette image without PLTE chunk"))?;
        if palette.len() / 3 > 1 << depth {
            return Err(InvalidPng(
                "the palette has more entries than the bit depth allows",
            ));
        }
        let key = match transparency.as_deref() {
            None => None,
            Some(alphas) => {
                // only a single transparent entry fits a color key mask
                let mut transparent = alphas
                    .iter()
                    .enumerate()
                    .filter(|(_, &alpha)| alpha != 255);
                match (transparent.next(), transparent.next()) {
                    (None, _) => None,
                    (Some((index, 0)), None) => Some(vec![index as u32, index as u32]),
                    _ => {
                        return Err(UnsupportedPng(
                            "partially transparent palette entries need decoded pixels",
                        ))
                    }
                }
            }
        };
        (Some(palette), key)
    } else {
        let key = match transparency.as_deref() {
            None => None,
            Some(t) => {
                if t.len() != 2 * components {
                    return Err(InvalidPng("invalid tRNS chunk"));
                }
                let max = (1u32 << depth) - 1;
                let mut key = Vec::new();
                for sample in t.chunks_exact(2) {
                    let value = u32::from(u16::from_be_bytes([sample[0], sample[1]]));
                    if value > max {
                        return Err(InvalidPng("invalid tRNS chunk"));
                    }
                    key.extend_from_slice(&[value, value]);
                }
                Some(key)
            }
        };
        (None, key)
    };

    // inflate once to check that the data has the size the header promises
    let row = (u64::from(width) * components as u64 * u64::from(depth) + 7) / 8 + 1;
    let expected = row * u64::from(height);
    let mut decoder = flate2::read::ZlibDecoder::new(idat.as_slice()).take(expected + 1);
    let actual = std::io::copy(&mut decoder, &mut std::io::sink())
        .map_err(|_| InvalidPng("the pixel data is not valid zlib data"))?;
    if actual != expected {
        return Err(InvalidPng(
            "the pixel data does not have the size of the image",
        ));
    }

    Ok(PngInfo {
        width,
        height,
        bit_depth: depth,
        color_space,
        palette,
        color_key,
        idat,
    })
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

    /// A PNG file with the given header fields, extra chunks (before the pixel data) and
    /// `rows` filtered rows (each row without the filter type byte). The checksums are zero.
    fn png(
        width: u32,
        height: u32,
        depth: u8,
        color_type: u8,
        interlace: u8,
        extra: &[(&[u8; 4], Vec<u8>)],
        row: &[u8],
    ) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out.extend_from_slice(&[0; 4]);
        }
        let mut out = PNG_SIGNATURE.to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[depth, color_type, 0, 0, interlace]);
        chunk(&mut out, b"IHDR", &header);
        for (kind, body) in extra {
            chunk(&mut out, kind, body);
        }
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        let zlib = encoder.finish().unwrap();
        // split the pixel data over two chunks
        let (a, b) = zlib.split_at(zlib.len() / 2);
        chunk(&mut out, b"IDAT", a);
        chunk(&mut out, b"IDAT", b);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn png_pixel_data_is_kept() {
        let file = png(3, 2, 8, 2, 0, &[], &[0; 9]);
        let image = Image::from_png(&file).unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
        assert_eq!(image.color_space(), ColorSpace::DeviceRGB);
        assert!(!image.has_alpha());
        assert_eq!(image.bits_per_component, 8);
        let ImageData::Png(idat) = &image.data else {
            panic!("not a PNG")
        };
        // the two IDAT chunks are joined into one zlib stream
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(idat.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), 2 * (1 + 9));
    }

    #[test]
    fn png_color_types() {
        for (depth, color_type, row, space) in [
            (1, 0, vec![0], ColorSpace::DeviceGray),
            (16, 0, vec![0; 6], ColorSpace::DeviceGray),
            (16, 2, vec![0; 18], ColorSpace::DeviceRGB),
        ] {
            let image = Image::from_png(&png(3, 1, depth, color_type, 0, &[], &row)).unwrap();
            assert_eq!(image.color_space(), space);
            assert_eq!(image.bits_per_component, u32::from(depth));
            assert!(image.palette.is_none());
        }
        let palette = (b"PLTE", vec![255, 0, 0, 0, 255, 0, 0, 0, 255]);
        let image = Image::from_png(&png(3, 1, 2, 3, 0, &[palette], &[0])).unwrap();
        assert_eq!(image.palette.as_deref(), Some(&[255, 0, 0, 0, 255, 0, 0, 0, 255][..]));
        assert_eq!(image.bits_per_component, 2);
    }

    #[test]
    fn png_transparency() {
        let key = (b"tRNS", vec![0, 7]);
        let image = Image::from_png(&png(2, 1, 8, 0, 0, &[key], &[0; 2])).unwrap();
        assert_eq!(image.color_key, Some(vec![7, 7]));
        let key = (b"tRNS", vec![0, 1, 0, 2, 0, 3]);
        let image = Image::from_png(&png(1, 1, 8, 2, 0, &[key], &[0; 3])).unwrap();
        assert_eq!(image.color_key, Some(vec![1, 1, 2, 2, 3, 3]));
        // a color that cannot occur
        let key = (b"tRNS", vec![1, 0]);
        assert!(matches!(
            Image::from_png(&png(1, 1, 8, 0, 0, &[key], &[0])),
            Err(ImageError::InvalidPng(_))
        ));

        let palette = (b"PLTE", vec![0; 9]);
        let key = (b"tRNS", vec![255, 0]);
        let image = Image::from_png(&png(1, 1, 8, 3, 0, &[palette.clone(), key], &[0])).unwrap();
        assert_eq!(image.color_key, Some(vec![1, 1]));
        // opaque entries are listed as 255
        let key = (b"tRNS", vec![255, 255]);
        let image = Image::from_png(&png(1, 1, 8, 3, 0, &[palette.clone(), key], &[0])).unwrap();
        assert_eq!(image.color_key, None);
        for alphas in [vec![0, 0], vec![128]] {
            let key = (b"tRNS", alphas);
            assert!(matches!(
                Image::from_png(&png(1, 1, 8, 3, 0, &[palette.clone(), key], &[0])),
                Err(ImageError::UnsupportedPng(_))
            ));
        }
    }

    #[test]
    fn png_files_that_need_decoding_are_refused() {
        assert!(matches!(
            Image::from_png(&png(1, 1, 8, 6, 0, &[], &[0; 4])),
            Err(ImageError::UnsupportedPng(_))
        ));
        assert!(matches!(
            Image::from_png(&png(1, 1, 8, 4, 0, &[], &[0; 2])),
            Err(ImageError::UnsupportedPng(_))
        ));
        assert!(matches!(
            Image::from_png(&png(1, 1, 8, 2, 1, &[], &[0; 3])),
            Err(ImageError::UnsupportedPng(_))
        ));
    }

    #[test]
    fn invalid_png_files_are_rejected() {
        let valid = png(2, 2, 8, 2, 0, &[], &[0; 6]);
        assert!(Image::from_png(&valid).is_ok());
        assert!(matches!(Image::from_png(b"\xFF\xD8"), Err(ImageError::InvalidPng(_))));
        // truncated in a chunk and before IEND
        for cut in [valid.len() - 1, valid.len() - 12, 20] {
            assert!(matches!(
                Image::from_png(&valid[..cut]),
                Err(ImageError::InvalidPng(_))
            ), "{}", cut);
        }
        // a header that promises more pixels than there are
        assert!(matches!(
            Image::from_png(&png(2, 2, 8, 2, 0, &[], &[0; 6]).iter().enumerate().map(|(i, &b)| if i == 19 { 3 } else { b }).collect::<Vec<_>>()),
            Err(ImageError::InvalidPng(_))
        ));
        // a 16 bit palette, a palette image without palette, a palette that is too large
        assert!(matches!(Image::from_png(&png(1, 1, 16, 3, 0, &[], &[0; 2])), Err(ImageError::InvalidPng(_))));
        assert!(matches!(Image::from_png(&png(1, 1, 8, 3, 0, &[], &[0])), Err(ImageError::InvalidPng(_))));
        let palette = (b"PLTE", vec![0; 9]);
        assert!(matches!(Image::from_png(&png(1, 1, 1, 3, 0, &[palette], &[0])), Err(ImageError::InvalidPng(_))));
        assert_eq!(Image::from_png(&png(0, 1, 8, 0, 0, &[], &[])), Err(ImageError::Empty));
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
