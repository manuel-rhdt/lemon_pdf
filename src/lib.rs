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

pub mod array;
mod document;
pub mod content;
mod crossref;
pub mod dictionary;
pub mod font;
pub mod object;
pub mod pagetree;
pub mod stream;
mod trailer;
mod serializer;
mod deserializer;

pub use crate::document::*;
pub use self::pagetree::Page;
pub use self::content::Pt;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
