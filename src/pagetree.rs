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

use lemon_pdf_derive::PdfFormat;

use crate::array::Array;
use crate::content::PageContext;
use crate::dictionary::Dictionary;
use crate::document::DocumentContext;
use crate::object::{IndirectReference, Object, PdfFormat, Value};
use crate::stream::{StreamEncoder, StreamFilter};
use crate::Pt;

#[derive(Debug, PartialEq, Clone, PdfFormat)]
pub struct Pages {
    count: i64,
    #[skip_if("Option::is_none")]
    parent: Option<IndirectReference<Pages>>,
    kids: Vec<Object<PageTreeNode>>,
}

impl Default for Pages {
    fn default() -> Self {
        Pages {
            kids: Vec::new(),
            parent: None,
            count: -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, PdfFormat)]
enum PageTreeNode {
    Tree(Pages),
    Page(Page),
}

impl Pages {
    pub fn new() -> Self {
        Default::default()
    }

    #[allow(unused)]
    pub fn add_pagetree(&mut self, tree: Pages) {
        self.kids.push(Object::Direct(PageTreeNode::Tree(tree)))
    }

    pub fn add_page(&mut self, page: Page) {
        self.kids.push(Object::Direct(PageTreeNode::Page(page)))
    }

    // make all children indirect objects
    pub fn write_to_context(
        mut self,
        context: &mut DocumentContext,
    ) -> Result<IndirectReference<Pages>> {
        context.write_object_fn(|context, self_reference| {
            let mut array = Vec::new();
            for child in self.kids {
                match child {
                    Object::Direct(PageTreeNode::Tree(mut tree)) => {
                        tree.parent = Some(self_reference);
                        let tree_obj = tree.write_to_context(context)?;
                        array.push(tree_obj.convert().into())
                    }
                    Object::Direct(PageTreeNode::Page(mut page)) => {
                        page.set_parent(self_reference);
                        let page_obj = context.write_object(page)?;
                        array.push(page_obj.convert().into());
                    }
                    _ => unreachable!(),
                }
            }
            self.kids = array;
            self.count = self.kids.len() as i64;
            Ok(self)
        })
    }
}

/// A convenience struct to create PDF pages.
#[derive(Clone, Debug, PartialEq, PdfFormat)]
pub struct Page {
    pub resources: Option<Object<Value>>,
    pub media_box: [Pt; 4],
    #[skip_if("Option::is_none")]
    pub parent: Option<IndirectReference<Pages>>,
    pub contents: Array,
}
impl Page {
    pub fn new() -> Self {
        Page {
            resources: Some(Object::Direct(Dictionary::new().into())),
            media_box: MediaBox::paper_din_a(4).as_array(),
            parent: None,
            contents: Array::new(),
        }
    }

    pub fn set_media_box(&mut self, media_box: MediaBox) {
        self.media_box = media_box.as_array()
    }

    fn get_context<'context, 'borrow>(
        &mut self,
        context: &'borrow mut DocumentContext<'context>,
        stream_filter: Option<StreamFilter>,
    ) -> PageContext<'_, 'context, 'borrow> {
        PageContext {
            fonts: HashMap::new(),
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

    pub fn set_resources(&mut self, resources: impl Into<Object<Value>>) {
        self.resources = Some(resources.into())
    }

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
