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

use std::collections::HashMap;
use std::f64::consts::SQRT_2;
use std::io::Result;

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::annotation::Annotation;
use crate::array::Array;
use crate::content::PageContext;
use crate::document::DocumentContext;
use crate::font::Font;
use crate::graphics_state::ExtGState;
use crate::image::XObject;
use crate::object::{IndirectReference, Object, Value};
use crate::stream::{Stream, StreamEncoder, StreamFilter};
use crate::Pt;

/// A node of the page tree with other page tree nodes or pages as its kids.
#[derive(Debug, Clone, Default, PartialEq, PdfFormat)]
pub struct Pages {
    /// The number of pages (leaf nodes) below this node.
    pub count: i64,
    #[skip_if("Option::is_none")]
    pub parent: Option<IndirectReference<Pages>>,
    pub kids: Vec<IndirectReference<PageTreeNode>>,
}

/// A kid of a page tree node.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum PageTreeNode {
    Tree(Pages),
    Page(Page),
}

impl Pages {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct ResourceDictionary {
    #[skip_if("HashMap::is_empty")]
    pub font: HashMap<String, IndirectReference<Font>>,
    #[rename("XObject")]
    #[skip_if("HashMap::is_empty")]
    pub x_object: HashMap<String, IndirectReference<XObject>>,
    #[rename("ExtGState")]
    #[skip_if("HashMap::is_empty")]
    pub ext_g_state: HashMap<String, ExtGState>,
}

/// A convenience struct to create PDF pages.
#[derive(Clone, Debug, PartialEq, PdfFormat)]
pub struct Page {
    #[skip_if("Option::is_none")]
    pub resources: Option<ResourceDictionary>,
    pub media_box: [Pt; 4],
    /// The annotations of the page. Only emitted if the vector is not empty.
    #[rename("Annots")]
    #[skip_if("Vec::is_empty")]
    pub annotations: Vec<Object<Annotation>>,
    #[skip_if("Option::is_none")]
    pub parent: Option<IndirectReference<Pages>>,
    pub contents: Vec<IndirectReference<Stream>>,
    /// The key of the page in the parent tree of the structure tree (tagged PDF).
    #[skip_if("Option::is_none")]
    pub struct_parents: Option<i64>,
    /// The tab order of the annotations; `S` follows the structure tree.
    #[skip_if("Option::is_none")]
    pub tabs: Option<String>,
}
impl Page {
    pub fn new() -> Self {
        Page {
            resources: None,
            media_box: MediaBox::paper_din_a(4).as_array(),
            parent: None,
            annotations: Vec::new(),
            contents: Vec::new(),
            struct_parents: None,
            tabs: None,
        }
    }

    pub fn set_media_box(&mut self, media_box: MediaBox) {
        self.media_box = media_box.as_array()
    }

    /// Writes an annotation as an indirect object and adds it to the page.
    pub fn add_annotation(
        &mut self,
        context: &mut DocumentContext,
        annotation: Annotation,
    ) -> Result<IndirectReference<Annotation>> {
        let annotation = context.write_object(annotation)?;
        self.annotations.push(annotation.into());
        Ok(annotation)
    }

    fn get_context<'context, 'borrow>(
        &mut self,
        context: &'borrow mut DocumentContext<'context>,
        stream_filter: Option<StreamFilter>,
    ) -> PageContext<'_, 'context, 'borrow> {
        PageContext {
            fonts: HashMap::new(),
            x_objects: HashMap::new(),
            ext_g_states: Vec::new(),
            page: self,
            content_stream: StreamEncoder::new(stream_filter),
            pdf_context: context,
        }
    }

    pub fn add_content<'context, 'borrow>(
        &mut self,
        context: &mut DocumentContext<'context>,
        stream_filter: Option<StreamFilter>,
        content_f: impl FnOnce(&mut PageContext<'_, 'context, '_>) -> Result<()>,
    ) -> Result<()> {
        let mut page_context = self.get_context(context, stream_filter);
        content_f(&mut page_context)?;
        page_context.finish()?;
        Ok(())
    }

    // pub fn set_resources(&mut self, resources: impl Into<Object<Value>>) {
    //     self.resources = Some(resources.into())
    // }

    pub(crate) fn set_parent(&mut self, parent: IndirectReference<Pages>) {
        self.parent = Some(parent)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MediaBox {
    pub xmin: Pt,
    pub ymin: Pt,
    pub xmax: Pt,
    pub ymax: Pt,
}

impl MediaBox {
    pub fn new(xmin: Pt, ymin: Pt, xmax: Pt, ymax: Pt) -> Self {
        MediaBox {
            xmin,
            ymin,
            xmax,
            ymax,
        }
    }

    pub fn paper_din_a(num: i32) -> Self {
        const A_AREA: f64 = 1.0;
        const M_TO_PT: f64 = 2834.65;

        let height = M_TO_PT * (A_AREA * SQRT_2).sqrt() / SQRT_2.powi(num);
        MediaBox::new(Pt(0.0), Pt(0.0), Pt(height / SQRT_2), Pt(height))
    }

    pub fn as_array(self) -> [Pt; 4] {
        [self.xmin, self.ymin, self.xmax, self.ymax]
    }
}

impl Default for Page {
    fn default() -> Self {
        Page::new()
    }
}

impl From<MediaBox> for Object<Value> {
    fn from(mediabox: MediaBox) -> Object<Value> {
        let mut array = Array::new();
        array.push(Object::Direct(mediabox.xmin.0.into()));
        array.push(Object::Direct(mediabox.ymin.0.into()));
        array.push(Object::Direct(mediabox.xmax.0.into()));
        array.push(Object::Direct(mediabox.ymax.0.into()));
        Object::Direct(array.into())
    }
}
