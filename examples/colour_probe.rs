//! Writes the PDF of a colour probe from a text spec (see `tools/colour-probe/spec.py`):
//!
//!     cargo run --example colour_probe -- probe.spec probe.pdf
//!
//! The spec has one command per line (coordinates are in points from the top left of the page):
//!
//! ```text
//! page WIDTH HEIGHT
//! profile ID lemon srgb_v2 | p3_v4 | rec2020_v4 | rec2020_g24
//! profile ID matrix DESCRIPTION R(x y z) G(x y z) B(x y z) g a b c d   (D50 colorants, parametric curve)
//! profile ID file PATH
//! space ID device | gray
//! space ID icc PROFILE
//! space ID calrgb WX WY WZ G1 G2 G3 M0..M8
//! space ID lab WX WY WZ AMIN AMAX BMIN BMAX
//! rect X Y W H SPACE C1 [C2 C3] [ri=INTENT] [gs=INTENT]
//! stroke X Y W H GRAY LINE_WIDTH
//! text X Y SIZE BOLD GRAY STRING...
//! ```
//!
//! The numbers are the ones that the spec says: this program converts nothing.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;

use lemon_pdf::color_space::{CalRgbColorSpace, IccBasedColorSpace, LabColorSpace};
use lemon_pdf::content::PageContext;
use lemon_pdf::font::builtin::BuiltInFont;
use lemon_pdf::font::encoding::{EncodingEntry, PredefinedEncoding};
use lemon_pdf::font::Font;
use lemon_pdf::graphics_state::{ExtGState, RenderingIntent};
use lemon_pdf::object::{Name, TextString};
use lemon_pdf::pagetree::MediaBox;
use lemon_pdf::stream::StreamFilter;
use lemon_pdf::{DocumentContext, Page, Pt, Version};

enum Command {
    Rect { x: f64, y: f64, w: f64, h: f64, space: String, c: Vec<f64>, ri: Option<RenderingIntent>, gs: Option<usize> },
    Stroke { x: f64, y: f64, w: f64, h: f64, gray: f64, width: f64 },
    Text { x: f64, y: f64, size: f64, bold: bool, gray: f64, string: String },
}

struct PageSpec {
    width: f64,
    height: f64,
    commands: Vec<Command>,
}

fn intent(name: &str) -> RenderingIntent {
    match name {
        "Perceptual" => RenderingIntent::Perceptual,
        "RelativeColorimetric" => RenderingIntent::RelativeColorimetric,
        "Saturation" => RenderingIntent::Saturation,
        "AbsoluteColorimetric" => RenderingIntent::AbsoluteColorimetric,
        other => panic!("no rendering intent {}", other),
    }
}

/// The index of an intent, which marks the ExtGState that the harness gives an /RI afterwards: the
/// fill alpha 0.9001, 0.9002, ... (lemon_pdf has no /RI in ExtGState; see tools/colour-probe/README.md).
fn gs_index(name: &str) -> usize {
    ["Perceptual", "RelativeColorimetric", "Saturation", "AbsoluteColorimetric"].iter().position(|n| *n == name).expect("an intent")
}

fn numbers(parts: &[&str]) -> Vec<f64> {
    parts.iter().map(|p| p.parse().unwrap_or_else(|_| panic!("not a number: {p}"))).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let spec = std::fs::read_to_string(&args[1]).expect("the spec");
    let mut profiles: HashMap<String, Vec<u8>> = HashMap::new();
    let mut spaces: Vec<(String, Vec<String>)> = Vec::new();
    let mut pages: Vec<PageSpec> = Vec::new();
    for line in spec.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "page" => pages.push(PageSpec { width: parts[1].parse().unwrap(), height: parts[2].parse().unwrap(), commands: vec![] }),
            "profile" => {
                let id = parts[1].to_owned();
                let bytes = match parts[2] {
                    "lemon" => match parts[3] {
                        "srgb_v2" => lemon_pdf::icc::srgb_profile(),
                        "p3_v4" => lemon_pdf::icc::display_p3_profile(),
                        "rec2020_v4" => lemon_pdf::icc::rec2020_profile(),
                        "rec2020_g24" => lemon_pdf::icc::rec2020_gamma24_profile(),
                        other => panic!("no lemon profile {other}"),
                    },
                    "matrix" => {
                        // DESCRIPTION (one word) then 9 colorant numbers and 5 curve numbers
                        let n = numbers(&parts[4..]);
                        lemon_pdf::icc::rgb_matrix_profile(
                            &parts[3].replace('_', " "),
                            (n[0], n[1], n[2]),
                            (n[3], n[4], n[5]),
                            (n[6], n[7], n[8]),
                            lemon_pdf::icc::ParametricCurve { g: n[9], a: n[10], b: n[11], c: n[12], d: n[13] },
                        )
                    }
                    "file" => std::fs::read(parts[3]).expect("a profile file"),
                    other => panic!("no profile source {other}"),
                };
                profiles.insert(id, bytes);
            }
            "space" => spaces.push((parts[1].to_owned(), parts[2..].iter().map(|s| s.to_string()).collect())),
            "rect" => {
                let page = pages.last_mut().expect("a page");
                let mut c = vec![];
                let (mut ri, mut gs) = (None, None);
                for part in &parts[6..] {
                    if let Some(name) = part.strip_prefix("ri=") {
                        ri = Some(intent(name));
                    } else if let Some(name) = part.strip_prefix("gs=") {
                        gs = Some(gs_index(name));
                    } else {
                        c.push(part.parse().unwrap_or_else(|_| panic!("not a number: {part}")));
                    }
                }
                let n = numbers(&parts[1..5]);
                page.commands.push(Command::Rect { x: n[0], y: n[1], w: n[2], h: n[3], space: parts[5].to_owned(), c, ri, gs });
            }
            "stroke" => {
                let n = numbers(&parts[1..]);
                pages.last_mut().unwrap().commands.push(Command::Stroke { x: n[0], y: n[1], w: n[2], h: n[3], gray: n[4], width: n[5] });
            }
            "text" => {
                let n = numbers(&parts[1..6]);
                pages.last_mut().unwrap().commands.push(Command::Text {
                    x: n[0], y: n[1], size: n[2], bold: n[3] != 0.0, gray: n[4], string: parts[6..].join(" "),
                });
            }
            other => panic!("no command {other}"),
        }
    }

    let file = BufWriter::new(File::create(&args[2]).unwrap());
    let mut context = DocumentContext::with_writer(file, Version::Pdf1_7).unwrap();
    context.document_info.insert("Title".to_owned(), TextString("Colour probe".to_owned()));
    context.document_info.insert("Producer".to_owned(), TextString("lemon_pdf colour probe".to_owned()));

    // the ICC profiles are streams of the document, in the order of the spec
    let mut icc: HashMap<String, IccBasedColorSpace> = HashMap::new();
    let mut ids: Vec<&String> = profiles.keys().collect();
    ids.sort();
    for id in ids {
        icc.insert(id.clone(), IccBasedColorSpace(context.write_icc_profile(profiles[id].clone(), 3, None).unwrap()));
    }

    for page_spec in &pages {
        let mut page = Page::new();
        page.set_media_box(MediaBox::new(Pt(0.0), Pt(0.0), Pt(page_spec.width), Pt(page_spec.height)));
        page.add_content(&mut context, Some(StreamFilter::Deflate), |page: &mut PageContext| {
            let mut font = BuiltInFont::Helvetica.font(page.pdf_context).unwrap();
            font.encoding = Some(EncodingEntry::Predefined(PredefinedEncoding::WinAnsiEncoding));
            let regular = page.pdf_context.write_object(Font::Simple(font)).unwrap();
            let mut font = BuiltInFont::HelveticaBold.font(page.pdf_context).unwrap();
            font.encoding = Some(EncodingEntry::Predefined(PredefinedEncoding::WinAnsiEncoding));
            let bold = page.pdf_context.write_object(Font::Simple(font)).unwrap();
            let (regular, bold) = (page.add_font(regular), page.add_font(bold));

            // the colour spaces of the page, by the ids of the spec
            let mut names: HashMap<String, Option<Name>> = HashMap::new();
            for (id, def) in &spaces {
                let f = |i: usize| -> f64 { def[i].parse().unwrap() };
                let name = match def[0].as_str() {
                    "device" | "gray" => None,
                    "icc" => Some(page.add_color_space(icc[&def[1]])),
                    "calrgb" => Some(page.add_color_space(CalRgbColorSpace {
                        white_point: [f(1), f(2), f(3)],
                        gamma: [f(4), f(5), f(6)],
                        matrix: [f(7), f(8), f(9), f(10), f(11), f(12), f(13), f(14), f(15)],
                    })),
                    "lab" => Some(page.add_color_space(LabColorSpace { white_point: [f(1), f(2), f(3)], range: [f(4), f(5), f(6), f(7)] })),
                    other => panic!("no colour space {other}"),
                };
                names.insert(id.clone(), name);
            }
            let kinds: HashMap<&str, &str> = spaces.iter().map(|(id, def)| (id.as_str(), def[0].as_str())).collect();
            let mut gs_names: HashMap<usize, Name> = HashMap::new();

            for command in &page_spec.commands {
                match command {
                    Command::Rect { x, y, w, h, space, c, ri, gs } => {
                        page.save_graphics_state().unwrap();
                        if let Some(index) = gs {
                            let name = gs_names.entry(*index).or_insert_with(|| {
                                // the marker that the harness replaces by /RI
                                page.add_ext_gstate(ExtGState::new().with_fill_alpha(0.91 + 0.01 * *index as f64))
                            }).clone();
                            page.set_ext_gstate(&name).unwrap();
                        }
                        if let Some(intent) = ri {
                            page.set_rendering_intent(*intent).unwrap();
                        }
                        // `device` and `gray` are always there; the other ids are the ones of `space` lines
                        let kind = match space.as_str() {
                            "device" => "device",
                            "gray" => "gray",
                            other => kinds[other],
                        };
                        match kind {
                            "device" => page.device_rgb_fill_color(c[0], c[1], c[2]).unwrap(),
                            "gray" => page.device_gray_fill_color(c[0]).unwrap(),
                            _ => page.set_fill_color_components(names[space].as_ref().expect("a colour space"), c).unwrap(),
                        }
                        page.rect(Pt(*x), Pt(page_spec.height - y - h), Pt(*w), Pt(*h)).unwrap();
                        page.fill_path().unwrap();
                        page.restore_graphics_state().unwrap();
                    }
                    Command::Stroke { x, y, w, h, gray, width } => {
                        page.device_gray_stroke_color(*gray).unwrap();
                        page.line_width(Pt(*width)).unwrap();
                        page.rect(Pt(*x), Pt(page_spec.height - y - h), Pt(*w), Pt(*h)).unwrap();
                        page.stroke_path().unwrap();
                    }
                    Command::Text { x, y, size, bold: is_bold, gray, string } => {
                        page.device_gray_fill_color(*gray).unwrap();
                        page.begin_text().unwrap();
                        page.set_font(if *is_bold { &bold } else { &regular }, Pt(*size)).unwrap();
                        page.set_position(Pt(*x), Pt(page_spec.height - y)).unwrap();
                        page.draw_simple_glyphs(string.as_bytes()).unwrap();
                        page.end_text().unwrap();
                    }
                }
            }
            Ok(())
        })
        .unwrap();
        context.add_page(page).unwrap();
    }
    context.finish().unwrap();
}
