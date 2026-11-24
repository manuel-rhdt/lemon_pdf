//! End-to-end tests: build complete documents in memory, then check them with an independent
//! reader (`lopdf`) and with a strict cross-reference checker written for these tests.
//!
//! Every test runs twice, with object streams enabled (cross-reference stream) and disabled
//! (classic cross-reference table), because the two modes share almost no code.

use std::collections::BTreeMap;
use std::io::Read;

use encoding_rs::WINDOWS_1252;
use flate2::read::ZlibDecoder;
use lopdf::{Document, Object, ObjectId};

use lemon_pdf::annotation::{Annotation, LinkAction, LinkAnnotation, UriAction};
use lemon_pdf::font::builtin::BuiltInFont;
use lemon_pdf::form::TransparencyGroup;
use lemon_pdf::graphics_state::{BlendMode, ExtGState, FillRule, LineCap, LineJoin, SoftMask};
use lemon_pdf::image::ColorSpace;
use lemon_pdf::image::Image;
use lemon_pdf::mesh::{CoonsMesh, FreeFormMesh, LatticeMesh, Patch, Vertex};
use lemon_pdf::shading::{
    AxialShading, Circle, ColorStop, Gradient, Gray, Point, RadialShading, Rgb, ShadingPattern,
};
use lemon_pdf::font::encoding::{EncodingEntry, PredefinedEncoding};
use lemon_pdf::font::Font;
use lemon_pdf::dictionary::Dictionary;
use lemon_pdf::object::{ByteString, Name, TextString};
use lemon_pdf::stream::{Stream, StreamFilter};
use lemon_pdf::structure_tree::{
    NumberTree, StructElem, StructParent, StructTreeRoot, StructType,
};
use lemon_pdf::{DocumentContext, Matrix, Page, Pt, Version};

const MODES: [bool; 2] = [true, false];

/// Builds a document in memory. `object_streams` selects the cross-reference mode.
fn build(object_streams: bool, fill: impl FnOnce(&mut DocumentContext<'_>)) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut context = DocumentContext::with_writer(&mut output, Version::Pdf1_7).unwrap();
        context.object_streams = object_streams;
        fill(&mut context);
        context.finish().unwrap();
    }
    output
}

/// Builds a document, checks its cross-reference data and loads it with `lopdf`.
fn build_and_load(
    object_streams: bool,
    fill: impl FnOnce(&mut DocumentContext<'_>),
) -> (Vec<u8>, Document) {
    let bytes = build(object_streams, fill);
    check_xref(&bytes);
    let document = Document::load_mem(&bytes).unwrap_or_else(|err| {
        panic!(
            "lopdf failed to load the document (object_streams = {}): {}\n{}",
            object_streams,
            err,
            String::from_utf8_lossy(&bytes)
        )
    });
    (bytes, document)
}

fn add_text_page(context: &mut DocumentContext<'_>, text: &str, filter: Option<StreamFilter>) {
    let mut page = Page::new();
    page.add_content(context, filter, |page_context| {
        let mut font = BuiltInFont::Helvetica.font(page_context.pdf_context)?;
        font.encoding = Some(EncodingEntry::Predefined(
            PredefinedEncoding::WinAnsiEncoding,
        ));
        let font_ref = page_context.pdf_context.write_object(Font::Simple(font))?;
        let font_key = page_context.add_font(font_ref);
        page_context.begin_text()?;
        page_context.set_font(&font_key, Pt(12.0))?;
        page_context.set_position(Pt(72.0), Pt(720.0))?;
        page_context.draw_simple_glyphs(&WINDOWS_1252.encode(text).0)?;
        page_context.end_text()
    })
    .unwrap();
    context.add_page(page).unwrap();
}

/// The `/ID` entry of the trailer of a file.
fn file_id(bytes: &[u8]) -> String {
    let at = rfind(bytes, b"/ID").expect("the trailer has an /ID");
    String::from_utf8_lossy(&bytes[at..at + 50]).into_owned()
}

#[test]
fn the_same_document_is_written_to_the_same_bytes() {
    for mode in MODES {
        let write = |text: &str| build(mode, |context| add_text_page(context, text, Some(StreamFilter::Deflate)));
        let (first, second) = (write("same"), write("same"));
        assert_eq!(first, second, "mode object_streams={mode}");
        // a different document gets a different identifier
        assert_ne!(file_id(&first), file_id(&write("other")), "mode object_streams={mode}");
    }
}

fn name(object: &Object) -> &str {
    std::str::from_utf8(object.as_name().unwrap()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Strict cross-reference checker
//
// lopdf can rebuild a broken cross-reference table by scanning the file, so a document that
// loads is not necessarily correct. These functions check the cross-reference data byte by
// byte instead.
// ---------------------------------------------------------------------------------------------

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

fn starts_with_at(bytes: &[u8], offset: usize, prefix: &str) -> bool {
    bytes.get(offset..).map_or(false, |rest| rest.starts_with(prefix.as_bytes()))
}

/// The integers following `/Key` in a dictionary (one for a number, several for an array).
fn dict_ints(dict: &str, key: &str) -> Vec<u64> {
    let pattern = format!("/{} ", key);
    let start = dict
        .find(&pattern)
        .unwrap_or_else(|| panic!("missing /{} in {}", key, dict))
        + pattern.len();
    let rest = dict[start..].trim_start_matches(|c| c == '[' || c == ' ');
    let mut values = Vec::new();
    for token in rest.split(|c: char| c == ' ' || c == '\n') {
        match token.parse() {
            Ok(value) => values.push(value),
            Err(_) => break,
        }
    }
    values
}

/// The dictionary and the (still encoded) data of the stream object at `offset`.
fn stream_at(bytes: &[u8], offset: usize) -> (String, &[u8]) {
    let dict_end = offset + find(&bytes[offset..], b"\nstream\n").expect("no stream keyword");
    let dict = String::from_utf8_lossy(&bytes[offset..dict_end]).into_owned();
    let length = dict_ints(&dict, "Length")[0] as usize;
    let data_start = dict_end + b"\nstream\n".len();
    let data = &bytes[data_start..data_start + length];
    assert!(
        bytes[data_start + length..].starts_with(b"\nendstream"),
        "/Length {} of the stream at {} is wrong",
        length,
        offset
    );
    (dict, data)
}

fn inflate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    ZlibDecoder::new(data).read_to_end(&mut out).unwrap();
    out
}

fn check_xref(bytes: &[u8]) {
    assert!(bytes.starts_with(b"%PDF-1.7\n"), "missing header");
    assert!(bytes.ends_with(b"%%EOF\n"), "missing %%EOF");

    let startxref = rfind(bytes, b"startxref\n").expect("no startxref");
    let xref_offset: usize = String::from_utf8_lossy(&bytes[startxref + 10..])
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();

    if starts_with_at(bytes, xref_offset, "xref\n") {
        check_xref_table(bytes, xref_offset);
    } else {
        check_xref_stream(bytes, xref_offset);
    }
}

fn check_xref_table(bytes: &[u8], offset: usize) {
    let text = &bytes[offset..];
    let header_end = find(text, b"\n").unwrap() + 1;
    let subsection_end = header_end + find(&text[header_end..], b"\n").unwrap() + 1;
    let subsection = std::str::from_utf8(&text[header_end..subsection_end - 1]).unwrap();
    let mut parts = subsection.split(' ');
    assert_eq!(parts.next(), Some("0"), "the subsection must start at object 0");
    let count: usize = parts.next().unwrap().parse().unwrap();

    let mut free_list = BTreeMap::new();
    for num in 0..count {
        let start = subsection_end + num * 20;
        let entry = &text[start..start + 20];
        assert!(entry.ends_with(b"\r\n"), "xref entry {} is not 20 bytes long", num);
        let entry = std::str::from_utf8(&entry[..18]).unwrap();
        let (obj_offset, generation, kind) = (&entry[0..10], &entry[11..16], &entry[17..18]);
        if num == 0 {
            assert_eq!(&entry[10..], " 65535 f", "object 0 must be free with generation 65535");
        }
        if kind == "f" {
            free_list.insert(num, obj_offset.parse::<usize>().unwrap());
            continue;
        }
        assert_eq!(kind, "n", "bad entry type for object {}", num);
        let obj_offset: usize = obj_offset.parse().unwrap();
        let generation: u32 = generation.parse().unwrap();
        let expected = format!("{} {} obj", num, generation);
        assert!(
            starts_with_at(bytes, obj_offset, &expected),
            "xref entry for object {} points to {:?} instead of {:?}",
            num,
            String::from_utf8_lossy(&bytes[obj_offset..(obj_offset + 20).min(bytes.len())]),
            expected
        );
    }

    check_free_list(&free_list);

    let trailer = String::from_utf8_lossy(&text[subsection_end + count * 20..]);
    assert!(trailer.starts_with("trailer\n"), "no trailer after the xref table");
    assert_eq!(dict_ints(&trailer, "Size"), vec![count as u64]);
}

fn check_xref_stream(bytes: &[u8], offset: usize) {
    let header = String::from_utf8_lossy(&bytes[offset..offset + 30]).into_owned();
    let xref_num: usize = header.split(' ').next().unwrap().parse().unwrap();
    assert!(header.starts_with(&format!("{} 0 obj\n", xref_num)));

    let (dict, data) = stream_at(bytes, offset);
    assert!(dict.contains("/Type /XRef"), "{}", dict);
    let widths = dict_ints(&dict, "W");
    assert_eq!(widths.len(), 3, "/W must have three entries");
    let size = dict_ints(&dict, "Size")[0] as usize;
    let data = inflate(data);
    let row_len = widths.iter().sum::<u64>() as usize;
    assert_eq!(data.len(), size * row_len, "the xref stream must have /Size rows");

    let field = |row: &[u8], index: usize| -> u64 {
        let start: usize = widths[..index].iter().sum::<u64>() as usize;
        row[start..start + widths[index] as usize]
            .iter()
            .fold(0, |acc, &byte| (acc << 8) | u64::from(byte))
    };

    let rows: Vec<&[u8]> = data.chunks(row_len).collect();
    assert_eq!((field(rows[0], 0), field(rows[0], 2)), (0, 65535));
    assert_eq!(field(rows[xref_num], 0), 1, "the xref stream must list itself");

    let mut free_list = BTreeMap::new();
    free_list.insert(0, field(rows[0], 1) as usize);
    for (num, row) in rows.iter().enumerate().skip(1) {
        match field(row, 0) {
            0 => {
                free_list.insert(num, field(row, 1) as usize);
            }
            1 => {
                let obj_offset = field(row, 1) as usize;
                let expected = format!("{} {} obj", num, field(row, 2));
                assert!(
                    starts_with_at(bytes, obj_offset, &expected),
                    "xref entry for object {} points to {:?} instead of {:?}",
                    num,
                    String::from_utf8_lossy(
                        &bytes[obj_offset..(obj_offset + 20).min(bytes.len())]
                    ),
                    expected
                );
            }
            2 => {
                let (stream_num, index) = (field(row, 1) as usize, field(row, 2) as usize);
                let stream_row = rows[stream_num];
                assert_eq!(
                    field(stream_row, 0),
                    1,
                    "object stream {} of object {} must be stored uncompressed",
                    stream_num,
                    num
                );
                let objects = object_stream_contents(bytes, field(stream_row, 1) as usize);
                let (contained_num, object) = &objects[index];
                assert_eq!(
                    *contained_num, num,
                    "object stream {} has object {} at index {}, expected {}",
                    stream_num, contained_num, index, num
                );
                assert!(
                    find(object, b"stream").is_none(),
                    "stream object {} was put into an object stream",
                    num
                );
            }
            other => panic!("unknown xref entry type {} for object {}", other, num),
        }
    }
    check_free_list(&free_list);
}

/// The free entries (object number -> next free object number) must form one linked list that
/// starts at object 0, visits every free object and ends with 0.
fn check_free_list(free_list: &BTreeMap<usize, usize>) {
    let mut visited = vec![0];
    let mut current = free_list[&0];
    while current != 0 {
        assert!(
            free_list.contains_key(&current) && !visited.contains(&current),
            "broken free list: {:?}",
            free_list
        );
        visited.push(current);
        current = free_list[&current];
    }
    assert_eq!(visited.len(), free_list.len(), "free list misses entries: {:?}", free_list);
}

/// The objects (number, bytes) of the object stream at `offset`.
fn object_stream_contents(bytes: &[u8], offset: usize) -> Vec<(usize, Vec<u8>)> {
    let (dict, data) = stream_at(bytes, offset);
    assert!(dict.contains("/Type /ObjStm"), "{}", dict);
    let count = dict_ints(&dict, "N")[0] as usize;
    let first = dict_ints(&dict, "First")[0] as usize;
    let data = inflate(data);
    let header: Vec<usize> = std::str::from_utf8(&data[..first])
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(header.len(), 2 * count, "/N does not match the object stream header");
    (0..count)
        .map(|i| {
            let start = first + header[2 * i + 1];
            let end = header
                .get(2 * i + 3)
                .map_or(data.len(), |&next| first + next);
            (header[2 * i], data[start..end].to_vec())
        })
        .collect()
}

fn count_object_streams(bytes: &[u8]) -> usize {
    bytes
        .windows(b"/Type /ObjStm".len())
        .filter(|w| *w == b"/Type /ObjStm")
        .count()
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[test]
fn empty_document_is_valid() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |_| {});
        assert!(document.get_pages().is_empty());
        let pages = document.catalog().unwrap().get(b"Pages").unwrap();
        let pages = document.get_dictionary(pages.as_reference().unwrap()).unwrap();
        assert_eq!(pages.get(b"Count").unwrap().as_i64().unwrap(), 0);
    }
}

#[test]
fn text_page_round_trips() {
    for &mode in &MODES {
        for &filter in &[None, Some(StreamFilter::Deflate)] {
            let (_, document) = build_and_load(mode, |context| {
                add_text_page(context, "Hello World!", filter)
            });
            let text = document.extract_text(&[1]).unwrap();
            assert!(
                text.contains("Hello World!"),
                "mode {}, filter {:?}: extracted {:?}",
                mode,
                filter,
                text
            );
        }
    }
}

#[test]
fn page_tree_is_consistent() {
    const PAGES: usize = 30;
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            for i in 0..PAGES {
                add_text_page(context, &format!("Page {}", i + 1), None);
            }
        });

        let pages = document.get_pages();
        assert_eq!(pages.len(), PAGES);

        let root_ref = document
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let root = document.get_dictionary(root_ref).unwrap();
        assert_eq!(name(root.get(b"Type").unwrap()), "Pages");
        assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), PAGES as i64);

        let kids: Vec<ObjectId> = root
            .get(b"Kids")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|kid| kid.as_reference().unwrap())
            .collect();
        assert_eq!(kids, pages.values().copied().collect::<Vec<_>>());

        for (&number, &page_id) in &pages {
            let page = document.get_dictionary(page_id).unwrap();
            assert_eq!(name(page.get(b"Type").unwrap()), "Page");
            assert_eq!(page.get(b"Parent").unwrap().as_reference().unwrap(), root_ref);
            let text = document.extract_text(&[number]).unwrap();
            assert!(text.contains(&format!("Page {}", number)), "{:?}", text);
        }
    }
}

#[test]
fn many_objects_are_split_over_several_object_streams() {
    const OBJECTS: i64 = 450;
    for &mode in &MODES {
        let mut references = Vec::new();
        let (bytes, document) = build_and_load(mode, |context| {
            for i in 0..OBJECTS {
                references.push(context.write_object(i * 7).unwrap());
            }
        });

        // 200 objects per object stream at most, plus the catalog and the page tree
        let expected_streams = if mode { 3 } else { 0 };
        assert_eq!(count_object_streams(&bytes), expected_streams, "mode {}", mode);

        for (i, reference) in references.iter().enumerate() {
            let id = (reference.number() as u32, reference.generation() as u16);
            let value = document.get_object(id).unwrap().as_i64().unwrap();
            assert_eq!(value, i as i64 * 7);
        }
    }
}

#[test]
fn large_object_stream_is_flushed_by_size() {
    // a single object of more than 256 KiB fills an object stream on its own
    let big = ByteString(vec![b'x'; 300 * 1024]);
    let (bytes, document) = build_and_load(true, |context| {
        let reference = context.write_object(big.clone()).unwrap();
        let small = context.write_object(1i64).unwrap();
        assert_ne!(reference, small.convert());
    });
    assert_eq!(count_object_streams(&bytes), 2);
    assert!(document.get_pages().is_empty());
}

#[test]
fn document_info_is_written_when_not_empty() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            context
                .document_info
                .insert("Title".to_owned(), TextString("Bücher ✓ 𝄞".to_owned()));
            context
                .document_info
                .insert("Author".to_owned(), TextString("Jane (Doe)".to_owned()));
        });
        let info = document.trailer.get(b"Info").unwrap().as_reference().unwrap();
        let info = document.get_dictionary(info).unwrap();
        let title = lopdf::decode_text_string(info.get(b"Title").unwrap()).unwrap();
        assert_eq!(title, "Bücher ✓ 𝄞");
        let author = lopdf::decode_text_string(info.get(b"Author").unwrap()).unwrap();
        assert_eq!(author, "Jane (Doe)");
    }
}

#[test]
fn document_info_is_omitted_when_empty() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |_| {});
        assert!(!document.trailer.has(b"Info"), "mode {}", mode);
        assert!(document.trailer.has(b"ID"), "mode {}", mode);
    }
}

#[test]
fn tagged_pdf_catalog_entries_are_written() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let root_ref = context.create_reference();
            let elem =
                StructElem::new(StructType::Document, StructParent::Root(root_ref));
            let elem_ref = context.write_object(elem).unwrap();
            let root = StructTreeRoot {
                k: vec![elem_ref],
                parent_tree: NumberTree::default(),
                parent_tree_next_key: 0,
                role_map: BTreeMap::new(),
            };
            context.write_reserved_object(root_ref, root).unwrap();
            context.set_struct_tree_root(root_ref);
            context.set_marked(true);
            context.set_lang("de-DE");
            context.set_display_doc_title(true);
            context.set_metadata("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>").unwrap();
            add_text_page(context, "Tagged", None);
        });

        let catalog = document.catalog().unwrap();
        assert_eq!(name(catalog.get(b"Type").unwrap()), "Catalog");

        let root_ref = catalog.get(b"StructTreeRoot").unwrap().as_reference().unwrap();
        let root = document.get_dictionary(root_ref).unwrap();
        assert_eq!(name(root.get(b"Type").unwrap()), "StructTreeRoot");
        let kids = root.get(b"K").unwrap().as_array().unwrap();
        assert_eq!(kids.len(), 1);
        let elem = document.get_dictionary(kids[0].as_reference().unwrap()).unwrap();
        assert_eq!(name(elem.get(b"S").unwrap()), "Document");
        assert_eq!(elem.get(b"P").unwrap().as_reference().unwrap(), root_ref);

        let mark_info = catalog.get(b"MarkInfo").unwrap().as_dict().unwrap();
        assert!(mark_info.get(b"Marked").unwrap().as_bool().unwrap());
        let lang = lopdf::decode_text_string(catalog.get(b"Lang").unwrap()).unwrap();
        assert_eq!(lang, "de-DE");
        let preferences = catalog.get(b"ViewerPreferences").unwrap().as_dict().unwrap();
        assert!(preferences.get(b"DisplayDocTitle").unwrap().as_bool().unwrap());
        let metadata = catalog.get(b"Metadata").unwrap().as_reference().unwrap();
        let metadata = document.get_object(metadata).unwrap().as_stream().unwrap();
        assert_eq!(name(metadata.dict.get(b"Type").unwrap()), "Metadata");
        assert_eq!(name(metadata.dict.get(b"Subtype").unwrap()), "XML");
        assert!(!metadata.dict.has(b"Filter"));
        assert_eq!(metadata.content, b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>");
    }
}

#[test]
fn untagged_catalog_has_no_optional_entries() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| add_text_page(context, "x", None));
        let catalog = document.catalog().unwrap();
        for key in &[&b"StructTreeRoot"[..], b"MarkInfo", b"Lang", b"ViewerPreferences", b"Metadata"] {
            assert!(
                !catalog.has(key),
                "unexpected /{} (mode {})",
                String::from_utf8_lossy(key),
                mode
            );
        }
    }
}

#[test]
fn link_annotation_is_attached_to_page() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            let link = LinkAnnotation::new(
                [Pt(10.0), Pt(10.0), Pt(110.0), Pt(30.0)],
                LinkAction::Uri(UriAction {
                    uri: ByteString::from("https://example.com/a(b)"),
                }),
            );
            page.add_annotation(context, Annotation::Link(link)).unwrap();
            context.add_page(page).unwrap();
        });

        let (_, &page_id) = document.get_pages().iter().next().unwrap();
        let page = document.get_dictionary(page_id).unwrap();
        let annots = page.get(b"Annots").unwrap().as_array().unwrap();
        assert_eq!(annots.len(), 1);
        let annot = document.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
        assert_eq!(name(annot.get(b"Subtype").unwrap()), "Link");
        let action = annot.get(b"A").unwrap().as_dict().unwrap();
        assert_eq!(action.get(b"URI").unwrap().as_str().unwrap(), b"https://example.com/a(b)");
    }
}

#[test]
fn names_with_special_characters_round_trip() {
    for &mode in &MODES {
        let mut stream_ref = None;
        let (_, document) = build_and_load(mode, |context| {
            let mut dict = Dictionary::new();
            dict.insert("My Key".to_owned(), Box::new(Name::from("a/b#c (d)")));
            let stream = Stream::new(dict, b"data".to_vec());
            stream_ref = Some(context.write_object(stream).unwrap());
        });
        let stream_ref = stream_ref.unwrap();
        let id = (stream_ref.number() as u32, stream_ref.generation() as u16);
        let stream = document.get_object(id).unwrap().as_stream().unwrap();
        assert_eq!(stream.content, b"data");
        let value = stream.dict.get(b"My Key").unwrap();
        assert_eq!(value.as_name().unwrap(), b"a/b#c (d)");
    }
}

#[test]
fn reserved_reference_can_be_written_later() {
    for &mode in &MODES {
        let mut reference = None;
        let (_, document) = build_and_load(mode, |context| {
            let reserved = context.create_reference::<i64>();
            context.write_object(5i64).unwrap();
            context.write_reserved_object(reserved, 42).unwrap();
            reference = Some(reserved);
        });
        let reference = reference.unwrap();
        let id = (reference.number() as u32, reference.generation() as u16);
        assert_eq!(document.get_object(id).unwrap().as_i64().unwrap(), 42);
    }
}

#[test]
fn unwritten_reference_does_not_corrupt_the_document() {
    // A reference that is never written must end up as a free entry, not as an entry that
    // points to offset 0.
    for &mode in &MODES {
        build_and_load(mode, |context| {
            let _unused = context.create_reference::<i64>();
            add_text_page(context, "x", None);
        });
    }
}

#[test]
fn example_document_is_valid() {
    // the same document as `examples/empty_pdf.rs`
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            add_text_page(context, "Hello World!", None);
            context
                .document_info
                .insert("Title".to_owned(), TextString("My Document".to_owned()));
        });
        assert_eq!(document.get_pages().len(), 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Graphics: transparency, images and vector paths
// ---------------------------------------------------------------------------------------------

fn id<T>(reference: lemon_pdf::object::IndirectReference<T>) -> ObjectId {
    (reference.number() as u32, reference.generation() as u16)
}

/// The resource dictionary and the decoded content of the only page of the document.
fn single_page(document: &Document) -> (&lopdf::Dictionary, String) {
    let (_, &page_id) = document.get_pages().iter().next().unwrap();
    let page = document.get_dictionary(page_id).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let content = document.get_page_content(page_id);
    (resources, String::from_utf8(content).unwrap())
}

#[test]
fn image_with_alpha_is_embedded_with_soft_mask() {
    let rgba = [
        255, 0, 0, 255, /**/ 0, 255, 0, 128, //
        0, 0, 255, 0, /**/ 10, 20, 30, 255,
    ];
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let image = Image::from_rgba8(2, 2, &rgba).unwrap();
            let image = context.write_image(image).unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |page_context| {
                let key = page_context.add_xobject(image);
                assert_eq!(page_context.add_xobject(image), key, "added twice");
                page_context.draw_image(&key, Pt(10.0), Pt(20.0), Pt(200.0), Pt(100.0))
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert_eq!(content.trim(), "q 200 0 0 100 10 20 cm /X0 Do Q");
        let x_objects = resources.get(b"XObject").unwrap().as_dict().unwrap();
        let image_ref = x_objects.get(b"X0").unwrap().as_reference().unwrap();
        let image = document.get_object(image_ref).unwrap().as_stream().unwrap();
        assert_eq!(name(image.dict.get(b"Type").unwrap()), "XObject");
        assert_eq!(name(image.dict.get(b"Subtype").unwrap()), "Image");
        assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 2);
        assert_eq!(image.dict.get(b"Height").unwrap().as_i64().unwrap(), 2);
        assert_eq!(name(image.dict.get(b"ColorSpace").unwrap()), "DeviceRGB");
        assert_eq!(image.dict.get(b"BitsPerComponent").unwrap().as_i64().unwrap(), 8);
        assert_eq!(
            image.decompressed_content().unwrap(),
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30]
        );

        let mask_ref = image.dict.get(b"SMask").unwrap().as_reference().unwrap();
        let mask = document.get_object(mask_ref).unwrap().as_stream().unwrap();
        assert_eq!(name(mask.dict.get(b"ColorSpace").unwrap()), "DeviceGray");
        assert_eq!(mask.dict.get(b"Width").unwrap().as_i64().unwrap(), 2);
        assert!(!mask.dict.has(b"SMask"));
        assert_eq!(mask.decompressed_content().unwrap(), vec![255, 128, 0, 255]);
    }
}

#[test]
fn opaque_image_has_no_soft_mask() {
    for &mode in &MODES {
        let mut reference = None;
        let (_, document) = build_and_load(mode, |context| {
            let image = Image::from_rgba8(1, 1, &[1, 2, 3, 255]).unwrap();
            reference = Some(context.write_image(image).unwrap());
        });
        let image = document.get_object(id(reference.unwrap())).unwrap();
        let image = image.as_stream().unwrap();
        assert!(!image.dict.has(b"SMask"));
        assert_eq!(image.decompressed_content().unwrap(), vec![1, 2, 3]);
    }
}

#[test]
fn jpeg_is_embedded_unchanged() {
    // An Adobe CMYK JPEG header (enough for the dimensions, not decodable).
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xEE, 0x00, 0x0E];
    jpeg.extend_from_slice(b"Adobe\x00\x64\x00\x00\x00\x00\x02");
    jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x14, 8, 0x00, 0x03, 0x00, 0x05, 4]);
    jpeg.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 4, 0x11, 0]);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    for &mode in &MODES {
        let mut reference = None;
        let (_, document) = build_and_load(mode, |context| {
            let image = Image::from_jpeg(jpeg.clone()).unwrap();
            assert_eq!((image.width(), image.height()), (5, 3));
            let image = image.with_alpha(vec![7; 15]).unwrap();
            reference = Some(context.write_image(image).unwrap());
        });
        let image = document.get_object(id(reference.unwrap())).unwrap();
        let image = image.as_stream().unwrap();
        assert_eq!(name(image.dict.get(b"Filter").unwrap()), "DCTDecode");
        assert_eq!(name(image.dict.get(b"ColorSpace").unwrap()), "DeviceCMYK");
        assert_eq!(image.content, jpeg);
        let decode: Vec<f32> = image
            .dict
            .get(b"Decode")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_float().unwrap())
            .collect();
        assert_eq!(decode, vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
        assert!(image.dict.has(b"SMask"));
    }
}

#[test]
fn transparency_is_set_through_ext_gstate() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |page_context| {
                page_context.set_fill_alpha(0.5)?;
                page_context.set_stroke_alpha(0.25)?;
                // equal dictionaries share a resource name
                page_context.set_fill_alpha(0.5)?;
                page_context.set_blend_mode(BlendMode::Multiply)?;
                page_context.rect(Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0))?;
                page_context.fill_path()
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert_eq!(
            content.trim(),
            "/GS0 gs /GS1 gs /GS0 gs /GS2 gs 0 0 10 10 re f"
        );
        let states = resources.get(b"ExtGState").unwrap().as_dict().unwrap();
        assert_eq!(states.len(), 3);
        let state = |key: &[u8]| states.get(key).unwrap().as_dict().unwrap();
        assert_eq!(name(state(b"GS0").get(b"Type").unwrap()), "ExtGState");
        assert_eq!(state(b"GS0").get(b"ca").unwrap().as_float().unwrap(), 0.5);
        assert!(!state(b"GS0").has(b"CA"));
        assert_eq!(state(b"GS1").get(b"CA").unwrap().as_float().unwrap(), 0.25);
        assert_eq!(name(state(b"GS2").get(b"BM").unwrap()), "Multiply");
        assert!(!resources.has(b"Font"));
        assert!(!resources.has(b"XObject"));
    }
}

#[test]
fn ensure_alpha_skips_what_is_in_effect() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                // a page starts out opaque
                c.ensure_opaque()?;
                c.ensure_fill_alpha(0.5)?;
                c.ensure_fill_alpha(0.5)?;
                // `q` and `Q` save and restore the opacity
                c.save_graphics_state()?;
                c.ensure_fill_alpha(0.25)?;
                c.restore_graphics_state()?;
                c.ensure_fill_alpha(0.5)?;
                // a state applied some other way is followed too
                c.apply_ext_gstate(ExtGState::new().with_fill_alpha(1.0))?;
                c.ensure_fill_alpha(1.0)?;
                // what is not followed has to be forgotten
                c.forget_alpha();
                c.ensure_fill_alpha(1.0)?;
                c.ensure_stroke_alpha(1.0)?;
                c.ensure_opaque()
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let (_, content) = single_page(&document);
        assert_eq!(
            content.trim(),
            "/GS0 gs q /GS1 gs Q /GS2 gs /GS2 gs /GS3 gs"
        );
    }
}

#[test]
fn colors_are_written_in_the_space_that_holds_them() {
    use lemon_pdf::color::{ColorValue, RgbSpace};
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                c.set_srgb_device_defaults()?;
                c.set_fill_color(ColorValue::Srgb([1.0, 0.0, 0.5]))?;
                c.set_stroke_color(ColorValue::Rgb(RgbSpace::DisplayP3, [0.1, 0.2, 0.3]))?;
                // D65 white is neutral CIELAB
                c.set_fill_color(ColorValue::XyzD65([0.950_455, 1.0, 1.089_058]))?;
                // the green of Rec. 2020: a* is beyond -128, so it is in the Rec. 2020 space
                c.set_fill_color(ColorValue::XyzD65([0.1446, 0.678, 0.0281]))?;
                Ok(())
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let (resources, content) = single_page(&document);
        let spaces = resources.get(b"ColorSpace").unwrap().as_dict().unwrap();
        assert!(spaces.has(b"DefaultRGB"));
        assert!(content.contains("1 0 0.5 rg"), "{}", content);
        assert!(content.contains("0.1 0.2 0.3 SC"), "{}", content);
        assert!(content.contains("/CS2 cs 100 "), "{}", content);
        // the fourth space (after the default, P3 and Lab) is the one of Rec. 2020
        assert!(content.trim_end().ends_with("/CS3 cs 0 1 0.01216 sc"), "{}", content);
        assert!(spaces.has(b"CS3"));
    }
}

#[test]
fn shadings_and_patterns_are_resources() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let axial = AxialShading::new(
                Point::new(0.0, 0.0),
                Point::new(100.0, 0.0),
                &Gradient::new(&[
                    ColorStop::new(0.0, Rgb(1.0, 0.0, 0.0)),
                    ColorStop::new(0.5, Rgb(1.0, 1.0, 0.0)),
                    ColorStop::new(1.0, Rgb(0.0, 0.0, 1.0)),
                ])
                .unwrap()
                .extended(),
            );
            let circle = |radius| Circle {
                x: Pt(50.0),
                y: Pt(50.0),
                radius: Pt(radius),
            };
            let radial = RadialShading::new(
                circle(0.0),
                circle(50.0),
                &Gradient::new(&[ColorStop::new(0.0, Gray(1.0)), ColorStop::new(1.0, Gray(0.0))])
                    .unwrap(),
            );
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                let first = c.add_shading(axial.clone());
                let second = c.add_shading(radial);
                // an equal shading shares its resource name
                assert_eq!(c.add_shading(axial.clone()), first);
                c.save_graphics_state()?;
                c.rect(Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0))?;
                c.clip(FillRule::NonZero)?;
                c.end_path()?;
                c.paint_shading(&first)?;
                c.restore_graphics_state()?;
                c.paint_shading(&second)?;
                let pattern = c.add_shading_pattern(
                    ShadingPattern::new(axial)
                        .with_matrix(Matrix::translation(Pt(10.0), Pt(0.0))),
                );
                c.set_fill_pattern(&pattern)?;
                c.set_stroke_pattern(&pattern)
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert_eq!(
            content.trim(),
            "q 0 0 100 50 re W n /Sh0 sh Q /Sh1 sh /Pattern cs /P0 scn /Pattern CS /P0 SCN"
        );
        let shadings = resources.get(b"Shading").unwrap().as_dict().unwrap();
        assert_eq!(shadings.len(), 2);
        let axial = shadings.get(b"Sh0").unwrap().as_dict().unwrap();
        assert_eq!(axial.get(b"ShadingType").unwrap().as_i64().unwrap(), 2);
        assert_eq!(name(axial.get(b"ColorSpace").unwrap()), "DeviceRGB");
        assert_eq!(axial.get(b"Coords").unwrap().as_array().unwrap().len(), 4);
        let function = axial.get(b"Function").unwrap().as_dict().unwrap();
        assert_eq!(function.get(b"FunctionType").unwrap().as_i64().unwrap(), 3);
        assert_eq!(function.get(b"Functions").unwrap().as_array().unwrap().len(), 2);
        let radial = shadings.get(b"Sh1").unwrap().as_dict().unwrap();
        assert_eq!(radial.get(b"ShadingType").unwrap().as_i64().unwrap(), 3);
        assert_eq!(radial.get(b"Coords").unwrap().as_array().unwrap().len(), 6);

        let patterns = resources.get(b"Pattern").unwrap().as_dict().unwrap();
        let pattern = patterns.get(b"P0").unwrap().as_dict().unwrap();
        assert_eq!(name(pattern.get(b"Type").unwrap()), "Pattern");
        assert_eq!(pattern.get(b"PatternType").unwrap().as_i64().unwrap(), 2);
        assert!(pattern.get(b"Shading").unwrap().as_dict().is_ok());
        assert_eq!(pattern.get(b"Matrix").unwrap().as_array().unwrap().len(), 6);
    }
}

#[test]
fn mesh_shadings_are_streams_referenced_from_the_resources() {
    let red = |x: f64, y: f64| Vertex::new(x, y, Rgb(1.0, 0.0, 0.0));
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let triangles = context
                .write_mesh_shading(&FreeFormMesh::new([
                    red(0.0, 0.0),
                    red(10.0, 0.0),
                    red(0.0, 10.0),
                ]))
                .unwrap();
            let lattice = context
                .write_mesh_shading(
                    &LatticeMesh::new(vec![
                        vec![red(0.0, 0.0), red(1.0, 0.0)],
                        vec![red(0.0, 1.0), red(1.0, 1.0)],
                    ])
                    .unwrap(),
                )
                .unwrap();
            let mut points = [Point::default(); 12];
            for (i, point) in points.iter_mut().enumerate() {
                *point = Point::new(i as f64, 0.0);
            }
            let coons = context
                .write_mesh_shading(&CoonsMesh::new(Patch {
                    points,
                    colors: [Gray(0.0); 4],
                }))
                .unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                for shading in [triangles.clone(), lattice, coons] {
                    let key = c.add_shading(shading);
                    c.paint_shading(&key)?;
                }
                // an equal shading shares its name
                assert_eq!(c.add_shading(triangles.clone()), c.add_shading(triangles.clone()));
                Ok(())
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert_eq!(content.trim(), "/Sh0 sh /Sh1 sh /Sh2 sh");
        let shadings = resources.get(b"Shading").unwrap().as_dict().unwrap();
        assert_eq!(shadings.len(), 3);
        for (key, shading_type) in [(&b"Sh0"[..], 4), (b"Sh1", 5), (b"Sh2", 6)] {
            let id = shadings.get(key).unwrap().as_reference().unwrap();
            let Object::Stream(stream) = document.get_object(id).unwrap() else {
                panic!("a mesh shading is a stream")
            };
            assert_eq!(
                stream.dict.get(b"ShadingType").unwrap().as_i64().unwrap(),
                shading_type
            );
            assert_eq!(
                stream.dict.get(b"BitsPerCoordinate").unwrap().as_i64().unwrap(),
                32
            );
            assert!(stream.dict.has(b"Decode"));
            assert!(!stream.content.is_empty());
        }
    }
}

#[test]
fn soft_masks_are_forms_in_the_graphics_state() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            // a mask that fades from transparent to opaque
            let fade = Gradient::new(&[ColorStop::new(0.0, Gray(0.0)), ColorStop::new(1.0, Gray(1.0))])
                .unwrap()
                .extended();
            let shading = AxialShading::new(Point::new(0.0, 0.0), Point::new(100.0, 0.0), &fade);
            let mask = context
                .write_form(
                    [Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0)],
                    Some(TransparencyGroup::new(ColorSpace::DeviceGray)),
                    None,
                    |c| {
                        let key = c.add_shading(shading);
                        c.paint_shading(&key)
                    },
                )
                .unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                c.apply_ext_gstate(ExtGState::new().with_soft_mask(SoftMask::luminosity(mask)))?;
                c.rect(Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0))?;
                c.fill_path()?;
                c.apply_ext_gstate(ExtGState::new().with_soft_mask(SoftMask::None))
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert_eq!(content.split_whitespace().filter(|t| *t == "gs").count(), 2);
        let states = resources.get(b"ExtGState").unwrap().as_dict().unwrap();
        assert_eq!(states.len(), 2);
        let mut masks = 0;
        for (_, state) in states.iter() {
            let state = state.as_dict().unwrap();
            match state.get(b"SMask").unwrap() {
                Object::Name(name) => assert_eq!(name, b"None"),
                Object::Dictionary(mask) => {
                    masks += 1;
                    assert_eq!(mask.get(b"Type").unwrap().as_name().unwrap(), b"Mask");
                    assert_eq!(mask.get(b"S").unwrap().as_name().unwrap(), b"Luminosity");
                    let id = mask.get(b"G").unwrap().as_reference().unwrap();
                    let Object::Stream(form) = document.get_object(id).unwrap() else {
                        panic!("the group of a soft mask is a form")
                    };
                    assert_eq!(form.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
                    assert_eq!(form.dict.get(b"BBox").unwrap().as_array().unwrap().len(), 4);
                    let group = form.dict.get(b"Group").unwrap().as_dict().unwrap();
                    assert_eq!(group.get(b"S").unwrap().as_name().unwrap(), b"Transparency");
                    assert_eq!(group.get(b"CS").unwrap().as_name().unwrap(), b"DeviceGray");
                    // the form has resources of its own
                    let form_resources = form.dict.get(b"Resources").unwrap().as_dict().unwrap();
                    assert_eq!(form_resources.get(b"Shading").unwrap().as_dict().unwrap().len(), 1);
                    assert_eq!(String::from_utf8_lossy(&form.content).trim(), "/Sh0 sh");
                }
                other => panic!("unexpected soft mask {:?}", other),
            }
        }
        assert_eq!(masks, 1);
    }
}

#[test]
fn forms_can_be_painted_as_xobjects() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let form = context
                .write_form([Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0)], None, None, |c| {
                    c.rect(Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0))?;
                    c.fill_path()
                })
                .unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                let key = c.add_xobject(form);
                c.draw_xobject(&key)
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let (resources, content) = single_page(&document);
        assert_eq!(content.trim(), "/X0 Do");
        let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
        let id = xobjects.get(b"X0").unwrap().as_reference().unwrap();
        let Object::Stream(form) = document.get_object(id).unwrap() else { panic!() };
        assert_eq!(form.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
        assert!(!form.dict.has(b"Group"));
        assert!(!form.dict.has(b"Matrix"));
    }
}

#[test]
fn pages_without_shadings_have_no_shading_resources() {
    let (_, document) = build_and_load(true, |context| {
        let mut page = Page::new();
        page.add_content(context, None, |c| c.rect(Pt(0.0), Pt(0.0), Pt(1.0), Pt(1.0)))
            .unwrap();
        context.add_page(page).unwrap();
    });
    let (resources, _) = single_page(&document);
    assert!(!resources.has(b"Shading"));
    assert!(!resources.has(b"Pattern"));
}

#[test]
fn vector_path_operators_are_written() {
    let (_, document) = build_and_load(true, |context| {
        let mut page = Page::new();
        page.add_content(context, Some(StreamFilter::Deflate), |c| {
            c.line_width(Pt(2.0))?;
            c.line_cap(LineCap::Round)?;
            c.line_join(LineJoin::Bevel)?;
            c.miter_limit(4.0)?;
            c.dash_pattern(&[Pt(3.0), Pt(1.5)], Pt(0.0))?;
            c.device_gray_stroke_color(0.5)?;
            c.device_cmyk_fill_color(0.0, 1.0, 1.0, 0.0)?;
            c.move_to(Pt(0.0), Pt(0.0))?;
            c.curve_to(Pt(1.0), Pt(2.0), Pt(3.0), Pt(4.0), Pt(5.0), Pt(6.0))?;
            c.close_fill_and_stroke_path(FillRule::EvenOdd)?;
            c.rect(Pt(0.0), Pt(0.0), Pt(5.0), Pt(5.0))?;
            c.clip(FillRule::NonZero)?;
            c.end_path()?;
            c.dash_pattern(&[], Pt(0.0))
        })
        .unwrap();
        context.add_page(page).unwrap();
    });
    let (_, content) = single_page(&document);
    assert_eq!(
        content.trim(),
        "2 w 1 J 2 j 4 M [ 3 1.5 ] 0 d 0.5 G 0 1 1 0 k 0 0 m 1 2 3 4 5 6 c b* \
         0 0 5 5 re W n [ ] 0 d"
    );
}

#[test]
fn tiling_patterns_repeat_a_cell() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let pattern = context
                .write_tiling_pattern(
                    [Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0)],
                    Pt(12.0),
                    Pt(14.0),
                    lemon_pdf::content::Matrix::new(1.0, 0.0, 0.0, 1.0, 5.0, 6.0),
                    None,
                    |c| {
                        c.device_rgb_fill_color(1.0, 0.0, 0.0)?;
                        c.rect(Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0))?;
                        c.fill_path()
                    },
                )
                .unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |c| {
                let key = c.add_tiling_pattern(pattern);
                assert_eq!(c.add_tiling_pattern(pattern), key, "added twice");
                c.set_fill_pattern(&key)?;
                c.rect(Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0))?;
                c.fill_path()
            })
            .unwrap();
            context.add_page(page).unwrap();
        });

        let (resources, content) = single_page(&document);
        assert!(content.contains("/Pattern cs /P0 scn"), "{}", content);
        let patterns = resources.get(b"Pattern").unwrap().as_dict().unwrap();
        let id = patterns.get(b"P0").unwrap().as_reference().unwrap();
        let Object::Stream(pattern) = document.get_object(id).unwrap() else {
            panic!("a tiling pattern is a stream")
        };
        let dict = &pattern.dict;
        assert_eq!(dict.get(b"PatternType").unwrap().as_i64().unwrap(), 1);
        assert_eq!(dict.get(b"PaintType").unwrap().as_i64().unwrap(), 1);
        assert_eq!(dict.get(b"TilingType").unwrap().as_i64().unwrap(), 1);
        assert_eq!(dict.get(b"XStep").unwrap().as_float().unwrap(), 12.0);
        assert_eq!(dict.get(b"YStep").unwrap().as_float().unwrap(), 14.0);
        assert_eq!(dict.get(b"BBox").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(dict.get(b"Matrix").unwrap().as_array().unwrap().len(), 6);
        assert!(dict.has(b"Resources"));
        assert!(String::from_utf8_lossy(&pattern.content).contains("1 0 0 rg"));
    }
}

/// A PNG file of 3 x 2 palette pixels whose palette entry 1 is transparent.
fn small_palette_png() -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0; 4]);
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut png, b"IHDR", &[0, 0, 0, 3, 0, 0, 0, 2, 8, 3, 0, 0, 0]);
    chunk(&mut png, b"PLTE", &[255, 0, 0, 0, 255, 0]);
    chunk(&mut png, b"tRNS", &[255, 0]);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &[0, 0, 1, 0, 0, 1, 0, 1]).unwrap();
    chunk(&mut png, b"IDAT", &encoder.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    png
}

#[test]
fn png_pixel_data_is_embedded_unchanged() {
    let png = small_palette_png();
    for &mode in &MODES {
        let mut reference = None;
        let (_, document) = build_and_load(mode, |context| {
            let image = Image::from_png(&png).unwrap();
            assert_eq!((image.width(), image.height()), (3, 2));
            reference = Some(context.write_image(image).unwrap());
        });
        let image = document.get_object(id(reference.unwrap())).unwrap();
        let image = image.as_stream().unwrap();
        assert_eq!(name(image.dict.get(b"Filter").unwrap()), "FlateDecode");
        // `[/Indexed /DeviceRGB 1 <FF000000FF00>]`
        let space = image.dict.get(b"ColorSpace").unwrap().as_array().unwrap();
        assert_eq!(name(&space[0]), "Indexed");
        assert_eq!(name(&space[1]), "DeviceRGB");
        assert_eq!(space[2].as_i64().unwrap(), 1);
        assert_eq!(space[3].as_str().unwrap(), &[255, 0, 0, 0, 255, 0]);
        assert_eq!(image.dict.get(b"BitsPerComponent").unwrap().as_i64().unwrap(), 8);
        let parms = image.dict.get(b"DecodeParms").unwrap().as_dict().unwrap();
        assert_eq!(parms.get(b"Predictor").unwrap().as_i64().unwrap(), 15);
        assert_eq!(parms.get(b"Colors").unwrap().as_i64().unwrap(), 1);
        assert_eq!(parms.get(b"Columns").unwrap().as_i64().unwrap(), 3);
        let mask = image.dict.get(b"Mask").unwrap().as_array().unwrap();
        assert_eq!(
            mask.iter().map(|v| v.as_i64().unwrap()).collect::<Vec<_>>(),
            [1, 1]
        );
        // the stream is the zlib data of the PNG file, not recompressed
        let idat_start = png.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
        assert!(png[idat_start..].starts_with(&image.content));
    }
}

#[test]
fn outline_is_a_linked_tree() {
    use lemon_pdf::annotation::Destination;
    use lemon_pdf::outline::OutlineItem;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let pages: Vec<_> = (0..2).map(|_| context.create_reference()).collect();
            let dest = |page: usize| Destination { page: pages[page], point: (Pt(0.0), Pt(700.0)) };
            let mut chapter = OutlineItem::new("Chapter ä", dest(0));
            chapter.children = vec![OutlineItem::new("Section 1", dest(0)), OutlineItem::new("Section 2", dest(1))];
            let mut closed = OutlineItem::new("Appendix", dest(1));
            closed.open = false;
            closed.children = vec![OutlineItem::new("A", dest(1))];
            context.set_outlines(&[chapter, closed]).unwrap();
            for (i, reference) in pages.into_iter().enumerate() {
                let mut page = Page::new();
                page.set_media_box(lemon_pdf::pagetree::MediaBox::new(Pt(0.0), Pt(0.0), Pt(595.0), Pt(842.0)));
                let _ = i;
                context.add_reserved_page(reference, page).unwrap();
            }
        });
        let root = document.catalog().unwrap().get(b"Outlines").unwrap();
        let root = document.get_dictionary(root.as_reference().unwrap()).unwrap();
        // chapter, 2 sections, appendix (closed: its child is hidden)
        assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 4);
        let first = document.get_dictionary(root.get(b"First").unwrap().as_reference().unwrap()).unwrap();
        assert_eq!(first.get(b"Count").unwrap().as_i64().unwrap(), 2);
        assert_eq!(first.get(b"Dest").unwrap().as_array().unwrap().len(), 5);
        let next = document.get_dictionary(first.get(b"Next").unwrap().as_reference().unwrap()).unwrap();
        assert_eq!(next.get(b"Count").unwrap().as_i64().unwrap(), -1);
        assert_eq!(root.get(b"Last").unwrap(), &Object::Reference(first.get(b"Next").unwrap().as_reference().unwrap()));
        let section = document.get_dictionary(first.get(b"First").unwrap().as_reference().unwrap()).unwrap();
        assert!(section.get(b"Prev").is_err());
        assert!(section.get(b"Next").is_ok());
    }
}

#[test]
fn page_mode_is_written_to_the_catalog() {
    use lemon_pdf::PageMode;
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| context.set_page_mode(PageMode::UseOutlines));
        let page_mode = document.catalog().unwrap().get(b"PageMode").unwrap();
        assert_eq!(page_mode.as_name().unwrap(), b"UseOutlines");
        let (_, document) = build_and_load(mode, |context| context.set_page_mode(PageMode::UseThumbs));
        assert_eq!(document.catalog().unwrap().get(b"PageMode").unwrap().as_name().unwrap(), b"UseThumbs");
        // a variant that is written under another name than its own (`#[rename]`)
        let (_, document) = build_and_load(mode, |context| context.set_page_mode(PageMode::UseOc));
        assert_eq!(document.catalog().unwrap().get(b"PageMode").unwrap().as_name().unwrap(), b"UseOC");
    }
}

#[test]
fn named_destinations_are_a_sorted_name_tree() {
    use lemon_pdf::annotation::Destination;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let page = context.create_reference();
            let at = |y: f64| Destination { page, point: (Pt(0.0), Pt(y)) };
            // given out of order, and `b` twice: the last one counts
            context.set_named_destinations(vec![
                (b"b".to_vec(), at(1.0)),
                (b"a".to_vec(), at(2.0)),
                (b"b".to_vec(), at(3.0)),
            ]);
            context.add_reserved_page(page, Page::new()).unwrap();
        });
        let names = document.catalog().unwrap().get(b"Names").unwrap();
        let names = document.dereference(names).unwrap().1.as_dict().unwrap();
        let dests = document.dereference(names.get(b"Dests").unwrap()).unwrap().1.as_dict().unwrap();
        let entries = dests.get(b"Names").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].as_str().unwrap(), b"a");
        assert_eq!(entries[2].as_str().unwrap(), b"b");
        let destination = entries[3].as_array().unwrap();
        assert_eq!(destination[1].as_name().unwrap(), b"XYZ");
        assert_eq!(destination[3].as_float().unwrap(), 3.0);
    }
}

#[test]
fn page_labels_are_a_number_tree_with_a_first_range() {
    use lemon_pdf::page_labels::{PageLabel, PageLabelStyle};

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            for i in 0..6 {
                add_text_page(context, &format!("Page {}", i + 1), None);
            }
            // no range at page 0: a decimal one is added
            context.set_page_labels(vec![
                (2, PageLabel::numbered(PageLabelStyle::LowerRoman, 3)),
                (4, PageLabel::numbered(PageLabelStyle::UpperLetters, 1).with_prefix("A-")),
            ]);
        });
        let labels = document.catalog().unwrap().get(b"PageLabels").unwrap();
        let labels = document.dereference(labels).unwrap().1.as_dict().unwrap();
        let nums = labels.get(b"Nums").unwrap().as_array().unwrap();
        assert_eq!(nums.len(), 6);
        assert_eq!(nums[0].as_i64().unwrap(), 0);
        assert_eq!(nums[1].as_dict().unwrap().get(b"S").unwrap().as_name().unwrap(), b"D");
        assert_eq!(nums[2].as_i64().unwrap(), 2);
        let roman = nums[3].as_dict().unwrap();
        assert_eq!(roman.get(b"S").unwrap().as_name().unwrap(), b"r");
        assert_eq!(roman.get(b"St").unwrap().as_i64().unwrap(), 3);
        assert!(roman.get(b"P").is_err());
        let appendix = nums[5].as_dict().unwrap();
        assert_eq!(appendix.get(b"S").unwrap().as_name().unwrap(), b"A");
        assert_eq!(appendix.get(b"P").unwrap().as_str().unwrap(), b"A-");
    }
}

#[test]
fn viewer_preferences_are_written_entry_by_entry() {
    use lemon_pdf::viewer_preferences::{Direction, Duplex, PrintScaling, ViewerPreferences};

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            context.set_display_doc_title(true);
            // the title preference stays when another entry is set
            context.viewer_preferences_mut().print_scaling = Some(PrintScaling::None);
            context.viewer_preferences_mut().direction = Some(Direction::R2L);
            context.viewer_preferences_mut().duplex = Some(Duplex::DuplexFlipLongEdge);
            context.viewer_preferences_mut().print_page_range = Some(vec![1, 3, 5, 6]);
            context.viewer_preferences_mut().num_copies = Some(2);
            context.viewer_preferences_mut().pick_tray_by_pdf_size = Some(true);
            context.viewer_preferences_mut().hide_window_ui = Some(false);
        });
        let preferences = document.catalog().unwrap().get(b"ViewerPreferences").unwrap();
        let preferences = document.dereference(preferences).unwrap().1.as_dict().unwrap();
        assert!(preferences.get(b"DisplayDocTitle").unwrap().as_bool().unwrap());
        assert_eq!(preferences.get(b"PrintScaling").unwrap().as_name().unwrap(), b"None");
        assert_eq!(preferences.get(b"Direction").unwrap().as_name().unwrap(), b"R2L");
        assert_eq!(preferences.get(b"Duplex").unwrap().as_name().unwrap(), b"DuplexFlipLongEdge");
        assert_eq!(preferences.get(b"PrintPageRange").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(preferences.get(b"NumCopies").unwrap().as_i64().unwrap(), 2);
        assert!(preferences.get(b"PickTrayByPDFSize").unwrap().as_bool().unwrap());
        assert!(!preferences.get(b"HideWindowUI").unwrap().as_bool().unwrap());
        // entries that are not set are not written
        assert!(preferences.get(b"HideToolbar").is_err());
        assert!(preferences.get(b"ViewArea").is_err());

        let (_, document) = build_and_load(mode, |context| {
            context.set_viewer_preferences(ViewerPreferences { fit_window: Some(true), ..Default::default() })
        });
        let preferences = document.catalog().unwrap().get(b"ViewerPreferences").unwrap();
        let preferences = document.dereference(preferences).unwrap().1.as_dict().unwrap();
        assert!(preferences.get(b"FitWindow").unwrap().as_bool().unwrap());
        assert!(preferences.get(b"DisplayDocTitle").is_err());
    }
}

#[test]
fn output_intents_embed_their_profile() {
    use lemon_pdf::output_intent::{OutputIntentDesc, OutputIntentSubtype};

    for &mode in &MODES {
        for &filter in &[None, Some(StreamFilter::Deflate)] {
            let (_, document) = build_and_load(mode, |context| {
                context
                    .add_output_intent(
                        OutputIntentDesc {
                            subtype: OutputIntentSubtype::PdfA,
                            output_condition_identifier: "sRGB IEC61966-2.1".to_owned(),
                            info: Some("sRGB".to_owned()),
                            profile: lemon_pdf::icc::srgb_profile(),
                            components: 3,
                        },
                        filter,
                    )
                    .unwrap();
            });
            let intents = document.catalog().unwrap().get(b"OutputIntents").unwrap();
            let intents = document.dereference(intents).unwrap().1.as_array().unwrap();
            assert_eq!(intents.len(), 1);
            let intent = document.dereference(&intents[0]).unwrap().1.as_dict().unwrap();
            assert_eq!(intent.get(b"S").unwrap().as_name().unwrap(), b"GTS_PDFA1");
            assert_eq!(intent.get(b"Type").unwrap().as_name().unwrap(), b"OutputIntent");
            let profile = document.dereference(intent.get(b"DestOutputProfile").unwrap()).unwrap().1;
            let profile = profile.as_stream().unwrap();
            assert_eq!(profile.dict.get(b"N").unwrap().as_i64().unwrap(), 3);
            let content = profile.decompressed_content().unwrap_or_else(|_| profile.content.clone());
            assert_eq!(content, lemon_pdf::icc::srgb_profile());
        }
    }
}

#[test]
fn link_annotations_are_printed() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            let link = LinkAnnotation::new(
                [Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0)],
                LinkAction::Uri(UriAction { uri: ByteString::from("https://example.com") }),
            );
            page.add_annotation(context, Annotation::Link(link)).unwrap();
            context.add_page(page).unwrap();
        });
        let page = document.get_pages().values().next().copied().unwrap();
        let annotations = document.get_page_annotations(page).unwrap();
        assert_eq!(annotations.len(), 1);
        // the print flag, and none of the flags that PDF/A forbids
        assert_eq!(annotations[0].get(b"F").unwrap().as_i64().unwrap(), 4);
    }
}

#[test]
fn lab_colors_use_a_named_color_space() {
    use lemon_pdf::color_space::LabColorSpace;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |page| {
                let key = page.add_color_space(LabColorSpace::d50());
                // the same space again has the same name
                assert_eq!(page.add_color_space(LabColorSpace::d50()), key);
                page.lab_fill_color(&key, 54.0, 106.5, 40.0)?;
                page.lab_stroke_color(&key, 10.0, -20.0, 30.0)?;
                Ok(())
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let page = document.get_pages().values().next().copied().unwrap();
        let content = String::from_utf8(document.get_page_content(page)).unwrap();
        let content: Vec<&str> = content.split_whitespace().collect();
        let operations = content.join(" ");
        assert!(operations.contains("/CS0 cs 54 106.5 40 sc"), "{operations}");
        assert!(operations.contains("/CS0 CS 10 -20 30 SC"), "{operations}");
        let (resources, _) = document.get_page_resources(page).unwrap();
        let spaces = resources.unwrap().get(b"ColorSpace").unwrap().as_dict().unwrap();
        let space = spaces.get(b"CS0").unwrap().as_array().unwrap();
        assert_eq!(space[0].as_name().unwrap(), b"Lab");
        let parameters = space[1].as_dict().unwrap();
        assert_eq!(parameters.get(b"WhitePoint").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(parameters.get(b"Range").unwrap().as_array().unwrap().len(), 4);
    }
}

#[test]
fn color_spaces_of_every_kind_are_resources() {
    use lemon_pdf::color_space::{CalRgbColorSpace, CalRgbDict, IccBasedColorSpace, LabColorSpace};
    use lemon_pdf::object::Precise;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let profile = context.write_icc_profile(lemon_pdf::icc::display_p3_profile(), 3, Some(StreamFilter::Deflate)).unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |page| {
                let lab = page.add_color_space(LabColorSpace::d50());
                let cal = page.add_color_space(CalRgbColorSpace(CalRgbDict {
                    white_point: [0.9505, 1.0, 1.089].map(Precise),
                    black_point: None,
                    gamma: Some([2.2, 2.2, 2.2].map(Precise)),
                    matrix: Some(
                        [0.4124, 0.2126, 0.0193, 0.3576, 0.7152, 0.1192, 0.1805, 0.0722, 0.9505]
                            .map(Precise),
                    ),
                }));
                let icc = page.add_color_space(IccBasedColorSpace(profile));
                assert_eq!([lab.clone(), cal.clone(), icc.clone()].len(), 3);
                assert_ne!(lab, cal);
                assert_ne!(cal, icc);
                page.set_fill_color_components(&icc, &[1.0, 0.25, 0.0])?;
                page.set_stroke_color_components(&cal, &[0.5, 0.5, 0.5])?;
                Ok(())
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let page = document.get_pages().values().next().copied().unwrap();
        let content = String::from_utf8(document.get_page_content(page)).unwrap();
        let content = content.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(content.contains("/CS2 cs 1 0.25 0 sc"), "{}", content);
        assert!(content.contains("/CS1 CS 0.5 0.5 0.5 SC"), "{}", content);
        let (resources, _) = document.get_page_resources(page).unwrap();
        let spaces = resources.unwrap().get(b"ColorSpace").unwrap().as_dict().unwrap();
        assert_eq!(spaces.get(b"CS0").unwrap().as_array().unwrap()[0].as_name().unwrap(), b"Lab");
        let cal = spaces.get(b"CS1").unwrap().as_array().unwrap();
        assert_eq!(cal[0].as_name().unwrap(), b"CalRGB");
        assert_eq!(cal[1].as_dict().unwrap().get(b"Matrix").unwrap().as_array().unwrap().len(), 9);
        let icc = spaces.get(b"CS2").unwrap().as_array().unwrap();
        assert_eq!(icc[0].as_name().unwrap(), b"ICCBased");
        let stream = document.dereference(&icc[1]).unwrap().1.as_stream().unwrap();
        assert_eq!(stream.dict.get(b"N").unwrap().as_i64().unwrap(), 3);
        let data = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
        assert_eq!(data, lemon_pdf::icc::display_p3_profile());
    }
}

#[test]
fn the_rendering_intent_is_an_operator() {
    use lemon_pdf::graphics_state::RenderingIntent;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |page| page.set_rendering_intent(RenderingIntent::Perceptual)).unwrap();
            context.add_page(page).unwrap();
        });
        let page = document.get_pages().values().next().copied().unwrap();
        let content = String::from_utf8(document.get_page_content(page)).unwrap();
        assert!(content.split_whitespace().collect::<Vec<_>>().join(" ").contains("/Perceptual ri"), "{}", content);
    }
}

#[test]
fn an_icc_profile_is_written_once_for_all_pages() {
    for &mode in &MODES {
        let (bytes, document) = build_and_load(mode, |context| {
            for _ in 0..3 {
                let space = context.icc_color_space("p3", lemon_pdf::icc::display_p3_profile, 3, None).unwrap();
                let mut page = Page::new();
                page.add_content(context, None, |page| {
                    let name = page.add_color_space(space);
                    page.set_fill_color_components(&name, &[1.0, 0.0, 0.0])
                })
                .unwrap();
                context.add_page(page).unwrap();
            }
        });
        assert_eq!(document.get_pages().len(), 3);
        // one profile (its ICC signature is in the file once: the stream is not compressed)
        assert_eq!(bytes.windows(4).filter(|window| *window == b"acsp").count(), 1, "mode object_streams={mode}");
    }
}

#[test]
fn colors_are_written_with_more_than_two_decimals() {
    use lemon_pdf::color_space::{CalRgbColorSpace, CalRgbDict};
    use lemon_pdf::object::Precise;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let mut page = Page::new();
            page.add_content(context, None, |page| {
                // 187 / 255 would be 0.73 with two decimals, which is a different byte
                page.device_rgb_fill_color(187.0 / 255.0, 0.2, 1.0)?;
                page.device_gray_stroke_color(1.0 / 3.0)?;
                let space = page.add_color_space(CalRgbColorSpace(CalRgbDict {
                    white_point: [0.950_455_927, 1.0, 1.089_057_751].map(Precise),
                    black_point: None,
                    gamma: Some([1.0; 3].map(Precise)),
                    matrix: Some(
                        [0.486_570_949, 0.228_974_564, 0.0, 0.265_667_693, 0.691_738_522, 0.045_113_382, 0.198_217_285, 0.079_286_914, 1.043_944_369]
                            .map(Precise),
                    ),
                }));
                page.set_fill_color_components(&space, &[0.123_456_789, 0.5, 0.000_001_4])
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let page = document.get_pages().values().next().copied().unwrap();
        let content = String::from_utf8(document.get_page_content(page)).unwrap();
        let content = content.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(content.contains("0.733333 0.2 1 rg"), "{}", content);
        assert!(content.contains("0.333333 G"), "{}", content);
        assert!(content.contains("0.123457 0.5 0.000001 sc"), "{}", content);
        let (resources, _) = document.get_page_resources(page).unwrap();
        let spaces = resources.unwrap().get(b"ColorSpace").unwrap().as_dict().unwrap();
        let cal = spaces.get(b"CS0").unwrap().as_array().unwrap()[1].as_dict().unwrap();
        let matrix: Vec<f64> = cal.get(b"Matrix").unwrap().as_array().unwrap().iter().map(|v| v.as_float().unwrap() as f64).collect();
        assert!((matrix[0] - 0.486_571).abs() < 1e-6 && (matrix[7] - 0.079_287).abs() < 1e-6, "{matrix:?}");
    }
}

#[test]
fn the_defaults_of_the_colour_spaces_are_the_ones_that_were_measured() {
    use lemon_pdf::color_space::{CalRgbColorSpace, LabColorSpace, D50_WHITE_POINT};
    use lemon_pdf::object::Precise;

    // Lab: +-128 is right on an iPhone, +-256 shows a* and b* at half
    assert_eq!(LabColorSpace::d50().0.range, Some([-128.0, 128.0, -128.0, 128.0].map(Precise)));
    // the XYZ carrier: the components (1, 1, 1) are the white point, whatever the white point is
    for white in [D50_WHITE_POINT, [0.950_456, 1.0, 1.089_058]] {
        let space = CalRgbColorSpace::xyz_carrier(white);
        let m = space.0.matrix.unwrap().map(|value| value.0);
        assert_eq!(space.0.gamma, Some([1.0; 3].map(Precise)));
        // the columns of the matrix add up to the white point
        assert_eq!([m[0] + m[3] + m[6], m[1] + m[4] + m[7], m[2] + m[5] + m[8]], white);
    }
    // the gamma 2.4 profile of Rec. 2020 is a profile like the other ones
    let profile = lemon_pdf::icc::rec2020_gamma24_profile();
    assert_eq!(&profile[36..40], b"acsp");
    assert!(profile.len() < 700);
}

#[test]
fn a_default_colour_space_is_in_the_resources_of_the_page() {
    use lemon_pdf::color_space::DefaultColorSpace;

    for &mode in &MODES {
        let (bytes, document) = build_and_load(mode, |context| {
            for _ in 0..2 {
                let srgb = context.icc_color_space("srgb", lemon_pdf::icc::srgb_profile, 3, None).unwrap();
                let mut page = Page::new();
                page.add_content(context, None, |page| {
                    page.set_default_color_space(DefaultColorSpace::Rgb, srgb);
                    // setting it again replaces it, and the names of other spaces stay distinct
                    page.set_default_color_space(DefaultColorSpace::Rgb, srgb);
                    let p3 = page.pdf_context.icc_color_space("p3", lemon_pdf::icc::display_p3_profile, 3, None).unwrap();
                    let name = page.add_color_space(p3);
                    page.set_fill_color_components(&name, &[1.0, 0.0, 0.0])?;
                    // the same space as the default one is still a space of its own, not the default
                    let same = page.add_color_space(srgb);
                    assert_ne!(same.as_str(), "DefaultRGB");
                    page.set_fill_color_components(&same, &[0.2, 0.4, 0.6])?;
                    page.device_rgb_fill_color(0.2, 0.4, 0.6)
                })
                .unwrap();
                context.add_page(page).unwrap();
            }
        });
        for page in document.get_pages().values() {
            let (resources, _) = document.get_page_resources(*page).unwrap();
            let spaces = document.dereference(resources.unwrap().get(b"ColorSpace").unwrap()).unwrap().1.as_dict().unwrap();
            let default = spaces.get(b"DefaultRGB").expect("DefaultRGB");
            let array = document.dereference(default).unwrap().1.as_array().unwrap();
            assert_eq!(array[0].as_name().unwrap(), b"ICCBased");
            assert_eq!(spaces.len(), 3, "DefaultRGB, the space of the red, and the sRGB one that is an ordinary space");
        }
        // the sRGB profile is in the file once
        assert_eq!(bytes.windows(4).filter(|window| *window == b"acsp").count(), 2, "mode object_streams={mode}");
    }
}

#[test]
fn an_image_can_have_an_icc_profile() {
    use lemon_pdf::image::Image;

    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| {
            let p3 = context.icc_color_space("p3", lemon_pdf::icc::display_p3_profile, 3, None).unwrap();
            let image = Image::from_rgb8(1, 1, vec![255, 0, 0]).unwrap().with_icc_profile(p3);
            let reference = context.write_image(image).unwrap();
            let mut page = Page::new();
            page.add_content(context, None, |page| {
                let name = page.add_xobject(reference);
                page.draw_image(&name, Pt(0.0), Pt(0.0), Pt(10.0), Pt(10.0))
            })
            .unwrap();
            context.add_page(page).unwrap();
        });
        let image = document.objects.values().find_map(|o| o.as_stream().ok().filter(|s| s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()) == Some(b"Image".as_slice()))).unwrap();
        let space = image.dict.get(b"ColorSpace").unwrap();
        let space = document.dereference(space).unwrap().1.as_array().unwrap();
        assert_eq!(space[0].as_name().unwrap(), b"ICCBased");
    }
}

#[test]
fn the_header_of_an_icc_profile_is_checked() {
    use lemon_pdf::icc::{inspect_profile, srgb_profile, display_p3_profile};

    assert_eq!(inspect_profile(&srgb_profile()).unwrap().components, 3);
    assert_eq!(inspect_profile(&display_p3_profile()).unwrap().major_version, 4);
    assert!(inspect_profile(&[0; 10]).is_err());
    let mut bad = srgb_profile();
    bad[37] = b'x';
    assert!(inspect_profile(&bad).is_err());
    let mut truncated = srgb_profile();
    truncated.pop();
    assert!(inspect_profile(&truncated).is_err());
}
