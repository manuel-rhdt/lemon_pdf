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

use lemon_pdf_derive::PdfFormat;

use crate::object::PdfFormat;

pub mod builtin;
pub mod descriptor;
pub mod encoding;

#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Font {
    Simple(builtin::SimpleFont),
}

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
    Type1,
    TrueType,
}

impl Default for FontType {
    fn default() -> Self {
        FontType::Type1
    }
}
