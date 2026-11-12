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

use std::collections::{BTreeMap, HashMap};
use std::io::{Result, Write};

use lemon_pdf_derive::{PdfDictionary, PdfFormat};
use crate as lemon_pdf;

use crate::font::Font;
use crate::graphics_state::{BlendMode, ExtGState, FillRule, LineCap, LineJoin};
use crate::image::XObject;
use crate::object::{
    write_literal_string, Formatter, IndirectReference, Name, PdfFormat, WriteEscaped,
};
use crate::pagetree::{Page, ResourceDictionary};
use crate::shading::{Shading, ShadingPattern};
use crate::stream::{Stream, StreamEncoder};
use crate::DocumentContext;

#[derive(Debug, Default, Copy, Clone, PartialEq, PdfFormat)]
pub struct Pt(pub f64);

/// The dictionary of a content stream, which has no entries besides `Length` and `Filter`.
#[derive(Debug, Default, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct ContentStreamDict {}

/// A content stream (ISO 32000-1, 7.8.2): the operators that paint a page.
pub type ContentStream = Stream<ContentStreamDict>;

#[derive(Debug)]
pub struct PageContext<'page, 'context, 'context_borrow> {
    pub(crate) fonts: HashMap<IndirectReference<Font>, Name>,
    pub(crate) x_objects: HashMap<IndirectReference<XObject>, Name>,
    /// Graphics state parameter dictionaries with their resource names. A `Vec` because
    /// `ExtGState` holds floats and cannot be hashed; pages only use a handful of them.
    pub(crate) ext_g_states: Vec<(ExtGState, Name)>,
    /// Shadings and shading patterns with their resource names, a `Vec` for the same reason.
    pub(crate) shadings: Vec<(Shading, Name)>,
    pub(crate) patterns: Vec<(ShadingPattern, Name)>,
    pub(crate) content_stream: StreamEncoder,
    pub(crate) page: &'page mut Page,
    pub pdf_context: &'context_borrow mut DocumentContext<'context>,
}

/// Turns a map from objects to their resource names into a resource dictionary.
fn by_name<T>(map: HashMap<IndirectReference<T>, Name>) -> BTreeMap<Name, IndirectReference<T>> {
    map.into_iter().map(|(reference, key)| (key, reference)).collect()
}

impl<'page, 'context, 'context_borrow> PageContext<'page, 'context, 'context_borrow> {
    /// Adds a font to the resources of the page and returns its resource name.
    pub fn add_font(&mut self, font: IndirectReference<Font>) -> Name {
        let num_fonts = self.fonts.len();
        self.fonts
            .entry(font)
            .or_insert_with(|| Name::from(format!("F{}", num_fonts)))
            .clone()
    }

    /// Adds an XObject (for example an image from `DocumentContext::write_image`) to the
    /// resources of the page and returns its resource name.
    pub fn add_xobject(&mut self, x_object: impl Into<IndirectReference<XObject>>) -> Name {
        let num_x_objects = self.x_objects.len();
        self.x_objects
            .entry(x_object.into())
            .or_insert_with(|| Name::from(format!("X{}", num_x_objects)))
            .clone()
    }

    /// Adds a graphics state parameter dictionary to the resources of the page and returns its
    /// resource name. Adding an equal dictionary again returns the same name.
    pub fn add_ext_gstate(&mut self, state: ExtGState) -> Name {
        if let Some((_, key)) = self.ext_g_states.iter().find(|(other, _)| *other == state) {
            return key.clone();
        }
        let key = Name::from(format!("GS{}", self.ext_g_states.len()));
        self.ext_g_states.push((state, key.clone()));
        key
    }

    /// Applies a graphics state parameter dictionary from `add_ext_gstate` (`gs` operator).
    pub fn set_ext_gstate(&mut self, key: &Name) -> Result<()> {
        self.write_operation1(key, "gs")
    }

    /// Adds `state` to the resources of the page and applies it.
    pub fn apply_ext_gstate(&mut self, state: ExtGState) -> Result<()> {
        let key = self.add_ext_gstate(state);
        self.set_ext_gstate(&key)
    }

    /// Sets the opacity (0 to 1) of fills, text and images.
    pub fn set_fill_alpha(&mut self, alpha: f64) -> Result<()> {
        self.apply_ext_gstate(ExtGState::new().with_fill_alpha(alpha))
    }

    /// Sets the opacity (0 to 1) of strokes.
    pub fn set_stroke_alpha(&mut self, alpha: f64) -> Result<()> {
        self.apply_ext_gstate(ExtGState::new().with_stroke_alpha(alpha))
    }

    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) -> Result<()> {
        self.apply_ext_gstate(ExtGState::new().with_blend_mode(blend_mode))
    }

    /// Adds a shading to the resources of the page and returns its resource name. Adding an
    /// equal shading again returns the same name.
    pub fn add_shading(&mut self, shading: Shading) -> Name {
        if let Some((_, key)) = self.shadings.iter().find(|(other, _)| *other == shading) {
            return key.clone();
        }
        let key = Name::from(format!("Sh{}", self.shadings.len()));
        self.shadings.push((shading, key.clone()));
        key
    }

    /// Paints a shading from `add_shading` (`sh` operator) everywhere inside of the current
    /// clipping path, in the current user space. Clip to the shape that is to be painted first
    /// (see `clip`), inside of a saved graphics state.
    pub fn paint_shading(&mut self, key: &Name) -> Result<()> {
        self.write_operation1(key, "sh")
    }

    /// Adds a shading pattern to the resources of the page and returns its resource name.
    /// Adding an equal pattern again returns the same name.
    pub fn add_shading_pattern(&mut self, pattern: ShadingPattern) -> Name {
        if let Some((_, key)) = self.patterns.iter().find(|(other, _)| *other == pattern) {
            return key.clone();
        }
        let key = Name::from(format!("P{}", self.patterns.len()));
        self.patterns.push((pattern, key.clone()));
        key
    }

    /// Makes fills use a pattern from `add_shading_pattern` as their color
    /// (`/Pattern cs` and `scn` operators).
    pub fn set_fill_pattern(&mut self, key: &Name) -> Result<()> {
        self.write_operation1(Name::from("Pattern"), "cs")?;
        self.write_operation1(key, "scn")
    }

    /// Makes strokes use a pattern from `add_shading_pattern` as their color
    /// (`/Pattern CS` and `SCN` operators).
    pub fn set_stroke_pattern(&mut self, key: &Name) -> Result<()> {
        self.write_operation1(Name::from("Pattern"), "CS")?;
        self.write_operation1(key, "SCN")
    }

    /// Paints an XObject from `add_xobject` (`Do` operator) in the current user space.
    pub fn draw_xobject(&mut self, key: &Name) -> Result<()> {
        self.write_operation1(key, "Do")
    }

    /// Paints an image from `add_xobject` into the rectangle with the lower left corner
    /// (`x`, `y`). The graphics state is left unchanged.
    pub fn draw_image(&mut self, key: &Name, x: Pt, y: Pt, width: Pt, height: Pt) -> Result<()> {
        self.save_graphics_state()?;
        // images are painted into the unit square
        self.concatenate_matrix(width.0, 0.0, 0.0, height.0, x, y)?;
        self.draw_xobject(key)?;
        self.restore_graphics_state()
    }

    pub fn push_operand(&mut self, operand: impl PdfFormat) -> Result<()> {
        let mut formatter = Formatter {
            writer: &mut self.content_stream,
        };
        operand.write(&mut formatter)?;
        write!(self.content_stream, " ")
    }

    pub fn apply_operator(&mut self, operator: &str) -> Result<()> {
        write!(self.content_stream, "{} ", operator)
    }

    pub fn write_operation1(&mut self, arg1: impl PdfFormat, operator: &str) -> Result<()> {
        self.push_operand(arg1)?;
        self.apply_operator(operator)
    }

    pub fn write_operation2(
        &mut self,
        arg1: impl PdfFormat,
        arg2: impl PdfFormat,
        operator: &str,
    ) -> Result<()> {
        self.push_operand(arg1)?;
        self.push_operand(arg2)?;
        self.apply_operator(operator)
    }

    pub fn write_operation3(
        &mut self,
        arg1: impl PdfFormat,
        arg2: impl PdfFormat,
        arg3: impl PdfFormat,
        operator: &str,
    ) -> Result<()> {
        self.push_operand(arg1)?;
        self.push_operand(arg2)?;
        self.push_operand(arg3)?;
        self.apply_operator(operator)
    }

    pub fn write_operation4(
        &mut self,
        arg1: impl PdfFormat,
        arg2: impl PdfFormat,
        arg3: impl PdfFormat,
        arg4: impl PdfFormat,
        operator: &str,
    ) -> Result<()> {
        self.push_operand(arg1)?;
        self.push_operand(arg2)?;
        self.push_operand(arg3)?;
        self.push_operand(arg4)?;
        self.apply_operator(operator)
    }

    pub fn save_graphics_state(&mut self) -> Result<()> {
        self.apply_operator("q")
    }

    pub fn restore_graphics_state(&mut self) -> Result<()> {
        self.apply_operator("Q")
    }

    pub fn concatenate_matrix(
        &mut self,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: Pt,
        f: Pt,
    ) -> Result<()> {
        self.push_operand(a)?;
        self.push_operand(b)?;
        self.push_operand(c)?;
        self.push_operand(d)?;
        self.push_operand(e)?;
        self.push_operand(f)?;
        self.apply_operator("cm")
    }

    pub fn line_width(&mut self, width: Pt) -> Result<()> {
        self.write_operation1(width, "w")
    }

    pub fn line_cap(&mut self, cap: LineCap) -> Result<()> {
        self.write_operation1(cap as i64, "J")
    }

    pub fn line_join(&mut self, join: LineJoin) -> Result<()> {
        self.write_operation1(join as i64, "j")
    }

    /// Limits the length of miter joins, as a ratio of the miter length to the line width.
    pub fn miter_limit(&mut self, limit: f64) -> Result<()> {
        self.write_operation1(limit, "M")
    }

    /// Sets the dash pattern: alternating lengths of dashes and gaps, starting `phase` into
    /// the pattern. An empty `dashes` slice draws solid lines.
    pub fn dash_pattern(&mut self, dashes: &[Pt], phase: Pt) -> Result<()> {
        self.write_operation2(dashes, phase, "d")
    }

    pub fn device_gray_fill_color(&mut self, gray: f64) -> Result<()> {
        self.write_operation1(gray, "g")
    }

    pub fn device_gray_stroke_color(&mut self, gray: f64) -> Result<()> {
        self.write_operation1(gray, "G")
    }

    pub fn device_rgb_fill_color(&mut self, red: f64, green: f64, blue: f64) -> Result<()> {
        self.write_operation3(red, green, blue, "rg")
    }

    pub fn device_rgb_stroke_color(&mut self, red: f64, green: f64, blue: f64) -> Result<()> {
        self.write_operation3(red, green, blue, "RG")
    }

    pub fn device_cmyk_fill_color(&mut self, c: f64, m: f64, y: f64, k: f64) -> Result<()> {
        self.write_operation4(c, m, y, k, "k")
    }

    pub fn device_cmyk_stroke_color(&mut self, c: f64, m: f64, y: f64, k: f64) -> Result<()> {
        self.write_operation4(c, m, y, k, "K")
    }

    pub fn move_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "m")
    }

    pub fn line_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "l")
    }

    /// Appends a cubic Bézier curve from the current point to (`x3`, `y3`) with the control
    /// points (`x1`, `y1`) and (`x2`, `y2`).
    pub fn curve_to(&mut self, x1: Pt, y1: Pt, x2: Pt, y2: Pt, x3: Pt, y3: Pt) -> Result<()> {
        for operand in &[x1, y1, x2, y2, x3, y3] {
            self.push_operand(operand)?;
        }
        self.apply_operator("c")
    }

    pub fn rect(&mut self, x: Pt, y: Pt, width: Pt, height: Pt) -> Result<()> {
        self.write_operation4(x, y, width, height, "re")
    }

    pub fn close_path(&mut self) -> Result<()> {
        self.apply_operator("h")
    }

    pub fn stroke_path(&mut self) -> Result<()> {
        self.apply_operator("S")
    }

    pub fn close_and_stroke_path(&mut self) -> Result<()> {
        self.apply_operator("s")
    }

    /// Fills the path with the nonzero winding number rule.
    pub fn fill_path(&mut self) -> Result<()> {
        self.apply_operator("f")
    }

    /// Fills the path with the given fill rule.
    pub fn fill_path_with_rule(&mut self, rule: FillRule) -> Result<()> {
        match rule {
            FillRule::NonZero => self.apply_operator("f"),
            FillRule::EvenOdd => self.apply_operator("f*"),
        }
    }

    /// Fills, then strokes the path.
    pub fn fill_and_stroke_path(&mut self, rule: FillRule) -> Result<()> {
        match rule {
            FillRule::NonZero => self.apply_operator("B"),
            FillRule::EvenOdd => self.apply_operator("B*"),
        }
    }

    /// Closes, fills, then strokes the path.
    pub fn close_fill_and_stroke_path(&mut self, rule: FillRule) -> Result<()> {
        match rule {
            FillRule::NonZero => self.apply_operator("b"),
            FillRule::EvenOdd => self.apply_operator("b*"),
        }
    }

    /// Ends the path without painting it, for example after `clip`.
    pub fn end_path(&mut self) -> Result<()> {
        self.apply_operator("n")
    }

    /// Intersects the clipping path with the current path. The clipping path takes effect
    /// after the next path painting operator (usually `end_path`) and lasts until the graphics
    /// state is restored.
    pub fn clip(&mut self, rule: FillRule) -> Result<()> {
        match rule {
            FillRule::NonZero => self.apply_operator("W"),
            FillRule::EvenOdd => self.apply_operator("W*"),
        }
    }

    pub fn begin_text(&mut self) -> Result<()> {
        self.apply_operator("BT")
    }

    pub fn end_text(&mut self) -> Result<()> {
        self.apply_operator("ET")
    }

    pub fn set_font(&mut self, font_key: &Name, size: Pt) -> Result<()> {
        self.write_operation2(font_key, size, "Tf")
    }

    pub fn set_position(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "Td")
    }

    /// Shows glyphs of a simple font, one byte per glyph (`Tj` operator).
    pub fn draw_simple_glyphs(&mut self, characters: &[u8]) -> Result<()> {
        write_literal_string(&mut self.content_stream, characters)?;
        write!(self.content_stream, " Tj ")
    }

    pub fn draw_cid_glyphs(&mut self, glyphs: impl IntoIterator<Item = u16>) -> Result<()> {
        write!(self.content_stream, "<")?;
        for glyph in glyphs {
            self.content_stream.write_hex_escaped(&glyph.to_be_bytes())?;
        }
        write!(self.content_stream, "> Tj ")?;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<()> {
        let resources = ResourceDictionary {
            font: by_name(self.fonts),
            x_object: by_name(self.x_objects),
            ext_g_state: self
                .ext_g_states
                .into_iter()
                .map(|(state, key)| (key, state))
                .collect(),
            shading: self
                .shadings
                .into_iter()
                .map(|(shading, key)| (key, shading))
                .collect(),
            pattern: self
                .patterns
                .into_iter()
                .map(|(pattern, key)| (key, pattern))
                .collect(),
        };
        self.page.resources = Some(resources);

        let content_stream = Stream::from_encoder(ContentStreamDict {}, self.content_stream)?;
        let content_stream_ref = self.pdf_context.write_object(content_stream)?;
        self.page.contents = vec![content_stream_ref];
        Ok(())
    }
}
