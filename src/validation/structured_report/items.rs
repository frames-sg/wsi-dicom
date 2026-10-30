//! The Values of the Content Item Macros (PS3.3 C.17.3.3 and C.18) by Value
//! Type, beyond the presence and format the IOD tables check: Text Value
//! control characters (Table C.17-5); Measured Value Sequence Values (C.18.1);
//! Referenced Frame Numbers and Icon Images (C.18.4); Graphic Data by Graphic
//! Type (C.18.6, C.18.9); temporal points by Temporal Range Type (C.18.7); and
//! TABLE rows, columns and cells (C.18.10). The CODE, DATE, TIME, DATETIME,
//! UIDREF, PNAME, COMPOSITE, WAVEFORM and CONTAINER Macros bind no Value
//! beyond the Type, VR and Value Set the tables check.

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::tree::{At, Tree};
use super::{Findings, CITATION};
use crate::validation::disclosure::Nesting;
use crate::validation::image_semantics::{integer, integers, multiplicity, numbers, strings};
use crate::validation::iod::{items, string_value};

/// The largest Icon Image (Table C.18.4-1).
const MAX_ICON: i64 = 128;
/// Coordinates of a POLYGON within this fraction of its extent (or of its
/// largest coordinate) of the plane through it are coplanar: FL Values carry
/// about 7 significant digits.
const COPLANAR_TOLERANCE: f64 = 1e-5;

pub(super) fn check(tree: &Tree<'_>, findings: &mut Findings) {
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.is_by_reference() {
            continue;
        }
        let at = tree.at(index);
        let object = node.object;
        match node.value_type() {
            Some("TEXT") => text_value(object, at, findings),
            Some("NUM") => numeric(object, at, findings),
            Some("IMAGE") => image(object, at, findings),
            Some("SCOORD") => spatial(object, at, findings),
            Some("SCOORD3D") => spatial_3d(object, at, findings),
            Some("TCOORD") => temporal(object, at, findings),
            Some("TABLE") => table(object, at, findings),
            _ => {}
        }
    }
}

/// Table C.17-5: "The text value may contain spaces, as well as multiple
/// lines separated by CR LF, but otherwise no format control characters".
/// ESC introduces the code extensions of Specific Character Set (PS3.5
/// 6.1.2.5), not format control.
fn text_value(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    let Some(text) = object
        .get(tags::TEXT_VALUE)
        .and_then(|element| element.to_str().ok())
    else {
        return;
    };
    let text = text.trim_end_matches([' ', '\0']);
    let characters: Vec<char> = text.chars().collect();
    let offending = characters.iter().enumerate().find(|(position, character)| {
        let previous = position.checked_sub(1).map(|before| characters[before]);
        let following = characters.get(position + 1).copied();
        match character {
            '\r' => following != Some('\n'),
            '\n' => previous != Some('\r'),
            '\u{1b}' => false,
            other => other.is_control() && u32::from(**other) < 0x80,
        }
    });
    if let Some((position, character)) = offending {
        let name = control_name(*character);
        findings.push(|| {
            format!(
                "{at} > Text Value (0040,A160) holds {name} at character {}; besides spaces and lines separated by CR LF, no format control characters shall be present ({CITATION} Table C.17-5)",
                position + 1
            )
        });
    }
}

fn control_name(character: char) -> String {
    let name = match character {
        '\u{0}' => "NUL",
        '\u{8}' => "BS",
        '\t' => "HT",
        '\n' => "LF without a preceding CR",
        '\u{b}' => "VT",
        '\u{c}' => "FF",
        '\r' => "CR without a following LF",
        _ => "",
    };
    if name.is_empty() {
        format!("the control character U+{:04X}", u32::from(character))
    } else {
        format!("{name} (U+{:04X})", u32::from(character))
    }
}

/// C.18.1: each Value of the Measured Value Sequence Item is single, an
/// integer or real number, and its floating point and rational
/// representations represent Numeric Value.
fn numeric(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    // The measured values of a Concept Table E.3.4-1 removes, such as
    // Subject Age, are withheld.
    let nesting = Nesting::default()
        .content_item(object)
        .below(tags::MEASURED_VALUE_SEQUENCE);
    for (position, value) in items(object, tags::MEASURED_VALUE_SEQUENCE)
        .iter()
        .enumerate()
    {
        let place = || format!("{at} > Measured Value Sequence[{}]", position + 1);
        for (tag, name) in [
            (tags::NUMERIC_VALUE, "Numeric Value (0040,A30A)"),
            (
                tags::FLOATING_POINT_VALUE,
                "Floating Point Value (0040,A161)",
            ),
            (
                tags::RATIONAL_NUMERATOR_VALUE,
                "Rational Numerator Value (0040,A162)",
            ),
            (
                tags::RATIONAL_DENOMINATOR_VALUE,
                "Rational Denominator Value (0040,A163)",
            ),
        ] {
            if let Some(count) = multiplicity(value, tag).filter(|count| *count > 1) {
                findings.push(|| {
                    format!(
                        "{} > {name} has {count} Values; only a single Value shall be present ({CITATION} Table C.18.1-1)",
                        place()
                    )
                });
            }
        }
        measured_value(value, (&place, nesting), findings);
    }
}

/// A measured value of `tag` as a message shows it: as it is, or withheld
/// (`disclosure`).
fn number(nesting: Nesting, tag: Tag, value: &dyn std::fmt::Display) -> String {
    let value = value.to_string();
    if nesting.withholds(tag) {
        nesting.shown(tag, &value).to_string()
    } else {
        value
    }
}

fn measured_value(
    value: &InMemDicomObject,
    (place, nesting): (&dyn Fn() -> String, Nesting),
    findings: &mut Findings,
) {
    let decimal = strings(value, tags::NUMERIC_VALUE)
        .and_then(|values| values.into_iter().next())
        .and_then(|text| Some((text.trim().parse::<f64>().ok()?, text)));
    if let Some(floating) =
        numbers(value, tags::FLOATING_POINT_VALUE).and_then(|values| values.first().copied())
    {
        if !floating.is_finite() {
            findings.push(|| {
                format!(
                    "{} > Floating Point Value (0040,A161) is {}, which is not an integer or real number; the Measured Value Sequence shall convey measured values that represent integers or real numbers, and Numeric Value Qualifier Code Sequence (0040,A301) conveys a value that is not ({CITATION} Table C.18.1-1)",
                    place(),
                    number(nesting, tags::FLOATING_POINT_VALUE, &floating)
                )
            });
        } else if let Some((numeric, text)) = &decimal {
            if (floating - numeric).abs() > resolution(text) {
                findings.push(|| {
                    format!(
                        "{} > Floating Point Value (0040,A161) {} is not the floating point representation of Numeric Value (0040,A30A) {} ({CITATION} Table C.18.1-1)",
                        place(),
                        number(nesting, tags::FLOATING_POINT_VALUE, &floating),
                        number(nesting, tags::NUMERIC_VALUE, text)
                    )
                });
            }
        }
    }
    let numerator = integer(value, tags::RATIONAL_NUMERATOR_VALUE);
    let denominator = integer(value, tags::RATIONAL_DENOMINATOR_VALUE);
    match (numerator, denominator) {
        (_, Some(0)) => findings.push(|| {
            format!(
                "{} > Rational Denominator Value (0040,A163) is 0; it is encoded as a non-zero unsigned integer, so that the rational value represents a real number ({CITATION} Table C.18.1-1)",
                place()
            )
        }),
        (Some(numerator), Some(denominator)) => {
            if let Some((numeric, text)) = &decimal {
                // Exact for the 32-bit Values.
                let rational = numerator as f64 / denominator as f64;
                if (rational - numeric).abs() > resolution(text) {
                    findings.push(|| {
                        format!(
                            "{} > Rational Numerator Value (0040,A162) / Rational Denominator Value (0040,A163) {}/{} is not a rational representation of Numeric Value (0040,A30A) {} ({CITATION} Table C.18.1-1)",
                            place(),
                            number(nesting, tags::RATIONAL_NUMERATOR_VALUE, &numerator),
                            number(nesting, tags::RATIONAL_DENOMINATOR_VALUE, &denominator),
                            number(nesting, tags::NUMERIC_VALUE, text)
                        )
                    });
                }
            }
        }
        _ => {}
    }
}

/// The value of the last digit a DS Value writes: within it, another
/// representation represents the same number.
fn resolution(text: &str) -> f64 {
    let text = text.trim();
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(split) => (
            &text[..split],
            text[split + 1..].parse::<i32>().unwrap_or(0),
        ),
        None => (text, 0),
    };
    let decimals = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let decimals = i32::try_from(decimals).unwrap_or(i32::MAX);
    10f64.powi(exponent.saturating_sub(decimals))
}

/// C.18.4: Referenced Frame Numbers count from 1, and an Icon Image is no
/// larger than 128 rows by 128 columns.
fn image(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    for reference in items(object, tags::REFERENCED_SOP_SEQUENCE) {
        if let Some(frames) = integers(reference, tags::REFERENCED_FRAME_NUMBER) {
            if let Some(frame) = frames.iter().find(|frame| **frame < 1) {
                findings.push(|| {
                    format!(
                        "{at} > Referenced SOP Sequence > Referenced Frame Number (0008,1160) has Value {frame}; the first Frame shall be denoted as Frame number 1 ({CITATION} Table C.18.4-1)"
                    )
                });
            }
        }
        for icon in items(reference, tags::ICON_IMAGE_SEQUENCE) {
            let rows = integer(icon, tags::ROWS);
            let columns = integer(icon, tags::COLUMNS);
            if rows.is_some_and(|rows| rows > MAX_ICON)
                || columns.is_some_and(|columns| columns > MAX_ICON)
            {
                findings.push(|| {
                    format!(
                        "{at} > Referenced SOP Sequence > Icon Image Sequence (0088,0200) is {} rows by {} columns; the Icon Image shall be no greater than 128 rows by 128 columns ({CITATION} Table C.18.4-1)",
                        rows.map_or("?".into(), |value| value.to_string()),
                        columns.map_or("?".into(), |value| value.to_string())
                    )
                });
            }
        }
    }
}

/// The number of points a Graphic Type has: exactly, or at least.
#[derive(Clone, Copy)]
enum Points {
    Exactly(usize),
    AtLeast(usize),
}

impl Points {
    fn admits(self, count: usize) -> bool {
        match self {
            Self::Exactly(points) => count == points,
            Self::AtLeast(points) => count >= points,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Exactly(points) => format!("exactly {points}"),
            Self::AtLeast(points) => format!("at least {points}"),
        }
    }
}

/// C.18.6.1.2: the (column,row) pairs each Graphic Type is defined by.
fn planar_points(graphic_type: &str) -> Option<Points> {
    Some(match graphic_type {
        "POINT" => Points::Exactly(1),
        "MULTIPOINT" => Points::AtLeast(1),
        "POLYLINE" => Points::AtLeast(2),
        "CIRCLE" => Points::Exactly(2),
        "ELLIPSE" => Points::Exactly(4),
        _ => return None,
    })
}

/// C.18.9.1.2: the (x,y,z) triplets each Graphic Type is defined by; a
/// POLYGON closes on its first vertex, so a triangle has four.
fn volumetric_points(graphic_type: &str) -> Option<Points> {
    Some(match graphic_type {
        "POINT" => Points::Exactly(1),
        "MULTIPOINT" => Points::AtLeast(1),
        "POLYLINE" => Points::AtLeast(2),
        "POLYGON" => Points::AtLeast(4),
        "ELLIPSE" => Points::Exactly(4),
        "ELLIPSOID" => Points::Exactly(6),
        _ => return None,
    })
}

/// C.18.6: Graphic Data is (column,row) pairs as many as the Graphic Type
/// has, within 0\0 to Columns\Rows of the referenced image; the upper bound
/// is compared with the image when the validated set holds it.
fn spatial(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    let Some(values) = numbers(object, tags::GRAPHIC_DATA) else {
        return;
    };
    if values.len() % 2 != 0 {
        findings.push(|| {
            format!(
                "{at} > Graphic Data (0070,0022) has {} Values, which are not (column,row) pairs ({CITATION} Table C.18.6-1)",
                values.len()
            )
        });
        return;
    }
    if let Some(graphic_type) = string_value(object, tags::GRAPHIC_TYPE, 1) {
        point_count(
            &graphic_type,
            values.len() / 2,
            planar_points(&graphic_type),
            "(column,row) pairs",
            "C.18.6.1.2",
            at,
            findings,
        );
    }
    if let Some(negative) = values.iter().find(|value| **value < 0.0) {
        findings.push(|| {
            format!(
                "{at} > Graphic Data (0070,0022) has Value {negative}; the values must be within the range 0\\0 to Columns\\Rows ({CITATION} Table C.18.6-1)"
            )
        });
    }
}

fn point_count(
    graphic_type: &str,
    count: usize,
    points: Option<Points>,
    unit: &str,
    section: &str,
    at: At<'_, '_>,
    findings: &mut Findings,
) {
    let Some(points) = points.filter(|points| !points.admits(count)) else {
        return;
    };
    findings.push(|| {
        format!(
            "{at} > Graphic Data (0070,0022) has {count} {unit} for Graphic Type (0070,0023) {graphic_type}, which is defined by {} ({CITATION} {section})",
            points.describe()
        )
    });
}

/// C.18.9: Graphic Data is (x,y,z) triplets as many as the Graphic Type
/// has; "POLYGON ... the first and last vertices shall be the same forming a
/// polygon; the points shall be coplanar" (C.18.9.1.2).
fn spatial_3d(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    let Some(values) = numbers(object, tags::GRAPHIC_DATA) else {
        return;
    };
    if values.len() % 3 != 0 {
        findings.push(|| {
            format!(
                "{at} > Graphic Data (0070,0022) has {} Values, which are not (x,y,z) triplets ({CITATION} Table C.18.9-1)",
                values.len()
            )
        });
        return;
    }
    let Some(graphic_type) = string_value(object, tags::GRAPHIC_TYPE, 1) else {
        return;
    };
    let points: Vec<[f64; 3]> = values.as_chunks::<3>().0.to_vec();
    point_count(
        &graphic_type,
        points.len(),
        volumetric_points(&graphic_type),
        "(x,y,z) triplets",
        "C.18.9.1.2",
        at,
        findings,
    );
    if graphic_type != "POLYGON" || points.len() < 2 {
        return;
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    if first != last {
        findings.push(|| {
            format!(
                "{at} > Graphic Data (0070,0022) of a POLYGON starts at {first:?} and ends at {last:?}; the first and last vertices shall be the same ({CITATION} C.18.9.1.2)"
            )
        });
    }
    if let Some(distance) = off_plane(&points) {
        findings.push(|| {
            format!(
                "{at} > Graphic Data (0070,0022) of a POLYGON has a vertex {distance} mm off the plane of the others; the points shall be coplanar ({CITATION} C.18.9.1.2)"
            )
        });
    }
}

/// The largest distance of a vertex from the plane Newell's method fits to
/// `points`, when it exceeds the tolerance; `None` for coplanar points
/// (collinear and coincident ones included).
fn off_plane(points: &[[f64; 3]]) -> Option<f64> {
    let count = points.len() as f64;
    let centroid = points.iter().fold([0.0; 3], |sum, point| {
        [sum[0] + point[0], sum[1] + point[1], sum[2] + point[2]]
    });
    let centroid = centroid.map(|sum| sum / count);
    let mut normal = [0.0f64; 3];
    for (index, current) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }
    let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = points
        .iter()
        .flat_map(|point| point.iter().map(|value| value.abs()))
        .fold(0.0f64, f64::max)
        .max(f64::MIN_POSITIVE);
    if length <= f64::EPSILON * scale * scale {
        return None;
    }
    let unit = normal.map(|value| value / length);
    let distance = points
        .iter()
        .map(|point| {
            ((point[0] - centroid[0]) * unit[0]
                + (point[1] - centroid[1]) * unit[1]
                + (point[2] - centroid[2]) * unit[2])
                .abs()
        })
        .fold(0.0f64, f64::max);
    (distance > COPLANAR_TOLERANCE * scale).then_some(distance)
}

/// C.18.7.1.1: the temporal points each Temporal Range Type has, in
/// whichever of Referenced Sample Positions, Time Offsets or DateTime
/// conveys them; sample positions count from 1 (Table C.18.7-1).
fn temporal(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    let Some(range) = string_value(object, tags::TEMPORAL_RANGE_TYPE, 1) else {
        return;
    };
    let points = match range.as_str() {
        "POINT" | "BEGIN" | "END" => Points::Exactly(1),
        "MULTIPOINT" => Points::AtLeast(1),
        "SEGMENT" => Points::Exactly(2),
        "MULTISEGMENT" => Points::AtLeast(2),
        _ => return,
    };
    for (tag, name) in [
        (
            tags::REFERENCED_SAMPLE_POSITIONS,
            "Referenced Sample Positions (0040,A132)",
        ),
        (
            tags::REFERENCED_TIME_OFFSETS,
            "Referenced Time Offsets (0040,A138)",
        ),
        (
            tags::REFERENCED_DATE_TIME,
            "Referenced DateTime (0040,A13A)",
        ),
    ] {
        let Some(count) = multiplicity(object, tag).map(|count| count as usize) else {
            continue;
        };
        let paired = range != "MULTISEGMENT" || count % 2 == 0;
        if !points.admits(count) || !paired {
            findings.push(|| {
                format!(
                    "{at} > {name} has {count} Values for Temporal Range Type (0040,A130) {range}, which is defined by {} temporal points{} ({CITATION} C.18.7.1.1)",
                    points.describe(),
                    if range == "MULTISEGMENT" { ", two per segment" } else { "" }
                )
            });
        }
        if tag == tags::REFERENCED_SAMPLE_POSITIONS {
            if let Some(position) =
                integers(object, tag).and_then(|values| values.into_iter().find(|value| *value < 1))
            {
                findings.push(|| {
                    format!(
                        "{at} > {name} has Value {position}; the position of the first sample is 1 ({CITATION} Table C.18.7-1)"
                    )
                });
            }
        }
    }
}

/// The Selector Value Attribute of each Selector Attribute VR (Table
/// C.18.10-1); SQ cells are Concept Code Sequence Items.
const SELECTOR_VALUES: [(&str, Tag); 12] = [
    ("DS", tags::SELECTOR_DS_VALUE),
    ("DT", tags::SELECTOR_DT_VALUE),
    ("FD", tags::SELECTOR_FD_VALUE),
    ("FL", tags::SELECTOR_FL_VALUE),
    ("IS", tags::SELECTOR_IS_VALUE),
    ("SL", tags::SELECTOR_SL_VALUE),
    ("SS", tags::SELECTOR_SS_VALUE),
    ("SV", tags::SELECTOR_SV_VALUE),
    ("UC", tags::SELECTOR_UC_VALUE),
    ("UL", tags::SELECTOR_UL_VALUE),
    ("US", tags::SELECTOR_US_VALUE),
    ("UV", tags::SELECTOR_UV_VALUE),
];

/// C.18.10: the Tabulated Values Item conveys rows and columns of the table
/// it sizes; the Row and Column Definition Sequences are sorted by number,
/// and the Cell Values Sequence is in row-major order, then by column.
fn table(object: &InMemDicomObject, at: At<'_, '_>, findings: &mut Findings) {
    let Some(tabulated) = items(object, tags::TABULATED_VALUES_SEQUENCE).first() else {
        return;
    };
    let rows = integer(tabulated, tags::NUMBER_OF_TABLE_ROWS);
    let columns = integer(tabulated, tags::NUMBER_OF_TABLE_COLUMNS);
    let place = |sequence: &str| format!("{at} > Tabulated Values Sequence > {sequence}");
    definitions(
        tabulated,
        tags::TABLE_ROW_DEFINITION_SEQUENCE,
        tags::TABLE_ROW_NUMBER,
        rows,
        (
            "Table Row Definition Sequence (0040,A806)",
            "Table Row Number (0040,A804)",
            "Number of Table Rows (0040,A802)",
        ),
        &place,
        findings,
    );
    definitions(
        tabulated,
        tags::TABLE_COLUMN_DEFINITION_SEQUENCE,
        tags::TABLE_COLUMN_NUMBER,
        columns,
        (
            "Table Column Definition Sequence (0040,A807)",
            "Table Column Number (0040,A805)",
            "Number of Table Columns (0040,A803)",
        ),
        &place,
        findings,
    );
    let cells = items(tabulated, tags::CELL_VALUES_SEQUENCE);
    let mut previous: Option<(i64, i64)> = None;
    for (position, cell) in cells.iter().enumerate() {
        let row = integer(cell, tags::TABLE_ROW_NUMBER);
        let column = integer(cell, tags::TABLE_COLUMN_NUMBER);
        let key = (row.unwrap_or(0), column.unwrap_or(0));
        let item = || {
            format!(
                "{}[{}]",
                place("Cell Values Sequence (0040,A808)"),
                position + 1
            )
        };
        if previous.is_some_and(|previous| key < previous) {
            findings.push(|| {
                format!(
                    "{} (row {}, column {}) follows a later cell; Items of this Sequence shall be in row-major order, then sorted by column ({CITATION} Table C.18.10-1)",
                    item(),
                    row.map_or("all".into(), |value| value.to_string()),
                    column.map_or("all".into(), |value| value.to_string())
                )
            });
        }
        previous = Some(key);
        cell_extent(cell, row, column, rows, columns, &item, findings);
    }
}

/// A Definition Sequence's numbers, sorted and within the table.
fn definitions(
    tabulated: &InMemDicomObject,
    sequence: Tag,
    number: Tag,
    limit: Option<i64>,
    (sequence_name, number_name, limit_name): (&str, &str, &str),
    place: &dyn Fn(&str) -> String,
    findings: &mut Findings,
) {
    let numbers: Vec<i64> = items(tabulated, sequence)
        .iter()
        .filter_map(|item| integer(item, number))
        .collect();
    if !numbers.is_sorted() {
        findings.push(|| {
            format!(
                "{} has {number_name} Values {numbers:?}; Items of this Sequence shall be sorted by {number_name} ({CITATION} Table C.18.10-1)",
                place(sequence_name)
            )
        });
    }
    if let Some(outside) = numbers
        .iter()
        .find(|value| **value < 1 || limit.is_some_and(|limit| **value > limit))
    {
        findings.push(|| {
            format!(
                "{} has {number_name} {outside}, outside the table's 1 to {} of {limit_name}; the Tabulated Values Item conveys the rows and columns of its table ({CITATION} Table C.18.10-1)",
                place(sequence_name),
                limit.map_or("?".into(), |value| value.to_string())
            )
        });
    }
}

/// A Cell Values Item names a cell, a whole row or a whole column of the
/// table, with one Value per cell it fills.
fn cell_extent(
    cell: &InMemDicomObject,
    row: Option<i64>,
    column: Option<i64>,
    rows: Option<i64>,
    columns: Option<i64>,
    item: &dyn Fn() -> String,
    findings: &mut Findings,
) {
    let outside = |value: Option<i64>, limit: Option<i64>| {
        value.is_some_and(|value| value < 1 || limit.is_some_and(|limit| value > limit))
    };
    if outside(row, rows) || outside(column, columns) {
        findings.push(|| {
            format!(
                "{} names row {} and column {} of a table of {} rows and {} columns; the Tabulated Values Item conveys the cells of its table ({CITATION} Table C.18.10-1)",
                item(),
                row.map_or("all".into(), |value| value.to_string()),
                column.map_or("all".into(), |value| value.to_string()),
                rows.map_or("?".into(), |value| value.to_string()),
                columns.map_or("?".into(), |value| value.to_string())
            )
        });
        return;
    }
    let Some(vr) = string_value(cell, tags::SELECTOR_ATTRIBUTE_VR, 1) else {
        return;
    };
    let count = if vr == "SQ" {
        Some(items(cell, tags::CONCEPT_CODE_SEQUENCE).len())
    } else {
        SELECTOR_VALUES
            .iter()
            .find(|(name, _)| *name == vr)
            .and_then(|(_, tag)| multiplicity(cell, *tag))
            .map(|count| count as usize)
    };
    let expected = match (row, column) {
        (Some(_), Some(_)) => Some(1),
        (Some(_), None) => columns,
        (None, Some(_)) => rows,
        (None, None) => None,
    };
    if let (Some(count), Some(expected)) = (
        count,
        expected.and_then(|value| usize::try_from(value).ok()),
    ) {
        if count != expected {
            findings.push(|| {
                format!(
                    "{} has {count} Selector {vr} Values for {}, which has {expected} cells; the Tabulated Values Item conveys the cell values of its rows and columns ({CITATION} Table C.18.10-1)",
                    item(),
                    match (row, column) {
                        (Some(_), Some(_)) => "one cell".to_string(),
                        (Some(row), None) => format!("all of row {row}"),
                        _ => format!("all of column {}", column.unwrap_or_default()),
                    }
                )
            });
        }
    }
}
