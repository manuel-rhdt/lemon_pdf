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

use std::collections::{BTreeMap, HashSet};
use std::io::prelude::*;
use std::io::Error;

use lemon_pdf_derive::{PdfDictionary, PdfFormat};
use crate as lemon_pdf;

use crate::crossref::CrossRef;
use crate::object::{Formatter, IndirectObject, IndirectReference, RawIndirectReference, TextString};
use crate::pagetree::{Page, Pages};
use crate::stream::Stream;
use crate::{structure_tree::StructTreeRoot, trailer::Trailer};
use log::error;

/// The document information dictionary, like `Title`, `Author` or `Subject`.
pub type DocumentInfo = BTreeMap<String, TextString>;

#[derive(Debug, Copy, Clone)]
pub enum Version {
    Pdf1_7,
}

impl Version {
    fn header(self) -> &'static str {
        match self {
            Version::Pdf1_7 => "%PDF-1.7",
        }
    }
}

#[derive(Debug)]
pub(crate) struct OffsetTrackingWriter<T> {
    offset: u64,
    inner: T,
}

impl<T> OffsetTrackingWriter<T> {
    pub fn new(writer: T) -> Self {
        OffsetTrackingWriter {
            inner: writer,
            offset: 0,
        }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    #[allow(unused)]
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Write for OffsetTrackingWriter<T>
where
    T: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        match self.inner.write(buf) {
            Ok(size) => {
                self.offset += size as u64;
                Ok(size)
            }
            Err(err) => Err(err),
        }
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        self.inner
            .write_all(buf)
            .map(|_| self.offset += buf.len() as u64)
    }
}

#[derive(Debug, PdfFormat)]
#[rename("Catalog")]
pub(crate) struct DocumentCatalog {
    pages: IndirectReference<Pages>,
    struct_tree_root: Option<IndirectReference<StructTreeRoot>>,
    mark_info: Option<MarkInfo>,
    lang: Option<TextString>,
    viewer_preferences: Option<ViewerPreferences>,
}

/// The dictionary of an object stream (ISO 32000-1, 7.5.7).
#[derive(Debug, PdfDictionary)]
#[rename("ObjStm")]
struct ObjectStreamDict {
    /// The number of objects in the stream.
    n: usize,
    /// The offset of the first object in the decoded data.
    first: usize,
}

/// Whether the document is a tagged PDF.
#[derive(Debug, PdfFormat, Default)]
#[omit_type(true)]
struct MarkInfo {
    marked: bool,
}

#[derive(Debug, PdfFormat, Default)]
#[omit_type(true)]
struct ViewerPreferences {
    /// Show the `Title` of the document information dictionary in the title bar of the viewer
    /// instead of the file name.
    display_doc_title: bool,
}

pub struct DocumentContext<'a> {
    pub(crate) output: OffsetTrackingWriter<Box<dyn Write + 'a>>,
    pub version: Version,
    pub(crate) crossref: CrossRef,
    document_catalog: Option<DocumentCatalog>,
    /// The root of the page tree. It is written when the document is finished, to the reference
    /// in the document catalog.
    page_tree: Pages,
    /// If this hash map is not empty, a document information dictionary with the corresponding
    /// entries will be created.
    pub document_info: DocumentInfo,
    dangling_references: HashSet<RawIndirectReference>,
    /// Store small non-stream objects in compressed object streams and write a
    /// cross-reference stream instead of a cross-reference table. Enabled by default.
    pub object_streams: bool,
    /// Objects (number, serialized bytes) waiting to be written into an object stream.
    pending_objects: Vec<(u32, Vec<u8>)>,
    pending_bytes: usize,
}

/// Flush an object stream once it holds this many objects or bytes.
const OBJECT_STREAM_MAX_OBJECTS: usize = 200;
const OBJECT_STREAM_MAX_BYTES: usize = 256 * 1024;

impl<'a> std::fmt::Debug for DocumentContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentContext")
            .field("version", &self.version)
            .field("crossref", &self.crossref)
            .field("document_catalog", &self.document_catalog)
            .field("page_tree", &self.page_tree)
            .field("document_info", &self.document_info)
            .finish()
    }
}

impl<'a> DocumentContext<'a> {
    pub fn with_writer<W: 'a + Write>(writer: W, version: Version) -> Result<Self, Error> {
        let mut context = DocumentContext {
            output: OffsetTrackingWriter::new(Box::new(writer)),
            version,
            crossref: Default::default(),
            document_catalog: None,
            page_tree: Pages::new(),
            document_info: Default::default(),
            dangling_references: Default::default(),
            object_streams: true,
            pending_objects: Vec::new(),
            pending_bytes: 0,
        };
        context.document_catalog = Some(DocumentCatalog {
            pages: context.create_reference(),
            struct_tree_root: None,
            mark_info: None,
            lang: None,
            viewer_preferences: None,
        });
        context.start_pdf()?;
        Ok(context)
    }

    fn start_pdf(&mut self) -> Result<(), Error> {
        writeln!(self.output, "{}", self.version.header())?;
        self.output.write_all(b"%\xff\xff\xff\xff\n")?;
        Ok(())
    }

    /// Creates a reference for an object that is written later with `write_reserved_object`, so
    /// that other objects can refer to it before it exists.
    pub fn create_reference<T>(&mut self) -> IndirectReference<T> {
        let num = self.crossref.add_entry(0, 0);
        let reference = IndirectReference::new(i64::from(num), 0);
        self.dangling_references.insert(reference.raw());
        reference
    }

    /// Write an indirect object to the context and return an indirect reference to it.
    pub fn write_object<T: IndirectObject>(
        &mut self,
        object: T,
    ) -> Result<IndirectReference<T>, Error> {
        self.write_object_fn(|_, _| Ok(object))
    }

    pub fn write_object_fn<T: IndirectObject>(
        &mut self,
        fun: impl FnOnce(&mut Self, IndirectReference<T>) -> Result<T, Error>,
    ) -> Result<IndirectReference<T>, Error> {
        // add placeholder entry to crossref
        let num = self.crossref.add_entry(0, 0);
        let reference = IndirectReference::new(i64::from(num), 0);
        let object = fun(self, reference)?;

        self.write_indirect_object(object, reference.raw())?;
        Ok(reference)
    }

    fn write_indirect_object<T: IndirectObject>(
        &mut self,
        object: T,
        reference: RawIndirectReference,
    ) -> Result<(), Error> {
        let mut bytes = Vec::new();
        object.write_indirect(&mut Formatter { writer: &mut bytes })?;

        if self.object_streams && reference.1 == 0 && !T::IS_STREAM {
            self.pending_bytes += bytes.len();
            self.pending_objects.push((reference.0 as u32, bytes));
            if self.pending_objects.len() >= OBJECT_STREAM_MAX_OBJECTS
                || self.pending_bytes >= OBJECT_STREAM_MAX_BYTES
            {
                self.flush_object_stream()?;
            }
            return Ok(());
        }

        // update placeholder entry in crossref
        self.crossref.get_entry_mut(reference.0 as u32).0 = self.output.offset();
        writeln!(self.output, "{} {} obj", reference.0, reference.1)?;
        self.output.write_all(&bytes)?;
        write!(self.output, "\nendobj\n")?;
        Ok(())
    }

    /// Writes all pending objects into one compressed object stream.
    fn flush_object_stream(&mut self) -> Result<(), Error> {
        if self.pending_objects.is_empty() {
            return Ok(());
        }
        let objects = std::mem::take(&mut self.pending_objects);
        self.pending_bytes = 0;

        let mut header = String::new();
        let mut body = Vec::new();
        for (num, bytes) in &objects {
            header.push_str(&format!("{} {} ", num, body.len()));
            body.extend_from_slice(bytes);
            body.push(b'\n');
        }
        let dict = ObjectStreamDict {
            n: objects.len(),
            first: header.len(),
        };
        let mut data = header.into_bytes();
        data.extend_from_slice(&body);
        let stream = Stream::deflated(dict, &data)?;

        let stream_ref = self.write_object(stream)?;
        for (index, (num, _)) in objects.iter().enumerate() {
            self.crossref
                .set_compressed(*num, stream_ref.number() as u32, index as u32);
        }
        Ok(())
    }

    /// Writes a page and adds it to the page tree of the document.
    pub fn add_page(&mut self, page: Page) -> Result<IndirectReference<Page>, Error> {
        let reference = self.create_reference();
        self.add_reserved_page(reference, page)?;
        Ok(reference)
    }

    /// Like `add_page`, but writes the page to a reference from `create_reference`, for when
    /// other objects have to refer to the page before it is finished.
    pub fn add_reserved_page(
        &mut self,
        reference: IndirectReference<Page>,
        mut page: Page,
    ) -> Result<(), Error> {
        page.set_parent(self.page_tree_reference());
        self.write_reserved_object(reference, page)?;
        self.page_tree.kids.push(reference.into());
        self.page_tree.count += 1;
        Ok(())
    }

    fn page_tree_reference(&self) -> IndirectReference<Pages> {
        self.document_catalog
            .as_ref()
            .expect("No document catalog")
            .pages
    }

    /// Writes an object to a reference obtained from `create_reference`.
    pub fn write_reserved_object<T: IndirectObject>(
        &mut self,
        reference: IndirectReference<T>,
        object: T,
    ) -> Result<(), Error> {
        self.dangling_references.remove(&reference.raw());
        self.write_indirect_object(object, reference.raw())
    }

    /// Sets the root of the structure tree (`/StructTreeRoot`) of the document catalog.
    pub fn set_struct_tree_root(&mut self, root: IndirectReference<StructTreeRoot>) {
        if let Some(catalog) = self.document_catalog.as_mut() {
            catalog.struct_tree_root = Some(root);
        }
    }

    /// Sets whether the document is a tagged PDF (`/MarkInfo << /Marked true >>`).
    pub fn set_marked(&mut self, marked: bool) {
        if let Some(catalog) = self.document_catalog.as_mut() {
            catalog.mark_info = Some(MarkInfo { marked });
        }
    }

    /// Sets the natural language of the document (`/Lang`), a BCP 47 language tag.
    pub fn set_lang(&mut self, lang: &str) {
        if let Some(catalog) = self.document_catalog.as_mut() {
            catalog.lang = Some(TextString(lang.to_owned()));
        }
    }

    /// Makes viewers show the `Title` of the document information instead of the file name.
    pub fn set_display_doc_title(&mut self, display: bool) {
        if let Some(catalog) = self.document_catalog.as_mut() {
            catalog.viewer_preferences = Some(ViewerPreferences {
                display_doc_title: display,
            });
        }
    }

    fn write_trailer(
        &mut self,
        crossref_offset: u32,
        document_catalog: IndirectReference<DocumentCatalog>,
        document_info: Option<IndirectReference<DocumentInfo>>,
    ) -> Result<(), Error> {
        let mut trailer = Trailer {
            crossref_offset,
            document_catalog,
            document_info,
        };
        trailer.write(self)?;
        Ok(())
    }

    fn write_document_catalog(&mut self) -> Result<IndirectReference<DocumentCatalog>, Error> {
        let page_tree = std::mem::take(&mut self.page_tree);
        self.write_reserved_object(self.page_tree_reference(), page_tree)?;
        let catalog = self.document_catalog.take().expect("No document catalog");
        self.write_object(catalog)
    }

    fn write_document_info(&mut self) -> Result<IndirectReference<DocumentInfo>, Error> {
        let doc_info = std::mem::take(&mut self.document_info);
        self.write_object(doc_info)
    }
    /// Finish the `Context` and flush all remaining writes.
    pub fn finish(mut self) -> Result<(), Error> {
        let document_catalog = self.write_document_catalog()?;
        let document_info = if !self.document_info.is_empty() {
            Some(self.write_document_info()?)
        } else {
            None
        };

        // the page tree is only written with the document catalog
        for dangling_ref in &self.dangling_references {
            error!(
                "Created indirect reference {:?} but never assigned an object to it!",
                dangling_ref
            )
        }

        self.flush_object_stream()?;

        if self.object_streams {
            let mut trailer = Trailer {
                crossref_offset: 0,
                document_catalog,
                document_info,
            };
            trailer.write_xref_stream(&mut self)?;
            return self.output.flush();
        }

        let offset = self.output.offset();
        self.crossref.write_table(&mut self.output)?;
        self.write_trailer(offset as u32, document_catalog, document_info)?;
        self.output.flush()
    }

    pub(crate) fn current_offset(&self) -> u64 {
        self.output.offset()
    }
}
