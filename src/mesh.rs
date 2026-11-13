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

//! Mesh shadings (ISO 32000-1, 8.7.4.5.5 to 8.7.4.5.8): colors that are interpolated over
//! triangles or over curved patches.
//!
//! - [`FreeFormMesh`] (type 4): triangles, each given by three vertices or continuing the
//!   previous one.
//! - [`LatticeMesh`] (type 5): a grid of vertices, drawn as triangles between neighbours.
//! - [`CoonsMesh`] (type 6): patches bounded by four Bézier curves.
//! - [`TensorMesh`] (type 7): like Coons patches, with the four inner control points given too.
//!
//! A mesh is written to the document with `DocumentContext::write_mesh_shading`, which returns
//! a [`Shading`] to use like the others (`PageContext::add_shading`, [`ShadingPattern`]).
//!
//! The colors have a type ([`Gray`], [`Rgb`] or [`Cmyk`]), which fixes the color space of the
//! shading. Coordinates are stored with 32 bits and color components with 16 bits, and a mesh
//! always uses the colors of its vertices directly (a function that maps a single value to a
//! color is not supported).
//!
//! [`ShadingPattern`]: crate::shading::ShadingPattern

use std::fmt;
use std::io::Error;

use lemon_pdf_derive::{IndirectObject, PdfDictionary};

use crate as lemon_pdf;
use crate::image::ColorSpace;
use crate::shading::{Color, Point, Shading};
use crate::stream::Stream;
use crate::DocumentContext;

/// An error from building a mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    /// A lattice has fewer than two rows.
    TooFewRows,
    /// A lattice has fewer than two vertices in a row.
    TooFewColumns,
    /// A row of a lattice has `actual` vertices, but the first one has `expected`.
    RaggedRows { expected: usize, actual: usize },
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::TooFewRows => write!(f, "a lattice needs at least two rows"),
            MeshError::TooFewColumns => {
                write!(f, "a lattice needs at least two vertices in a row")
            }
            MeshError::RaggedRows { expected, actual } => write!(
                f,
                "a row of the lattice has {} vertices, expected {}",
                actual, expected
            ),
        }
    }
}

impl std::error::Error for MeshError {}

/// A point of a mesh with its color.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Vertex<C> {
    pub position: Point,
    pub color: C,
}

impl<C> Vertex<C> {
    pub fn new(x: f64, y: f64, color: C) -> Self {
        Vertex {
            position: Point::new(x, y),
            color,
        }
    }
}

/// The dictionary of a free-form triangle mesh (type 4).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
#[constant(ShadingType = 4, BitsPerCoordinate = 32, BitsPerComponent = 16, BitsPerFlag = 8)]
pub struct FreeFormDict {
    color_space: ColorSpace,
    decode: Vec<f64>,
}

/// The dictionary of a lattice-form triangle mesh (type 5).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
#[constant(ShadingType = 5, BitsPerCoordinate = 32, BitsPerComponent = 16)]
pub struct LatticeDict {
    color_space: ColorSpace,
    vertices_per_row: u32,
    decode: Vec<f64>,
}

/// The dictionary of a Coons patch mesh (type 6).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
#[constant(ShadingType = 6, BitsPerCoordinate = 32, BitsPerComponent = 16, BitsPerFlag = 8)]
pub struct CoonsDict {
    color_space: ColorSpace,
    decode: Vec<f64>,
}

/// The dictionary of a tensor-product patch mesh (type 7).
#[derive(Debug, Clone, PartialEq, PdfDictionary)]
#[omit_type(true)]
#[constant(ShadingType = 7, BitsPerCoordinate = 32, BitsPerComponent = 16, BitsPerFlag = 8)]
pub struct TensorDict {
    color_space: ColorSpace,
    decode: Vec<f64>,
}

/// The stream of a mesh shading. References to the variants convert into references to
/// `MeshShading` (`.into()`).
#[derive(Debug, IndirectObject)]
#[indirect_variants]
pub enum MeshShading {
    FreeForm(Stream<FreeFormDict>),
    Lattice(Stream<LatticeDict>),
    Coons(Stream<CoonsDict>),
    Tensor(Stream<TensorDict>),
}

/// A mesh that can be written with `DocumentContext::write_mesh_shading`.
pub trait Mesh {
    /// The stream of the shading.
    fn to_stream(&self) -> Result<MeshShading, Error>;
}

impl<'a> DocumentContext<'a> {
    /// Writes a mesh shading to the document. The returned shading can be added to any number
    /// of pages with `PageContext::add_shading` and used in shading patterns.
    pub fn write_mesh_shading(&mut self, mesh: &impl Mesh) -> Result<Shading, Error> {
        let stream = mesh.to_stream()?;
        Ok(Shading::Mesh(self.write_object(stream)?))
    }
}

/// The data of a mesh: the stream bytes, and the ranges that the coordinates and components are
/// scaled into (`Decode`).
struct Encoder<C> {
    bytes: Vec<u8>,
    range: [f64; 4],
    _color: std::marker::PhantomData<C>,
}

impl<C: Color> Encoder<C> {
    /// The coordinates are scaled into the bounding box of `points`.
    fn new(points: impl Iterator<Item = Point>) -> Self {
        let mut range = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for point in points {
            for (value, bounds) in [(point.x.0, 0), (point.y.0, 2)] {
                if value.is_finite() {
                    range[bounds] = range[bounds].min(value);
                    range[bounds + 1] = range[bounds + 1].max(value);
                }
            }
        }
        for bounds in [0, 2] {
            if range[bounds] > range[bounds + 1] {
                // no finite point
                range[bounds] = 0.0;
                range[bounds + 1] = 0.0;
            }
        }
        Encoder {
            bytes: Vec::new(),
            range,
            _color: std::marker::PhantomData,
        }
    }

    /// The `Decode` array: the ranges of x and y, then 0 to 1 for every color component.
    fn decode(&self) -> Vec<f64> {
        let mut decode = self.range.to_vec();
        for _ in 0..C::SPACE.components() {
            decode.extend_from_slice(&[0.0, 1.0]);
        }
        decode
    }

    fn flag(&mut self, flag: u8) {
        self.bytes.push(flag);
    }

    fn point(&mut self, point: Point) {
        for (value, bounds) in [(point.x.0, 0), (point.y.0, 2)] {
            let (min, max) = (self.range[bounds], self.range[bounds + 1]);
            let quantized = if max > min && value.is_finite() {
                (((value - min) / (max - min)).max(0.0).min(1.0) * f64::from(u32::MAX)).round()
                    as u32
            } else {
                0
            };
            self.bytes.extend_from_slice(&quantized.to_be_bytes());
        }
    }

    fn color(&mut self, color: C) {
        for component in color.components() {
            let quantized = (component.max(0.0).min(1.0) * f64::from(u16::MAX)).round() as u16;
            self.bytes.extend_from_slice(&quantized.to_be_bytes());
        }
    }

    fn vertex(&mut self, vertex: Vertex<C>) {
        self.point(vertex.position);
        self.color(vertex.color);
    }
}

/// How a triangle after the first one of a free-form mesh continues the one before it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TriangleStep {
    /// The triangle uses the second and third vertex of the previous triangle (a strip).
    Strip,
    /// The triangle uses the first and third vertex of the previous triangle (a fan).
    Fan,
}

/// A triangle of a free-form mesh after the first one.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum NextTriangle<C> {
    /// A triangle with three vertices of its own.
    New([Vertex<C>; 3]),
    /// A triangle that shares an edge with the previous one and adds a vertex.
    Continue(TriangleStep, Vertex<C>),
}

/// A free-form triangle mesh (type 4).
#[derive(Debug, Clone, PartialEq)]
pub struct FreeFormMesh<C> {
    first: [Vertex<C>; 3],
    rest: Vec<NextTriangle<C>>,
}

impl<C: Color> FreeFormMesh<C> {
    /// A mesh with one triangle.
    pub fn new(first: [Vertex<C>; 3]) -> Self {
        FreeFormMesh {
            first,
            rest: Vec::new(),
        }
    }

    /// Adds a triangle with three vertices of its own.
    pub fn triangle(mut self, vertices: [Vertex<C>; 3]) -> Self {
        self.rest.push(NextTriangle::New(vertices));
        self
    }

    /// Adds a triangle from the last two vertices of the previous triangle and `vertex`.
    pub fn strip(mut self, vertex: Vertex<C>) -> Self {
        self.rest
            .push(NextTriangle::Continue(TriangleStep::Strip, vertex));
        self
    }

    /// Adds a triangle from the first and the last vertex of the previous triangle and `vertex`.
    pub fn fan(mut self, vertex: Vertex<C>) -> Self {
        self.rest
            .push(NextTriangle::Continue(TriangleStep::Fan, vertex));
        self
    }
}

impl<C: Color> Mesh for FreeFormMesh<C> {
    fn to_stream(&self) -> Result<MeshShading, Error> {
        let mut positions: Vec<Point> = self.first.iter().map(|v| v.position).collect();
        for next in &self.rest {
            match next {
                NextTriangle::New(vertices) => {
                    positions.extend(vertices.iter().map(|v| v.position))
                }
                NextTriangle::Continue(_, vertex) => positions.push(vertex.position),
            }
        }
        let mut encoder = Encoder::<C>::new(positions.into_iter());
        // the flags of the vertices after the first of a new triangle are not used
        for &vertex in &self.first {
            encoder.flag(0);
            encoder.vertex(vertex);
        }
        for next in &self.rest {
            match *next {
                NextTriangle::New(vertices) => {
                    for vertex in vertices.iter() {
                        encoder.flag(0);
                        encoder.vertex(*vertex);
                    }
                }
                NextTriangle::Continue(step, vertex) => {
                    encoder.flag(match step {
                        TriangleStep::Strip => 1,
                        TriangleStep::Fan => 2,
                    });
                    encoder.vertex(vertex);
                }
            }
        }
        let dict = FreeFormDict {
            color_space: C::SPACE,
            decode: encoder.decode(),
        };
        Ok(MeshShading::FreeForm(Stream::deflated(dict, &encoder.bytes)?))
    }
}

/// A lattice-form triangle mesh (type 5): rows of vertices, where every four neighbours make
/// two triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct LatticeMesh<C> {
    rows: Vec<Vec<Vertex<C>>>,
}

impl<C: Color> LatticeMesh<C> {
    /// A mesh from rows of vertices: at least two rows with the same number of at least two
    /// vertices.
    pub fn new(rows: Vec<Vec<Vertex<C>>>) -> Result<Self, MeshError> {
        if rows.len() < 2 {
            return Err(MeshError::TooFewRows);
        }
        let expected = rows[0].len();
        if expected < 2 {
            return Err(MeshError::TooFewColumns);
        }
        if let Some(row) = rows.iter().find(|row| row.len() != expected) {
            return Err(MeshError::RaggedRows {
                expected,
                actual: row.len(),
            });
        }
        Ok(LatticeMesh { rows })
    }
}

impl<C: Color> Mesh for LatticeMesh<C> {
    fn to_stream(&self) -> Result<MeshShading, Error> {
        let mut encoder = Encoder::<C>::new(
            self.rows
                .iter()
                .flat_map(|row| row.iter().map(|vertex| vertex.position)),
        );
        for &vertex in self.rows.iter().flatten() {
            encoder.vertex(vertex);
        }
        let dict = LatticeDict {
            color_space: C::SPACE,
            vertices_per_row: self.rows[0].len() as u32,
            decode: encoder.decode(),
        };
        Ok(MeshShading::Lattice(Stream::deflated(dict, &encoder.bytes)?))
    }
}

/// The edge of the previous patch that a patch after the first one shares. The edges are
/// numbered in the order of the control points of the boundary, so that the first edge runs
/// through the first four points, the second through the fourth to the seventh and so on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SharedEdge {
    /// The edge from the fourth to the seventh control point of the previous patch.
    Second,
    /// The edge from the seventh to the tenth control point.
    Third,
    /// The edge from the tenth control point to the first.
    Fourth,
}

impl SharedEdge {
    fn flag(self) -> u8 {
        match self {
            SharedEdge::Second => 1,
            SharedEdge::Third => 2,
            SharedEdge::Fourth => 3,
        }
    }
}

/// A patch with all of its control points and the colors of its four corners.
///
/// The points are in the order of the data stream: the control points of the four Bézier curves
/// of the boundary, going around the patch (12 points, the end of each curve is the start of
/// the next one and the first point is also the end of the last curve), then, for a tensor
/// patch, the four interior points, each next to one of the corners, in the order of the
/// corners. The colors are those of the corners at the points 0, 3, 6 and 9.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Patch<C, const N: usize> {
    pub points: [Point; N],
    pub colors: [C; 4],
}

/// A patch after the first one that shares an edge with the previous patch: it gives the points
/// and the two corner colors the shared edge does not provide. The shared edge becomes the
/// first edge of the new patch.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PatchContinuation<C, const M: usize> {
    pub edge: SharedEdge,
    pub points: [Point; M],
    /// The colors of the corners at the points 6 and 9 of the new patch.
    pub colors: [C; 2],
}

/// A patch of a mesh after the first one.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum NextPatch<C, const N: usize, const M: usize> {
    New(Patch<C, N>),
    Continue(PatchContinuation<C, M>),
}

/// A mesh of patches. `N` is the number of control points of a patch, `M` the number of the
/// points that a patch sharing an edge adds; use [`CoonsMesh`] or [`TensorMesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct PatchMesh<C, const N: usize, const M: usize> {
    first: Patch<C, N>,
    rest: Vec<NextPatch<C, N, M>>,
}

/// A Coons patch mesh (type 6): every patch has 12 control points.
pub type CoonsMesh<C> = PatchMesh<C, 12, 8>;
/// A tensor-product patch mesh (type 7): every patch has 16 control points.
pub type TensorMesh<C> = PatchMesh<C, 16, 12>;
pub type CoonsPatch<C> = Patch<C, 12>;
pub type TensorPatch<C> = Patch<C, 16>;

impl<C: Color, const N: usize, const M: usize> PatchMesh<C, N, M> {
    /// A mesh with one patch.
    pub fn new(first: Patch<C, N>) -> Self {
        PatchMesh {
            first,
            rest: Vec::new(),
        }
    }

    /// Adds a patch with all of its control points.
    pub fn patch(mut self, patch: Patch<C, N>) -> Self {
        self.rest.push(NextPatch::New(patch));
        self
    }

    /// Adds a patch that shares an edge with the previous one.
    pub fn continue_patch(mut self, patch: PatchContinuation<C, M>) -> Self {
        self.rest.push(NextPatch::Continue(patch));
        self
    }

    /// The stream data and the `Decode` array.
    fn encode(&self) -> (Vec<u8>, Vec<f64>) {
        let mut points: Vec<Point> = self.first.points.to_vec();
        for next in &self.rest {
            match next {
                NextPatch::New(patch) => points.extend_from_slice(&patch.points),
                NextPatch::Continue(patch) => points.extend_from_slice(&patch.points),
            }
        }
        let mut encoder = Encoder::<C>::new(points.into_iter());
        let new_patch = |encoder: &mut Encoder<C>, patch: &Patch<C, N>| {
            encoder.flag(0);
            for &point in &patch.points {
                encoder.point(point);
            }
            for &color in &patch.colors {
                encoder.color(color);
            }
        };
        new_patch(&mut encoder, &self.first);
        for next in &self.rest {
            match next {
                NextPatch::New(patch) => new_patch(&mut encoder, patch),
                NextPatch::Continue(patch) => {
                    encoder.flag(patch.edge.flag());
                    for &point in &patch.points {
                        encoder.point(point);
                    }
                    for &color in &patch.colors {
                        encoder.color(color);
                    }
                }
            }
        }
        let decode = encoder.decode();
        (encoder.bytes, decode)
    }
}

impl<C: Color> Mesh for CoonsMesh<C> {
    fn to_stream(&self) -> Result<MeshShading, Error> {
        let (bytes, decode) = self.encode();
        let dict = CoonsDict {
            color_space: C::SPACE,
            decode,
        };
        Ok(MeshShading::Coons(Stream::deflated(dict, &bytes)?))
    }
}

impl<C: Color> Mesh for TensorMesh<C> {
    fn to_stream(&self) -> Result<MeshShading, Error> {
        let (bytes, decode) = self.encode();
        let dict = TensorDict {
            color_space: C::SPACE,
            decode,
        };
        Ok(MeshShading::Tensor(Stream::deflated(dict, &bytes)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Formatter;
    use crate::shading::{Gray, Rgb};
    use crate::stream::Stream;

    fn inflate(stream: &Stream<impl Sized>) -> Vec<u8> {
        use std::io::Read;
        let mut data = Vec::new();
        flate2::read::ZlibDecoder::new(stream.bytes())
            .read_to_end(&mut data)
            .unwrap();
        data
    }

    fn red(x: f64, y: f64) -> Vertex<Rgb> {
        Vertex::new(x, y, Rgb(1.0, 0.0, 0.0))
    }

    fn written_dict(stream: &MeshShading) -> String {
        macro_rules! dict_of {
            ($stream:expr) => {{
                let mut buffer = Vec::new();
                lemon_pdf::object::PdfDictionary::write_entries(
                    &$stream.dict,
                    (&mut Formatter {
                        writer: &mut buffer,
                    })
                    .format_dictionary(),
                )
                .finish()
                .unwrap();
                String::from_utf8(buffer).unwrap()
            }};
        }
        match stream {
            MeshShading::FreeForm(stream) => dict_of!(stream),
            MeshShading::Lattice(stream) => dict_of!(stream),
            MeshShading::Coons(stream) => dict_of!(stream),
            MeshShading::Tensor(stream) => dict_of!(stream),
        }
    }

    #[test]
    fn free_form_vertices_have_flag_position_and_color() {
        let mesh = FreeFormMesh::new([
            Vertex::new(0.0, 0.0, Rgb(1.0, 0.0, 0.0)),
            Vertex::new(10.0, 0.0, Rgb(0.0, 1.0, 0.0)),
            Vertex::new(0.0, 20.0, Rgb(0.0, 0.0, 1.0)),
        ])
        .strip(red(10.0, 20.0))
        .fan(red(5.0, 5.0))
        .triangle([red(1.0, 1.0), red(2.0, 2.0), red(3.0, 3.0)]);
        let stream = mesh.to_stream().unwrap();
        let MeshShading::FreeForm(stream) = &stream else {
            panic!()
        };
        let data = inflate(stream);
        // 1 flag byte, 2 coordinates of 4 bytes and 3 components of 2 bytes
        let vertex = 1 + 8 + 6;
        assert_eq!(data.len(), 3 * vertex + 2 * vertex + 3 * vertex);
        // the first vertex is the corner of the bounding box: both coordinates are 0
        assert_eq!(&data[..9], &[0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // and its color is red
        assert_eq!(&data[9..15], &[0xFF, 0xFF, 0, 0, 0, 0]);
        // the second vertex has the maximum x
        assert_eq!(&data[vertex + 1..vertex + 5], &[0xFF, 0xFF, 0xFF, 0xFF]);
        // flags: 0 0 0, strip 1, fan 2, then a new triangle 0 0 0
        let flags: Vec<u8> = (0..8).map(|i| data[i * vertex]).collect();
        assert_eq!(flags, vec![0, 0, 0, 1, 2, 0, 0, 0]);
    }

    #[test]
    fn free_form_dictionary() {
        let mesh = FreeFormMesh::new([red(10.0, 20.0), red(30.0, 20.0), red(10.0, 60.0)]);
        let output = written_dict(&mesh.to_stream().unwrap());
        assert!(output.contains("/ShadingType 4"), "{}", output);
        assert!(output.contains("/BitsPerCoordinate 32"), "{}", output);
        assert!(output.contains("/BitsPerComponent 16"), "{}", output);
        assert!(output.contains("/BitsPerFlag 8"), "{}", output);
        assert!(output.contains("/ColorSpace /DeviceRGB"), "{}", output);
        assert!(
            output.contains("/Decode [ 10 30 20 60 0 1 0 1 0 1 ]"),
            "{}",
            output
        );
    }

    #[test]
    fn lattice_has_no_flags_and_checks_its_shape() {
        let row = |y| vec![red(0.0, y), red(1.0, y), red(2.0, y)];
        let mesh = LatticeMesh::new(vec![row(0.0), row(1.0)]).unwrap();
        let stream = mesh.to_stream().unwrap();
        let output = written_dict(&stream);
        assert!(output.contains("/ShadingType 5"), "{}", output);
        assert!(output.contains("/VerticesPerRow 3"), "{}", output);
        assert!(!output.contains("BitsPerFlag"), "{}", output);
        let MeshShading::Lattice(stream) = &stream else {
            panic!()
        };
        assert_eq!(inflate(stream).len(), 6 * (8 + 6));

        assert_eq!(
            LatticeMesh::<Rgb>::new(vec![row(0.0)]).unwrap_err(),
            MeshError::TooFewRows
        );
        assert_eq!(
            LatticeMesh::new(vec![vec![red(0.0, 0.0)], vec![red(0.0, 1.0)]]).unwrap_err(),
            MeshError::TooFewColumns
        );
        assert_eq!(
            LatticeMesh::new(vec![row(0.0), vec![red(0.0, 1.0), red(1.0, 1.0)]]).unwrap_err(),
            MeshError::RaggedRows {
                expected: 3,
                actual: 2
            }
        );
    }

    fn coons_patch(offset: f64) -> CoonsPatch<Gray> {
        let mut points = [Point::default(); 12];
        for (i, point) in points.iter_mut().enumerate() {
            *point = Point::new(offset + i as f64, offset - i as f64);
        }
        Patch {
            points,
            colors: [Gray(0.0), Gray(0.25), Gray(0.5), Gray(1.0)],
        }
    }

    #[test]
    fn coons_patches() {
        let mesh = CoonsMesh::new(coons_patch(0.0))
            .continue_patch(PatchContinuation {
                edge: SharedEdge::Third,
                points: [Point::new(1.0, 1.0); 8],
                colors: [Gray(0.5), Gray(0.5)],
            })
            .patch(coons_patch(5.0));
        let stream = mesh.to_stream().unwrap();
        let output = written_dict(&stream);
        assert!(output.contains("/ShadingType 6"), "{}", output);
        assert!(output.contains("/ColorSpace /DeviceGray"), "{}", output);
        let MeshShading::Coons(stream) = &stream else {
            panic!()
        };
        let data = inflate(stream);
        let full = 1 + 12 * 8 + 4 * 2;
        let continued = 1 + 8 * 8 + 2 * 2;
        assert_eq!(data.len(), full + continued + full);
        assert_eq!(data[0], 0);
        assert_eq!(data[full], 2);
        assert_eq!(data[full + continued], 0);
    }

    #[test]
    fn tensor_patches() {
        let patch = |offset| {
            let mut points = [Point::default(); 16];
            for (i, point) in points.iter_mut().enumerate() {
                *point = Point::new(i as f64 + offset, 0.0);
            }
            Patch {
                points,
                colors: [Rgb(0.0, 0.0, 0.0); 4],
            }
        };
        let mesh = TensorMesh::new(patch(0.0)).continue_patch(PatchContinuation {
            edge: SharedEdge::Fourth,
            points: [Point::default(); 12],
            colors: [Rgb(1.0, 1.0, 1.0); 2],
        });
        let stream = mesh.to_stream().unwrap();
        assert!(written_dict(&stream).contains("/ShadingType 7"));
        let MeshShading::Tensor(stream) = &stream else {
            panic!()
        };
        let data = inflate(stream);
        let full = 1 + 16 * 8 + 4 * 6;
        let continued = 1 + 12 * 8 + 2 * 6;
        assert_eq!(data.len(), full + continued);
        assert_eq!(data[full], 3);
    }

    #[test]
    fn degenerate_and_non_finite_input_is_encoded() {
        // all points are equal, and one is not finite
        let mesh = FreeFormMesh::new([
            red(1.0, 1.0),
            red(1.0, 1.0),
            Vertex::new(f64::NAN, f64::INFINITY, Rgb(2.0, -1.0, f64::NAN)),
        ]);
        let stream = mesh.to_stream().unwrap();
        let MeshShading::FreeForm(stream) = &stream else {
            panic!()
        };
        let data = inflate(stream);
        assert_eq!(data.len(), 3 * 15);
        let output = written_dict(&MeshShading::FreeForm(Stream::new(
            FreeFormDict {
                color_space: ColorSpace::DeviceRGB,
                decode: Encoder::<Rgb>::new(std::iter::empty()).decode(),
            },
            Vec::new(),
        )));
        assert!(output.contains("/Decode [ 0 0 0 0 0 1 0 1 0 1 ]"), "{}", output);
    }

    #[test]
    fn colors_are_clamped() {
        let mut encoder = Encoder::<Rgb>::new(std::iter::empty());
        encoder.color(Rgb(2.0, -1.0, 0.5));
        assert_eq!(encoder.bytes, vec![0xFF, 0xFF, 0, 0, 0x80, 0x00]);
    }
}
