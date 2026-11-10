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

//! Draws transparent vector shapes and an image with an alpha channel to `graphics.pdf`.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use lemon_pdf::graphics_state::{BlendMode, FillRule, LineCap, LineJoin};
use lemon_pdf::image::Image;
use lemon_pdf::pagetree::Page;
use lemon_pdf::stream::StreamFilter;
use lemon_pdf::{DocumentContext, Pt, Version};

/// A radial gradient from opaque orange in the center to transparent at the edge.
fn fading_disc(size: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size as f64 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let distance = ((x as f64 - center).powi(2) + (y as f64 - center).powi(2)).sqrt();
            let alpha = (1.0 - distance / center).max(0.0);
            rgba.extend_from_slice(&[255, (165.0 * alpha) as u8, 0, (255.0 * alpha) as u8]);
        }
    }
    rgba
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut file = BufWriter::new(File::create("graphics.pdf")?);
    let mut context = DocumentContext::with_writer(&mut file, Version::Pdf1_7)?;

    let image = context.write_image(Image::from_rgba8(64, 64, &fading_disc(64))?)?;

    let mut page = Page::new();
    page.add_content(&mut context, Some(StreamFilter::Deflate), |c| {
        // a striped background shows the transparency
        for i in 0..12 {
            c.device_gray_fill_color(if i % 2 == 0 { 0.85 } else { 0.6 })?;
            c.rect(Pt(0.0), Pt(f64::from(i) * 70.0), Pt(600.0), Pt(35.0))?;
            c.fill_path()?;
        }

        // three overlapping translucent circles, drawn with Bézier curves
        c.save_graphics_state()?;
        c.set_fill_alpha(0.6)?;
        c.set_blend_mode(BlendMode::Multiply)?;
        for &(x, y, (r, g, b)) in &[
            (200.0, 600.0, (1.0, 0.0, 0.0)),
            (280.0, 600.0, (0.0, 0.7, 0.0)),
            (240.0, 670.0, (0.0, 0.0, 1.0)),
        ] {
            c.device_rgb_fill_color(r, g, b)?;
            circle(c, x, y, 60.0)?;
            c.fill_path()?;
        }
        c.restore_graphics_state()?;

        // a dashed, translucent star filled with the even-odd rule
        c.save_graphics_state()?;
        c.set_stroke_alpha(0.8)?;
        c.set_fill_alpha(0.4)?;
        c.line_width(Pt(4.0))?;
        c.line_join(LineJoin::Round)?;
        c.line_cap(LineCap::Round)?;
        c.dash_pattern(&[Pt(12.0), Pt(8.0)], Pt(0.0))?;
        c.device_rgb_stroke_color(0.1, 0.1, 0.4)?;
        c.device_rgb_fill_color(1.0, 0.8, 0.0)?;
        for i in 0..5 {
            let angle =
                std::f64::consts::FRAC_PI_2 + f64::from(i) * 4.0 * std::f64::consts::PI / 5.0;
            let (x, y) = (440.0 + 90.0 * angle.cos(), 450.0 + 90.0 * angle.sin());
            if i == 0 {
                c.move_to(Pt(x), Pt(y))?;
            } else {
                c.line_to(Pt(x), Pt(y))?;
            }
        }
        c.close_fill_and_stroke_path(FillRule::EvenOdd)?;
        c.restore_graphics_state()?;

        // the image, clipped to the left half
        let key = c.add_xobject(image);
        c.save_graphics_state()?;
        c.rect(Pt(60.0), Pt(240.0), Pt(120.0), Pt(240.0))?;
        c.clip(FillRule::NonZero)?;
        c.end_path()?;
        c.draw_image(&key, Pt(60.0), Pt(240.0), Pt(240.0), Pt(240.0))?;
        c.restore_graphics_state()?;
        // the same image again, half transparent
        c.set_fill_alpha(0.5)?;
        c.draw_image(&key, Pt(320.0), Pt(80.0), Pt(200.0), Pt(200.0))
    })?;
    context.add_page(page)?;
    context.finish()?;
    Ok(())
}

/// Appends a circle as four Bézier curves.
fn circle(c: &mut lemon_pdf::content::PageContext, x: f64, y: f64, r: f64) -> std::io::Result<()> {
    // the distance of the control points for a quarter circle
    let k = r * 0.552_284_749_8;
    c.move_to(Pt(x + r), Pt(y))?;
    c.curve_to(Pt(x + r), Pt(y + k), Pt(x + k), Pt(y + r), Pt(x), Pt(y + r))?;
    c.curve_to(Pt(x - k), Pt(y + r), Pt(x - r), Pt(y + k), Pt(x - r), Pt(y))?;
    c.curve_to(Pt(x - r), Pt(y - k), Pt(x - k), Pt(y - r), Pt(x), Pt(y - r))?;
    c.curve_to(Pt(x + k), Pt(y - r), Pt(x + r), Pt(y - k), Pt(x + r), Pt(y))?;
    c.close_path()
}
