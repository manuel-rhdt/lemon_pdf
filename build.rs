use glob::glob;
use proc_macro2::{Ident, Span};
use quote::quote;

use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::{char, u32};
use std::{env, fs};

use inflector::cases::pascalcase::to_pascal_case;
use inflector::cases::snakecase::to_snake_case;

const GLYPH_LIST: &'static str = "resources/font/adobe_glyph_list/glyphlist.txt";
const GLYPH_LIST_ZAPF_DINGBATS: &'static str = "resources/font/adobe_glyph_list/zapfdingbats.txt";

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let dest_path = Path::new(&out_dir).join("base_14_fonts.rs");
    let mut f = fs::File::create(&dest_path)?;

    let mut tokens = vec![];
    let mut font_names = vec![];
    println!("cargo:rerun-if-changed=resources/font/base_14_fonts");
    println!("cargo:rerun-if-changed={}", GLYPH_LIST);
    println!("cargo:rerun-if-changed={}", GLYPH_LIST_ZAPF_DINGBATS);

    let mut glyph_list = build_glyph_list(GLYPH_LIST)?;
    let glyph_list_zapf_dingbats = build_glyph_list(GLYPH_LIST_ZAPF_DINGBATS)?;
    glyph_list.extend_from_slice(&glyph_list_zapf_dingbats);

    for entry in glob("resources/font/base_14_fonts/*.afm")
        .expect("Could not find font metrics specifications for base 14 fonts.")
    {
        let path = entry.expect("Could not read afm file");
        let metrics = parse_afm(&fs::read_to_string(&path)?)
            .map_err(|err| format!("{}: {}", path.display(), err))?;
        let font_name = metrics.font_name;
        let family_name = metrics.family_name;
        let ascender = metrics.ascender;
        let descender = metrics.descender;
        let cap_height = metrics.cap_height;
        let font_bbox = metrics.font_bbox;
        let italic_angle = metrics.italic_angle;
        let stem_h = metrics.stem_h;
        let stem_v = metrics.stem_v;
        let x_height = metrics.x_height;

        let mut char_names = vec![];
        let char_metrics: Vec<_> = metrics
            .char_metrics
            .into_iter()
            .map(|metric| {
                let name = metric.name;
                char_names.push(name.clone());
                let bbox = metric.bbox;
                let character_code = metric.character_code;
                let wx = metric.wx;
                quote!(CharMetric {
                    name: #name,
                    bbox: [#(FontUnit(#bbox)),*],
                    character_code: #character_code,
                    advance_width: FontUnit(#wx),
                })
            })
            .collect();

        let unicode_mapping_ident = Ident::new(
            &format!("_{}_mapping", to_snake_case(&font_name)),
            Span::call_site(),
        );
        let unicode_mapping =
            build_unicode_mapping(&unicode_mapping_ident, &char_names, &glyph_list)?;

        tokens.push(quote!(
            #unicode_mapping

            BuiltInFontMetrics {
                font_name: #font_name,
                family_name: #family_name,
                ascender: FontUnit(#ascender),
                descender: FontUnit(#descender),
                cap_height: FontUnit(#cap_height),
                stem_h: FontUnit(#stem_h),
                stem_v: FontUnit(#stem_v),
                x_height: FontUnit(#x_height),
                italic_angle: #italic_angle,
                font_bbox: [#(FontUnit(#font_bbox)),*],
                char_metrics: &[#(#char_metrics),*],
                unicode_mapping: #unicode_mapping_ident,
            }
        ));

        font_names.push(font_name);
    }

    let font_names_case_corrected = font_names
        .iter()
        .map(|s| Ident::new(&to_pascal_case(&s), Span::call_site()));
    let font_names_case_corrected2 = font_names_case_corrected.clone();

    let token_stream = quote!(
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
        pub enum BuiltInFont {
            #(#font_names_case_corrected),*
        }

        impl BuiltInFont {
            pub fn metrics(self) -> BuiltInFontMetrics {
                match self {
                    #( BuiltInFont::#font_names_case_corrected2 => { #tokens }),*
                }
            }
        }
    );

    write!(f, "{}", token_stream)?;

    Ok(())
}

/// The parts of an Adobe Font Metrics file (AFM, Adobe Technical Note #5004) that are needed
/// for the base 14 fonts. Missing numbers are 0 and missing names are empty.
#[derive(Debug, Default)]
struct FontMetrics {
    font_name: String,
    family_name: String,
    ascender: f64,
    descender: f64,
    cap_height: f64,
    x_height: f64,
    italic_angle: f64,
    /// `StdHW`, the dominant width of horizontal stems.
    stem_h: f64,
    /// `StdVW`, the dominant width of vertical stems.
    stem_v: f64,
    font_bbox: [f64; 4],
    char_metrics: Vec<CharMetric>,
}

#[derive(Debug, Default)]
struct CharMetric {
    /// `C`: the code of the character in the font's encoding, -1 if it is not encoded.
    character_code: i32,
    /// `WX`: the advance width.
    wx: f64,
    /// `N`: the glyph name.
    name: String,
    /// `B`: the bounding box.
    bbox: [f64; 4],
}

fn parse_number<T: std::str::FromStr>(value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number {:?}", value))
}

fn parse_bbox(value: &str) -> Result<[f64; 4], String> {
    let numbers = value
        .split_whitespace()
        .map(parse_number)
        .collect::<Result<Vec<f64>, _>>()?;
    match numbers[..] {
        [llx, lly, urx, ury] => Ok([llx, lly, urx, ury]),
        _ => Err(format!("a bounding box needs four numbers, got {:?}", value)),
    }
}

/// Parses an AFM file. Only the keys of `FontMetrics` are read; all others (like kerning
/// pairs) are skipped.
fn parse_afm(text: &str) -> Result<FontMetrics, String> {
    let mut metrics = FontMetrics::default();
    let mut expected_char_metrics = None;
    let mut in_char_metrics = false;

    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        let context = |err: String| format!("line {}: {}", line_number + 1, err);
        if in_char_metrics {
            match line {
                "EndCharMetrics" => in_char_metrics = false,
                "" => {}
                _ => metrics
                    .char_metrics
                    .push(parse_char_metric(line).map_err(context)?),
            }
            continue;
        }

        let (key, value) = match line.find(char::is_whitespace) {
            Some(index) => (&line[..index], line[index..].trim()),
            None => (line, ""),
        };
        match key {
            "FontName" => metrics.font_name = value.to_owned(),
            "FamilyName" => metrics.family_name = value.to_owned(),
            "Ascender" => metrics.ascender = parse_number(value).map_err(context)?,
            "Descender" => metrics.descender = parse_number(value).map_err(context)?,
            "CapHeight" => metrics.cap_height = parse_number(value).map_err(context)?,
            "XHeight" => metrics.x_height = parse_number(value).map_err(context)?,
            "ItalicAngle" => metrics.italic_angle = parse_number(value).map_err(context)?,
            "StdHW" => metrics.stem_h = parse_number(value).map_err(context)?,
            "StdVW" => metrics.stem_v = parse_number(value).map_err(context)?,
            "FontBBox" => metrics.font_bbox = parse_bbox(value).map_err(context)?,
            "StartCharMetrics" => {
                expected_char_metrics = Some(parse_number::<usize>(value).map_err(context)?);
                in_char_metrics = true;
            }
            _ => {}
        }
    }

    if in_char_metrics {
        return Err("missing EndCharMetrics".to_owned());
    }
    match expected_char_metrics {
        None => Err("missing StartCharMetrics".to_owned()),
        Some(count) if count != metrics.char_metrics.len() => Err(format!(
            "StartCharMetrics announces {} characters, but there are {}",
            count,
            metrics.char_metrics.len()
        )),
        Some(_) if metrics.font_name.is_empty() => Err("missing FontName".to_owned()),
        Some(_) => Ok(metrics),
    }
}

/// Parses a line like `C 32 ; WX 600 ; N space ; B 0 0 0 0 ;`.
fn parse_char_metric(line: &str) -> Result<CharMetric, String> {
    let mut metric = CharMetric::default();
    for command in line.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let (key, value) = match command.find(char::is_whitespace) {
            Some(index) => (&command[..index], command[index..].trim()),
            None => (command, ""),
        };
        match key {
            "C" => metric.character_code = parse_number(value)?,
            "WX" => metric.wx = parse_number(value)?,
            "N" => metric.name = value.to_owned(),
            "B" => metric.bbox = parse_bbox(value)?,
            // ligatures (`L`), vertical metrics and so on are not needed
            _ => {}
        }
    }
    if metric.name.is_empty() {
        return Err(format!("character without a name: {:?}", line));
    }
    Ok(metric)
}

fn build_unicode_mapping(
    ident: &Ident,
    names: &[String],
    glyph_list: &[(String, Vec<char>)],
) -> Result<proc_macro2::TokenStream, Box<dyn Error>> {
    let mut match_clauses = vec![];
    for (glyph_name, unicode_values) in glyph_list {
        if let Some(index) = names.iter().position(|name| name == glyph_name) {
            let index = std::iter::repeat(index);
            match_clauses.push(quote! { #(#unicode_values => Some(#index)),* });
        }
    }

    Ok(quote! {
        fn #ident(unicode: char) -> Option<usize> {
            match unicode {
                #(#match_clauses),*,
                _ => None
            }
        }
    })
}

fn build_glyph_list(filename: &str) -> Result<Vec<(String, Vec<char>)>, std::io::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .delimiter(b';')
        .has_headers(false)
        .from_path(filename)?;

    let mut out = vec![];
    for result in reader.records() {
        let record = result?;
        let glyph_name = record[0].to_owned();
        let unicode_values = record[1].split_whitespace().map(|uval| {
            char::from_u32(u32::from_str_radix(uval, 16).expect("Could not parse unicode value"))
                .expect("invalid char")
        });

        out.push((glyph_name, unicode_values.collect()))
    }
    Ok(out)
}
