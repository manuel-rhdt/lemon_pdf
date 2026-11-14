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

//! Form XObjects (ISO 32000-1, 8.10): content streams with their own resources that are painted
//! like an image, or used as the group of a soft mask (ISO 32000-1, 11.6.5.2).
//!
//! `DocumentContext::write_form` writes one. It is given the same `PageContext` that pages
//! are drawn with, so everything that a page can paint can be in a form.

use crate as lemon_pdf;
use crate::content::{Matrix, Pt};
use crate::image::ColorSpace;
use crate::object::IndirectReference;
use crate::pagetree::ResourceDictionary;
use crate::stream::Stream;
use lemon_pdf_derive::{PdfDictionary, PdfFormat};

fn is_false(value: &bool) -> bool {
    !*value
}

/// The attributes of a transparency group (ISO 32000-1, 11.6.6): a form whose content is
/// composited as a whole.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
#[constant(S = "Transparency")]
pub struct TransparencyGroup {
    /// The color space the group is composited in.
    #[rename("CS")]
    pub color_space: Option<ColorSpace>,
    /// Whether the group is composited against a transparent backdrop instead of the content
    /// that is behind it.
    #[rename("I")]
    #[skip_if(is_false)]
    pub isolated: bool,
    /// Whether the objects in the group replace each other where they overlap instead of
    /// being composited.
    #[rename("K")]
    #[skip_if(is_false)]
    pub knockout: bool,
}

impl TransparencyGroup {
    /// A group that is composited in `color_space`, isolated and without knockout.
    pub fn new(color_space: ColorSpace) -> Self {
        TransparencyGroup {
            color_space: Some(color_space),
            isolated: true,
            knockout: false,
        }
    }
}

/// The dictionary of a form XObject (ISO 32000-1, 8.10.2).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[rename("XObject")]
#[constant(Subtype = "Form")]
pub struct FormDict {
    /// The rectangle in form space (`xmin ymin xmax ymax`) that the content is clipped to.
    #[rename("BBox")]
    pub bbox: [Pt; 4],
    /// Maps form space to user space; the identity if absent.
    pub matrix: Option<Matrix>,
    pub resources: ResourceDictionary,
    pub group: Option<TransparencyGroup>,
}

/// A form XObject.
pub type FormXObject = Stream<FormDict>;

/// A reference to a form XObject, as the group of a soft mask.
pub type FormReference = IndirectReference<FormXObject>;
