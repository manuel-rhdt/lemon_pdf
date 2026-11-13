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

//! One page with an example of each supported shading type (2 to 7), written to
//! `shading_gallery.pdf`.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use encoding_rs::WINDOWS_1252;
use lemon_pdf::font::builtin::BuiltInFont;
use lemon_pdf::font::encoding::{EncodingEntry, PredefinedEncoding};
use lemon_pdf::font::Font;
use lemon_pdf::graphics_state::FillRule;
use lemon_pdf::mesh::{CoonsMesh, FreeFormMesh, LatticeMesh, Patch, TensorMesh, Vertex};
use lemon_pdf::object::TextString;
use lemon_pdf::pagetree::Page;
use lemon_pdf::shading::{
    AxialShading, Circle, ColorStop, Gradient, Point, RadialShading, Rgb, Shading,
};
use lemon_pdf::stream::StreamFilter;
use lemon_pdf::{DocumentContext, Pt, Version};

const NAVY: Rgb = Rgb(0.06, 0.11, 0.18);
const ORANGE: Rgb = Rgb(1.0, 0.42, 0.24);
const SAND: Rgb = Rgb(0.98, 0.84, 0.55);
const TEAL: Rgb = Rgb(0.13, 0.62, 0.63);
const SKY: Rgb = Rgb(0.45, 0.72, 0.95);
const PLUM: Rgb = Rgb(0.45, 0.2, 0.55);

const PANEL_WIDTH: f64 = 235.0;
const PANEL_HEIGHT: f64 = 170.0;

/// The lower left corner of the panel in the given column and row (counted from the top).
fn panel_origin(column: usize, row: usize) -> (f64, f64) {
    (50.0 + 260.0 * column as f64, 565.0 - 225.0 * row as f64)
}

/// The boundary of a patch with slightly bulging sides, inside of the panel at (`ox`, `oy`).
fn boundary(ox: f64, oy: f64) -> [Point; 12] {
    let (x0, x1) = (ox + 30.0, ox + PANEL_WIDTH - 30.0);
    let (y0, y1) = (oy + 30.0, oy + PANEL_HEIGHT - 30.0);
    let (dx, dy) = (x1 - x0, y1 - y0);
    [
        Point::new(x0, y0),
        Point::new(x0 - 12.0, y0 + dy / 3.0),
        Point::new(x0 + 12.0, y0 + 2.0 * dy / 3.0),
        Point::new(x0, y1),
        Point::new(x0 + dx / 3.0, y1 + 18.0),
        Point::new(x0 + 2.0 * dx / 3.0, y1 - 18.0),
        Point::new(x1, y1),
        Point::new(x1 + 12.0, y1 - dy / 3.0),
        Point::new(x1 - 12.0, y1 - 2.0 * dy / 3.0),
        Point::new(x1, y0),
        Point::new(x1 - dx / 3.0, y0 - 18.0),
        Point::new(x1 - 2.0 * dx / 3.0, y0 + 18.0),
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut file = BufWriter::new(File::create("shading_gallery.pdf")?);
    let mut context = DocumentContext::with_writer(&mut file, Version::Pdf1_7)?;

    // type 2: an axial shading with four stops along a diagonal
    let (ox, oy) = panel_origin(0, 0);
    let axial = AxialShading::new(
        Point::new(ox, oy),
        Point::new(ox + PANEL_WIDTH, oy + PANEL_HEIGHT),
        &Gradient::new(&[
            ColorStop::new(0.0, NAVY),
            ColorStop::new(0.4, TEAL),
            ColorStop::new(0.7, SAND),
            ColorStop::new(1.0, ORANGE),
        ])?
        .extended(),
    );

    // type 3: a radial shading between two circles, which makes a cone
    let (ox, oy) = panel_origin(1, 0);
    let radial = RadialShading::new(
        Circle {
            x: Pt(ox + 70.0),
            y: Pt(oy + 85.0),
            radius: Pt(8.0),
        },
        Circle {
            x: Pt(ox + 150.0),
            y: Pt(oy + 85.0),
            radius: Pt(70.0),
        },
        &Gradient::new(&[
            ColorStop::new(0.0, SAND),
            ColorStop::new(0.5, ORANGE),
            ColorStop::new(1.0, PLUM),
        ])?,
    );

    // type 4: a fan of triangles around a center
    let (ox, oy) = panel_origin(0, 1);
    let (cx, cy) = (ox + PANEL_WIDTH / 2.0, oy + PANEL_HEIGHT / 2.0);
    let ring = [ORANGE, SAND, TEAL, SKY, PLUM, NAVY];
    let ring_vertex = |i: usize| {
        let angle = std::f64::consts::TAU * i as f64 / 6.0;
        Vertex::new(
            cx + 85.0 * angle.cos(),
            cy + 70.0 * angle.sin(),
            ring[i % 6],
        )
    };
    let mut triangles = FreeFormMesh::new([Vertex::new(cx, cy, Rgb(1.0, 1.0, 1.0)), ring_vertex(0), ring_vertex(1)]);
    for i in 2..=6 {
        triangles = triangles.fan(ring_vertex(i));
    }

    // type 5: a wavy grid of vertices
    let (ox, oy) = panel_origin(1, 1);
    let palette = [ORANGE, SAND, TEAL, SKY, PLUM];
    let lattice = LatticeMesh::new(
        (0..4)
            .map(|row| {
                (0..6)
                    .map(|column| {
                        let x = ox + 25.0 + (PANEL_WIDTH - 50.0) * column as f64 / 5.0;
                        let y = oy + 30.0
                            + (PANEL_HEIGHT - 60.0) * row as f64 / 3.0
                            + 14.0 * (column as f64 * 1.3 + row as f64).sin();
                        Vertex::new(x, y, palette[(column + 2 * row) % 5])
                    })
                    .collect()
            })
            .collect(),
    )?;

    // type 6: a Coons patch
    let (ox, oy) = panel_origin(0, 2);
    let corners = [ORANGE, SAND, TEAL, NAVY];
    let coons = CoonsMesh::new(Patch {
        points: boundary(ox, oy),
        colors: corners,
    });

    // type 7: the same boundary with the interior points pulled to the sides
    let (ox, oy) = panel_origin(1, 2);
    let mut points = [Point::default(); 16];
    points[..12].copy_from_slice(&boundary(ox, oy));
    let (x0, y0) = (ox + 30.0, oy + 30.0);
    let (dx, dy) = (PANEL_WIDTH - 60.0, PANEL_HEIGHT - 60.0);
    points[12] = Point::new(x0 + 0.85 * dx, y0 + 0.9 * dy);
    points[13] = Point::new(x0 + 0.15 * dx, y0 + 0.85 * dy);
    points[14] = Point::new(x0 + 0.1 * dx, y0 + 0.1 * dy);
    points[15] = Point::new(x0 + 0.9 * dx, y0 + 0.15 * dy);
    let tensor = TensorMesh::new(Patch {
        points,
        colors: corners,
    });

    let shadings: [Shading; 6] = [
        axial.into(),
        radial.into(),
        context.write_mesh_shading(&triangles)?,
        context.write_mesh_shading(&lattice)?,
        context.write_mesh_shading(&coons)?,
        context.write_mesh_shading(&tensor)?,
    ];
    let captions = [
        "Type 2 \u{b7} axial",
        "Type 3 \u{b7} radial, between two circles",
        "Type 4 \u{b7} free-form triangle mesh",
        "Type 5 \u{b7} lattice-form triangle mesh",
        "Type 6 \u{b7} Coons patch mesh",
        "Type 7 \u{b7} tensor-product patch mesh",
    ];

    let mut page = Page::new();
    page.add_content(&mut context, Some(StreamFilter::Deflate), |c| {
        let mut regular = BuiltInFont::Helvetica.font(c.pdf_context)?;
        regular.encoding = Some(EncodingEntry::Predefined(PredefinedEncoding::WinAnsiEncoding));
        let regular = c.pdf_context.write_object(Font::Simple(regular))?;
        let regular = c.add_font(regular);
        let mut bold = BuiltInFont::HelveticaBold.font(c.pdf_context)?;
        bold.encoding = Some(EncodingEntry::Predefined(PredefinedEncoding::WinAnsiEncoding));
        let bold = c.pdf_context.write_object(Font::Simple(bold))?;
        let bold = c.add_font(bold);

        let text = |c: &mut lemon_pdf::content::PageContext<'_, '_, '_>, font, size, x, y, string: &str| {
            c.begin_text()?;
            c.set_font(font, Pt(size))?;
            c.set_position(Pt(x), Pt(y))?;
            c.draw_simple_glyphs(&WINDOWS_1252.encode(string).0)?;
            c.end_text()
        };

        c.device_rgb_fill_color(NAVY.0, NAVY.1, NAVY.2)?;
        text(c, &bold, 26.0, 50.0, 775.0, "PDF shadings")?;
        c.device_rgb_fill_color(0.35, 0.4, 0.5)?;
        text(c, &regular, 11.0, 50.0, 755.0, "All seven shading types of lemon_pdf that are implemented: 2 to 7.")?;

        for (index, shading) in shadings.iter().enumerate() {
            let (ox, oy) = panel_origin(index % 2, index / 2);
            let key = c.add_shading(shading.clone());
            c.save_graphics_state()?;
            // a light backdrop shows where a shading does not paint
            c.device_gray_fill_color(0.94)?;
            c.rect(Pt(ox), Pt(oy), Pt(PANEL_WIDTH), Pt(PANEL_HEIGHT))?;
            c.fill_path()?;
            c.rect(Pt(ox), Pt(oy), Pt(PANEL_WIDTH), Pt(PANEL_HEIGHT))?;
            c.clip(FillRule::NonZero)?;
            c.end_path()?;
            c.paint_shading(&key)?;
            c.restore_graphics_state()?;
            c.device_rgb_fill_color(NAVY.0, NAVY.1, NAVY.2)?;
            text(c, &regular, 10.5, ox, oy - 16.0, captions[index])?;
        }
        Ok(())
    })?;
    context.add_page(page)?;
    context
        .document_info
        .insert("Title".to_owned(), TextString("PDF shadings".to_owned()));
    context.finish()?;
    Ok(())
}
