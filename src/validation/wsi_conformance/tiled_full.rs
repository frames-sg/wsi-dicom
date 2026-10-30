//! The implicit layout of TILED_FULL Frames (PS3.3 C.7.6.17.3): every tile of
//! the Total Pixel Matrix at every focal plane for every optical path, varying
//! along the row, then the column, then the depth from the glass slide, then
//! the Optical Path Sequence, across all Frames of a Concatenation.

use std::collections::BTreeMap;

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::wsi_image::{integer, sequence, string, strings};
use crate::validation::disclosure::shown;
use crate::validation::iod::dicom_edition;

const CITATION: &str = concat!("PS3.3 ", dicom_edition!(), " C.7.6.17.3");

/// The counts the implicit layout is derived from, all positive. They are
/// held as i128, in which no product of UL and US counts overflows.
struct Layout {
    rows: i128,
    columns: i128,
    across: i128,
    down: i128,
    planes: i128,
    /// Optical Path Identifiers in Optical Path Sequence order.
    paths: Vec<Option<String>>,
}

impl Layout {
    fn tiles(&self) -> i128 {
        self.across * self.down
    }

    /// Frames of the whole layout.
    fn frames(&self) -> i128 {
        self.tiles() * self.planes * self.paths.len() as i128
    }

    /// (tile, plane, path) of Frame `ordinal` (0-based) of the layout.
    fn place(&self, ordinal: i128) -> (i128, i128, usize) {
        let tile = ordinal % self.tiles();
        let plane = (ordinal / self.tiles()) % self.planes;
        let path = ordinal / (self.tiles() * self.planes);
        (tile, plane, usize::try_from(path).unwrap_or(usize::MAX))
    }

    /// Row and column of the top left pixel of `tile`.
    fn position(&self, tile: i128) -> (i128, i128) {
        (
            (tile / self.across) * self.rows + 1,
            (tile % self.across) * self.columns + 1,
        )
    }
}

/// Violations of the TILED_FULL layout; nothing to check for other
/// organizations, or while the attributes it derives from are absent or not
/// positive, which the IOD checks report.
pub(super) fn validate_tiled_full(object: &InMemDicomObject, violations: &mut Vec<String>) {
    if string(object, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() != Some("TILED_FULL") {
        return;
    }
    let Some(layout) = layout(object) else {
        return;
    };
    let Some(frames) = integer(object, tags::NUMBER_OF_FRAMES).filter(|frames| *frames > 0) else {
        return;
    };
    let frames = i128::from(frames);
    let flavor = strings(object, tags::IMAGE_TYPE).and_then(|values| values.get(2).cloned());
    if flavor.as_deref().is_some_and(|flavor| flavor != "VOLUME") {
        // A single-Frame THUMBNAIL, LABEL or OVERVIEW image may cover an extent
        // other than the Total Pixel Matrix; TILED_FULL then only places its
        // one tile (C.7.6.17.3 NOTE), at the first position.
        explicit_positions(object, &layout, (0, 1), violations);
        return;
    }
    let total = layout.frames();
    let offset = frame_count(object, total, frames, &layout, violations);
    explicit_positions(object, &layout, (offset, frames), violations);
}

fn layout(object: &InMemDicomObject) -> Option<Layout> {
    let positive = |tag| {
        integer(object, tag)
            .filter(|value| *value > 0)
            .map(i128::from)
    };
    let rows = positive(tags::ROWS)?;
    let columns = positive(tags::COLUMNS)?;
    let matrix_rows = positive(tags::TOTAL_PIXEL_MATRIX_ROWS)?;
    let matrix_columns = positive(tags::TOTAL_PIXEL_MATRIX_COLUMNS)?;
    let planes = positive(tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES)?;
    let paths: Vec<Option<String>> = sequence(object, tags::OPTICAL_PATH_SEQUENCE)
        .iter()
        .map(|path| string(path, tags::OPTICAL_PATH_IDENTIFIER))
        .collect();
    if paths.is_empty() {
        return None;
    }
    Some(Layout {
        rows,
        columns,
        across: (matrix_columns + columns - 1) / columns,
        down: (matrix_rows + rows - 1) / rows,
        planes,
        paths,
    })
}

/// Number of Frames against the layout, for the whole image or this member
/// of a Concatenation; the member's first Frame in the layout.
fn frame_count(
    object: &InMemDicomObject,
    total: i128,
    frames: i128,
    layout: &Layout,
    violations: &mut Vec<String>,
) -> i128 {
    let describe = || {
        format!(
            "{}x{} tiles of {}x{} pixels, {} focal plane(s) and {} optical path(s) make {total} Frames",
            layout.across,
            layout.down,
            layout.columns,
            layout.rows,
            layout.planes,
            layout.paths.len()
        )
    };
    if object.element(tags::CONCATENATION_UID).is_err() {
        if frames != total {
            violations.push(format!(
                "TILED_FULL Frames are every tile at every focal plane for every optical path: {}, but Number of Frames is {frames} ({CITATION})",
                describe()
            ));
        }
        return 0;
    }
    let offset = i128::from(integer(object, tags::CONCATENATION_FRAME_OFFSET_NUMBER).unwrap_or(0));
    let last = integer(object, tags::IN_CONCATENATION_NUMBER)
        .zip(integer(object, tags::IN_CONCATENATION_TOTAL_NUMBER))
        .is_some_and(|(number, members)| number == members);
    let end = offset + frames;
    if end > total || (last && end != total) {
        violations.push(format!(
            "TILED_FULL Frames across the Concatenation are every tile at every focal plane for every optical path: {}, but this {}member holds Frames {} to {end} (Concatenation Frame Offset Number {offset}, Number of Frames {frames}) ({CITATION})",
            describe(),
            if last { "last " } else { "" },
            offset + 1
        ));
    }
    offset
}

/// Explicit Plane Position (Slide) and Optical Path Identification Items
/// agree with the implicit layout, and each tile's focal planes go from the
/// glass slide towards the coverslip. `(offset, frames)` are the first Frame
/// of the layout this instance holds, and how many it holds; a Shared Item
/// describes every Frame.
fn explicit_positions(
    object: &InMemDicomObject,
    layout: &Layout,
    (offset, frames): (i128, i128),
    violations: &mut Vec<String>,
) {
    let shared = sequence(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE).first();
    let per_frame = sequence(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
    // Shared Items describe every Frame alike, so the first two Frames
    // already disagree with the layout when any do.
    let count = if per_frame.is_empty() {
        usize::try_from(frames.min(2)).unwrap_or(0)
    } else {
        per_frame.len()
    };
    let mut misplaced = Vec::new();
    let mut depths = BTreeMap::<i128, Vec<(i128, f64, usize)>>::new();
    for index in 0..count {
        let ordinal = offset + index as i128;
        let (tile, plane, path) = layout.place(ordinal);
        let group = |tag| {
            per_frame
                .get(index)
                .and_then(|frame| sequence(frame, tag).first())
                .or_else(|| shared.and_then(|shared| sequence(shared, tag).first()))
        };
        if let Some(position) = group(tags::PLANE_POSITION_SLIDE_SEQUENCE) {
            let actual = (
                integer(position, tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
                integer(position, tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
            );
            let (row, column) = layout.position(tile);
            if let (Some(actual_row), Some(actual_column)) = actual {
                if (i128::from(actual_row), i128::from(actual_column)) != (row, column) {
                    misplaced.push(format!(
                        "Frame {} is at row {actual_row}, column {actual_column}, not at the implicit row {row}, column {column}",
                        index + 1
                    ));
                }
            }
            if let Some(depth) = position
                .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .ok()
                .and_then(|element| element.to_float64().ok())
            {
                depths
                    .entry(tile * layout.paths.len() as i128 + path as i128)
                    .or_default()
                    .push((plane, depth, index + 1));
            }
        }
        if let Some(identification) = group(tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE) {
            let expected = layout.paths.get(path).cloned().flatten();
            let actual = string(identification, tags::OPTICAL_PATH_IDENTIFIER);
            if actual.is_some() && expected.is_some() && actual != expected {
                misplaced.push(format!(
                    "Frame {} has Optical Path Identifier {}, not {} of Optical Path Sequence Item {}",
                    index + 1,
                    shown(tags::OPTICAL_PATH_IDENTIFIER, &actual.unwrap_or_default()),
                    shown(tags::OPTICAL_PATH_IDENTIFIER, &expected.unwrap_or_default()),
                    path + 1
                ));
            }
        }
    }
    if let Some(first) = misplaced.first() {
        violations.push(format!(
            "TILED_FULL Frames follow the implicit order of row, column, focal plane and optical path; {first} ({} Frame(s) disagree; {CITATION})",
            misplaced.len()
        ));
    }
    plane_order(depths, violations);
}

/// Each tile's Z offsets increase with its focal plane.
fn plane_order(depths: BTreeMap<i128, Vec<(i128, f64, usize)>>, violations: &mut Vec<String>) {
    for mut planes in depths.into_values() {
        planes.sort_by_key(|(plane, _, _)| *plane);
        if let Some(pair) = planes
            .windows(2)
            .find(|pair| pair[0].0 < pair[1].0 && pair[1].1 <= pair[0].1)
        {
            violations.push(format!(
                "TILED_FULL focal planes go from the glass slide towards the coverslip, but Frame {} (Z Offset {} µm) follows Frame {} (Z Offset {} µm) of the same tile ({CITATION})",
                pair[1].2, pair[1].1, pair[0].2, pair[0].1
            ));
            return;
        }
    }
}
