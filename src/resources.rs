//! The named resources of a content stream (ISO 32000-1, 7.8.3): the fonts, XObjects, graphics
//! states, colour spaces, shadings and patterns that its operators refer to by name.
//!
//! [`Resources`] hands out a typed key for every resource that is added: an operator takes the
//! key of the kind of resource that it needs, so that the name of a font cannot be passed where
//! the name of an XObject is wanted.

use std::collections::{BTreeMap, HashMap};

use crate as lemon_pdf;
use lemon_pdf_derive::PdfFormat;

use crate::color_space::{ColorSpaceResource, DefaultColorSpace, IccBasedColorSpace};
use crate::font::Font;
use crate::graphics_state::ExtGState;
use crate::image::XObject;
use crate::object::{IndirectReference, Name};
use crate::pagetree::ResourceDictionary;
use crate::pattern::{Pattern, TilingPattern};
use crate::shading::{Shading, ShadingPattern};

macro_rules! resource_keys {
    ($($(#[$meta:meta])* $key:ident),* $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, PartialEq, Eq, Hash, PdfFormat)]
            pub struct $key(Name);

            impl $key {
                /// The name that the resource has in the resource dictionary.
                pub fn name(&self) -> &Name {
                    &self.0
                }
            }
        )*
    };
}

resource_keys! {
    /// The name of a font in the resources (`Tf` operator).
    FontKey,
    /// The name of an XObject, an image or a form, in the resources (`Do` operator).
    XObjectKey,
    /// The name of a graphics state parameter dictionary in the resources (`gs` operator).
    ExtGStateKey,
    /// The name of a colour space in the resources (`cs` and `CS` operators).
    ColorSpaceKey,
    /// The name of a shading in the resources (`sh` operator).
    ShadingKey,
    /// The name of a pattern in the resources (`scn` and `SCN` operators).
    PatternKey,
}

/// The resources that a content stream uses. Adding an equal resource again gives the same key.
#[derive(Debug, Default)]
pub struct Resources {
    fonts: HashMap<IndirectReference<Font>, FontKey>,
    x_objects: HashMap<IndirectReference<XObject>, XObjectKey>,
    /// A `Vec` because `ExtGState` holds floats and cannot be hashed; pages only use a handful of
    /// them. The same goes for the shadings and patterns.
    ext_g_states: Vec<(ExtGState, ExtGStateKey)>,
    shadings: Vec<(Shading, ShadingKey)>,
    patterns: Vec<(Pattern, PatternKey)>,
    /// The named colour spaces, with the ones that redefine a device space.
    color_spaces: Vec<(ColorSpaceResource, Name)>,
}

fn by_name<T, K>(map: HashMap<IndirectReference<T>, K>, name: impl Fn(K) -> Name) -> BTreeMap<Name, IndirectReference<T>> {
    map.into_iter().map(|(reference, key)| (name(key), reference)).collect()
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a font to the resources.
    pub fn add_font(&mut self, font: IndirectReference<Font>) -> FontKey {
        let number = self.fonts.len();
        self.fonts.entry(font).or_insert_with(|| FontKey(Name::from(format!("F{}", number)))).clone()
    }

    /// Adds an XObject (for example an image from `DocumentContext::write_image`) to the
    /// resources.
    pub fn add_xobject(&mut self, x_object: impl Into<IndirectReference<XObject>>) -> XObjectKey {
        let number = self.x_objects.len();
        self.x_objects
            .entry(x_object.into())
            .or_insert_with(|| XObjectKey(Name::from(format!("X{}", number))))
            .clone()
    }

    /// Adds a graphics state parameter dictionary to the resources.
    pub fn add_ext_gstate(&mut self, state: ExtGState) -> ExtGStateKey {
        if let Some((_, key)) = self.ext_g_states.iter().find(|(other, _)| *other == state) {
            return key.clone();
        }
        let key = ExtGStateKey(Name::from(format!("GS{}", self.ext_g_states.len())));
        self.ext_g_states.push((state, key.clone()));
        key
    }

    /// Adds a colour space (Lab, CalRGB or one from an ICC profile) to the resources.
    pub fn add_color_space(&mut self, space: impl Into<ColorSpaceResource>) -> ColorSpaceKey {
        let space = space.into();
        if let Some((_, key)) = self.color_spaces.iter().find(|(other, key)| *other == space && !key.as_str().starts_with("Default")) {
            return ColorSpaceKey(key.clone());
        }
        let mut number = self.color_spaces.len();
        let key = loop {
            let key = Name::from(format!("CS{}", number));
            if !self.color_spaces.iter().any(|(_, other)| *other == key) {
                break key;
            }
            number += 1;
        };
        self.color_spaces.push((space, key.clone()));
        ColorSpaceKey(key)
    }

    /// Defines what the device colours of the content mean (`DefaultRGB` and the like, ISO
    /// 32000-1, 8.6.5.6): with an sRGB profile, `DeviceRGB` colours are sRGB in every viewer,
    /// images and shadings included, instead of whatever the viewer takes the device to be. The
    /// space must have as many components as the device space (`Rgb`: 3). Setting it again
    /// replaces it.
    pub fn set_default_color_space(&mut self, kind: DefaultColorSpace, space: IccBasedColorSpace) {
        let key = Name::from(kind.resource_name().to_owned());
        self.color_spaces.retain(|(_, name)| *name != key);
        self.color_spaces.push((space.into(), key));
    }

    /// Adds a shading to the resources.
    pub fn add_shading(&mut self, shading: impl Into<Shading>) -> ShadingKey {
        let shading = shading.into();
        if let Some((_, key)) = self.shadings.iter().find(|(other, _)| *other == shading) {
            return key.clone();
        }
        let key = ShadingKey(Name::from(format!("Sh{}", self.shadings.len())));
        self.shadings.push((shading, key.clone()));
        key
    }

    /// Adds a shading pattern to the resources.
    pub fn add_shading_pattern(&mut self, pattern: ShadingPattern) -> PatternKey {
        self.add_pattern(pattern.into())
    }

    /// Adds a tiling pattern from `DocumentContext::write_tiling_pattern` to the resources.
    pub fn add_tiling_pattern(&mut self, pattern: IndirectReference<TilingPattern>) -> PatternKey {
        self.add_pattern(pattern.into())
    }

    fn add_pattern(&mut self, pattern: Pattern) -> PatternKey {
        if let Some((_, key)) = self.patterns.iter().find(|(other, _)| *other == pattern) {
            return key.clone();
        }
        let key = PatternKey(Name::from(format!("P{}", self.patterns.len())));
        self.patterns.push((pattern, key.clone()));
        key
    }

    pub(crate) fn into_dictionary(self) -> ResourceDictionary {
        ResourceDictionary {
            font: by_name(self.fonts, |key| key.0),
            x_object: by_name(self.x_objects, |key| key.0),
            ext_g_state: self.ext_g_states.into_iter().map(|(state, key)| (key.0, state)).collect(),
            shading: self.shadings.into_iter().map(|(shading, key)| (key.0, shading)).collect(),
            pattern: self.patterns.into_iter().map(|(pattern, key)| (key.0, pattern)).collect(),
            color_space: self.color_spaces.into_iter().map(|(space, key)| (key, space)).collect(),
        }
    }
}
