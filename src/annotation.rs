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

use std::io::Result;

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::object::{Formatter, PdfFormat};
use crate::Pt;

/// An action that associates a resource (such as a URL) with an annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct UriAction {
    /// The URI to open when the annotation is activated.
    pub uri: String,
}

impl PdfFormat for UriAction {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let uri: &[u8] = self.uri.as_bytes();
        f.format_dictionary()
            .key_value(&"S", &"URI")
            .key_value(&"URI", &uri)
            .finish()
    }
}

/// An action that jumps to a destination within the document.
#[derive(Debug, Clone, PartialEq)]
pub struct GoToAction {
    /// A destination reference within the PDF document.
    pub destination: Destination,
}

impl PdfFormat for GoToAction {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        f.format_dictionary()
            .key_value(&"S", &"GoTo")
            .key_value(&"D", &self.destination)
            .finish()
    }
}

/// A destination inside the document with a layouted point to jump to.
#[derive(Debug, Clone, PartialEq)]
pub struct Destination {
    /// The page number (zero-based index), as a PDF page label.
    pub page_index: u32,
    /// The coordinates on the page.
    pub point: (Pt, Pt),
}

impl PdfFormat for Destination {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let top = self.point.1;
        // [page /XYZ left top null]
        let left = self.point.0;
        let null_name: Option<&str> = None;
        f.format_array()
            .value(&self.page_index)
            .value(&"XYZ")
            .value(&left)
            .value(&top)
            .value(&null_name)
            .finish()
    }
}

/// The action of a link annotation.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkAction {
    /// Opens a URI.
    Uri(UriAction),
    /// Jumps to a destination within the document.
    GoTo(GoToAction),
}

impl PdfFormat for LinkAction {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        use LinkAction::*;
        match *self {
            Uri(ref action) => action.write(f),
            GoTo(ref action) => action.write(f),
        }
    }
}

/// A hyperlink annotation on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkAnnotation {
    /// The rectangle of the link, in default user space coordinates of the page.
    pub rect: [Pt; 4],
    /// What happens when the link is activated.
    pub action: LinkAction,
}

impl PdfFormat for LinkAnnotation {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        let rect = self.rect;
        let border = [0i64, 0, 0];
        f.format_dictionary()
            .key_value(&"Type", &"Annot")
            .key_value(&"Subtype", &"Link")
            .key_value(&"Rect", &rect)
            .key_value(&"Border", &border)
            .key_value(&"A", &self.action)
            .finish()
    }
}

/// An annotation attached to a page.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Annotation {
    Link(LinkAnnotation),
}