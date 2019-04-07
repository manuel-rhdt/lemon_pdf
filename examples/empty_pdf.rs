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

extern crate failure;
extern crate lemon_pdf;

use std::fs::File;
use std::io::BufWriter;

use failure::Error;
use lemon_pdf::font::builtin::BuiltInFont;
use lemon_pdf::font::Font;
use lemon_pdf::pagetree::Page;
use lemon_pdf::{Context, Version, Pt};

fn main() -> Result<(), Error> {
    let file = BufWriter::new(File::create("testpdf.pdf")?);

    let mut context = Context::with_writer(file, Version::Pdf1_7)?;

    let mut page = Page::new();
    page.add_content(&mut context, None, |page_context| {
        let font = BuiltInFont::TimesItalic.font(&mut page_context.pdf_context)?;
        let font_ref = page_context.pdf_context.write_object(Font::Simple(font))?;
        let font_key = page_context.add_font(font_ref);

        page_context.begin_text()?;
        page_context.set_font(&font_key, Pt(48.0))?;
        page_context.set_position(Pt(20.0), Pt(40.0))?;
        page_context.draw_simple_glyphs(b"Die Kr\xFAnung allen Gluecks")?;
        page_context.end_text()?;
        Ok(())
    })?;

    context.add_page(page);
    context.finish()?;
    Ok(())
}
