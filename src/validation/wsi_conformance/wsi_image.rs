//! VL Whole Slide Microscopy Image constraints that the generated IOD tables
//! cannot express (PS3.3 C.8.12, C.7.4.1.1.2, C.7.6.1.1.5, C.7.6.16.2.1, 10.7.1.3
//! and A.32.8.4.1.2), checked in every profile.

use std::collections::{BTreeMap, BTreeSet};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::VL_WSI;
use crate::validation::iod::dicom_edition;
pub(super) use crate::validation::iod::{integer_value as integer, items as sequence, tag_values};
use crate::validation::iod::{string_value, Coded};

const CITATION: &str = concat!("PS3.3 ", dicom_edition!());
/// Lossy Compression, CID 7203 Image Derivation.
const LOSSY_COMPRESSION_DERIVATION: (&str, &str) = ("DCM", "113040");

/// The VL WSI Image Module constraints (C.8.12.4 and A.32.8.4.1.2) in VL WSI
/// objects, and in those of every IOD the constraints of the Modules and
/// Macros it shares with VL WSI: slide coordinates, tiles, optical paths,
/// specimens, derivation and pixel spacing.
pub(super) fn validate_wsi_image_rule(
    object: &DefaultDicomObject,
    iod: &str,
) -> Result<(), String> {
    let mut violations = Vec::new();
    let image_type = strings(object, tags::IMAGE_TYPE);
    let groups = functional_group_items(object);
    if iod == VL_WSI {
        whole_slide_image(object, image_type.as_deref(), &groups, &mut violations);
    }
    tiles_overlap(object, &mut violations);
    if slide_related(object, iod, &groups) {
        slide_corner(object, &mut violations);
    }
    optical_paths(object, &groups, &mut violations);
    specimen_references(object, &groups, &mut violations);
    lossy_derivation(object, image_type.as_deref(), &groups, &mut violations);
    pixel_measures(object, &groups, &mut violations);
    slide_positions(object, &mut violations);
    super::violation_result(violations)
}

/// The Whole Slide Microscopy Image Module (C.8.12.4) and the VL WSI tiling
/// (A.32.8.4.1.2).
fn whole_slide_image(
    object: &DefaultDicomObject,
    image_type: Option<&[String]>,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    if let Some(values) = image_type {
        four_values("Image Type (0008,0008)", values, violations);
    }
    let flavor = image_type
        .and_then(|values| values.get(2))
        .map(String::as_str);
    for (path, item) in groups {
        for frame_type in sequence(item, tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE) {
            if let Some(values) = strings(frame_type, tags::FRAME_TYPE) {
                four_values(
                    &format!("{path}Frame Type (0008,9007)"),
                    &values,
                    violations,
                );
            }
        }
    }
    specimen_label(object, flavor, violations);
    single_frame(object, flavor, violations);
    if number(object, tags::IMAGED_VOLUME_DEPTH) == Some(0.0) {
        violations.push(format!(
            "Imaged Volume Depth (0048,0003) is 0; it shall not be ({CITATION} C.8.12.4.1.2)"
        ));
    }
    tiling(object, groups, violations);
}

/// Whether the object's Frame of Reference is slide-related: always for VL
/// WSI, and for a Segmentation or Parametric Map whose Frames or Total Pixel
/// Matrix are placed in the Slide Coordinate System (C.8.12.2.1 and
/// C.8.12.6.1). A 3D Bulk Annotation's Z coordinates are in the Slide
/// Coordinate System of its Frame of Reference (Table C.37.1-2).
fn slide_related(
    object: &InMemDicomObject,
    iod: &str,
    groups: &[(String, &InMemDicomObject)],
) -> bool {
    match iod {
        VL_WSI | "A.87" => true,
        _ => {
            object.element(tags::IMAGE_ORIENTATION_SLIDE).is_ok()
                || object
                    .element(tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE)
                    .is_ok()
                || groups
                    .iter()
                    .any(|(_, item)| item.element(tags::PLANE_POSITION_SLIDE_SEQUENCE).is_ok())
        }
    }
}

/// C.7.4.1.1.2: the Position Reference Indicator of a slide-related Frame of
/// Reference is SLIDE_CORNER, if it has a Value.
fn slide_corner(object: &InMemDicomObject, violations: &mut Vec<String>) {
    if let Some(indicator) = string(object, tags::POSITION_REFERENCE_INDICATOR) {
        if indicator != "SLIDE_CORNER" {
            violations.push(format!(
                "Position Reference Indicator (0020,1040) is \"{indicator}\"; a slide-related Frame of Reference is identified by SLIDE_CORNER ({CITATION} C.7.4.1.1.2)"
            ));
        }
    }
}

/// C.8.12.4.1.1: four Values, the Image Flavor and Derived Pixel ones not empty.
fn four_values(name: &str, values: &[String], violations: &mut Vec<String>) {
    if values.len() != 4 {
        violations.push(format!(
            "{name} has {} Values; exactly four are required ({CITATION} C.8.12.4.1.1)",
            values.len()
        ));
    }
    for (index, what) in [
        (2, "Value 3 (Image Flavor)"),
        (3, "Value 4 (Derived Pixel)"),
    ] {
        if values.get(index).is_some_and(String::is_empty) {
            violations.push(format!(
                "{name} {what} is empty; it shall have one of its Defined Terms ({CITATION} C.8.12.4.1.1)"
            ));
        }
    }
}

fn specimen_label(object: &InMemDicomObject, flavor: Option<&str>, violations: &mut Vec<String>) {
    let expected = match flavor {
        Some("OVERVIEW" | "LABEL") => "YES",
        Some("THUMBNAIL" | "VOLUME") => "NO",
        _ => return,
    };
    if let Some(value) = string(object, tags::SPECIMEN_LABEL_IN_IMAGE) {
        if value != expected {
            violations.push(format!(
                "Specimen Label in Image (0048,0010) is {value}; it shall be {expected} for Image Type Value 3 {} ({CITATION} C.8.12.4)",
                flavor.unwrap_or_default()
            ));
        }
    }
}

/// C.8.12.4-1: THUMBNAIL, LABEL and OVERVIEW images are single-Frame.
fn single_frame(object: &InMemDicomObject, flavor: Option<&str>, violations: &mut Vec<String>) {
    let Some(flavor) = flavor else {
        return;
    };
    if !matches!(flavor, "THUMBNAIL" | "LABEL" | "OVERVIEW") {
        return;
    }
    if let Some(frames) = integer(object, tags::NUMBER_OF_FRAMES).filter(|frames| *frames != 1) {
        violations.push(format!(
            "Number of Frames (0028,0008) is {frames}; it is 1 for Image Type Value 3 {flavor} ({CITATION} C.8.12.4)"
        ));
    }
}

/// C.8.12.14-1: TILED_FULL tiles do not overlap, and NONE assures that no
/// two Frames of one optical path encode the same tile of one focal plane.
fn tiles_overlap(object: &InMemDicomObject, violations: &mut Vec<String>) {
    let Some(overlap) = string(object, tags::TILES_OVERLAP) else {
        return;
    };
    let tiled_full =
        string(object, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() == Some("TILED_FULL");
    if tiled_full && overlap != "NONE" {
        violations.push(format!(
            "Tiles Overlap (0048,0304) is {overlap}; it shall be NONE when Dimension Organization Type is TILED_FULL ({CITATION} C.8.12.14)"
        ));
        return;
    }
    if overlap != "NONE" {
        return;
    }
    let shared = sequence(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    let mut tiles = BTreeMap::<(i64, i64, i64, Option<String>), usize>::new();
    for (index, frame) in sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .enumerate()
    {
        let Some(position) = sequence(frame, tags::PLANE_POSITION_SLIDE_SEQUENCE).first() else {
            continue;
        };
        let (Some(row), Some(column)) = (
            integer(position, tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
            integer(position, tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
        ) else {
            continue;
        };
        // Z Offset in µm, compared to the nanometre.
        let depth = number(position, tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
            .map_or(0, |depth| (depth * 1000.0).round() as i64);
        let path = sequence(frame, tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE)
            .first()
            .or_else(|| {
                shared.and_then(|shared| {
                    sequence(shared, tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE).first()
                })
            })
            .and_then(|identification| string(identification, tags::OPTICAL_PATH_IDENTIFIER));
        if let Some(first) = tiles.insert((row, column, depth, path), index + 1) {
            violations.push(format!(
                "Tiles Overlap (0048,0304) is NONE, but Per-Frame Functional Groups Sequence Items {first} and {} encode the same tile: row {row}, column {column}, the same focal plane and optical path ({CITATION} C.8.12.14)",
                index + 1
            ));
            return;
        }
    }
}

/// C.8.12.6.3 and C.7.6.22-2: a specimen a Frame references is visible in it,
/// so the Specimen Description Sequence identifies it.
fn specimen_references(
    object: &InMemDicomObject,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    let described: BTreeSet<String> = sequence(object, tags::SPECIMEN_DESCRIPTION_SEQUENCE)
        .iter()
        .filter_map(|specimen| string(specimen, tags::SPECIMEN_UID))
        .collect();
    let mut undescribed = 0;
    let mut first = None;
    for (path, item) in groups {
        for reference in sequence(item, tags::SPECIMEN_REFERENCE_SEQUENCE) {
            let Some(uid) = string(reference, tags::SPECIMEN_UID) else {
                continue;
            };
            if !described.contains(&uid) {
                undescribed += 1;
                first.get_or_insert_with(|| (path.clone(), uid));
            }
        }
    }
    if let Some((path, uid)) = first {
        violations.push(format!(
            "{path}Specimen Reference Sequence (0048,0110) > Specimen UID {uid} is not in the Specimen Description Sequence (0040,0560), which identifies each specimen imaged in the Pixel Data ({undescribed} reference(s); {CITATION} C.8.12.6.3 and C.7.6.22)"
        ));
    }
}

/// C.8.12.5-1 and C.8.12.6.2: Number of Optical Paths counts the Items,
/// identifiers are unique, and every Frame's Optical Path Identification
/// names one of them.
fn optical_paths(
    object: &InMemDicomObject,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    let items = sequence(object, tags::OPTICAL_PATH_SEQUENCE).len();
    if let Some(declared) = integer(object, tags::NUMBER_OF_OPTICAL_PATHS) {
        if declared != items as i64 {
            violations.push(format!(
                "Number of Optical Paths (0048,0302) is {declared}, but the Optical Path Sequence (0048,0105) has {items} Items; it is the number of Items in that Sequence ({CITATION} C.8.12.5)"
            ));
        }
    }
    let mut identifiers = BTreeSet::new();
    for (index, item) in sequence(object, tags::OPTICAL_PATH_SEQUENCE)
        .iter()
        .enumerate()
    {
        if let Some(identifier) = string(item, tags::OPTICAL_PATH_IDENTIFIER) {
            if !identifiers.insert(identifier.clone()) {
                violations.push(format!(
                    "Optical Path Sequence[{}] > Optical Path Identifier (0048,0106) \"{identifier}\" repeats; it shall be unique within the Sequence ({CITATION} C.8.12.5)",
                    index + 1
                ));
            }
        }
    }
    let mut unresolved = 0;
    let mut first = None;
    for (path, item) in groups {
        for identification in sequence(item, tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE) {
            let Some(identifier) = string(identification, tags::OPTICAL_PATH_IDENTIFIER) else {
                continue;
            };
            if !identifiers.contains(&identifier) {
                unresolved += 1;
                first.get_or_insert_with(|| (path.clone(), identifier));
            }
        }
    }
    if let Some((path, identifier)) = first {
        violations.push(format!(
            "{path}Optical Path Identification Sequence > Optical Path Identifier \"{identifier}\" names no Optical Path Sequence Item ({unresolved} Frame group(s); {CITATION} C.8.12.6.2)"
        ));
    }
}

/// C.7.6.1.1.5 and C.7.6.1.1.2: a lossy compressed version of another image is
/// DERIVED, and a derived image is a new SOP Instance, never its own source.
fn lossy_derivation(
    object: &InMemDicomObject,
    image_type: Option<&[String]>,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    let own = string(object, tags::SOP_INSTANCE_UID);
    // The General Image Module records a derivation at the top level, the
    // Derivation Image Functional Group in a Frame's Items.
    let derivations = std::iter::once(object).chain(
        groups
            .iter()
            .flat_map(|(_, item)| sequence(item, tags::DERIVATION_IMAGE_SEQUENCE)),
    );
    let mut lossy = false;
    let mut self_source = false;
    for derivation in derivations {
        lossy |= sequence(derivation, tags::DERIVATION_CODE_SEQUENCE)
            .iter()
            .any(|code| is_code(code, LOSSY_COMPRESSION_DERIVATION));
        self_source |= sequence(derivation, tags::SOURCE_IMAGE_SEQUENCE)
            .iter()
            .any(|source| {
                own.is_some() && string(source, tags::REFERENCED_SOP_INSTANCE_UID) == own
            });
    }
    let value_1 = image_type
        .and_then(|values| values.first())
        .map(String::as_str);
    if lossy && string(object, tags::LOSSY_IMAGE_COMPRESSION).as_deref() != Some("01") {
        violations.push(format!(
            "a Derivation Code Sequence declares (113040, DCM, \"Lossy Compression\") but Lossy Image Compression (0028,2110) is not 01; a lossy compressed version of another image is 01 ({CITATION} C.7.6.1.1.5)"
        ));
    }
    if lossy && value_1 != Some("DERIVED") {
        violations.push(format!(
            "a Derivation Code Sequence declares (113040, DCM, \"Lossy Compression\") but Image Type Value 1 is {}; a compressed version of another image shall be DERIVED ({CITATION} C.7.6.1.1.5)",
            value_1.unwrap_or("absent")
        ));
    }
    if self_source {
        violations.push(format!(
            "a Source Image Sequence names this SOP Instance as its own source; a derived image has its own SOP Instance UID ({CITATION} C.7.6.1.1.2)"
        ));
    }
}

/// C.8.12.6.1 with C.8.12.14.1.2: a Frame's X and Y Offset in Slide
/// Coordinate System locate its top left pixel, at its Column and Row
/// Position in the Total Pixel Matrix, whose pixel 1\\1 the Total Pixel
/// Matrix Origin locates, with rows along the first and columns along the
/// second direction cosines of Image Orientation (Slide). So, as C.7.6.2.1.1
/// maps a pixel, the Offsets are the Origin plus (Column Position - 1)
/// column spacings along the row direction and (Row Position - 1) row
/// spacings along the column direction, Pixel Spacing holding the row
/// spacing first and the column spacing second (10.7.1.3); within half a
/// pixel along each direction. Z Offsets place each focal plane, so they
/// are not compared.
fn slide_positions(object: &InMemDicomObject, violations: &mut Vec<String>) {
    let Some(origin) = sequence(object, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE)
        .first()
        .and_then(|origin| {
            Some([
                number(origin, tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)?,
                number(origin, tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)?,
            ])
        })
    else {
        return;
    };
    let Some(orientation) = numbers(object, tags::IMAGE_ORIENTATION_SLIDE)
        .filter(|values| values.len() == 6 && values.iter().all(|value| value.is_finite()))
    else {
        return;
    };
    let (row_direction, column_direction) = (&orientation[..2], &orientation[3..5]);
    let shared = sequence(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    fn in_frame<'a>(
        frame: &'a InMemDicomObject,
        shared: Option<&'a InMemDicomObject>,
        group: dicom_core::Tag,
    ) -> Option<&'a InMemDicomObject> {
        sequence(frame, group)
            .first()
            .or_else(|| shared.and_then(|shared| sequence(shared, group).first()))
    }
    let mut misplaced = 0;
    let mut first = None;
    for (index, frame) in sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .enumerate()
    {
        let (Some(position), Some(spacing)) = (
            in_frame(frame, shared, tags::PLANE_POSITION_SLIDE_SEQUENCE),
            in_frame(frame, shared, tags::PIXEL_MEASURES_SEQUENCE)
                .and_then(|measures| numbers(measures, tags::PIXEL_SPACING))
                .filter(|spacing| spacing.len() == 2 && spacing.iter().all(|value| *value > 0.0)),
        ) else {
            continue;
        };
        let (Some(column), Some(row), Some(x), Some(y)) = (
            integer(position, tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
            integer(position, tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
            number(position, tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM),
            number(position, tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM),
        ) else {
            continue;
        };
        let (row_spacing, column_spacing) = (spacing[0], spacing[1]);
        let along_row = (column - 1) as f64 * column_spacing;
        let along_column = (row - 1) as f64 * row_spacing;
        let expected = [
            origin[0] + along_row * row_direction[0] + along_column * column_direction[0],
            origin[1] + along_row * row_direction[1] + along_column * column_direction[1],
        ];
        let difference = [x - expected[0], y - expected[1]];
        let project =
            |direction: &[f64]| difference[0] * direction[0] + difference[1] * direction[1];
        if project(row_direction).abs() > column_spacing / 2.0
            || project(column_direction).abs() > row_spacing / 2.0
        {
            misplaced += 1;
            first.get_or_insert_with(|| {
                format!(
                    "Per-Frame Functional Groups Sequence[{}] > Plane Position (Slide) Sequence > X and Y Offset in Slide Coordinate System are {x}\\{y} mm, but the Frame's top left pixel at Column Position {column} and Row Position {row} lies at {}\\{} mm: the Total Pixel Matrix Origin {}\\{} mm plus {} column spacings of {column_spacing} mm along the row direction and {} row spacings of {row_spacing} mm along the column direction of Image Orientation (Slide), the row spacing being the first Value of Pixel Spacing",
                    index + 1,
                    expected[0],
                    expected[1],
                    origin[0],
                    origin[1],
                    column - 1,
                    row - 1
                )
            });
        }
    }
    if let Some(first) = first {
        violations.push(format!(
            "{first}; {misplaced} Frame(s) lie more than half a pixel from where their Column and Row Positions place them ({CITATION} C.8.12.6.1, C.8.12.14.1.2, C.7.6.2.1.1 and 10.7.1.3)"
        ));
    }
}

/// 10.7.1.3 and C.7.6.16.2.1: spacing is positive, except that one row or
/// column may have zero spacing in that direction; Spacing Between Slices is
/// not negative. The pixel data of a tiled image is its Total Pixel Matrix,
/// so a one-row Frame does not make the image one row.
fn pixel_measures(
    object: &InMemDicomObject,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    let extent = |total, frame| integer(object, total).or_else(|| integer(object, frame));
    let single = [
        extent(tags::TOTAL_PIXEL_MATRIX_ROWS, tags::ROWS) == Some(1),
        extent(tags::TOTAL_PIXEL_MATRIX_COLUMNS, tags::COLUMNS) == Some(1),
    ];
    // A single-frame image places its spacing at the top level.
    for (tag, name) in [
        (tags::PIXEL_SPACING, "Pixel Spacing (0028,0030)"),
        (
            tags::IMAGER_PIXEL_SPACING,
            "Imager Pixel Spacing (0018,1164)",
        ),
        (
            tags::NOMINAL_SCANNED_PIXEL_SPACING,
            "Nominal Scanned Pixel Spacing (0018,2010)",
        ),
    ] {
        positive_spacing(object, tag, name, single, violations);
    }
    for (path, item) in groups {
        for measures in sequence(item, tags::PIXEL_MEASURES_SEQUENCE) {
            positive_spacing(
                measures,
                tags::PIXEL_SPACING,
                &format!("{path}Pixel Measures Sequence > Pixel Spacing (0028,0030)"),
                single,
                violations,
            );
            if let Some(spacing) = number(measures, tags::SPACING_BETWEEN_SLICES) {
                if spacing < 0.0 {
                    violations.push(format!(
                        "{path}Pixel Measures Sequence > Spacing Between Slices (0018,0088) is {spacing}; it shall not be negative ({CITATION} C.7.6.16.2.1)"
                    ));
                }
            }
        }
    }
}

/// 10.7.1.3: each spacing Value is positive, or zero along a single row or
/// column of `single` (rows, columns).
fn positive_spacing(
    holder: &InMemDicomObject,
    tag: dicom_core::Tag,
    name: &str,
    single: [bool; 2],
    violations: &mut Vec<String>,
) {
    let Some(spacing) = numbers(holder, tag) else {
        return;
    };
    for (index, value) in spacing.iter().enumerate().take(2) {
        let zero_allowed = single[index] && *value == 0.0;
        if !(*value > 0.0 || zero_allowed) {
            violations.push(format!(
                "{name} Value {} is {value}; pixel spacing shall be positive ({CITATION} 10.7.1.3)",
                index + 1
            ));
        }
    }
}

/// A.32.8.4.1.2: Frames occupy a regular tiling, so every Frame's column
/// position is a multiple of Columns plus one constant, and likewise rows.
fn tiling(
    object: &InMemDicomObject,
    groups: &[(String, &InMemDicomObject)],
    violations: &mut Vec<String>,
) {
    let (Some(rows), Some(columns)) = (integer(object, tags::ROWS), integer(object, tags::COLUMNS))
    else {
        return;
    };
    if rows <= 0 || columns <= 0 {
        return;
    }
    let mut residues: [Option<i64>; 2] = [None, None];
    for (path, item) in groups {
        for position in sequence(item, tags::PLANE_POSITION_SLIDE_SEQUENCE) {
            for (index, tag, size, axis) in [
                (
                    0,
                    tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                    columns,
                    "column",
                ),
                (
                    1,
                    tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                    rows,
                    "row",
                ),
            ] {
                let Some(value) = integer(position, tag) else {
                    continue;
                };
                let residue = value.rem_euclid(size);
                match residues[index] {
                    None => residues[index] = Some(residue),
                    Some(expected) if expected != residue => {
                        violations.push(format!(
                            "{path}Plane Position (Slide) Sequence > {axis} position {value} is off the tiling of the other Frames, whose {axis} positions are {expected} modulo {size} ({CITATION} A.32.8.4.1.2)"
                        ));
                        return;
                    }
                    Some(_) => {}
                }
            }
        }
    }
}

/// The Shared Item, then each Per-Frame Item, with its path.
pub(super) fn functional_group_items(
    object: &InMemDicomObject,
) -> Vec<(String, &InMemDicomObject)> {
    let shared = sequence(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .map(|item| ("Shared Functional Groups Sequence[1] > ".to_string(), item));
    let per_frame = sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .enumerate()
        .map(|(index, item)| {
            (
                format!("Per-Frame Functional Groups Sequence[{}] > ", index + 1),
                item,
            )
        });
    shared.chain(per_frame).collect()
}

/// Each Value of a string Attribute, without padding.
pub(super) fn strings(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<Vec<String>> {
    let values = object.element(tag).ok()?.to_multi_str().ok()?;
    Some(
        values
            .iter()
            .map(|value| value.trim_matches([' ', '\0']).to_string())
            .collect(),
    )
}

/// Value 1 of a string Attribute, when it is not empty.
pub(super) fn string(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    string_value(object, tag, 1)
}

fn numbers(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<Vec<f64>> {
    object.element(tag).ok()?.to_multi_float64().ok()
}

pub(super) fn number(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<f64> {
    numbers(object, tag)?.first().copied()
}

/// Whether a Code Sequence Item holds the coded entry (scheme, value) in
/// any of its Code Value forms.
pub(super) fn is_code(item: &InMemDicomObject, (scheme, value): (&str, &str)) -> bool {
    Coded::of(item).is_some_and(|coded| coded.is(scheme, value))
}
