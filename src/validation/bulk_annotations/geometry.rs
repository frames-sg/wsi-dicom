//! The plane geometry of annotation outlines: segment crossings, winding
//! areas and the vector arithmetic the shape checks use.

/// The first pair of line segments (0-based, in the order of the points)
/// that cross or touch other than at the vertex adjacent segments share, with
/// the outline projected onto the plane its normal is least aligned with.
/// Segments are compared by a sweep over their X extents, at most `budget`
/// pairs in all; `None` when no pair crosses or the budget runs out.
pub(super) fn first_crossing(
    points: &[[f64; 3]],
    closed: bool,
    budget: &mut u64,
) -> Option<(usize, usize)> {
    let plane = projection(points);
    let mut vertices: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for point in points {
        let projected = [point[plane.0], point[plane.1]];
        // A repeated vertex is a zero-length segment, not a crossing.
        if vertices.last() != Some(&projected) {
            vertices.push(projected);
        }
    }
    if closed && vertices.len() > 1 && vertices.first() == vertices.last() {
        vertices.pop();
    }
    let count = if closed {
        vertices.len()
    } else {
        vertices.len().saturating_sub(1)
    };
    if count < 2 {
        return None;
    }
    let segment = |index: usize| (vertices[index], vertices[(index + 1) % vertices.len()]);
    let mut order: Vec<usize> = (0..count).collect();
    let low = |index: usize| segment(index).0[0].min(segment(index).1[0]);
    let high = |index: usize| segment(index).0[0].max(segment(index).1[0]);
    order.sort_by(|a, b| low(*a).total_cmp(&low(*b)));
    let mut active: Vec<usize> = Vec::new();
    for current in order {
        active.retain(|other| high(*other) >= low(current));
        for other in &active {
            if *budget == 0 {
                return None;
            }
            *budget -= 1;
            let (first, second) = (current.min(*other), current.max(*other));
            let adjacent = second - first == 1 || (closed && first == 0 && second == count - 1);
            let crosses = if adjacent {
                folds_back(segment(first), segment(second))
            } else {
                intersects(segment(first), segment(second))
            };
            if crosses {
                return Some((first, second));
            }
        }
        active.push(current);
    }
    None
}

type Segment = ([f64; 2], [f64; 2]);

fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn on_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> bool {
    point[0] >= a[0].min(b[0])
        && point[0] <= a[0].max(b[0])
        && point[1] >= a[1].min(b[1])
        && point[1] <= a[1].max(b[1])
}

/// Whether two segments share any point.
fn intersects((a, b): Segment, (c, d): Segment) -> bool {
    let (o1, o2, o3, o4) = (
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    );
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    (o1 == 0.0 && on_segment(a, b, c))
        || (o2 == 0.0 && on_segment(a, b, d))
        || (o3 == 0.0 && on_segment(c, d, a))
        || (o4 == 0.0 && on_segment(c, d, b))
}

/// Whether two segments sharing a vertex overlap along a line beyond it.
fn folds_back(first: Segment, second: Segment) -> bool {
    let shared = if first.1 == second.0 || first.1 == second.1 {
        first.1
    } else {
        first.0
    };
    let far = |segment: Segment| {
        if segment.0 == shared {
            segment.1
        } else {
            segment.0
        }
    };
    let (p, q) = (far(first), far(second));
    if orientation(shared, p, q) != 0.0 {
        return false;
    }
    // Collinear: they overlap when both run the same way from the vertex.
    dot2(sub2(p, shared), sub2(q, shared)) > 0.0
}

/// The two axes to project an outline onto: those of the plane its normal
/// is least aligned with, so a slide-plane outline keeps X and Y.
fn projection(points: &[[f64; 3]]) -> (usize, usize) {
    let normal = newell_normal(points);
    let dropped = (0..3)
        .max_by(|a, b| normal[*a].abs().total_cmp(&normal[*b].abs()))
        .unwrap_or(2);
    match dropped {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// Twice the signed area of the outline closed through its first point, in
/// its first two coordinates; positive when counter-clockwise with the second
/// axis up.
pub(super) fn signed_area(points: &[[f64; 3]]) -> f64 {
    let count = points.len();
    (0..count)
        .map(|index| {
            let (a, b) = (points[index], points[(index + 1) % count]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

pub(super) fn newell_normal(points: &[[f64; 3]]) -> [f64; 3] {
    let count = points.len();
    (0..count).fold([0.0; 3], |normal, index| {
        let (a, b) = (points[index], points[(index + 1) % count]);
        [
            normal[0] + (a[1] - b[1]) * (a[2] + b[2]),
            normal[1] + (a[2] - b[2]) * (a[0] + b[0]),
            normal[2] + (a[0] - b[0]) * (a[1] + b[1]),
        ]
    })
}

pub(super) fn extent(points: &[[f64; 3]]) -> f64 {
    (0..3)
        .map(|axis| {
            let (low, high) = points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), point| {
                    (low.min(point[axis]), high.max(point[axis]))
                });
            high - low
        })
        .fold(0.0, f64::max)
}

pub(super) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
