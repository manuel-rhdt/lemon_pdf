use crate as lemon_pdf;

use crate::{Page, object::{Object, IndirectReference}};

use lemon_pdf_derive::PdfFormat;

#[derive(Debug, Clone, PdfFormat)]
pub struct StructTreeRoot {
    pub k: Vec<Object<StructElem>>
}

#[derive(Debug, Clone, PdfFormat)]
pub enum StandardStructureType {
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
}

#[derive(Debug, Clone, PdfFormat)]
pub enum StructureParent {
    Root(IndirectReference<StructTreeRoot>),
    Elem(IndirectReference<StructElem>)
}

#[derive(Debug, Clone, PdfFormat)]
pub struct StructElem {
    pub s: StandardStructureType,
    pub p: StructureParent,
    #[rename("ID")]
    #[skip_if(Vec::is_empty)]
    pub id: Vec<u8>,
    pub pg: Option<IndirectReference<Page>>,
    /// the children
    pub k: Vec<Object<StructElem>>
}