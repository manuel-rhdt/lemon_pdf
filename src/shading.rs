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
//! Both kinds of shading in this module have a single color per position, so they cannot be
//! transparent. Shadings of other types (function based, mesh) are not supported.

use std::fmt;

use lemon_pdf_derive::PdfFormat;

use crate as lemon_pdf;
use crate::content::Pt;
use crate::image::ColorSpace;

/// An error from building a function or a shading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadingError {
    /// There is no color stop.
    NoStops,
    /// The colors do not all have `expected` components.
    Components { expected: usize, actual: usize },
}

impl fmt::Display for ShadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShadingError::NoStops => write!(f, "a gradient needs at least one color stop"),
            ShadingError::Components { expected, actual } => write!(
                f,
                "a color has {} components, expected {}",
                actual, expected
            ),
        }
    }
}

impl std::error::Error for ShadingError {}

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
pub struct ExponentialFunction {
    function_type: i64,
    domain: [f64; 2],
    #[rename("C0")]
    c0: Vec<f64>,
    #[rename("C1")]
    c1: Vec<f64>,
    #[rename("N")]
    exponent: f64,
}

impl ExponentialFunction {
    /// Interpolates linearly from `from` at 0 to `to` at 1. Both have one value for every
    /// component of the color space.
    pub fn linear(from: Vec<f64>, to: Vec<f64>) -> Result<Self, ShadingError> {
        if from.len() != to.len() {
            return Err(ShadingError::Components {
                expected: from.len(),
                actual: to.len(),
            });
        }
        Ok(ExponentialFunction {
            function_type: 2,
            domain: [0.0, 1.0],
            c0: from,
            c1: to,
            exponent: 1.0,
        })
    }
}

/// A stitching function (type 3): several functions, each for a part of the domain.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct StitchingFunction {
    function_type: i64,
    domain: [f64; 2],
    functions: Vec<Function>,
    /// The ends of all parts of the domain but the last.
    bounds: Vec<f64>,
    /// Maps every part of the domain to the domain of its function, two numbers per function.
    encode: Vec<f64>,
}

/// A color at a position of a gradient, `offset` in `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorStop {
    pub offset: f64,
    /// One value from 0 to 1 for every component of the color space.
    pub color: Vec<f64>,
}

impl ColorStop {
    pub fn new(offset: f64, color: impl Into<Vec<f64>>) -> Self {
        ColorStop {
            offset,
            color: color.into(),
        }
    }
}

impl Function {
    /// The function of a gradient with the given color stops, for a domain of `0..=1`.
    ///
    /// Offsets are clamped to `0..=1` and must not decrease; one that is smaller than the one
    /// before it is raised to it. Two stops at the same offset make a hard transition. The
    /// color of the first stop is used before it and the one of the last stop after it.
    pub fn gradient(stops: &[ColorStop]) -> Result<Function, ShadingError> {
        let first = stops.first().ok_or(ShadingError::NoStops)?;
        let components = first.color.len();
        if let Some(stop) = stops.iter().find(|stop| stop.color.len() != components) {
            return Err(ShadingError::Components {
                expected: components,
                actual: stop.color.len(),
            });
        }

        let mut points: Vec<(f64, &[f64])> = Vec::with_capacity(stops.len() + 2);
        let mut previous = 0.0f64;
        for stop in stops {
            let offset = if stop.offset.is_nan() {
                previous
            } else {
                stop.offset.max(previous).min(1.0)
            };
            previous = offset;
            points.push((offset, &stop.color));
        }

        // the parts of the domain, each from one point to the next
        let mut parts: Vec<(f64, f64, &[f64], &[f64])> = Vec::new();
        if points[0].0 > 0.0 {
            parts.push((0.0, points[0].0, points[0].1, points[0].1));
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

        let functions = parts
            .iter()
            .map(|&(_, _, from, to)| {
                ExponentialFunction::linear(from.to_vec(), to.to_vec()).map(Function::Exponential)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if functions.len() == 1 {
            return Ok(functions.into_iter().next().unwrap());
        }
        if functions.is_empty() {
            // all stops are at 1: a hard transition to the last color at the end
            return ExponentialFunction::linear(last.1.to_vec(), last.1.to_vec())
                .map(Function::Exponential);
        }
        Ok(Function::Stitching(StitchingFunction {
            function_type: 3,
            domain: [0.0, 1.0],
            bounds: parts[..parts.len() - 1].iter().map(|part| part.1).collect(),
            encode: functions.iter().flat_map(|_| [0.0, 1.0]).collect(),
            functions,
        }))
    }
}

/// A shading (ISO 32000-1, 8.7.4.3).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
pub enum Shading {
    Axial(AxialShading),
    Radial(RadialShading),
}

impl Shading {
    /// A shading that changes along the line from (`x0`, `y0`) to (`x1`, `y1`) in the user
    /// space it is painted in: it has the color of the first stop at the start, the color of
    /// the last stop at the end, and is constant across the line. `extend` says whether the
    /// colors of the ends continue beyond the start and the end of the line.
    pub fn axial(
        color_space: ColorSpace,
        from: (f64, f64),
        to: (f64, f64),
        stops: &[ColorStop],
        extend: bool,
    ) -> Result<Shading, ShadingError> {
        Ok(Shading::Axial(AxialShading {
            shading_type: 2,
            color_space,
            background: None,
            bbox: None,
            anti_alias: false,
            coords: [from.0, from.1, to.0, to.1],
            domain: None,
            function: Self::checked_function(color_space, stops)?,
            extend: [extend, extend],
        }))
    }

    /// A shading between two circles, each given by its center and radius: the color of the
    /// first stop is at the first circle, the one of the last stop at the second, and the
    /// circles in between are interpolated. `extend` says whether the shading continues
    /// beyond both circles.
    pub fn radial(
        color_space: ColorSpace,
        start: (f64, f64, f64),
        end: (f64, f64, f64),
        stops: &[ColorStop],
        extend: bool,
    ) -> Result<Shading, ShadingError> {
        Ok(Shading::Radial(RadialShading {
            shading_type: 3,
            color_space,
            background: None,
            bbox: None,
            anti_alias: false,
            coords: [start.0, start.1, start.2, end.0, end.1, end.2],
            domain: None,
            function: Self::checked_function(color_space, stops)?,
            extend: [extend, extend],
        }))
    }

    fn checked_function(
        color_space: ColorSpace,
        stops: &[ColorStop],
    ) -> Result<Function, ShadingError> {
        let expected = color_space.components() as usize;
        if let Some(stop) = stops.iter().find(|stop| stop.color.len() != expected) {
            return Err(ShadingError::Components {
                expected,
                actual: stop.color.len(),
            });
        }
        Function::gradient(stops)
    }

    /// Sets the color that is painted where the shading does not paint (only used by shading
    /// patterns, not by `paint_shading`).
    pub fn with_background(mut self, background: Vec<f64>) -> Self {
        match &mut self {
            Shading::Axial(shading) => shading.background = Some(background),
            Shading::Radial(shading) => shading.background = Some(background),
        }
        self
    }

    /// Limits the shading to a rectangle (`x0`, `y0`, `x1`, `y1`) of the space it is painted in.
    pub fn with_bbox(mut self, bbox: [Pt; 4]) -> Self {
        match &mut self {
            Shading::Axial(shading) => shading.bbox = Some(bbox),
            Shading::Radial(shading) => shading.bbox = Some(bbox),
        }
        self
    }
}

/// An axial shading (type 2).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct AxialShading {
    shading_type: i64,
    color_space: ColorSpace,
    background: Option<Vec<f64>>,
    #[rename("BBox")]
    bbox: Option<[Pt; 4]>,
    #[skip_if(is_false)]
    anti_alias: bool,
    coords: [f64; 4],
    domain: Option<[f64; 2]>,
    function: Function,
    extend: [bool; 2],
}

/// A radial shading (type 3).
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[omit_type(true)]
pub struct RadialShading {
    shading_type: i64,
    color_space: ColorSpace,
    background: Option<Vec<f64>>,
    #[rename("BBox")]
    bbox: Option<[Pt; 4]>,
    #[skip_if(is_false)]
    anti_alias: bool,
    coords: [f64; 6],
    domain: Option<[f64; 2]>,
    function: Function,
    extend: [bool; 2],
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A shading pattern (ISO 32000-1, 8.7.4.2): a shading used as the color of fills and strokes.
///
/// The coordinates of the shading are in the pattern space, which is the default coordinate
/// system of the page transformed by `matrix`, independent of the transformations applied while
/// the page is drawn.
#[derive(Debug, Clone, PartialEq, PdfFormat)]
#[rename("Pattern")]
pub struct ShadingPattern {
    pattern_type: i64,
    shading: Shading,
    matrix: Option<[f64; 6]>,
}

impl ShadingPattern {
    pub fn new(shading: Shading) -> Self {
        ShadingPattern {
            pattern_type: 2,
            shading,
            matrix: None,
        }
    }

    /// Transforms the pattern space, like the operands of the `cm` operator.
    pub fn with_matrix(mut self, matrix: [f64; 6]) -> Self {
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

    fn stop(offset: f64, color: &[f64]) -> ColorStop {
        ColorStop::new(offset, color.to_vec())
    }

    #[test]
    fn two_stops_are_one_exponential_function() {
        let function =
            Function::gradient(&[stop(0.0, &[1.0, 0.0, 0.0]), stop(1.0, &[0.0, 0.0, 1.0])])
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
        let function = Function::gradient(&[
            stop(0.0, &[1.0]),
            stop(0.25, &[0.0]),
            stop(1.0, &[0.5]),
        ])
        .unwrap();
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
        let function = Function::gradient(&[stop(0.25, &[1.0]), stop(0.75, &[0.0])]).unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        assert_eq!(stitched.functions.len(), 3);
        assert_eq!(stitched.bounds, vec![0.25, 0.75]);
    }

    #[test]
    fn hard_transitions_have_no_part_of_their_own() {
        let function = Function::gradient(&[
            stop(0.0, &[1.0]),
            stop(0.5, &[1.0]),
            stop(0.5, &[0.0]),
            stop(1.0, &[0.0]),
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
            stop(-1.0, &[0.0]),
            stop(0.6, &[0.5]),
            stop(0.2, &[1.0]),
            stop(f64::NAN, &[1.0]),
        ])
        .unwrap();
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        // the stops are at 0, 0.6, 0.6 and 0.6: the parts are 0..0.6 and the constant 0.6..1
        assert_eq!(stitched.bounds, vec![0.6]);
    }

    #[test]
    fn a_single_stop_is_a_constant() {
        let function = Function::gradient(&[stop(0.3, &[0.2, 0.4, 0.6])]).unwrap();
        let output = written(&function);
        assert!(output.contains("/FunctionType 3"), "{}", output);
        let Function::Stitching(stitched) = &function else {
            panic!("{:?}", function)
        };
        assert_eq!(stitched.functions.len(), 2);
    }

    #[test]
    fn invalid_stops_are_errors() {
        assert_eq!(Function::gradient(&[]), Err(ShadingError::NoStops));
        assert_eq!(
            Function::gradient(&[stop(0.0, &[1.0]), stop(1.0, &[1.0, 0.0])]),
            Err(ShadingError::Components {
                expected: 1,
                actual: 2
            })
        );
        assert_eq!(
            Shading::axial(
                ColorSpace::DeviceRGB,
                (0.0, 0.0),
                (1.0, 0.0),
                &[stop(0.0, &[1.0]), stop(1.0, &[0.0])],
                false
            ),
            Err(ShadingError::Components {
                expected: 3,
                actual: 1
            })
        );
    }

    #[test]
    fn axial_shading_is_written() {
        let shading = Shading::axial(
            ColorSpace::DeviceRGB,
            (10.0, 20.0),
            (110.0, 20.0),
            &[stop(0.0, &[1.0, 1.0, 0.0]), stop(1.0, &[0.0, 0.0, 0.5])],
            true,
        )
        .unwrap();
        let output = written(&shading);
        assert!(output.contains("/ShadingType 2"), "{}", output);
        assert!(output.contains("/ColorSpace /DeviceRGB"), "{}", output);
        assert!(output.contains("/Coords [ 10 20 110 20 ]"), "{}", output);
        assert!(output.contains("/Extend [ true true ]"), "{}", output);
        assert!(output.contains("/Function <<"), "{}", output);
        assert!(!output.contains("/AntiAlias"), "{}", output);
        assert!(!output.contains("/Background"), "{}", output);
    }

    #[test]
    fn radial_shading_is_written_with_options() {
        let shading = Shading::radial(
            ColorSpace::DeviceGray,
            (50.0, 50.0, 0.0),
            (50.0, 50.0, 40.0),
            &[stop(0.0, &[1.0]), stop(1.0, &[0.0])],
            false,
        )
        .unwrap()
        .with_background(vec![0.5])
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
        let shading = Shading::axial(
            ColorSpace::DeviceGray,
            (0.0, 0.0),
            (1.0, 0.0),
            &[stop(0.0, &[0.0]), stop(1.0, &[1.0])],
            false,
        )
        .unwrap();
        let pattern = ShadingPattern::new(shading).with_matrix([2.0, 0.0, 0.0, 2.0, 5.0, 6.0]);
        let output = written(&pattern);
        assert!(output.contains("/Type /Pattern"), "{}", output);
        assert!(output.contains("/PatternType 2"), "{}", output);
        assert!(output.contains("/Matrix [ 2 0 0 2 5 6 ]"), "{}", output);
        assert!(output.contains("/Shading <<"), "{}", output);
    }
}
