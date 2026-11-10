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

use crate as lemon_pdf;
use crate::stream::Stream;
use lemon_pdf_derive::{PdfDictionary, PdfFormat};

pub mod builtin;
pub mod descriptor;
pub mod encoding;
pub mod type0;

#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[indirect_variants]
pub enum Font {
    Simple(builtin::SimpleFont),
    Composite(type0::CompositeFont),
}

/// The dictionary of a CMap stream that maps character codes to Unicode (`ToUnicode`,
/// ISO 32000-1, 9.10.3).
#[derive(Debug, Default, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
pub struct ToUnicodeCMapDict {}

/// A CMap that maps character codes to Unicode, for text extraction.
pub type ToUnicodeCMap = Stream<ToUnicodeCMapDict>;

/// A postscript font unit (1/1000 of an EM)
#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd, PdfFormat)]
pub struct FontUnit(pub f64);

impl std::ops::Add for FontUnit {
    type Output = FontUnit;

    fn add(self, other: FontUnit) -> FontUnit {
        FontUnit(self.0 + other.0)
    }
}

impl std::ops::Sub for FontUnit {
    type Output = FontUnit;

    fn sub(self, other: FontUnit) -> FontUnit {
        FontUnit(self.0 - other.0)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PdfFormat)]
pub enum FontType {
    Type0,
    Type1,
    TrueType,
}

impl Default for FontType {
    fn default() -> Self {
        FontType::Type1
    }
}
