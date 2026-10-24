//! Microscopy Bulk Simple Annotations constraints that the generated IOD
//! tables cannot express (PS3.3 A.87, C.37.1.2 and C.37.1.2.1): the referenced
//! image of 2D coordinates, Annotation Group numbering, the encoding of each
//! Graphic Type's points and indices, the geometry of the shapes, and the
//! Measurement Values counts. The winding of 2D outlines depends on the
//! referenced image's orientation and is checked with the set.

use std::path::Path;

use dicom_core::value::Value;
use dicom_core::{PrimitiveValue, Tag};
use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::image_semantics::{
    color_space, integer, integers, label, string, Findings, Iod, BULK_ANNOTATIONS_RULE, CITATION,
};
use super::iod::{dicom_edition, items};
use super::ValidationCheck;

mod geometry;

use geometry::{add, dot, extent, first_crossing, newell_normal, norm, scale, signed_area, sub};

/// Segment pairs one object's self-intersection tests may compare; a
/// hostile Annotation Group cannot make the check quadratic beyond it.
const MAX_SEGMENT_PAIRS: u64 = 50_000_000;
/// Coordinates within this fraction of a shape's extent agree.
const RELATIVE_TOLERANCE: f64 = 1e-4;

pub(super) fn run_bulk_annotations_check(
    path: &Path,
    object: &DefaultDicomObject,
) -> Option<ValidationCheck> {
    if Iod::of(object) != Some(Iod::BulkAnnotations) {
        return None;
    }
    let root: &InMemDicomObject = object;
    let mut findings = Findings::default();
    referenced_image(root, &mut findings);
    let three_d = string(root, tags::ANNOTATION_COORDINATE_TYPE).as_deref() == Some("3D");
    let mut budget = MAX_SEGMENT_PAIRS;
    for (index, group) in items(root, tags::ANNOTATION_GROUP_SEQUENCE)
        .iter()
        .enumerate()
    {
        let path = format!("Annotation Group Sequence[{}] > ", index + 1);
        group_number(group, index, &path, &mut findings);
        if let Some(shapes) = Shapes::read(group, three_d, &path, &mut findings) {
            shapes.check(&path, &mut budget, &mut findings);
            measurements(group, &shapes, &path, &mut findings);
        }
    }
    if budget == 0 {
        findings.unexamined(format!(
            "line segment crossings beyond the first {MAX_SEGMENT_PAIRS} segment pairs ({CITATION} C.37.1.2.1.1)"
        ));
    }
    color_space(root, "Table C.11.15-1", &mut findings);
    Some(findings.check(
        path,
        BULK_ANNOTATIONS_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.37.1.2"),
    ))
}

/// C.37.1.2.1.1: FRAME coordinates reference one Frame of one image, VOLUME
/// coordinates one image without a subset of Frames.
fn referenced_image(root: &InMemDicomObject, findings: &mut Findings) {
    if string(root, tags::ANNOTATION_COORDINATE_TYPE).as_deref() != Some("2D") {
        return;
    }
    let images = items(root, tags::REFERENCED_IMAGE_SEQUENCE);
    let frames = |image: &InMemDicomObject| {
        image.get(tags::REFERENCED_FRAME_NUMBER).map(|element| {
            element
                .to_multi_int::<i64>()
                .map_or(0, |values| values.len())
        })
    };
    match string(root, tags::PIXEL_ORIGIN_INTERPRETATION).as_deref() {
        Some("FRAME") => {
            let single = matches!(images, [image] if frames(image) == Some(1));
            if !single {
                findings.push(|| format!(
                    "Pixel Origin Interpretation (0048,0301) is FRAME, but Referenced Image Sequence (0008,1140) does not specify a single Frame of a single image ({} Items, Referenced Frame Number Values {:?}); it shall ({CITATION} C.37.1.2.1.1)",
                    images.len(),
                    images.iter().map(frames).collect::<Vec<_>>()
                ));
            }
        }
        Some("VOLUME") => {
            let single = matches!(images, [image] if frames(image).is_none());
            if !single {
                findings.push(|| format!(
                    "Pixel Origin Interpretation (0048,0301) is VOLUME, but Referenced Image Sequence (0008,1140) does not specify a single image without a subset of Frames ({} Items, {} with Referenced Frame Number); it shall ({CITATION} C.37.1.2.1.1)",
                    images.len(),
                    images.iter().filter(|image| frames(image).is_some()).count()
                ));
            }
        }
        _ => {}
    }
}

/// Table C.37.1-2: Annotation Group Numbers start at 1 and increase by 1.
fn group_number(group: &InMemDicomObject, index: usize, path: &str, findings: &mut Findings) {
    let Some(number) = integer(group, tags::ANNOTATION_GROUP_NUMBER) else {
        return;
    };
    let expected = i64::try_from(index + 1).unwrap_or(i64::MAX);
    if number != expected {
        findings.push(|| format!(
            "{path}Annotation Group Number (0040,A180) is {number}; numbers start at 1 and increase monotonically by 1, so this Item's is {expected} ({CITATION} Table C.37.1-2)"
        ));
    }
}

/// The coordinates of an Annotation Group as encoded, without copying them.
enum Coordinates<'a> {
    Single(&'a [f32]),
    Double(&'a [f64]),
    /// Values the parser kept as bytes, e.g., under an Implicit VR it could
    /// not type, `width` bytes each.
    Bytes {
        bytes: &'a [u8],
        width: usize,
    },
}

impl Coordinates<'_> {
    fn read(element_value: &Value<InMemDicomObject>, double: bool) -> Option<Coordinates<'_>> {
        match element_value {
            Value::Primitive(PrimitiveValue::F32(values)) => Some(Coordinates::Single(values)),
            Value::Primitive(PrimitiveValue::F64(values)) => Some(Coordinates::Double(values)),
            Value::Primitive(PrimitiveValue::U8(bytes)) => Some(Coordinates::Bytes {
                bytes,
                width: if double { 8 } else { 4 },
            }),
            Value::Primitive(PrimitiveValue::Empty) => Some(Coordinates::Single(&[])),
            _ => None,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Single(values) => values.len(),
            Self::Double(values) => values.len(),
            Self::Bytes { bytes, width } => bytes.len() / width,
        }
    }

    fn get(&self, index: usize) -> f64 {
        match self {
            Self::Single(values) => f64::from(values[index]),
            Self::Double(values) => values[index],
            Self::Bytes { bytes, width: 4 } => {
                let at = index * 4;
                f64::from(f32::from_le_bytes([
                    bytes[at],
                    bytes[at + 1],
                    bytes[at + 2],
                    bytes[at + 3],
                ]))
            }
            Self::Bytes { bytes, .. } => {
                let at = index * 8;
                let mut value = [0; 8];
                value.copy_from_slice(&bytes[at..at + 8]);
                f64::from_le_bytes(value)
            }
        }
    }
}

/// `<group path>Annotation <n>: `, for a message about the 0-based
/// Annotation `shape`; formatted only when a message names it.
#[derive(Clone, Copy)]
struct ShapePlace<'a> {
    path: &'a str,
    shape: usize,
}

impl std::fmt::Display for ShapePlace<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}Annotation {}: ", self.path, self.shape + 1)
    }
}

/// One Annotation Group's shapes: its points, tuple size and indices.
struct Shapes<'a> {
    graphic_type: String,
    coordinates: Coordinates<'a>,
    data: Tag,
    /// Values per point: 2 for (X, Y), 3 for (X, Y, Z).
    dimensions: usize,
    three_d: bool,
    annotations: Option<usize>,
    indices: Option<Vec<i64>>,
}

impl<'a> Shapes<'a> {
    fn read(
        group: &'a InMemDicomObject,
        three_d: bool,
        path: &str,
        findings: &mut Findings,
    ) -> Option<Self> {
        let graphic_type = string(group, tags::GRAPHIC_TYPE)?;
        let (data, element) = [
            tags::POINT_COORDINATES_DATA,
            tags::DOUBLE_POINT_COORDINATES_DATA,
        ]
        .into_iter()
        .find_map(|tag| group.get(tag).map(|element| (tag, element)))?;
        let coordinates =
            Coordinates::read(element.value(), data == tags::DOUBLE_POINT_COORDINATES_DATA)?;
        // 2D coordinates are (column, row) pairs (C.37.1.2.1.1); 3D ones are
        // pairs when Common Z Coordinate Value holds Z (Table C.37.1-2).
        let common_z = group.get(tags::COMMON_Z_COORDINATE_VALUE).is_some();
        let dimensions = if three_d && !common_z { 3 } else { 2 };
        let shapes = Self {
            graphic_type,
            coordinates,
            data,
            dimensions,
            three_d,
            annotations: integer(group, tags::NUMBER_OF_ANNOTATIONS)
                .and_then(|count| usize::try_from(count).ok()),
            indices: integers(group, tags::LONG_PRIMITIVE_POINT_INDEX_LIST),
        };
        if !shapes.coordinates.len().is_multiple_of(dimensions) {
            findings.push(|| format!(
                "{path}{} has {} Values, not a whole number of {} tuples; for each point {} coordinates shall be encoded ({CITATION} Table C.37.1-2)",
                label(data),
                shapes.coordinates.len(),
                if dimensions == 3 { "(X, Y, Z)" } else { "(X, Y)" },
                if dimensions == 3 { "three (Common Z Coordinate Value (006A,0010) is absent)" } else { "two" }
            ));
            return None;
        }
        Some(shapes)
    }

    fn points(&self) -> usize {
        self.coordinates.len() / self.dimensions
    }

    fn point(&self, index: usize) -> [f64; 3] {
        let at = index * self.dimensions;
        let z = if self.dimensions == 3 {
            self.coordinates.get(at + 2)
        } else {
            0.0
        };
        [self.coordinates.get(at), self.coordinates.get(at + 1), z]
    }

    /// C.37.1.2.1.1: POINT Annotations, one coordinate tuple each, need no
    /// index list.
    fn point_count(&self, path: &str, findings: &mut Findings) {
        self.no_index_list(
            path,
            "there is no need for a separate index of each Annotation",
            findings,
        );
        let points = self.points();
        if let Some(annotations) = self
            .annotations
            .filter(|annotations| *annotations != points)
        {
            findings.push(|| format!(
                "{path}Number of Annotations (006A,000C) is {annotations}, but {} holds {points} points; for POINT they shall be the same ({CITATION} C.37.1.2.1.1)",
                label(self.data)
            ));
        }
    }

    /// The point ranges of each polyline, polygon, ellipse or rectangle, once
    /// the encoding is consistent.
    fn ranges(&self, path: &str, findings: &mut Findings) -> Option<Vec<(usize, usize)>> {
        let points = self.points();
        let annotations = self.annotations?;
        match self.graphic_type.as_str() {
            "ELLIPSE" | "RECTANGLE" => {
                self.no_index_list(
                    path,
                    "the index of each Annotation can be computed",
                    findings,
                );
                if points != annotations * 4 {
                    findings.push(|| format!(
                        "{path}Number of Annotations (006A,000C) is {annotations}, but {} holds {points} points; for {} the Number of Annotations shall be one quarter of the number of coordinate tuples ({CITATION} C.37.1.2.1.1)",
                        label(self.data),
                        self.graphic_type
                    ));
                    return None;
                }
                Some(
                    (0..annotations)
                        .map(|shape| (shape * 4, shape * 4 + 4))
                        .collect(),
                )
            }
            "POLYLINE" | "POLYGON" => self.indexed_ranges(annotations, path, findings),
            _ => None,
        }
    }

    fn no_index_list(&self, path: &str, reason: &str, findings: &mut Findings) {
        if self.indices.is_some() {
            findings.push(|| format!(
                "{path}Long Primitive Point Index List (0066,0040) is present with Graphic Type {}; it shall not be present, because {reason} ({CITATION} C.37.1.2.1.1)",
                self.graphic_type
            ));
        }
    }

    /// C.37.1.2.1.1: the index list holds one 1-based Value index per
    /// polyline or polygon, the first 1, strictly increasing, each at the
    /// first Value of a coordinate tuple.
    fn indexed_ranges(
        &self,
        annotations: usize,
        path: &str,
        findings: &mut Findings,
    ) -> Option<Vec<(usize, usize)>> {
        let indices = self.indices.as_ref()?;
        let list = "Long Primitive Point Index List (0066,0040)";
        if indices.len() != annotations {
            findings.push(|| format!(
                "{path}{list} has {} Values, but Number of Annotations (006A,000C) is {annotations}; it shall contain Number of Annotations Values ({CITATION} C.37.1.2.1.1)",
                indices.len()
            ));
            return None;
        }
        if indices.first().is_some_and(|first| *first != 1) {
            findings.push(|| format!(
                "{path}{list} starts at {}; the index of the first Value of the first coordinate tuple shall be 1 ({CITATION} C.37.1.2.1.1)",
                indices[0]
            ));
            return None;
        }
        if let Some(window) = indices.windows(2).find(|pair| pair[1] <= pair[0]) {
            findings.push(|| format!(
                "{path}{list} Values {} and {} are not strictly increasing; the polylines and polygons shall be in the same order as the index list ({CITATION} C.37.1.2.1.1)",
                window[0], window[1]
            ));
            return None;
        }
        let values = i64::try_from(self.coordinates.len()).unwrap_or(i64::MAX);
        let dimensions = i64::try_from(self.dimensions).unwrap_or(i64::MAX);
        if let Some(index) = indices
            .iter()
            .find(|index| **index > values || (**index - 1) % dimensions != 0)
        {
            findings.push(|| format!(
                "{path}{list} Value {index} is not the index of the first Value of a coordinate tuple in {} ({} Values of {dimensions} per point) ({CITATION} C.37.1.2.1.1)",
                label(self.data),
                values
            ));
            return None;
        }
        let starts: Vec<usize> = indices
            .iter()
            .map(|index| usize::try_from((index - 1) / dimensions).unwrap_or(usize::MAX))
            .collect();
        Some(
            starts
                .iter()
                .enumerate()
                .map(|(shape, start)| {
                    (
                        *start,
                        starts.get(shape + 1).copied().unwrap_or(self.points()),
                    )
                })
                .collect(),
        )
    }

    fn check(&self, path: &str, budget: &mut u64, findings: &mut Findings) {
        self.common_z(path, findings);
        if self.graphic_type == "POINT" {
            self.point_count(path, findings);
            return;
        }
        let Some(ranges) = self.ranges(path, findings) else {
            return;
        };
        let mut points = Vec::new();
        for (shape, (start, end)) in ranges.iter().enumerate() {
            points.clear();
            points.extend((*start..*end).map(|index| self.point(index)));
            self.shape(ShapePlace { path, shape }, &points, budget, findings);
        }
    }

    /// Twice the signed area and the extent of points `start..end`, read in
    /// place, with the area closed through the first point.
    fn outline_area(&self, start: usize, end: usize) -> (f64, f64) {
        let count = end - start;
        let mut area = 0.0;
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for offset in 0..count {
            let (a, b) = (
                self.point(start + offset),
                self.point(start + (offset + 1) % count),
            );
            area += a[0] * b[1] - b[0] * a[1];
            for axis in 0..2 {
                low[axis] = low[axis].min(a[axis]);
                high[axis] = high[axis].max(a[axis]);
            }
        }
        (area, (high[0] - low[0]).max(high[1] - low[1]))
    }

    /// C.37.1.2.1.1 and Table C.37.1-2: a Z common to every point is
    /// factored out into Common Z Coordinate Value.
    fn common_z(&self, path: &str, findings: &mut Findings) {
        if self.dimensions != 3 || self.points() == 0 {
            return;
        }
        let z = self.point(0)[2];
        if (1..self.points()).all(|index| self.point(index)[2] == z) {
            findings.push(|| format!(
                "{path}every point in {} has Z {z}; a common Z shall be factored out into Common Z Coordinate Value (006A,0010), not sent in (X, Y, Z) triplets ({CITATION} C.37.1.2.1.1 and Table C.37.1-2)",
                label(self.data)
            ));
        }
    }

    fn shape(
        &self,
        at: ShapePlace<'_>,
        points: &[[f64; 3]],
        budget: &mut u64,
        findings: &mut Findings,
    ) {
        match self.graphic_type.as_str() {
            "POLYLINE" | "POLYGON" => {
                let closed = self.graphic_type == "POLYGON";
                let minimum = if closed { 3 } else { 2 };
                if points.len() < minimum {
                    findings.push(|| format!(
                        "{at}a {} of {} points; it is a series of connected line segments of at least {minimum} vertices ({CITATION} Table C.37.1-2)",
                        self.graphic_type,
                        points.len()
                    ));
                    return;
                }
                if closed && points.first() == points.last() {
                    findings.push(|| format!(
                        "{at}the first and last points of the POLYGON are the same; they shall not be, the polygon being implicitly closed ({CITATION} C.37.1.2.1.1)"
                    ));
                }
                self.coplanar(at, points, findings);
                self.winding_3d(at, points, "C.37.1.2.1.1", findings);
                crossing(at, points, closed, budget, findings);
            }
            "ELLIPSE" => {
                self.coplanar(at, points, findings);
                ellipse(at, points, findings);
            }
            "RECTANGLE" => {
                self.coplanar(at, points, findings);
                rectangle(at, points, findings);
                self.winding_3d(at, points, "Table C.37.1-2", findings);
            }
            _ => {}
        }
    }

    /// Table C.37.1-2: the points of a polyline, polygon, ellipse or
    /// rectangle are coplanar. Only (X, Y, Z) triplets can depart from a
    /// plane.
    fn coplanar(&self, at: ShapePlace<'_>, points: &[[f64; 3]], findings: &mut Findings) {
        if self.dimensions != 3 || points.len() < 4 {
            return;
        }
        let normal = newell_normal(points);
        let length = norm(normal);
        let extent = extent(points);
        if length <= f64::EPSILON * extent * extent {
            // Collinear points lie in every plane through their line.
            return;
        }
        let unit = scale(normal, 1.0 / length);
        let origin = points[0];
        if let Some(point) = points
            .iter()
            .find(|point| dot(sub(**point, origin), unit).abs() > RELATIVE_TOLERANCE * extent)
        {
            findings.push(|| format!(
                "{at}point {point:?} lies off the plane of the {}'s other points; the points shall be coplanar ({CITATION} Table C.37.1-2)",
                self.graphic_type
            ));
        }
    }

    /// C.37.1.2.1.1: viewed from the top surface of the slide (X to the
    /// right, Y up), polylines and polygons wind clockwise, and Table
    /// C.37.1-2 orders rectangle corners clockwise from the top left. 2D
    /// coordinates are the set check's, which knows the image orientation.
    fn winding_3d(
        &self,
        at: ShapePlace<'_>,
        points: &[[f64; 3]],
        section: &str,
        findings: &mut Findings,
    ) {
        if !self.three_d {
            return;
        }
        let area = signed_area(points);
        let extent = extent(points);
        if area.abs() <= RELATIVE_TOLERANCE * extent * extent {
            // An outline across the slide plane, or a straight one, has no
            // winding viewed from the top.
            return;
        }
        if area > 0.0 {
            findings.push(|| format!(
                "{at}the {}'s vertices wind counter-clockwise viewed from the top surface of the slide; they shall be in clockwise winding order ({CITATION} {section})",
                self.graphic_type
            ));
        }
    }
}

/// Table C.37.1-2: the first two points are the endpoints of the major axis
/// and the second two of the minor axis, so the axes bisect each other at
/// right angles and the first is not the shorter.
fn ellipse(at: ShapePlace<'_>, points: &[[f64; 3]], findings: &mut Findings) {
    let [first, second, third, fourth] = [points[0], points[1], points[2], points[3]];
    let major = sub(second, first);
    let minor = sub(fourth, third);
    let tolerance = RELATIVE_TOLERANCE * extent(points);
    let centers = sub(
        scale(add(first, second), 0.5),
        scale(add(third, fourth), 0.5),
    );
    let mut faults = Vec::new();
    if norm(centers) > tolerance {
        faults.push("the axes do not share their midpoint");
    }
    if dot(major, minor).abs() > tolerance * norm(major).max(norm(minor)) {
        faults.push("the axes are not perpendicular");
    }
    if norm(major) + tolerance < norm(minor) {
        faults.push("the first axis is shorter than the second");
    }
    if !faults.is_empty() {
        findings.push(|| format!(
            "{at}the ELLIPSE points {points:?} are not the endpoints of its major then minor axis: {}; they shall be ({CITATION} Table C.37.1-2)",
            faults.join(", ")
        ));
    }
}

/// Table C.37.1-2: four corners of a rectangle in order.
fn rectangle(at: ShapePlace<'_>, points: &[[f64; 3]], findings: &mut Findings) {
    let tolerance = RELATIVE_TOLERANCE * extent(points);
    let diagonal = sub(add(points[0], points[2]), add(points[1], points[3]));
    let edge = |from: usize, to: usize| sub(points[to], points[from]);
    let square_corner =
        dot(edge(0, 1), edge(1, 2)).abs() <= tolerance * norm(edge(0, 1)).max(norm(edge(1, 2)));
    if norm(diagonal) > tolerance || !square_corner {
        findings.push(|| format!(
            "{at}the RECTANGLE points {points:?} are not four corners of a rectangle in order around it; they shall be ({CITATION} Table C.37.1-2)"
        ));
    }
}

/// C.37.1.2.1.1: no two line segments of a polyline or polygon cross or
/// touch other than at the vertex adjacent segments share, which also
/// excludes keyhole holes.
fn crossing(
    at: ShapePlace<'_>,
    points: &[[f64; 3]],
    closed: bool,
    budget: &mut u64,
    findings: &mut Findings,
) {
    if let Some((first, second)) = first_crossing(points, closed, budget) {
        findings.push(|| format!(
            "{at}line segments {} and {} cross or overlap; the line segments shall not cross and shall not contain holes ({CITATION} C.37.1.2.1.1)",
            first + 1,
            second + 1
        ));
    }
}

/// Table C.37.1-2: each Measurement Values Item stores one Value per
/// Annotation it covers: those of Annotation Index List, else those of Long
/// Primitive Point Index List, else Number of Annotations.
fn measurements(
    group: &InMemDicomObject,
    shapes: &Shapes<'_>,
    path: &str,
    findings: &mut Findings,
) {
    let annotations = shapes.annotations;
    for (index, measurement) in items(group, tags::MEASUREMENTS_SEQUENCE).iter().enumerate() {
        for (value_index, values) in items(measurement, tags::MEASUREMENT_VALUES_SEQUENCE)
            .iter()
            .enumerate()
        {
            let at = format!(
                "{path}Measurements Sequence[{}] > Measurement Values Sequence[{}] > ",
                index + 1,
                value_index + 1
            );
            let Some(count) = values
                .get(tags::FLOATING_POINT_VALUES)
                .and_then(|element| Coordinates::read(element.value(), false))
                .map(|values| values.len())
            else {
                continue;
            };
            let (expected, basis, statement) = match integers(values, tags::ANNOTATION_INDEX_LIST) {
                Some(listed) => {
                    annotation_indices(&listed, annotations, &at, findings);
                    (
                        Some(listed.len()),
                        "Annotation Index List (006A,0011) has {} indices",
                        "If Annotation Index List (006A,0011) is present",
                    )
                }
                None => match &shapes.indices {
                    Some(indices) => (
                        Some(indices.len()),
                        "Long Primitive Point Index List (0066,0040) has {} Annotations",
                        "If Annotation Index List (006A,0011) is absent",
                    ),
                    None => (
                        annotations,
                        "Number of Annotations (006A,000C) is {}",
                        "If Annotation Index List (006A,0011) and Long Primitive Point Index List (0066,0040) are absent",
                    ),
                },
            };
            if let Some(expected) = expected.filter(|expected| *expected != count) {
                let basis = basis.replace("{}", &expected.to_string());
                findings.push(|| format!(
                    "{at}Floating Point Values (0066,0125) has {count} Values, but {basis}; {statement}, the number of values shall match ({CITATION} Table C.37.1-2)"
                ));
            }
        }
    }
}

/// Table C.37.1-2: Annotation Index List references Annotations of the
/// group by their 1-based position.
fn annotation_indices(
    listed: &[i64],
    annotations: Option<usize>,
    at: &str,
    findings: &mut Findings,
) {
    let Some(annotations) = annotations else {
        return;
    };
    let last = i64::try_from(annotations).unwrap_or(i64::MAX);
    if let Some(index) = listed.iter().find(|index| !(1..=last).contains(*index)) {
        findings.push(|| format!(
            "{at}Annotation Index List (006A,0011) Value {index} references no Annotation of the {annotations} in the group; it lists indices of the group's Annotations ({CITATION} Table C.37.1-2)"
        ));
    }
}

/// The polylines, polygons and rectangles of one 2D Annotation Group by the
/// sign of their area in (column, row) coordinates.
#[derive(Debug)]
pub(super) struct GroupWinding {
    /// The 1-based Annotation Group Sequence Item.
    pub(super) item: usize,
    pub(super) graphic_type: String,
    /// Annotations of positive and of negative area.
    pub(super) positive: Tally,
    pub(super) negative: Tally,
}

#[derive(Debug, Default)]
pub(super) struct Tally {
    pub(super) count: usize,
    /// The 1-based Annotation counted first.
    pub(super) first: Option<usize>,
}

impl Tally {
    fn add(&mut self, annotation: usize) {
        self.count += 1;
        self.first.get_or_insert(annotation);
    }
}

/// The windings of a 2D object's outlines, for the set check that relates
/// them to the referenced image; empty for 3D coordinates.
pub(super) fn screen_windings(root: &InMemDicomObject) -> Vec<GroupWinding> {
    if string(root, tags::ANNOTATION_COORDINATE_TYPE).as_deref() != Some("2D") {
        return Vec::new();
    }
    // The validity of the encoding is the per-object check's to report.
    let mut ignored = Findings::discarding();
    let mut windings = Vec::new();
    for (index, group) in items(root, tags::ANNOTATION_GROUP_SEQUENCE)
        .iter()
        .enumerate()
    {
        let Some(shapes) = Shapes::read(group, false, "", &mut ignored) else {
            continue;
        };
        if !matches!(
            shapes.graphic_type.as_str(),
            "POLYLINE" | "POLYGON" | "RECTANGLE"
        ) {
            continue;
        }
        let Some(ranges) = shapes.ranges("", &mut ignored) else {
            continue;
        };
        let mut winding = GroupWinding {
            item: index + 1,
            graphic_type: shapes.graphic_type.clone(),
            positive: Tally::default(),
            negative: Tally::default(),
        };
        for (shape, (start, end)) in ranges.iter().enumerate() {
            let (area, extent) = shapes.outline_area(*start, *end);
            if area > RELATIVE_TOLERANCE * extent * extent {
                winding.positive.add(shape + 1);
            } else if area < -RELATIVE_TOLERANCE * extent * extent {
                winding.negative.add(shape + 1);
            }
        }
        windings.push(winding);
    }
    windings
}
