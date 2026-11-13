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

//! Paints the four kinds of mesh shadings to `mesh.pdf`: free-form triangles, a lattice, a
//! Coons patch and a tensor-product patch.

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use lemon_pdf::mesh::{CoonsMesh, FreeFormMesh, LatticeMesh, Patch, TensorMesh, Vertex};
use lemon_pdf::pagetree::Page;
use lemon_pdf::shading::{Point, Rgb};
use lemon_pdf::stream::StreamFilter;
use lemon_pdf::{DocumentContext, Version};

const RED: Rgb = Rgb(0.9, 0.2, 0.2);
const YELLOW: Rgb = Rgb(0.95, 0.8, 0.2);
const GREEN: Rgb = Rgb(0.2, 0.7, 0.4);
const BLUE: Rgb = Rgb(0.2, 0.3, 0.8);

/// The 12 boundary points of a square-like patch whose lower left corner is at (`x`, `y`).
fn boundary(x: f64, y: f64) -> [Point; 12] {
    let at = |dx: f64, dy: f64| Point::new(x + dx, y + dy);
    [
        at(0.0, 0.0),
        at(0.0, 33.0),
        at(0.0, 66.0),
        at(0.0, 100.0),
        at(33.0, 120.0),
        at(66.0, 80.0),
        at(100.0, 100.0),
        at(110.0, 66.0),
        at(90.0, 33.0),
        at(100.0, 0.0),
        at(66.0, -20.0),
        at(33.0, 20.0),
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut file = BufWriter::new(File::create("mesh.pdf")?);
    let mut context = DocumentContext::with_writer(&mut file, Version::Pdf1_7)?;

    // a strip of three triangles and a fan
    let triangles = FreeFormMesh::new([
        Vertex::new(50.0, 650.0, RED),
        Vertex::new(150.0, 650.0, YELLOW),
        Vertex::new(50.0, 750.0, GREEN),
    ])
    .strip(Vertex::new(150.0, 750.0, BLUE))
    .strip(Vertex::new(250.0, 700.0, RED))
    .fan(Vertex::new(250.0, 760.0, YELLOW));

    // a 4 by 3 grid with a curved shape
    let colors = [RED, YELLOW, GREEN, BLUE];
    let lattice = LatticeMesh::new(
        (0..3)
            .map(|row| {
                (0..4)
                    .map(|column| {
                        let x = 320.0 + 60.0 * column as f64;
                        let y = 650.0 + 50.0 * row as f64 + 15.0 * (column as f64).sin() * 2.0;
                        Vertex::new(x, y, colors[(row + column) % 4])
                    })
                    .collect()
            })
            .collect(),
    )?;

    let coons = CoonsMesh::new(Patch {
        points: boundary(60.0, 450.0),
        colors: [RED, YELLOW, GREEN, BLUE],
    });

    let mut points = [Point::default(); 16];
    points[..12].copy_from_slice(&boundary(300.0, 450.0));
    // the interior points, each next to a corner
    points[12] = Point::new(300.0 + 20.0, 450.0 + 20.0);
    points[13] = Point::new(300.0 + 20.0, 450.0 + 80.0);
    points[14] = Point::new(300.0 + 80.0, 450.0 + 80.0);
    points[15] = Point::new(300.0 + 80.0, 450.0 + 20.0);
    let tensor = TensorMesh::new(Patch {
        points,
        colors: [RED, YELLOW, GREEN, BLUE],
    });

    let shadings = [
        context.write_mesh_shading(&triangles)?,
        context.write_mesh_shading(&lattice)?,
        context.write_mesh_shading(&coons)?,
        context.write_mesh_shading(&tensor)?,
    ];

    let mut page = Page::new();
    page.add_content(&mut context, Some(StreamFilter::Deflate), |c| {
        for shading in shadings {
            let key = c.add_shading(shading);
            c.paint_shading(&key)?;
        }
        Ok(())
    })?;
    context.add_page(page)?;
    context.finish()?;
    Ok(())
}
