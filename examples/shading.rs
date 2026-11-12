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

//! Paints gradients to `shading.pdf`: shadings clipped to shapes, and a shading pattern.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use lemon_pdf::graphics_state::FillRule;
use lemon_pdf::image::ColorSpace;
use lemon_pdf::pagetree::Page;
use lemon_pdf::shading::{ColorStop, Shading, ShadingPattern};
use lemon_pdf::stream::StreamFilter;
use lemon_pdf::{DocumentContext, Pt, Version};

fn main() -> Result<(), Box<dyn Error>> {
    let mut file = BufWriter::new(File::create("shading.pdf")?);
    let mut context = DocumentContext::with_writer(&mut file, Version::Pdf1_7)?;

    let rainbow = [
        ColorStop::new(0.0, [0.9, 0.2, 0.2]),
        ColorStop::new(0.35, [0.95, 0.8, 0.2]),
        ColorStop::new(0.7, [0.2, 0.7, 0.4]),
        ColorStop::new(1.0, [0.2, 0.3, 0.8]),
    ];
    // a hard transition in the middle
    let split = [
        ColorStop::new(0.0, [0.1, 0.1, 0.3]),
        ColorStop::new(0.5, [0.1, 0.1, 0.3]),
        ColorStop::new(0.5, [1.0, 0.45, 0.25]),
        ColorStop::new(1.0, [1.0, 0.45, 0.25]),
    ];

    let axial = Shading::axial(
        ColorSpace::DeviceRGB,
        (50.0, 0.0),
        (450.0, 0.0),
        &rainbow,
        true,
    )?;
    let radial = Shading::radial(
        ColorSpace::DeviceRGB,
        (150.0, 580.0, 0.0),
        (150.0, 580.0, 70.0),
        &rainbow,
        false,
    )?;
    let hard = Shading::axial(
        ColorSpace::DeviceRGB,
        (260.0, 510.0),
        (440.0, 650.0),
        &split,
        true,
    )?;

    let mut page = Page::new();
    page.add_content(&mut context, Some(StreamFilter::Deflate), |c| {
        // a linear gradient, clipped to a rectangle
        let axial_key = c.add_shading(axial.clone());
        c.save_graphics_state()?;
        c.rect(Pt(50.0), Pt(700.0), Pt(400.0), Pt(60.0))?;
        c.clip(FillRule::NonZero)?;
        c.end_path()?;
        c.paint_shading(&axial_key)?;
        c.restore_graphics_state()?;

        // a radial gradient, clipped to a circle
        let radial = c.add_shading(radial);
        c.save_graphics_state()?;
        circle(c, 150.0, 580.0, 70.0)?;
        c.clip(FillRule::NonZero)?;
        c.end_path()?;
        c.paint_shading(&radial)?;
        c.restore_graphics_state()?;

        // the hard transition, on a diagonal
        let hard = c.add_shading(hard);
        c.save_graphics_state()?;
        c.rect(Pt(260.0), Pt(510.0), Pt(180.0), Pt(140.0))?;
        c.clip(FillRule::NonZero)?;
        c.end_path()?;
        c.paint_shading(&hard)?;
        c.restore_graphics_state()?;

        // a shading pattern fills text-like shapes and strokes
        let pattern = c.add_shading_pattern(ShadingPattern::new(axial));
        c.set_fill_pattern(&pattern)?;
        c.set_stroke_pattern(&pattern)?;
        c.line_width(Pt(10.0))?;
        circle(c, 150.0, 380.0, 60.0)?;
        c.fill_and_stroke_path(FillRule::NonZero)?;
        c.rect(Pt(260.0), Pt(320.0), Pt(180.0), Pt(120.0))?;
        c.stroke_path()
    })?;
    context.add_page(page)?;
    context.finish()?;
    Ok(())
}

/// Appends a circle to the current path as four Bézier curves.
fn circle(
    c: &mut lemon_pdf::content::PageContext<'_, '_, '_>,
    x: f64,
    y: f64,
    r: f64,
) -> std::io::Result<()> {
    let k = 0.5523 * r;
    c.move_to(Pt(x + r), Pt(y))?;
    c.curve_to(Pt(x + r), Pt(y + k), Pt(x + k), Pt(y + r), Pt(x), Pt(y + r))?;
    c.curve_to(Pt(x - k), Pt(y + r), Pt(x - r), Pt(y + k), Pt(x - r), Pt(y))?;
    c.curve_to(Pt(x - r), Pt(y - k), Pt(x - k), Pt(y - r), Pt(x), Pt(y - r))?;
    c.curve_to(Pt(x + k), Pt(y - r), Pt(x + r), Pt(y - k), Pt(x + r), Pt(y))?;
    c.close_path()
}
