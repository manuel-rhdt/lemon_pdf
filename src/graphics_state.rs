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

//! Graphics state parameters that can only be set through a graphics state parameter
//! dictionary (ISO 32000-1, 8.4.5), most importantly transparency (ISO 32000-1, 11).

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

/// A graphics state parameter dictionary (`/ExtGState`).
///
/// Only the entries that are `Some` are written; applying the dictionary with the `gs`
/// operator (see `PageContext::set_ext_gstate`) leaves all other parameters unchanged.
#[derive(Debug, Clone, Default, PartialEq, PdfFormat)]
#[rename("ExtGState")]
pub struct ExtGState {
    /// The constant opacity for stroking operations, from 0 (transparent) to 1 (opaque).
    #[rename("CA")]
    #[skip_if("Option::is_none")]
    pub stroke_alpha: Option<f64>,
    /// The constant opacity for all other painting operations (fills, text, images), from 0
    /// (transparent) to 1 (opaque).
    #[rename("ca")]
    #[skip_if("Option::is_none")]
    pub fill_alpha: Option<f64>,
    /// The blend mode used to composite painted objects with the backdrop.
    #[rename("BM")]
    #[skip_if("Option::is_none")]
    pub blend_mode: Option<BlendMode>,
}

impl ExtGState {
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the opacity of fills, text and images. The value is clamped to `0..=1`.
    pub fn with_fill_alpha(mut self, alpha: f64) -> Self {
        self.fill_alpha = Some(clamp_alpha(alpha));
        self
    }

    /// Sets the opacity of strokes. The value is clamped to `0..=1`.
    pub fn with_stroke_alpha(mut self, alpha: f64) -> Self {
        self.stroke_alpha = Some(clamp_alpha(alpha));
        self
    }

    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = Some(blend_mode);
        self
    }
}

fn clamp_alpha(alpha: f64) -> f64 {
    if alpha.is_nan() {
        1.0
    } else {
        alpha.max(0.0).min(1.0)
    }
}

/// The blend modes of ISO 32000-1, 11.3.5.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PdfFormat)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// The shape at the ends of open stroked subpaths (`J` operator).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LineCap {
    Butt = 0,
    Round = 1,
    ProjectingSquare = 2,
}

/// The shape at the corners of stroked paths (`j` operator).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LineJoin {
    Miter = 0,
    Round = 1,
    Bevel = 2,
}

/// The rule that decides which points are inside a path, for filling and clipping.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Formatter, PdfFormat};

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
    fn only_set_entries_are_written() {
        assert_eq!(written(&ExtGState::new()), "<< /Type /ExtGState\n>>");
        let state = ExtGState::new()
            .with_fill_alpha(0.5)
            .with_stroke_alpha(0.25)
            .with_blend_mode(BlendMode::Multiply);
        let output = written(&state);
        assert!(output.contains("/ca 0.5\n"), "{}", output);
        assert!(output.contains("/CA 0.25\n"), "{}", output);
        assert!(output.contains("/BM /Multiply\n"), "{}", output);
    }

    #[test]
    fn alpha_is_clamped() {
        assert_eq!(ExtGState::new().with_fill_alpha(1.5).fill_alpha, Some(1.0));
        assert_eq!(ExtGState::new().with_fill_alpha(-1.0).fill_alpha, Some(0.0));
        assert_eq!(
            ExtGState::new().with_stroke_alpha(f64::NAN).stroke_alpha,
            Some(1.0)
        );
    }
}
