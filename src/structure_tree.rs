//! The logical structure of a tagged PDF document (ISO 32000-1, section 14.7).
//!
//! A tagged document has a tree of structure elements (`StructElem`) below the
//! `StructTreeRoot` of the document catalog. Structure elements point to the content they stand
//! for: marked-content sequences in the content streams of the pages (by their `MCID`) and
//! annotations (by object references). The `ParentTree` maps the other way, from the content to
//! its structure element.

use std::collections::BTreeMap;
use std::io::Result;

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::annotation::Annotation;
use crate::object::{Formatter, IndirectReference, PdfFormat, TextString};
use crate::Page;

/// The root of the structure tree, referenced from the document catalog.
#[derive(Debug, Clone, PdfFormat)]
pub struct StructTreeRoot {
    /// The top-level structure elements (usually a single `Document` element).
    pub k: Vec<IndirectReference<StructElem>>,
    /// Maps the `StructParents` key of every page and the `StructParent` key of every annotation
    /// to the structure element(s) that own its content.
    pub parent_tree: NumberTree<ParentTreeEntry>,
    /// The next unused key in the parent tree.
    pub parent_tree_next_key: i64,
    /// Maps non-standard structure types to standard ones.
    #[skip_if("BTreeMap::is_empty")]
    pub role_map: BTreeMap<String, StructType>,
}

/// A number tree with all its entries in a single node (`<< /Nums [...] >>`).
#[derive(Debug, Clone, PartialEq)]
pub struct NumberTree<T> {
    pub nums: BTreeMap<i64, T>,
}

impl<T> Default for NumberTree<T> {
    fn default() -> Self {
        NumberTree {
            nums: BTreeMap::new(),
        }
    }
}

// A basic container type like arrays and dictionaries: `/Nums` is a flat array of alternating
// keys and values.
impl<T: PdfFormat> PdfFormat for NumberTree<T> {
    fn write(&self, f: &mut Formatter) -> Result<()> {
        struct Nums<'a, T>(&'a BTreeMap<i64, T>);
        impl<'a, T: PdfFormat> std::fmt::Debug for Nums<'a, T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Nums")
            }
        }
        impl<'a, T: PdfFormat> PdfFormat for Nums<'a, T> {
            fn write(&self, f: &mut Formatter) -> Result<()> {
                let mut nums = f.format_array();
                for (key, value) in self.0 {
                    nums = nums.value(key).value(value);
                }
                nums.finish()
            }
        }
        f.format_dictionary()
            .key_value(&"Nums", &Nums(&self.nums))
            .finish()
    }
}

/// The value of an entry of the parent tree.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum ParentTreeEntry {
    /// The entry of a page: the structure element of every marked-content sequence of the page,
    /// indexed by its MCID. `None` for MCIDs that are not used.
    Page(Vec<Option<IndirectReference<StructElem>>>),
    /// The entry of an object (annotation) that is the content of a structure element.
    Object(IndirectReference<StructElem>),
}

/// A structure type (ISO 32000-1, 14.8.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, PdfFormat)]
pub enum StructType {
    // grouping elements
    Document,
    Part,
    Art,
    Sect,
    Div,
    BlockQuote,
    Caption,
    TOC,
    TOCI,
    Index,
    NonStruct,
    Private,
    // block-level elements
    P,
    H,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    L,
    LI,
    Lbl,
    LBody,
    Table,
    TR,
    TH,
    TD,
    THead,
    TBody,
    TFoot,
    // inline-level elements
    Span,
    Quote,
    Note,
    Reference,
    BibEntry,
    Code,
    Link,
    Annot,
    Ruby,
    RB,
    RT,
    RP,
    Warichu,
    WT,
    WP,
    // illustration elements
    Figure,
    Formula,
    Form,
    /// A non-standard structure type (map it to a standard type in the `RoleMap`).
    Other(String),
}

/// The parent of a structure element.
#[derive(Debug, Clone, Copy, PartialEq, PdfFormat)]
pub enum StructParent {
    Root(IndirectReference<StructTreeRoot>),
    Elem(IndirectReference<StructElem>),
}

/// A child of a structure element.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum StructKid {
    /// Another structure element.
    Elem(IndirectReference<StructElem>),
    /// A marked-content sequence on the page `Pg` of the parent element.
    Mcid(i64),
    /// A marked-content sequence, for example on another page than the `Pg` of the parent
    /// element.
    MarkedContent(MarkedContentReference),
    /// An annotation.
    Object(ObjectReference),
}

/// A marked-content reference dictionary.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("MCR")]
pub struct MarkedContentReference {
    /// The page of the marked content (default: the `Pg` of the parent element).
    #[skip_if("Option::is_none")]
    pub pg: Option<IndirectReference<Page>>,
    #[rename("MCID")]
    pub mcid: i64,
}

/// An object reference dictionary.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("OBJR")]
pub struct ObjectReference {
    /// The page of the object (default: the `Pg` of the parent element).
    #[skip_if("Option::is_none")]
    pub pg: Option<IndirectReference<Page>>,
    pub obj: IndirectReference<Annotation>,
}

/// The value of a structure attribute.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum AttributeValue {
    Name(String),
    Integer(i64),
    Number(f64),
    Text(TextString),
    Array(Vec<AttributeValue>),
}

/// The attributes of a structure element that belong to one owner (like `Table` or `List`).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct AttributeObject {
    /// The owner, like `Layout`, `List`, `Table` or `PrintField`.
    #[rename("O")]
    pub owner: String,
    #[flatten]
    pub attributes: BTreeMap<String, AttributeValue>,
}

/// A structure element.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub struct StructElem {
    /// The structure type.
    pub s: StructType,
    /// The parent element.
    pub p: StructParent,
    /// The element identifier (a byte string).
    #[rename("ID")]
    #[skip_if("Option::is_none")]
    pub id: Option<Vec<u8>>,
    /// The page that the `Mcid` children are on.
    #[skip_if("Option::is_none")]
    pub pg: Option<IndirectReference<Page>>,
    /// The children, in logical (reading) order.
    #[skip_if("Vec::is_empty")]
    pub k: Vec<StructKid>,
    /// The attribute objects.
    #[skip_if("Vec::is_empty")]
    pub a: Vec<AttributeObject>,
    /// The title of the element.
    #[skip_if("Option::is_none")]
    pub t: Option<TextString>,
    /// The language of the element if it differs from the language of its parent.
    #[skip_if("Option::is_none")]
    pub lang: Option<TextString>,
    /// An alternate description, for example for figures and formulas.
    #[skip_if("Option::is_none")]
    pub alt: Option<TextString>,
    /// The expanded form of an abbreviation.
    #[skip_if("Option::is_none")]
    pub e: Option<TextString>,
    /// The text that is equivalent to the content of the element.
    #[skip_if("Option::is_none")]
    pub actual_text: Option<TextString>,
}

impl StructElem {
    pub fn new(s: StructType, p: StructParent) -> Self {
        StructElem {
            s,
            p,
            id: None,
            pg: None,
            k: Vec::new(),
            a: Vec::new(),
            t: None,
            lang: None,
            alt: None,
            e: None,
            actual_text: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(object: &dyn PdfFormat) -> String {
        let mut buffer = Vec::new();
        object
            .write(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn struct_elem_is_written() {
        let mut elem = StructElem::new(
            StructType::P,
            StructParent::Root(IndirectReference::new(1, 0)),
        );
        elem.pg = Some(IndirectReference::new(5, 0));
        elem.k = vec![
            StructKid::Mcid(0),
            StructKid::MarkedContent(MarkedContentReference {
                pg: Some(IndirectReference::new(6, 0)),
                mcid: 3,
            }),
        ];
        elem.lang = Some(TextString("de".to_owned()));
        let output = written(&elem);
        assert_eq!(
            output,
            "<< /Type /StructElem\n/S /P\n/P 1 0 R\n/Pg 5 0 R\n\
             /K [ 0 << /Type /MCR\n/Pg 6 0 R\n/MCID 3\n>> ]\n/Lang (de)\n>>"
        );
    }

    #[test]
    fn custom_struct_type_and_attributes_are_written() {
        let mut elem = StructElem::new(
            StructType::Other("Aside".to_owned()),
            StructParent::Root(IndirectReference::new(1, 0)),
        );
        let mut attributes = BTreeMap::new();
        attributes.insert("Placement".to_owned(), AttributeValue::Name("Block".to_owned()));
        elem.a = vec![AttributeObject {
            owner: "Layout".to_owned(),
            attributes,
        }];
        let output = written(&elem);
        assert!(output.contains("/S /Aside\n"), "{}", output);
        assert!(
            output.contains("/A [ << /O /Layout\n/Placement /Block\n>> ]"),
            "{}",
            output
        );
    }

    #[test]
    fn parent_tree_is_written() {
        let mut tree = NumberTree::default();
        tree.nums.insert(1, ParentTreeEntry::Object(IndirectReference::new(8, 0)));
        tree.nums.insert(
            0,
            ParentTreeEntry::Page(vec![Some(IndirectReference::new(7, 0)), None]),
        );
        let output = written(&tree);
        assert_eq!(output, "<< /Nums [ 0 [ 7 0 R null ] 1 8 0 R ]\n>>");
    }
}
