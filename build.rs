use afm;
use glob::glob;
use pom;
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

    let parser = afm::afm();

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
        let metrics = fs::read_to_string(entry.expect("Could not read afm file"))?;
        let mut input = pom::DataInput::new(metrics.as_bytes());
        let metrics = parser.parse(&mut input)?;
        let font_name = metrics.font_name;
        let family_name = metrics.family_name;
        let ascender = metrics.ascender;
        let descender = metrics.descender;
        let cap_height = metrics.cap_height;
        let font_bbox = [
            metrics.font_bbox.xmin,
            metrics.font_bbox.ymin,
            metrics.font_bbox.xmax,
            metrics.font_bbox.ymax,
        ];
        let italic_angle = metrics.italic_angle;
        let stem_h = metrics.standard_horizontal_width;
        let stem_v = metrics.standard_vertical_width;
        let x_height = metrics.x_height;

        let mut char_names = vec![];
        let char_metrics: Vec<_> = metrics
            .char_metrics
            .into_iter()
            .map(|metric| {
                let name = metric.name;
                char_names.push(name.clone());
                let bbox = [
                    metric.bbox.xmin,
                    metric.bbox.ymin,
                    metric.bbox.xmax,
                    metric.bbox.ymax,
                ];
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
