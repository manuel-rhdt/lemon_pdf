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

use crate::object::{Formatter, PdfFormat, TextString};
use crate::Pt;

/// An action that associates a resource (such as a URL) with an annotation.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Action")]
#[constant(S = "URI")]
pub struct UriAction {
    /// The URI to open when the annotation is activated (an ASCII string).
    #[rename("URI")]
    pub uri: Vec<u8>,
}

/// An action that jumps to a destination within the document.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Action")]
#[constant(S = "GoTo")]
pub struct GoToAction {
    /// A destination reference within the PDF document.
    #[rename("D")]
    pub destination: Destination,
}

/// A destination inside the document with a layouted point to jump to.
#[derive(Debug, Clone, PartialEq)]
pub struct Destination {
    /// The page number (zero-based index), as a PDF page label.
    pub page_index: u32,
    /// The coordinates on the page.
    pub point: (Pt, Pt),
}

// A destination is an array of mixed types, which cannot be derived.
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
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum LinkAction {
    /// Opens a URI.
    Uri(UriAction),
    /// Jumps to a destination within the document.
    GoTo(GoToAction),
}

/// A hyperlink annotation on a page.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Annot")]
#[constant(Subtype = "Link")]
pub struct LinkAnnotation {
    /// The rectangle of the link, in default user space coordinates of the page.
    pub rect: [Pt; 4],
    /// The border of the link: horizontal and vertical corner radius and the width.
    pub border: [i64; 3],
    /// What happens when the link is activated.
    #[rename("A")]
    pub action: LinkAction,
    /// A description of the link for assistive technology.
    #[skip_if("Option::is_none")]
    pub contents: Option<TextString>,
    /// The key of the annotation in the parent tree of the structure tree (tagged PDF).
    #[skip_if("Option::is_none")]
    pub struct_parent: Option<i64>,
}

impl LinkAnnotation {
    /// A link without a border.
    pub fn new(rect: [Pt; 4], action: LinkAction) -> Self {
        LinkAnnotation {
            rect,
            border: [0, 0, 0],
            action,
            contents: None,
            struct_parent: None,
        }
    }
}

/// An annotation attached to a page.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Annotation {
    Link(LinkAnnotation),
}
