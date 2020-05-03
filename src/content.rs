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

use std::collections::HashMap;
use std::io::{Result, Write};

use byteorder::{BigEndian, WriteBytesExt};
use lemon_pdf_derive::PdfFormat;

use crate::array::Array;
use crate::dictionary::Dictionary;
use crate::font::Font;
use crate::object::{Formatter, IndirectReference, Object, PdfFormat};
use crate::pagetree::{Page, ResourceDictionary};
use crate::stream::StreamEncoder;
use crate::DocumentContext;

#[derive(Debug, Default, Copy, Clone, PartialEq, PdfFormat)]
pub struct Pt(pub f64);

#[derive(Debug)]
pub struct PageContext<'page, 'context, 'context_borrow> {
    pub(crate) fonts: HashMap<String, IndirectReference<Font>>,
    pub(crate) content_stream: StreamEncoder,
    pub(crate) page: &'page mut Page,
    pub pdf_context: &'context_borrow mut DocumentContext<'context>,
}

impl<'page, 'context, 'context_borrow> PageContext<'page, 'context, 'context_borrow> {
    pub fn add_font(&mut self, font: IndirectReference<Font>) -> String {
        let num_fonts = self.fonts.len();
        let key = format!("TT{}", num_fonts);
        self.fonts.insert(key.clone(), font);
        key
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

    pub fn device_rgb_fill_color(&mut self, red: f64, green: f64, blue: f64) -> Result<()> {
        self.write_operation3(red, green, blue, "rg")
    }

    pub fn device_rgb_stroke_color(&mut self, red: f64, green: f64, blue: f64) -> Result<()> {
        self.write_operation3(red, green, blue, "RG")
    }

    pub fn move_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "m")
    }

    pub fn line_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "l")
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

    pub fn fill_path(&mut self) -> Result<()> {
        self.apply_operator("f")
    }

    pub fn begin_text(&mut self) -> Result<()> {
        self.apply_operator("BT")
    }

    pub fn end_text(&mut self) -> Result<()> {
        self.apply_operator("ET")
    }

    pub fn set_font(&mut self, font_key: &str, size: Pt) -> Result<()> {
        self.write_operation2(font_key, size, "Tf")
    }

    pub fn set_position(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operation2(x, y, "Td")
    }

    pub fn draw_simple_glyphs(&mut self, characters: &[u8]) -> Result<()> {
        self.write_operation1(characters, "Tj")
    }

    pub fn draw_cid_glyphs(&mut self, glyphs: impl IntoIterator<Item = u16>) -> Result<()> {
        write!(self.content_stream, "(")?;
        // TODO: escaping
        for glyph in glyphs {
            self.content_stream.write_u16::<BigEndian>(glyph)?;
        }
        write!(self.content_stream, ") Tj ")?;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<()> {
        let mut font_dict = HashMap::new();
        for (key, fontref) in self.fonts {
            font_dict.insert(key, fontref);
        }
        let resources = ResourceDictionary { font: font_dict };
        self.page.resources = Some(resources);

        let content_stream = self.content_stream.into_stream();
        let content_stream_ref = self.pdf_context.write_object(content_stream)?;
        let mut array = Vec::new();
        array.push(content_stream_ref);
        self.page.contents = array;

        Ok(())
    }
}
