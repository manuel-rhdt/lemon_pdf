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

//! A color gradient that fades out through a soft mask. The mask is a form XObject with a
//! gray gradient. It is written to `soft_mask.pdf`.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use lemon_pdf::form::TransparencyGroup;
use lemon_pdf::graphics_state::{ExtGState, SoftMask};
use lemon_pdf::image::ColorSpace;
use lemon_pdf::pagetree::{MediaBox, Page};
use lemon_pdf::shading::{AxialShading, ColorStop, Gradient, Gray, Point, Rgb};
use lemon_pdf::{DocumentContext, Pt, Version};


fn main() -> Result<(), Box<dyn Error>> {
    let mut file = BufWriter::new(File::create("soft_mask.pdf")?);
    let mut context = DocumentContext::with_writer(&mut file, Version::Pdf1_7)?;

    // the mask: opaque on the left, transparent on the right
    let fade = Gradient::new(&[ColorStop::new(0.0, Gray(1.0)), ColorStop::new(1.0, Gray(0.0))])?
        .extended();
    let mask_shading = AxialShading::new(Point::new(50.0, 0.0), Point::new(350.0, 0.0), &fade);
    let mask = context.write_form(
        [Pt(0.0), Pt(0.0), Pt(400.0), Pt(300.0)],
        Some(TransparencyGroup::new(ColorSpace::DeviceGray)),
        None,
        |c| {
            let shading = c.add_shading(mask_shading);
            c.paint_shading(&shading)
        },
    )?;

    // the content: a diagonal rainbow
    let rainbow = Gradient::new(&[
        ColorStop::new(0.0, Rgb(0.9, 0.1, 0.1)),
        ColorStop::new(0.33, Rgb(0.95, 0.8, 0.1)),
        ColorStop::new(0.66, Rgb(0.1, 0.7, 0.4)),
        ColorStop::new(1.0, Rgb(0.1, 0.3, 0.9)),
    ])?
    .extended();
    let rainbow = AxialShading::new(Point::new(50.0, 50.0), Point::new(350.0, 250.0), &rainbow);

    let mut page = Page::new();
    page.set_media_box(MediaBox::new(Pt(0.0), Pt(0.0), Pt(400.0), Pt(300.0)));
    page.add_content(&mut context, None, |c| {
        // stripes behind show that the mask makes the gradient transparent
        c.device_gray_fill_color(0.85)?;
        for i in 0..10 {
            c.rect(Pt(0.0), Pt(30.0 * f64::from(i) + 10.0), Pt(400.0), Pt(10.0))?;
            c.fill_path()?;
        }
        c.save_graphics_state()?;
        c.apply_ext_gstate(ExtGState::new().with_soft_mask(SoftMask::luminosity(mask)))?;
        c.rect(Pt(50.0), Pt(50.0), Pt(300.0), Pt(200.0))?;
        c.clip(lemon_pdf::graphics_state::FillRule::NonZero)?;
        c.end_path()?;
        let shading = c.add_shading(rainbow);
        c.paint_shading(&shading)?;
        c.restore_graphics_state()
    })?;
    context.add_page(page)?;
    context.finish()?;
    Ok(())
}
