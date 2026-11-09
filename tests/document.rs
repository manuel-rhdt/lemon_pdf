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
use lemon_pdf::font::encoding::{EncodingEntry, PredefinedEncoding};
use lemon_pdf::font::Font;
use lemon_pdf::object::TextString;
use lemon_pdf::stream::{Stream, StreamFilter};
use lemon_pdf::structure_tree::{
    NumberTree, StructElem, StructParent, StructTreeRoot, StructType,
};
use lemon_pdf::{DocumentContext, Page, Pt, Version};

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
    let big = vec![b'x'; 300 * 1024];
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
    }
}

#[test]
fn untagged_catalog_has_no_optional_entries() {
    for &mode in &MODES {
        let (_, document) = build_and_load(mode, |context| add_text_page(context, "x", None));
        let catalog = document.catalog().unwrap();
        for key in &[&b"StructTreeRoot"[..], b"MarkInfo", b"Lang", b"ViewerPreferences"] {
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
                    uri: b"https://example.com/a(b)".to_vec(),
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
            let mut stream = Stream::with_bytes(b"data".to_vec(), None);
            stream.add_key_value("My Key".to_owned(), Box::new("a/b#c (d)"));
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
