//! Drawings and texts as primitives: silkscreen for the checks, and copper
//! drawings as copper.

use lamina_model::*;

use crate::font;
use crate::prim::{Prim, circle_points};

/// A line through the points, as capsules.
pub fn stroke(pts: &[Point], width: f64, closed: bool) -> Vec<Prim> {
    let r = width / 2.0;
    let capsule = |a: Point, b: Point| Prim::Capsule { a, b, r };
    let mut out: Vec<Prim> = pts.windows(2).map(|w| capsule(w[0], w[1])).collect();
    if let (true, Some(first), Some(last)) = (closed && pts.len() > 2, pts.first(), pts.last()) {
        out.push(capsule(*last, *first));
    }
    out
}

fn poly(pts: Vec<Point>) -> Vec<Prim> {
    vec![Prim::Poly {
        outer: pts,
        holes: Vec::new(),
    }]
}

/// A drawing as primitives.
pub fn shape_prims(
    shape: &Shape,
    width: f64,
    filled: bool,
    tf: &dyn Fn(Point) -> Point,
) -> Vec<Prim> {
    let w = width.max(0.02);
    let mapped = |pts: Vec<Point>| -> Vec<Point> { pts.into_iter().map(tf).collect() };
    match shape {
        Shape::Line { .. } | Shape::Arc { .. } => stroke(&mapped(shape.points()), w, false),
        Shape::Circle { center, radius } if filled => vec![Prim::Circle {
            c: tf(*center),
            r: *radius,
        }],
        Shape::Circle { center, radius } => {
            stroke(&mapped(circle_points(*center, *radius, 0.01)), w, true)
        }
        Shape::Rect { min, max } => {
            let pts = mapped(vec![
                *min,
                Point::new(max.x, min.y),
                *max,
                Point::new(min.x, max.y),
            ]);
            if filled {
                poly(pts)
            } else {
                stroke(&pts, w, true)
            }
        }
        Shape::Polygon { points } => {
            let pts = mapped(points.clone());
            if filled {
                poly(pts)
            } else {
                stroke(&pts, w, true)
            }
        }
    }
}

/// A text as the capsules of its strokes.
pub fn text_prims(
    text: &str,
    at: Point,
    size: f64,
    rotation: f64,
    width: f64,
    side: Side,
) -> Vec<Prim> {
    font::side_text_strokes(text, at, size, rotation, side)
        .iter()
        .flat_map(|s| stroke(s, width, false))
        .collect()
}
