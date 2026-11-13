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

//! Smooth color transitions: axial (linear) and radial shadings (ISO 32000-1, 8.7.4.5.3 and
//! 8.7.4.5.4) with the functions that give their colors (ISO 32000-1, 7.10), and shading
//! patterns (ISO 32000-1, 8.7.3.3).
//!
//! A shading is painted into the current clipping path with `PageContext::paint_shading`, in the
//! current user space, or used as the color of fills and strokes through a [`ShadingPattern`].
//!
//! The colors of a gradient have a type ([`Gray`], [`Rgb`] or [`Cmyk`]) that decides the color
//! space of the shading, so all colors of a gradient are in the same one.
//!
//! The shadings in this module have a single color per position, so they cannot be
//! transparent. Mesh shadings (types 4 to 7) are in the [`mesh`](crate::mesh) module; function-based
//! shadings (type 1) are not supported.

use std::fmt;

use lemon_pdf_derive::PdfFormat;

use crate as lemon_pdf;
use crate::content::{Matrix, Pt};
use crate::image::ColorSpace;
use crate::mesh::MeshShading;
use crate::object::IndirectReference;

/// An error from building a function or a gradient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadingError {
    /// There is no color stop.
    NoStops,
}

impl fmt::Display for ShadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShadingError::NoStops => write!(f, "a gradient needs at least one color stop"),
        }
    }
}

impl std::error::Error for ShadingError {}

/// A color in one of the device color spaces. Every component is from 0 to 1.
pub trait Color: Copy {
    /// The color space of the colors of this type.
    const SPACE: ColorSpace;

    /// The components of the color, as many as the color space has.
    fn components(&self) -> Vec<f64>;
}

/// A gray level: 0 is black and 1 is white.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Gray(pub f64);

/// A color with red, green and blue components.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

/// A color with cyan, magenta, yellow and black components.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Cmyk(pub f64, pub f64, pub f64, pub f64);

impl Color for Gray {
    const SPACE: ColorSpace = ColorSpace::DeviceGray;

    fn components(&self) -> Vec<f64> {
        vec![self.0]
    }
}

impl Color for Rgb {
    const SPACE: ColorSpace = ColorSpace::DeviceRGB;

    fn components(&self) -> Vec<f64> {
        vec![self.0, self.1, self.2]
    }
}

impl Color for Cmyk {
    const SPACE: ColorSpace = ColorSpace::DeviceCMYK;

    fn components(&self) -> Vec<f64> {
        vec![self.0, self.1, self.2, self.3]
    }
}

/// A function from one input to several outputs (ISO 32000-1, 7.10), which gives the color at a
/// position of a shading.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Function {
    Exponential(ExponentialFunction),
    Stitching(StitchingFunction),
}

/// An exponential interpolation function (type 2): `C0 + x^N * (C1 - C0)` for every output.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
#[constant(FunctionType = 2)]
pub struct ExponentialFunction {
    domain: [f64; 2],
    #[rename("C0")]
    c0: Vec<f64>,
    #[rename("C1")]
    c1: Vec<f64>,
    #[rename("N")]
    exponent: f64,
}

impl ExponentialFunction {
    /// Interpolates linearly from `from` at 0 to `to` at 1.
    pub fn linear<C: Color>(from: C, to: C) -> Self {
        ExponentialFunction {
            domain: [0.0, 1.0],
            c0: from.components(),
            c1: to.components(),
            exponent: 1.0,
        }
    }
}

/// A stitching function (type 3): several functions, each for a part of the domain.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
#[constant(FunctionType = 3)]
pub struct StitchingFunction {
    domain: [f64; 2],
    functions: Vec<Function>,
    /// The ends of all parts of the domain but the last.
    bounds: Vec<f64>,
    /// Maps every part of the domain to the domain of its function, two numbers per function.
    encode: Vec<f64>,
}

/// A color at a position of a gradient, `offset` in `0..=1`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ColorStop<C> {
    pub offset: f64,
    pub color: C,
}

impl<C> ColorStop<C> {
    pub fn new(offset: f64, color: C) -> Self {
        ColorStop { offset, color }
    }
}

impl Function {
    /// The function of a gradient with the given color stops, for a domain of `0..=1`.
    ///
    /// Offsets are clamped to `0..=1` and must not decrease; one that is smaller than the one
    /// before it is raised to it. Two stops at the same offset make a hard transition. The
    /// color of the first stop is used before it and the one of the last stop after it.
    pub fn gradient<C: Color>(stops: &[ColorStop<C>]) -> Result<Function, ShadingError> {
        if stops.is_empty() {
            return Err(ShadingError::NoStops);
        }

        let mut points: Vec<(f64, C)> = Vec::with_capacity(stops.len());
        let mut previous = 0.0f64;
        for stop in stops {
            let offset = if stop.offset.is_nan() {
                previous
            } else {
                stop.offset.max(previous).min(1.0)
            };
            previous = offset;
            points.push((offset, stop.color));
        }

        // the parts of the domain, each from one point to the next
        let mut parts: Vec<(f64, f64, C, C)> = Vec::new();
        let first = points[0];
        if first.0 > 0.0 {
            parts.push((0.0, first.0, first.1, first.1));
        }
        for pair in points.windows(2) {
            // a hard transition has no part of its own
            if pair[1].0 > pair[0].0 {
                parts.push((pair[0].0, pair[1].0, pair[0].1, pair[1].1));
            }
        }
        let last = points[points.len() - 1];
        if last.0 < 1.0 {
            parts.push((last.0, 1.0, last.1, last.1));
        }

        let mut functions: Vec<Function> = parts
            .iter()
            .map(|&(_, _, from, to)| Function::Exponential(ExponentialFunction::linear(from, to)))
            .collect();
        match functions.len() {
            // the first part starts at 0 and the last one ends at 1, so there is at least one
            0 => unreachable!("a gradient has at least one part"),
            1 => Ok(functions.remove(0)),
            _ => Ok(Function::Stitching(StitchingFunction {
                domain: [0.0, 1.0],
                bounds: parts[..parts.len() - 1].iter().map(|part| part.1).collect(),
                encode: functions.iter().flat_map(|_| [0.0, 1.0]).collect(),
                functions,
            })),
        }
    }
}

/// The colors of a gradient: its color stops, whether the colors of the ends continue beyond
/// the shading, and the color painted where a shading pattern has no color.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient<C> {
    function: Function,
    extend: bool,
    background: Option<C>,
}

impl<C: Color> Gradient<C> {
    /// A gradient through the color stops, see [`Function::gradient`].
    pub fn new(stops: &[ColorStop<C>]) -> Result<Self, ShadingError> {
        Ok(Gradient {
            function: Function::gradient(stops)?,
            extend: false,
            background: None,
        })
    }

    /// Continues the color of the start before the start of the shading and the color of the
    /// end beyond its end.
    pub fn extended(mut self) -> Self {
        self.extend = true;
        self
    }

    /// Sets the color painted where the shading does not paint. Only a shading pattern uses it,
    /// `PageContext::paint_shading` leaves those places alone.
    pub fn with_background(mut self, background: C) -> Self {
        self.background = Some(background);
        self
    }
}

/// A position in the user space.
#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct Point {
    pub x: Pt,
    pub y: Pt,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point { x: Pt(x), y: Pt(y) }
    }
}

/// A circle by its center and radius.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Circle {
    pub x: Pt,
    pub y: Pt,
    pub radius: Pt,
}

/// A shading (ISO 32000-1, 8.7.4.3). `AxialShading` and `RadialShading` convert into it
/// (`.into()`).
#[derive(Debug, Clone, PartialEq, From, PdfFormat)]
pub enum Shading {
    Axial(AxialShading),
    Radial(RadialShading),
    /// A mesh shading, which is a stream of its own: write it with
    /// `DocumentContext::write_mesh_shading`.
    Mesh(IndirectReference<MeshShading>),
}

/// An axial shading (type 2).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
#[constant(ShadingType = 2)]
pub struct AxialShading {
    color_space: ColorSpace,
    background: Option<Vec<f64>>,
    #[rename("BBox")]
    bbox: Option<[Pt; 4]>,
    coords: [Pt; 4],
    function: Function,
    extend: [bool; 2],
}

impl AxialShading {
    /// A shading that changes along the line from `from` to `to` in the user space it is
    /// painted in: it has the color of the first stop at the start and the color of the last
    /// stop at the end, and is constant across the line.
    pub fn new<C: Color>(from: Point, to: Point, gradient: &Gradient<C>) -> Self {
        AxialShading {
            color_space: C::SPACE,
            background: gradient.background.map(|color| color.components()),
            bbox: None,
            coords: [from.x, from.y, to.x, to.y],
            function: gradient.function.clone(),
            extend: [gradient.extend; 2],
        }
    }

    /// Limits the shading to a rectangle (`x0`, `y0`, `x1`, `y1`) of the space it is painted in.
    pub fn with_bbox(mut self, bbox: [Pt; 4]) -> Self {
        self.bbox = Some(bbox);
        self
    }
}

/// A radial shading (type 3).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
#[constant(ShadingType = 3)]
pub struct RadialShading {
    color_space: ColorSpace,
    background: Option<Vec<f64>>,
    #[rename("BBox")]
    bbox: Option<[Pt; 4]>,
    coords: [Pt; 6],
    function: Function,
    extend: [bool; 2],
}

impl RadialShading {
    /// A shading between two circles: the color of the first stop is at `start`, the one of the
    /// last stop at `end`, and the circles in between are interpolated.
    pub fn new<C: Color>(start: Circle, end: Circle, gradient: &Gradient<C>) -> Self {
        RadialShading {
            color_space: C::SPACE,
            background: gradient.background.map(|color| color.components()),
            bbox: None,
            coords: [start.x, start.y, start.radius, end.x, end.y, end.radius],
            function: gradient.function.clone(),
            extend: [gradient.extend; 2],
        }
    }

    /// Limits the shading to a rectangle (`x0`, `y0`, `x1`, `y1`) of the space it is painted in.
    pub fn with_bbox(mut self, bbox: [Pt; 4]) -> Self {
        self.bbox = Some(bbox);
        self
    }
}

/// A shading pattern (ISO 32000-1, 8.7.4.2): a shading used as the color of fills and strokes.
///
/// The coordinates of the shading are in the pattern space, which is the default coordinate
/// system of the page transformed by `matrix`, independent of the transformations applied while
/// the page is drawn.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Pattern")]
#[constant(PatternType = 2)]
pub struct ShadingPattern {
    shading: Shading,
    matrix: Option<Matrix>,
}

impl ShadingPattern {
    pub fn new(shading: impl Into<Shading>) -> Self {
        ShadingPattern {
            shading: shading.into(),
            matrix: None,
        }
    }

    /// Transforms the pattern space.
    pub fn with_matrix(mut self, matrix: Matrix) -> Self {
        self.matrix = Some(matrix);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Formatter, PdfFormat};

    fn written(object: &dyn PdfFormat) -> String {
        let mut buffer = Vec::new();
        object
            .write(&mut Formatter {
                writer: &mut buffer,
            })
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    fn gray(offset: f64, level: f64) -> ColorStop<Gray> {
        ColorStop::new(offset, Gray(level))
    }

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn two_stops_are_one_exponential_function() {
        let function = Function::gradient(&[
            ColorStop::new(0.0, Rgb(1.0, 0.0, 0.0)),
            ColorStop::new(1.0, Rgb(0.0, 0.0, 1.0)),
        ])
        .unwrap();
        let output = written(&function);
        assert!(output.contains("/FunctionType 2"), "{}", output);
        assert!(output.contains("/C0 [ 1 0 0 ]"), "{}", output);
        assert!(output.contains("/C1 [ 0 0 1 ]"), "{}", output);
        assert!(output.contains("/N 1"), "{}", output);
        assert!(!output.contains("/Type"), "{}", output);
    }

    #[test]
    fn several_stops_are_stitched() {
        let function =
            Function::gradient(&[gray(0.0, 1.0), gray(0.25, 0.0), gray(1.0, 0.5)]).unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        assert_eq!(stitched.functions.len(), 2);
        assert_eq!(stitched.bounds, vec![0.25]);
        assert_eq!(stitched.encode, vec![0.0, 1.0, 0.0, 1.0]);
        let output = written(&function);
        assert!(output.contains("/FunctionType 3"), "{}", output);
        assert!(output.contains("/Bounds [ 0.25 ]"), "{}", output);
    }

    #[test]
    fn colors_are_held_before_the_first_and_after_the_last_stop() {
        let function = Function::gradient(&[gray(0.25, 1.0), gray(0.75, 0.0)]).unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        assert_eq!(stitched.functions.len(), 3);
        assert_eq!(stitched.bounds, vec![0.25, 0.75]);
    }

    #[test]
    fn hard_transitions_have_no_part_of_their_own() {
        let function = Function::gradient(&[
            gray(0.0, 1.0),
            gray(0.5, 1.0),
            gray(0.5, 0.0),
            gray(1.0, 0.0),
        ])
        .unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        assert_eq!(stitched.functions.len(), 2);
        assert_eq!(stitched.bounds, vec![0.5]);
    }

    #[test]
    fn offsets_are_clamped_and_made_monotone() {
        let function = Function::gradient(&[
            gray(-1.0, 0.0),
            gray(0.6, 0.5),
            gray(0.2, 1.0),
            gray(f64::NAN, 1.0),
        ])
        .unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        // the stops are at 0, 0.6, 0.6 and 0.6: the parts are 0..0.6 and the constant 0.6..1
        assert_eq!(stitched.bounds, vec![0.6]);
    }

    #[test]
    fn stops_at_the_end_hold_the_first_color_before_them() {
        let function = Function::gradient(&[gray(1.0, 0.2), gray(1.0, 0.4)]).unwrap();
        let output = written(&function);
        assert!(output.contains("/FunctionType 2"), "{}", output);
        assert!(output.contains("/C0 [ 0.2 ]"), "{}", output);
        assert!(output.contains("/C1 [ 0.2 ]"), "{}", output);
    }

    #[test]
    fn no_stops_is_an_error() {
        assert_eq!(Function::gradient::<Gray>(&[]), Err(ShadingError::NoStops));
        assert_eq!(
            Gradient::<Rgb>::new(&[]).unwrap_err(),
            ShadingError::NoStops
        );
    }

    #[test]
    fn the_color_type_decides_the_color_space() {
        let spaces = [
            written(&AxialShading::new(
                pt(0.0, 0.0),
                pt(1.0, 0.0),
                &Gradient::new(&[gray(0.0, 0.0)]).unwrap(),
            )),
            written(&AxialShading::new(
                pt(0.0, 0.0),
                pt(1.0, 0.0),
                &Gradient::new(&[ColorStop::new(0.0, Cmyk(0.0, 0.0, 0.0, 1.0))]).unwrap(),
            )),
        ];
        assert!(spaces[0].contains("/ColorSpace /DeviceGray"), "{}", spaces[0]);
        assert!(spaces[1].contains("/ColorSpace /DeviceCMYK"), "{}", spaces[1]);
        assert!(spaces[1].contains("/C0 [ 0 0 0 1 ]"), "{}", spaces[1]);
    }

    #[test]
    fn axial_shading_is_written() {
        let gradient = Gradient::new(&[
            ColorStop::new(0.0, Rgb(1.0, 1.0, 0.0)),
            ColorStop::new(1.0, Rgb(0.0, 0.0, 0.5)),
        ])
        .unwrap()
        .extended();
        let output = written(&AxialShading::new(pt(10.0, 20.0), pt(110.0, 20.0), &gradient));
        assert!(output.contains("/ShadingType 2"), "{}", output);
        assert!(output.contains("/ColorSpace /DeviceRGB"), "{}", output);
        assert!(output.contains("/Coords [ 10 20 110 20 ]"), "{}", output);
        assert!(output.contains("/Extend [ true true ]"), "{}", output);
        assert!(output.contains("/Function <<"), "{}", output);
        assert!(!output.contains("/Background"), "{}", output);
        assert!(!output.contains("/Domain [ 0 1 ]\n/Function"), "{}", output);
    }

    #[test]
    fn radial_shading_is_written_with_options() {
        let gradient = Gradient::new(&[gray(0.0, 1.0), gray(1.0, 0.0)])
            .unwrap()
            .with_background(Gray(0.5));
        let circle = |radius| Circle {
            x: Pt(50.0),
            y: Pt(50.0),
            radius: Pt(radius),
        };
        let shading = RadialShading::new(circle(0.0), circle(40.0), &gradient)
            .with_bbox([Pt(0.0), Pt(0.0), Pt(100.0), Pt(100.0)]);
        let output = written(&shading);
        assert!(output.contains("/ShadingType 3"), "{}", output);
        assert!(output.contains("/Coords [ 50 50 0 50 50 40 ]"), "{}", output);
        assert!(output.contains("/Background [ 0.5 ]"), "{}", output);
        assert!(output.contains("/BBox [ 0 0 100 100 ]"), "{}", output);
        assert!(output.contains("/Extend [ false false ]"), "{}", output);
    }

    #[test]
    fn shading_pattern_is_written() {
        let gradient = Gradient::new(&[gray(0.0, 0.0), gray(1.0, 1.0)]).unwrap();
        let shading = AxialShading::new(pt(0.0, 0.0), pt(1.0, 0.0), &gradient);
        let pattern = ShadingPattern::new(shading)
            .with_matrix(Matrix::new(2.0, 0.0, 0.0, 2.0, 5.0, 6.0));
        let output = written(&pattern);
        assert!(output.contains("/Type /Pattern"), "{}", output);
        assert!(output.contains("/PatternType 2"), "{}", output);
        assert!(output.contains("/Matrix [ 2 0 0 2 5 6 ]"), "{}", output);
        assert!(output.contains("/Shading <<"), "{}", output);
    }

    #[test]
    fn matrices() {
        assert_eq!(written(&Matrix::IDENTITY), "[ 1 0 0 1 0 0 ]");
        assert_eq!(
            written(&Matrix::translation(Pt(3.0), Pt(4.0))),
            "[ 1 0 0 1 3 4 ]"
        );
        assert_eq!(written(&Matrix::scale(2.0, 0.5)), "[ 2 0 0 0.5 0 0 ]");
    }
}
