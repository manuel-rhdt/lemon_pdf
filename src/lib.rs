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
#![deny(missing_debug_implementations)]

#[macro_use]
extern crate derive_more;

pub mod annotation;
pub mod array;
pub mod content;
mod crossref;
pub mod dictionary;
mod document;
pub mod font;
pub mod graphics_state;
pub mod image;
pub mod mesh;
pub mod object;
pub mod pagetree;
pub mod shading;
pub mod stream;
pub mod structure_tree;
mod trailer;

pub use self::content::{Matrix, Pt};
pub use self::object::{IndirectObject, PdfDictionary, PdfFormat};
pub use self::pagetree::Page;
pub use crate::document::*;

#[cfg(test)]
mod tests {
    use crate::annotation::{Annotation, Destination, GoToAction, LinkAction, LinkAnnotation, UriAction};
    use crate::object::{ByteString, IndirectReference, PdfFormat, TextString};
    use crate::Pt;

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn link_annotation_is_written() {
        let mut link = LinkAnnotation::new(
            [Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0)],
            LinkAction::Uri(UriAction {
                uri: ByteString::from("https://example.com"),
            }),
        );
        link.contents = Some(TextString("Beispiel für einen Link".to_owned()));
        let link = Annotation::Link(link);
        let mut buffer = Vec::new();
        let mut formatter = crate::object::Formatter {
            writer: &mut buffer,
        };
        link.write(&mut formatter).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("/Subtype /Link"), "output was: {}", output);
        assert!(output.contains("/URI (https://example.com)"), "output was: {}", output);
        assert!(output.contains("/Rect"), "output was: {}", output);
        assert!(output.contains("/Type /Annot\n/Subtype /Link"), "output was: {}", output);
        assert!(output.contains("/S /URI"), "output was: {}", output);
        assert!(output.contains("/Contents <FEFF"), "output was: {}", output);
        assert!(!output.contains("StructParent"), "output was: {}", output);
    }

    #[test]
    fn goto_link_annotation_is_written() {
        let link = Annotation::Link(LinkAnnotation::new(
            [Pt(0.0), Pt(0.0), Pt(100.0), Pt(50.0)],
            LinkAction::GoTo(GoToAction {
                destination: Destination {
                    page: IndirectReference::new(2, 0),
                    point: (Pt(10.0), Pt(20.0)),
                },
            }),
        ));
        let mut buffer = Vec::new();
        let mut formatter = crate::object::Formatter {
            writer: &mut buffer,
        };
        link.write(&mut formatter).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("/S /GoTo"), "output was: {}", output);
        assert!(output.contains("/D [ 2 0 R /XYZ 10 20 null ]"), "output was: {}", output);
    }
}
