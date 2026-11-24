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

use lemon_pdf_derive::{PdfDictionary, PdfFormat};
use crate as lemon_pdf;

use crate::graphics_state::{FillRule, LineCap, LineJoin, RenderingIntent};
use crate::object::Precise;
use crate::object::{write_literal_string, Formatter, IndirectReference, Name, PdfFormat, WriteEscaped};
use crate::form::{FormDict, FormXObject, TransparencyGroup};
use crate::pagetree::Page;
use crate::pattern::{TilingPattern, TilingPatternDict};
use crate::resources::{ColorSpaceKey, ExtGStateKey, FontKey, PatternKey, Resources, ShadingKey, XObjectKey};
use crate::stream::StreamFilter;
use crate::stream::{Stream, StreamEncoder};
use crate::DocumentContext;

#[derive(Debug, Default, Copy, Clone, PartialEq, PdfFormat)]
pub struct Pt(pub f64);

/// A transformation matrix `[a b c d e f]` (ISO 32000-1, 8.3.3), as the operands of the `cm`
/// operator: a point (`x`, `y`) is mapped to (`a*x + c*y + e`, `b*x + d*y + f`).
#[derive(Debug, Copy, Clone, PartialEq, PdfFormat)]
pub struct Matrix(pub [f64; 6]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix([a, b, c, d, e, f])
    }

    pub fn translation(x: Pt, y: Pt) -> Self {
        Matrix([1.0, 0.0, 0.0, 1.0, x.0, y.0])
    }

    pub fn scale(x: f64, y: f64) -> Self {
        Matrix([x, 0.0, 0.0, y, 0.0, 0.0])
    }
}

/// The dictionary of a content stream, which has no entries besides `Length` and `Filter`.
#[derive(Debug, Default, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct ContentStreamDict {}

/// A content stream (ISO 32000-1, 7.8.2): the operators that paint a page.
pub type ContentStream = Stream<ContentStreamDict>;

/// The operands of the colour operators (ISO 32000-1, 8.6.8): the colour that fills or strokes
/// use, in the space that its variant says. The components have six decimals, because two
/// would be steps of more than 8 bits.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ColorOperand<'a> {
    /// A device gray (`g` and `G` operators).
    Gray(f64),
    /// A device RGB colour (`rg` and `RG`).
    Rgb(f64, f64, f64),
    /// A device CMYK colour (`k` and `K`).
    Cmyk(f64, f64, f64, f64),
    /// The components of a colour in a space of the resources (`cs` and `sc`, or `CS` and `SC`):
    /// three for Lab, CalRGB and an ICC profile of RGB.
    Space(&'a ColorSpaceKey, &'a [f64]),
    /// A pattern of the resources (`/Pattern cs` and `scn`, or `/Pattern CS` and `SCN`).
    Pattern(&'a PatternKey),
}

/// Writes the operators of a content stream (ISO 32000-1, 8.2 to 9.4), one method for each, in
/// the order that the caller calls them. It does not check that they make a valid content stream:
/// that graphics states are nested, text is between `begin_text` and `end_text`, and so on.
///
/// Methods that name a resource take its key from [`Resources`].
#[derive(Debug)]
pub struct ContentWriter {
    stream: StreamEncoder,
}

impl ContentWriter {
    pub(crate) fn new(filter: Option<StreamFilter>) -> Self {
        ContentWriter { stream: StreamEncoder::new(filter) }
    }

    pub(crate) fn into_encoder(self) -> StreamEncoder {
        self.stream
    }

    /// Writes an operator with its operands, for the operators that have no method.
    pub fn write_operator(&mut self, operator: &str, operands: &[&dyn PdfFormat]) -> Result<()> {
        for operand in operands {
            operand.write(&mut Formatter { writer: &mut self.stream })?;
            write!(self.stream, " ")?;
        }
        write!(self.stream, "{} ", operator)
    }

    // graphics state (8.4)

    /// `q`
    pub fn save_graphics_state(&mut self) -> Result<()> {
        self.write_operator("q", &[])
    }

    /// `Q`
    pub fn restore_graphics_state(&mut self) -> Result<()> {
        self.write_operator("Q", &[])
    }

    /// `cm`: concatenates `matrix` to the current transformation matrix.
    pub fn concatenate_matrix(&mut self, matrix: Matrix) -> Result<()> {
        let [a, b, c, d, e, f] = matrix.0;
        self.write_operator("cm", &[&a, &b, &c, &d, &e, &f])
    }

    /// `w`
    pub fn set_line_width(&mut self, width: Pt) -> Result<()> {
        self.write_operator("w", &[&width])
    }

    /// `J`
    pub fn set_line_cap(&mut self, cap: LineCap) -> Result<()> {
        self.write_operator("J", &[&(cap as i64)])
    }

    /// `j`
    pub fn set_line_join(&mut self, join: LineJoin) -> Result<()> {
        self.write_operator("j", &[&(join as i64)])
    }

    /// `M`: limits the length of miter joins, as a ratio of the miter length to the line width.
    pub fn set_miter_limit(&mut self, limit: f64) -> Result<()> {
        self.write_operator("M", &[&limit])
    }

    /// `d`: alternating lengths of dashes and gaps, starting `phase` into the pattern. An empty
    /// `dashes` slice draws solid lines.
    pub fn set_dash_pattern(&mut self, dashes: &[Pt], phase: Pt) -> Result<()> {
        self.write_operator("d", &[&dashes, &phase])
    }

    /// `ri`: tells a reader how to map the colours that the device cannot show.
    pub fn set_rendering_intent(&mut self, intent: RenderingIntent) -> Result<()> {
        self.write_operator("ri", &[&intent])
    }

    /// `gs`: applies a graphics state parameter dictionary.
    pub fn set_ext_gstate(&mut self, key: &ExtGStateKey) -> Result<()> {
        self.write_operator("gs", &[key])
    }

    // paths (8.5)

    /// `m`
    pub fn move_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operator("m", &[&x, &y])
    }

    /// `l`
    pub fn line_to(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operator("l", &[&x, &y])
    }

    /// `c`: appends a cubic Bézier curve from the current point to (`x3`, `y3`) with the control
    /// points (`x1`, `y1`) and (`x2`, `y2`).
    pub fn curve_to(&mut self, x1: Pt, y1: Pt, x2: Pt, y2: Pt, x3: Pt, y3: Pt) -> Result<()> {
        self.write_operator("c", &[&x1, &y1, &x2, &y2, &x3, &y3])
    }

    /// `re`
    pub fn rect(&mut self, x: Pt, y: Pt, width: Pt, height: Pt) -> Result<()> {
        self.write_operator("re", &[&x, &y, &width, &height])
    }

    /// `h`
    pub fn close_path(&mut self) -> Result<()> {
        self.write_operator("h", &[])
    }

    /// `S`
    pub fn stroke_path(&mut self) -> Result<()> {
        self.write_operator("S", &[])
    }

    /// `s`
    pub fn close_and_stroke_path(&mut self) -> Result<()> {
        self.write_operator("s", &[])
    }

    /// `f`: fills the path with the nonzero winding number rule.
    pub fn fill_path(&mut self) -> Result<()> {
        self.write_operator("f", &[])
    }

    /// `f` or `f*`: fills the path with the given fill rule.
    pub fn fill_path_with_rule(&mut self, rule: FillRule) -> Result<()> {
        self.write_operator(if rule == FillRule::EvenOdd { "f*" } else { "f" }, &[])
    }

    /// `B` or `B*`: fills, then strokes the path.
    pub fn fill_and_stroke_path(&mut self, rule: FillRule) -> Result<()> {
        self.write_operator(if rule == FillRule::EvenOdd { "B*" } else { "B" }, &[])
    }

    /// `b` or `b*`: closes, fills, then strokes the path.
    pub fn close_fill_and_stroke_path(&mut self, rule: FillRule) -> Result<()> {
        self.write_operator(if rule == FillRule::EvenOdd { "b*" } else { "b" }, &[])
    }

    /// `n`: ends the path without painting it, for example after `clip`.
    pub fn end_path(&mut self) -> Result<()> {
        self.write_operator("n", &[])
    }

    /// `W` or `W*`: intersects the clipping path with the current path. The clipping path takes
    /// effect after the next path painting operator (usually `end_path`) and lasts until the
    /// graphics state is restored.
    pub fn clip(&mut self, rule: FillRule) -> Result<()> {
        self.write_operator(if rule == FillRule::EvenOdd { "W*" } else { "W" }, &[])
    }

    // colour (8.6)

    /// Makes fills use `color`.
    pub fn set_fill_color(&mut self, color: ColorOperand<'_>) -> Result<()> {
        self.set_color(color, false)
    }

    /// Makes strokes use `color`.
    pub fn set_stroke_color(&mut self, color: ColorOperand<'_>) -> Result<()> {
        self.set_color(color, true)
    }

    fn set_color(&mut self, color: ColorOperand<'_>, stroke: bool) -> Result<()> {
        let pick = |fill: &'static str, stroke_operator: &'static str| if stroke { stroke_operator } else { fill };
        match color {
            ColorOperand::Gray(gray) => self.write_operator(pick("g", "G"), &[&Precise(gray)]),
            ColorOperand::Rgb(r, g, b) => self.write_operator(pick("rg", "RG"), &[&Precise(r), &Precise(g), &Precise(b)]),
            ColorOperand::Cmyk(c, m, y, k) => {
                self.write_operator(pick("k", "K"), &[&Precise(c), &Precise(m), &Precise(y), &Precise(k)])
            }
            ColorOperand::Space(space, components) => {
                self.write_operator(pick("cs", "CS"), &[space])?;
                let components: Vec<Precise> = components.iter().map(|value| Precise(*value)).collect();
                let operands: Vec<&dyn PdfFormat> = components.iter().map(|value| value as &dyn PdfFormat).collect();
                self.write_operator(pick("sc", "SC"), &operands)
            }
            ColorOperand::Pattern(pattern) => {
                self.write_operator(pick("cs", "CS"), &[&Name::from("Pattern")])?;
                self.write_operator(pick("scn", "SCN"), &[pattern])
            }
        }
    }

    // painting objects (8.7 to 8.10)

    /// `sh`: paints a shading everywhere inside of the current clipping path, in the current
    /// user space. Clip to the shape that is to be painted first (see `clip`), inside of a saved
    /// graphics state.
    pub fn paint_shading(&mut self, key: &ShadingKey) -> Result<()> {
        self.write_operator("sh", &[key])
    }

    /// `Do`: paints an XObject in the current user space.
    pub fn paint_xobject(&mut self, key: &XObjectKey) -> Result<()> {
        self.write_operator("Do", &[key])
    }

    // text (9)

    /// `BT`
    pub fn begin_text(&mut self) -> Result<()> {
        self.write_operator("BT", &[])
    }

    /// `ET`
    pub fn end_text(&mut self) -> Result<()> {
        self.write_operator("ET", &[])
    }

    /// `Tf`
    pub fn set_font(&mut self, font: &FontKey, size: Pt) -> Result<()> {
        self.write_operator("Tf", &[font, &size])
    }

    /// `Td`: moves to the start of the next line, offset from the start of the current one.
    pub fn move_text_position(&mut self, x: Pt, y: Pt) -> Result<()> {
        self.write_operator("Td", &[&x, &y])
    }

    /// `Tm`: sets the text matrix and the text line matrix.
    pub fn set_text_matrix(&mut self, matrix: Matrix) -> Result<()> {
        let [a, b, c, d, e, f] = matrix.0;
        self.write_operator("Tm", &[&a, &b, &c, &d, &e, &f])
    }

    /// `Tr`: 0 fills, 1 strokes, 2 fills and strokes, 3 neither (ISO 32000-1, 9.3.6).
    pub fn set_text_rendering_mode(&mut self, mode: i64) -> Result<()> {
        self.write_operator("Tr", &[&mode])
    }

    /// `Tw`
    pub fn set_word_spacing(&mut self, spacing: Pt) -> Result<()> {
        self.write_operator("Tw", &[&spacing])
    }

    /// `Ts`
    pub fn set_text_rise(&mut self, rise: Pt) -> Result<()> {
        self.write_operator("Ts", &[&rise])
    }

    /// `Tj`: shows glyphs of a simple font, one byte per glyph.
    pub fn show_simple_glyphs(&mut self, characters: &[u8]) -> Result<()> {
        write_literal_string(&mut self.stream, characters)?;
        write!(self.stream, " Tj ")
    }

    /// `Tj`: shows glyphs of a composite font, two bytes per glyph.
    pub fn show_cid_glyphs(&mut self, glyphs: impl IntoIterator<Item = u16>) -> Result<()> {
        write!(self.stream, "<")?;
        for glyph in glyphs {
            self.stream.write_hex_escaped(&glyph.to_be_bytes())?;
        }
        write!(self.stream, "> Tj ")
    }

    /// `TJ`: shows an array of strings and the adjustments between them.
    pub fn show_text_array(&mut self, array: &dyn PdfFormat) -> Result<()> {
        self.write_operator("TJ", &[array])
    }

    // marked content (14.6)

    /// `BMC`
    pub fn begin_marked_content(&mut self, tag: &Name) -> Result<()> {
        self.write_operator("BMC", &[tag])
    }

    /// `BDC`: `properties` is a property list (a dictionary).
    pub fn begin_marked_content_with_properties(&mut self, tag: &Name, properties: &dyn PdfFormat) -> Result<()> {
        self.write_operator("BDC", &[tag, properties])
    }

    /// `EMC`
    pub fn end_marked_content(&mut self) -> Result<()> {
        self.write_operator("EMC", &[])
    }
}

/// What a page, a form or a pattern is drawn with: the operators of its content stream and the
/// resources they use, and the document that the resources are written to.
#[derive(Debug)]
pub struct PageContext<'a, 'doc> {
    pub content: ContentWriter,
    pub resources: Resources,
    pub pdf_context: &'a mut DocumentContext<'doc>,
}

impl<'a, 'doc> PageContext<'a, 'doc> {
    pub(crate) fn new(pdf_context: &'a mut DocumentContext<'doc>, filter: Option<StreamFilter>) -> Self {
        PageContext { content: ContentWriter::new(filter), resources: Resources::new(), pdf_context }
    }

    /// Writes the content of the page: its content stream, and its resources.
    pub(crate) fn finish(self, page: &mut Page) -> Result<()> {
        let PageContext { content, resources, pdf_context } = self;
        page.resources = Some(resources.into_dictionary());
        let content_stream = Stream::from_encoder(ContentStreamDict {}, content.into_encoder())?;
        let content_stream_ref = pdf_context.write_object(content_stream)?;
        page.contents = vec![content_stream_ref];
        Ok(())
    }
}

impl<'doc> DocumentContext<'doc> {
    /// Writes a tiling pattern (ISO 32000-1, 8.7.3) and returns a reference to it.
    ///
    /// `content` draws one cell like a page, in pattern space; everything outside of `bbox`
    /// (`[xmin, ymin, xmax, ymax]`) is clipped. The cell is repeated every `x_step` and `y_step`
    /// in pattern space. `matrix` maps pattern space to the default coordinate system of the
    /// page, which does not depend on the transformations applied while the page is drawn.
    pub fn write_tiling_pattern(
        &mut self,
        bbox: [Pt; 4],
        x_step: Pt,
        y_step: Pt,
        matrix: Matrix,
        stream_filter: Option<StreamFilter>,
        content: impl FnOnce(&mut PageContext<'_, 'doc>) -> Result<()>,
    ) -> Result<IndirectReference<TilingPattern>> {
        let mut context = PageContext::new(self, stream_filter);
        content(&mut context)?;
        let PageContext { content, resources, .. } = context;
        let dict = TilingPatternDict {
            bbox,
            x_step,
            y_step,
            resources: resources.into_dictionary(),
            matrix: Some(matrix),
        };
        self.write_object(Stream::from_encoder(dict, content.into_encoder())?)
    }

    /// Writes a form XObject (ISO 32000-1, 8.10) and returns a reference to it.
    ///
    /// `content` draws the form like a page, in form space; everything outside of `bbox`
    /// (`[xmin, ymin, xmax, ymax]`) is clipped. The form can be painted with
    /// `Resources::add_xobject` and `ContentWriter::paint_xobject`, or be the group of a
    /// `SoftMask`. A form that is a soft mask group needs a transparency group.
    pub fn write_form(
        &mut self,
        bbox: [Pt; 4],
        group: Option<TransparencyGroup>,
        stream_filter: Option<StreamFilter>,
        content: impl FnOnce(&mut PageContext<'_, 'doc>) -> Result<()>,
    ) -> Result<IndirectReference<FormXObject>> {
        let mut context = PageContext::new(self, stream_filter);
        content(&mut context)?;
        let PageContext { content, resources, .. } = context;
        let dict = FormDict {
            bbox,
            matrix: None,
            resources: resources.into_dictionary(),
            group,
        };
        self.write_object(Stream::from_encoder(dict, content.into_encoder())?)
    }
}

impl PageContext<'_, '_> {
    /// Adds `state` to the resources and applies it (`gs` operator).
    pub fn apply_ext_gstate(&mut self, state: crate::graphics_state::ExtGState) -> Result<()> {
        let key = self.resources.add_ext_gstate(state);
        self.content.set_ext_gstate(&key)
    }
}
