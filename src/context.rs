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

use std::io::prelude::*;
use std::io::Error;

use lemon_pdf_derive::PdfFormat;

use crate::crossref::CrossRef;
use crate::object::{Formatter, IndirectReference, PdfFormat, RawIndirectReference};
use crate::pagetree::{Page, Pages};
use crate::trailer::Trailer;

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

#[derive(Debug, PdfFormat, Default)]
#[rename("Catalog")]
pub(crate) struct DocumentCatalog {
    pages: Option<IndirectReference<Pages>>,
}

#[derive(Debug)]
pub struct Context<W> {
    pub(crate) output: OffsetTrackingWriter<W>,
    pub version: Version,
    pub(crate) crossref: CrossRef,
    document_catalog: Option<DocumentCatalog>,
    page_tree: Option<Pages>,
}

impl<W: Write> Context<W> {
    pub fn with_writer(writer: W, version: Version) -> Result<Self, Error> {
        let mut context = Context {
            output: OffsetTrackingWriter::new(writer),
            version,
            crossref: Default::default(),
            document_catalog: Some(DocumentCatalog::default()),
            page_tree: Some(Pages::new()),
        };
        context.start_pdf()?;
        Ok(context)
    }

    fn start_pdf(&mut self) -> Result<(), Error> {
        writeln!(self.output, "{}", self.version.header())?;
        self.output.write_all(b"%\xff\xff\xff\xff\n")?;
        Ok(())
    }

    /// Write an indirect object to the context and return an indirect reference to it.
    pub fn write_object<T: PdfFormat>(&mut self, object: T) -> Result<IndirectReference<T>, Error> {
        self.write_object_fn(|_, _| Ok(object))
    }

    pub fn write_object_fn<T: PdfFormat>(
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

    fn write_indirect_object(
        &mut self,
        object: impl PdfFormat,
        reference: RawIndirectReference,
    ) -> Result<(), Error> {
        // update placeholder entry in crossref
        self.crossref.get_entry_mut(reference.number as usize).0 = self.output.offset();
        write!(
            self.output,
            "{} {} obj\n",
            reference.number, reference.generation
        )?;
        let mut formatter = Formatter {
            writer: &mut self.output,
        };
        object.write(&mut formatter)?;
        write!(self.output, "\nendobj\n")?;
        Ok(())
    }

    pub fn add_page(&mut self, page: Page) {
        if let Some(p) = self.page_tree.as_mut() {
            p.add_page(page)
        }
    }

    fn write_trailer(
        &mut self,
        crossref_offset: u32,
        document_catalog: IndirectReference<DocumentCatalog>,
    ) -> Result<(), Error> {
        let mut trailer = Trailer {
            crossref_offset,
            document_catalog,
        };
        trailer.write(self)?;
        Ok(())
    }

    fn write_document_catalog(&mut self) -> Result<IndirectReference<DocumentCatalog>, Error> {
        let page_tree = self
            .page_tree
            .take()
            .expect("Internal Error: context has no pagetree.");
        let page_tree = page_tree.write_to_context(self)?;
        let mut catalog = self.document_catalog.take().expect("No document catalog");
        catalog.pages = Some(page_tree);
        self.write_object(catalog)
    }

    /// Finish the `Context` and flush all remaining writes.
    pub fn finish(mut self) -> Result<(), Error> {
        let document_catalog = self.write_document_catalog()?;

        let offset = self.output.offset();
        let mut formatter = Formatter {
            writer: &mut self.output,
        };
        self.crossref.write(&mut formatter)?;
        self.write_trailer(offset as u32, document_catalog)?;
        self.output.flush()
    }
}
