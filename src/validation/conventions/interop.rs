//! `interop`: configurations that named viewers, archives, converters and
//! validators of whole-slide images do not handle, each an advisory citing
//! the tool, its version and the source lines that state the behavior.

use std::collections::BTreeSet;

use dicom_core::value::Value;
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::facts::{text, Facts, Kind, SetIndex};
use super::wg26::has_bytes;
use super::{advisory, ConventionEntry, ConventionProfile, Out, Subject};
use crate::validation::disclosure::{shown, withholds};
use crate::validation::iod::{dicom_edition, integer_value, items, MAX_NESTING};

const P: ConventionProfile = ConventionProfile::Interop;

macro_rules! dmv {
    ($lines:literal) => {
        concat!("dicom-microscopy-viewer 0.49.5 ", $lines)
    };
}
macro_rules! openslide {
    ($lines:literal) => {
        concat!("OpenSlide 4.0.1 src/openslide-vendor-dicom.c:", $lines)
    };
}
macro_rules! dciodvfy {
    ($lines:literal) => {
        concat!(
            "dicom3tools dciodvfy (1.00.snapshot.20260927113651) ",
            $lines
        )
    };
}

pub(super) const DMV_FRAME_OF_REFERENCE: ConventionEntry = advisory(
    P,
    "convention-interop-dmv-frame-of-reference",
    dmv!("src/pyramid.js:111-115"),
    "The viewer throws \"Images of pyramid must all have the same Frame of Reference UID.\": VOLUME images of one Series with different or missing Frame of Reference UIDs.",
);
pub(super) const DMV_CONTAINER: ConventionEntry = advisory(
    P,
    "convention-interop-dmv-container",
    dmv!("src/pyramid.js:116-120"),
    "The viewer throws \"Images of pyramid must all have the same Container Identifier.\": VOLUME images of one Series with different Container Identifiers.",
);
pub(super) const DMV_CHANNELS: ConventionEntry = advisory(
    P,
    "convention-interop-dmv-channels",
    dmv!("src/pyramid.js:132-138"),
    "The viewer throws \"Images of pyramid must all have the same number of channels\": VOLUME images of one Series with different Number of Optical Paths (0048,0302).",
);
pub(super) const SPLIT_LAYER: ConventionEntry = advisory(
    P,
    "convention-interop-split-layer",
    concat!(
        "dicom-microscopy-viewer 0.49.5 src/pyramid.js:174-181; ",
        "OpenSlide 4.0.1 src/openslide-vendor-dicom.c:776-792"
    ),
    "Both tools refuse a layer spread over several VOLUME images of the same Total Pixel Matrix size unless they are members of one Concatenation (\"are not part of a concatenation either\", \"Slide contains unexpected image\").",
);
pub(super) const DMV_FOCAL_PLANES: ConventionEntry = advisory(
    P,
    "convention-interop-dmv-focal-planes",
    dmv!("src/metadata.js:120-122"),
    "The viewer throws \"Images with multiple focal planes are not yet supported.\" when Number of Focal Planes (0048,0013), the planes of an extended depth of field image, exceeds 1.",
);
pub(super) const ICC_FIRST_OPTICAL_PATH: ConventionEntry = advisory(
    P,
    "convention-interop-icc-first-optical-path",
    concat!(
        "dicom-microscopy-viewer 0.49.5 src/pyramid.js:35-50; ",
        "OpenSlide 4.0.1 src/openslide-vendor-dicom.c:531-539"
    ),
    "Both tools read the ICC Profile from Optical Path Sequence Item 1 only (\"ICC Profile was not found for image\"): a color VOLUME image whose first Optical Path Item has no ICC Profile.",
);
pub(super) const OPENSLIDE_TRANSFER_SYNTAX: ConventionEntry = advisory(
    P,
    "convention-interop-openslide-transfer-syntax",
    openslide!("146-156 and 880-887"),
    "OpenSlide fails with \"Unsupported transfer syntax\" outside 1.2.840.10008.1.2.1, 1.2.840.10008.1.2.4.50, 1.2.840.10008.1.2.4.90 and 1.2.840.10008.1.2.4.91.",
);
pub(super) const OPENSLIDE_PIXEL_FORMAT: ConventionEntry = advisory(
    P,
    "convention-interop-openslide-pixel-format",
    openslide!("889-898"),
    "OpenSlide requires Planar Configuration 0, Bits Allocated 8, Bits Stored 8, High Bit 7, Samples per Pixel 3, Pixel Representation 0, and Total Pixel Matrix Focal Planes 1 when present.",
);
pub(super) const OPENSLIDE_PHOTOMETRIC: ConventionEntry = advisory(
    P,
    "convention-interop-openslide-photometric",
    openslide!("907-936"),
    "OpenSlide fails with \"Unsupported photometric interpretation\" unless it is RGB, or YBR_FULL_422 for JPEG, YBR_ICT or YBR_RCT for JPEG 2000, YBR_RCT for JPEG 2000 lossless.",
);
pub(super) const OPENSLIDE_ASSOCIATED: ConventionEntry = advisory(
    P,
    "convention-interop-openslide-associated-images",
    openslide!("650-654 and 680-694"),
    "OpenSlide fails with \"Slide contains unexpected associated image\" for a second LABEL, OVERVIEW or THUMBNAIL image of a slide, and with \"Found associated image as a concatenation member\".",
);
pub(super) const SLIM_TRANSFER_SYNTAX: ConventionEntry = advisory(
    P,
    "convention-interop-slim-transfer-syntax",
    "Slim 0.46.10 DICOM Conformance Statement DCS.md:223-237",
    "Slim decodes frames in 1.2.840.10008.1.2.1, 1.2.840.10008.1.2.4.50, .51, .80, .81, .90, .91, .201, .202, .203, .110, .111 and .112 only.",
);
pub(super) const ORTHANC_MODALITY: ConventionEntry = advisory(
    P,
    "convention-interop-orthanc-modality",
    "Orthanc WSI plugin 4.0 (2026-09-10) DicomPyramidInstance.cpp:182-186 and DicomPyramid.cpp:118",
    "Orthanc skips \"a DICOM instance that is not part of a whole-slide image\" unless its Modality (0008,0060) is SM.",
);
pub(super) const ORTHANC_BASE_CONSISTENCY: ConventionEntry = advisory(
    P,
    "convention-interop-orthanc-base-consistency",
    "Orthanc WSI plugin 4.0 (2026-09-10) DicomPyramid.cpp:270-330",
    "Orthanc warns \"Inconsistency of imaged volume width/height in series\" and \"Inconsistency of objective lens power in series\" when the instances of the finest level disagree.",
);
pub(super) const ORTHANC_SPARSE_TILES: ConventionEntry = advisory(
    P,
    "convention-interop-orthanc-sparse-tiles",
    "Orthanc WSI plugin 4.0 (2026-09-10) DicomPyramidInstance.cpp:225-247",
    "Orthanc drops a Frame \"with unexpected tile location\" whose Plane Position (Slide) is not a multiple of the tile size plus 1, or lies outside the Total Pixel Matrix.",
);
pub(super) const METADATA_SEQUENCE_SIZE: ConventionEntry = advisory(
    P,
    "convention-interop-metadata-sequence-size",
    "NCI Imaging Data Commons, learn.canceridc.dev/data/data-known-issues; Google Cloud Healthcare API DICOM conformance statement (updated 2026-10-20)",
    "\"DICOM sequence tags containing more than approximately 1 MiB of data will not be returned in metadata resources\": a top-level Sequence of at least 1 MiB, typically the Per-Frame Functional Groups Sequence of a TILED_SPARSE image.",
);
pub(super) const IDC_INDEX: ConventionEntry = advisory(
    P,
    "convention-interop-idc-index",
    "idc-index-data assets/sm_index.sql:25-31 (commit 45b17c9fc22f)",
    "The IDC slide index reads Objective Lens Power from Optical Path Sequence Item 1 and Pixel Spacing from the Shared Functional Groups: a VOLUME image lacking either leaves them empty in the index.",
);
pub(super) const WSIDICOM_TILE_SIZE: ConventionEntry = advisory(
    P,
    "convention-interop-wsidicom-tile-size",
    "wsidicom 0.36.1 README.md:33 and wsidicom/config.py:42-44 (strict_tile_size_check)",
    "\"Levels are required to have ... same tile size\": VOLUME layers of one Series with different Rows or Columns.",
);
pub(super) const WSIDICOM_SCALE: ConventionEntry = advisory(
    P,
    "convention-interop-wsidicom-scale",
    "wsidicom 0.36.1 README.md:33",
    "\"Levels are required to have (close to) 2 factor scale\": a VOLUME layer whose Pixel Spacing over the base layer's is more than a tenth of a doubling (log2 0.1, about 7%) from a power of two, the validator's reading of \"close to\".",
);
pub(super) const WSIDICOM_ORIGIN: ConventionEntry = advisory(
    P,
    "convention-interop-wsidicom-origin",
    "wsidicom 0.36.1 wsidicom/config.py:54 and wsidicom/series/pyramids.py:117-150",
    "wsidicom splits instances whose Total Pixel Matrix Origins are more than pyramids_origin_threshold = 0.02 mm apart into separate pyramids: VOLUME layers of one Series that far apart.",
);
pub(super) const HIGHDICOM_OFFSET_TABLE: ConventionEntry = advisory(
    P,
    "convention-interop-highdicom-offset-table",
    "highdicom 0.28.2 docs/image.rst:296-304",
    "\"Without an offset table, the potential speed benefits of using lazy frame retrieval are usually eliminated\": multi-frame encapsulated Pixel Data with an empty Basic Offset Table and no Extended Offset Table.",
);
pub(super) const DCIODVFY_PYRAMID_UID_LABEL: ConventionEntry = advisory(
    P,
    "convention-interop-dciodvfy-pyramid-uid-label",
    dciodvfy!("appsrc/dcfile/dciodvfy.cc:4146-4164"),
    "dciodvfy warns \"Pyramid UID present in LABEL or OVERVIEW image\".",
);
pub(super) const DCIODVFY_SINGLE_FRAME: ConventionEntry = advisory(
    P,
    "convention-interop-dciodvfy-single-frame",
    dciodvfy!("libsrc/standard/module/vl.tpl:566"),
    "dciodvfy reports an error unless Number of Frames (0028,0008) is 1 for Image Type Value 3 LOCALIZER, THUMBNAIL, LABEL or OVERVIEW.",
);
pub(super) const DCIODVFY_IMAGED_VOLUME: ConventionEntry = advisory(
    P,
    "convention-interop-dciodvfy-imaged-volume",
    dciodvfy!("appsrc/dcfile/dciodvfy.cc:3612-3670"),
    "dciodvfy warns \"ImagedVolumeWidth inconsistent with TotalPixelMatrixColumns and PixelSpacing\" (and Height) when Imaged Volume Width or Height differs from Total Pixel Matrix Columns or Rows times the shared Pixel Spacing by a relative 0.001 or more.",
);
pub(super) const ASSOCIATED_FRAME_OF_REFERENCE: ConventionEntry = advisory(
    P,
    "convention-interop-associated-frame-of-reference",
    concat!(
        "DICOM PS3.3 ",
        dicom_edition!(),
        " A.1.2.30 Note against Table A.32.8-1 (CP-2406)"
    ),
    "Conflict: the A.1.2.30 Note lets LABEL, OVERVIEW and THUMBNAIL images accompany a pyramid in its Series \"if they share the same Frame of Reference (but not otherwise ...)\", while normative Table A.32.8-1 requires the Frame of Reference Module only for VOLUME and THUMBNAIL images. The normative table is followed, so DICOM validation passes a LABEL or OVERVIEW image without a Frame of Reference UID in the pyramid's Series; a consumer reading the Note may not associate it.",
);
pub(super) const XYB_PHOTOMETRIC: ConventionEntry = advisory(
    P,
    "convention-interop-xyb-photometric",
    concat!(
        "DICOM PS3.3 ",
        dicom_edition!(),
        " C.8.12.4.1.5 against the Philips Pathology Scanner SGi 1.0 DICOM Conformance Statement (Doc 8001001301)"
    ),
    "Conflict: the scanner writes Photometric Interpretation XYB for JPEG XL, but the C.8.12.4.1.5 Enumerated Values for VL WSI are MONOCHROME2, RGB, YBR_FULL_422, YBR_ICT and YBR_RCT. The normative list is followed, so DICOM validation fails such an image; the C.8.12.4.1.5 Note anticipates new values for new Transfer Syntaxes, and no CP adds XYB yet.",
);

pub(super) const ENTRIES: &[&ConventionEntry] = &[
    &DMV_FRAME_OF_REFERENCE,
    &DMV_CONTAINER,
    &DMV_CHANNELS,
    &SPLIT_LAYER,
    &DMV_FOCAL_PLANES,
    &ICC_FIRST_OPTICAL_PATH,
    &OPENSLIDE_TRANSFER_SYNTAX,
    &OPENSLIDE_PIXEL_FORMAT,
    &OPENSLIDE_PHOTOMETRIC,
    &OPENSLIDE_ASSOCIATED,
    &SLIM_TRANSFER_SYNTAX,
    &ORTHANC_MODALITY,
    &ORTHANC_BASE_CONSISTENCY,
    &ORTHANC_SPARSE_TILES,
    &METADATA_SEQUENCE_SIZE,
    &IDC_INDEX,
    &WSIDICOM_TILE_SIZE,
    &WSIDICOM_SCALE,
    &WSIDICOM_ORIGIN,
    &HIGHDICOM_OFFSET_TABLE,
    &DCIODVFY_PYRAMID_UID_LABEL,
    &DCIODVFY_SINGLE_FRAME,
    &DCIODVFY_IMAGED_VOLUME,
    &ASSOCIATED_FRAME_OF_REFERENCE,
    &XYB_PHOTOMETRIC,
];

const OPENSLIDE_SYNTAXES: &[&str] = &[
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::JPEG_BASELINE8_BIT,
    uids::JPEG2000_LOSSLESS,
    uids::JPEG2000,
];
const SLIM_SYNTAXES: &[&str] = &[
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::JPEG_BASELINE8_BIT,
    uids::JPEG_EXTENDED12_BIT,
    uids::JPEGLS_LOSSLESS,
    uids::JPEGLS_NEAR_LOSSLESS,
    uids::JPEG2000_LOSSLESS,
    uids::JPEG2000,
    uids::HTJ2K_LOSSLESS,
    uids::HTJ2K_LOSSLESS_RPCL,
    uids::HTJ2K,
    uids::JPEGXL_LOSSLESS,
    uids::JPEGXLJPEG_RECOMPRESSION,
    uids::JPEGXL,
];
/// "approximately 1 MiB".
const METADATA_LIMIT_BYTES: u64 = 1024 * 1024;
/// wsidicom's pyramids_origin_threshold, in mm.
const WSIDICOM_ORIGIN_MM: f64 = 0.02;
/// The validator's reading of wsidicom's "(close to)" a power of two.
const WSIDICOM_SCALE_LOG2: f64 = 0.1;
/// Misplaced sparse Frames described in the Orthanc advisory.
const MISPLACED_SHOWN: usize = 5;
/// dciodvfy's relative tolerance for the Imaged Volume.
const DCIODVFY_IMAGED_VOLUME_RELATIVE: f64 = 0.001;

pub(super) fn file_advisories(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    if facts.kind != Kind::Wsi {
        return;
    }
    let root: &InMemDicomObject = object;
    let file = Subject::File(&facts.path);
    unevaluated(facts, out);
    // The Transfer Syntax, when the source names it.
    let syntax = facts
        .gaps
        .transfer_syntax
        .is_none()
        .then_some(facts.transfer_syntax.as_str());
    if facts.flavor_is(&["VOLUME", "LABEL", "OVERVIEW", "THUMBNAIL"]) {
        openslide(root, facts, syntax, out);
    }
    if let Some(syntax) = syntax.filter(|syntax| !SLIM_SYNTAXES.contains(syntax)) {
        out.advise(
            &SLIM_TRANSFER_SYNTAX,
            file,
            format!("Transfer Syntax {syntax} is not one Slim decodes"),
        );
    }
    if text(root, tags::MODALITY).as_deref() != Some("SM") {
        out.advise(
            &ORTHANC_MODALITY,
            file,
            format!(
                "Modality (0008,0060) is {:?}, not SM, so Orthanc leaves the image out of the slide",
                text(root, tags::MODALITY)
            ),
        );
    }
    if facts.is_layer() {
        sparse_tiles(root, facts, out);
    }
    metadata_size(root, facts, out);
    if facts.is_volume() {
        idc_index(facts, out);
        icc_first_path(root, facts, out);
    }
    if facts.gaps.pixel_data.is_none() {
        offset_table(object, facts, out);
    }
    dciodvfy(root, facts, out);
    if integer_value(root, tags::NUMBER_OF_FOCAL_PLANES).is_some_and(|planes| planes > 1) {
        out.advise(
            &DMV_FOCAL_PLANES,
            file,
            format!(
                "Number of Focal Planes (0048,0013) is {}",
                integer_value(root, tags::NUMBER_OF_FOCAL_PLANES).unwrap_or_default()
            ),
        );
    }
    if facts.photometric.as_deref() == Some("XYB") {
        out.advise(
            &XYB_PHOTOMETRIC,
            file,
            "Photometric Interpretation (0028,0004) is XYB".into(),
        );
    }
}

pub(super) fn set_advisories(index: &SetIndex<'_>, out: &mut Out) {
    for series in &index.series {
        let (uid, members) = (series.uid, &series.members);
        let subject = Subject::Series(uid);
        let volumes: Vec<&Facts> = members
            .iter()
            .copied()
            .filter(|fact| fact.is_volume())
            .collect();
        for (entry, (tag, what), values) in [
            (
                &DMV_FRAME_OF_REFERENCE,
                (tags::FRAME_OF_REFERENCE_UID, "Frame of Reference UIDs"),
                distinct(&volumes, |fact| fact.frame_of_reference_uid.clone()),
            ),
            (
                &DMV_CONTAINER,
                (tags::CONTAINER_IDENTIFIER, "Container Identifiers"),
                distinct(&volumes, |fact| fact.container_identifier.clone()),
            ),
            (
                &DMV_CHANNELS,
                (tags::NUMBER_OF_OPTICAL_PATHS, "Number of Optical Paths"),
                distinct(&volumes, |fact| {
                    fact.optical_paths.map(|paths| paths.to_string())
                }),
            ),
        ] {
            if values.len() > 1 {
                out.advise(
                    entry,
                    subject,
                    format!(
                        "the VOLUME images have {} {what}: {}",
                        values.len(),
                        values
                            .iter()
                            .map(|value| listed(tag, value))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
        split_layers(&volumes, subject, out);
        associated_images(members, subject, out);
        base_consistency(&volumes, subject, out);
        wsidicom(&volumes, subject, out);
        associated_frame_of_reference(members, &volumes, out);
    }
}

/// A `distinct` value of `tag` as a message lists it: as it is, unless
/// `disclosure` withholds it.
fn listed(tag: dicom_core::Tag, value: &str) -> String {
    if value != "absent" && withholds(tag) {
        shown(tag, value).to_string()
    } else {
        value.to_string()
    }
}

/// The distinct values, with absence as "absent".
fn distinct(facts: &[&Facts], value: impl Fn(&Facts) -> Option<String>) -> BTreeSet<String> {
    facts
        .iter()
        .map(|fact| value(fact).unwrap_or_else(|| "absent".into()))
        .collect()
}

/// The advisories a source without the Transfer Syntax or the encoded
/// Pixel Data cannot evaluate, reported as one skipped result.
fn unevaluated(facts: &Facts, out: &mut Out) {
    let mut missing = Vec::new();
    if let Some(reason) = &facts.gaps.transfer_syntax {
        missing.push(format!(
            "{reason}, so {}, {} and {} were not evaluated",
            OPENSLIDE_TRANSFER_SYNTAX.name, OPENSLIDE_PHOTOMETRIC.name, SLIM_TRANSFER_SYNTAX.name
        ));
    }
    if let Some(reason) = &facts.gaps.pixel_data {
        missing.push(format!(
            "{reason}, so {} was not evaluated",
            HIGHDICOM_OFFSET_TABLE.name
        ));
    }
    if !missing.is_empty() {
        out.skip(
            &super::EVALUABLE_INTEROP,
            Subject::File(&facts.path),
            missing.join("; "),
        );
    }
}

/// OpenSlide's file requirements; those of the Transfer Syntax only when
/// `syntax` is known.
fn openslide(root: &InMemDicomObject, facts: &Facts, syntax: Option<&str>, out: &mut Out) {
    let file = Subject::File(&facts.path);
    if let Some(syntax) = syntax.filter(|syntax| !OPENSLIDE_SYNTAXES.contains(syntax)) {
        out.advise(
            &OPENSLIDE_TRANSFER_SYNTAX,
            file,
            format!("Unsupported transfer syntax {syntax}"),
        );
        return;
    }
    let mut format = Vec::new();
    for (tag, name, expected, required) in [
        (tags::PLANAR_CONFIGURATION, "Planar Configuration", 0, true),
        (tags::BITS_ALLOCATED, "Bits Allocated", 8, true),
        (tags::BITS_STORED, "Bits Stored", 8, true),
        (tags::HIGH_BIT, "High Bit", 7, true),
        (tags::SAMPLES_PER_PIXEL, "Samples per Pixel", 3, true),
        (tags::PIXEL_REPRESENTATION, "Pixel Representation", 0, true),
        (
            tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
            "Total Pixel Matrix Focal Planes",
            1,
            false,
        ),
    ] {
        match integer_value(root, tag) {
            Some(value) if value == expected => {}
            None if !required => {}
            value => format.push(format!(
                "{name} is {}, not {expected}",
                value.map_or("absent".to_string(), |value| value.to_string())
            )),
        }
    }
    if !format.is_empty() {
        out.advise(&OPENSLIDE_PIXEL_FORMAT, file, format.join("; "));
    }
    let Some(syntax) = syntax else {
        return;
    };
    let photometric = facts.photometric.as_deref().unwrap_or("");
    let supported: &[&str] = match syntax {
        uids::JPEG2000 => &["YBR_ICT", "YBR_RCT", "RGB"],
        uids::JPEG2000_LOSSLESS => &["YBR_RCT", "RGB"],
        uids::JPEG_BASELINE8_BIT => &["YBR_FULL_422", "RGB"],
        _ => &["RGB"],
    };
    if !supported.contains(&photometric) {
        out.advise(
            &OPENSLIDE_PHOTOMETRIC,
            file,
            format!("Unsupported photometric interpretation {photometric:?} for {syntax}"),
        );
    }
}

fn sparse_tiles(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    let (Some((tile_rows, tile_columns)), Some((rows, columns))) = (facts.tile, facts.matrix)
    else {
        return;
    };
    if tile_rows == 0 || tile_columns == 0 {
        return;
    }
    // The misplaced Frames: the first `MISPLACED_SHOWN` described, all counted.
    let (mut shown, mut misplaced) = (Vec::new(), 0usize);
    for (index, frame) in items(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .iter()
        .enumerate()
    {
        let Some(position) = items(frame, tags::PLANE_POSITION_SLIDE_SEQUENCE).first() else {
            continue;
        };
        let column = integer_value(position, tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX);
        let row = integer_value(position, tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX);
        let (Some(column), Some(row)) = (column, row) else {
            continue;
        };
        let aligned = |value: i64, tile: u64, extent: u64| {
            u64::try_from(value - 1).is_ok_and(|offset| offset % tile == 0 && offset < extent)
        };
        if !aligned(column, tile_columns, columns) || !aligned(row, tile_rows, rows) {
            misplaced += 1;
            if shown.len() < MISPLACED_SHOWN {
                shown.push(format!("Frame {} at column {column}, row {row}", index + 1));
            }
        }
    }
    if misplaced > 0 {
        out.advise(
            &ORTHANC_SPARSE_TILES,
            Subject::File(&facts.path),
            format!(
                "{misplaced} Frame(s) with unexpected tile location for {tile_columns}x{tile_rows} tiles: {}{}",
                shown.join(", "),
                if misplaced > shown.len() { ", ..." } else { "" }
            ),
        );
    }
}

/// An estimate of the encoded bytes of a Data Set: Explicit VR element
/// headers, padded Values, and Item headers, to `MAX_NESTING` levels.
fn encoded_bytes(item: &InMemDicomObject, depth: usize) -> u64 {
    item.iter()
        .map(|element| {
            12 + match element.value() {
                Value::Primitive(value) => value.calculate_byte_len() as u64,
                Value::Sequence(sequence) if depth < MAX_NESTING => sequence
                    .items()
                    .iter()
                    .map(|nested| 8 + encoded_bytes(nested, depth + 1))
                    .sum(),
                Value::Sequence(_) => 0,
                Value::PixelSequence(_) => 0,
            }
        })
        .sum()
}

fn metadata_size(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    for element in root.iter() {
        let Value::Sequence(sequence) = element.value() else {
            continue;
        };
        let bytes: u64 = sequence
            .items()
            .iter()
            .map(|item| 8 + encoded_bytes(item, 1))
            .sum();
        if bytes >= METADATA_LIMIT_BYTES {
            out.advise(
                &METADATA_SEQUENCE_SIZE,
                Subject::File(&facts.path),
                format!(
                    "Sequence {} encodes about {bytes} bytes in {} Items, so Retrieve Metadata and the IDC BigQuery export drop it",
                    element.header().tag,
                    sequence.items().len()
                ),
            );
        }
    }
}

fn idc_index(facts: &Facts, out: &mut Out) {
    let mut missing = Vec::new();
    if facts.objective_lens_power.is_none() {
        missing.push("Objective Lens Power (0048,0112) in Optical Path Sequence Item 1");
    }
    if facts.spacing.is_none() {
        missing.push("Pixel Spacing (0028,0030) in the Shared Functional Groups Sequence");
    }
    if !missing.is_empty() {
        out.advise(
            &IDC_INDEX,
            Subject::File(&facts.path),
            format!("the VOLUME image has no {}", missing.join(" and no ")),
        );
    }
}

fn icc_first_path(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    if facts.photometric.as_deref() == Some("MONOCHROME2") {
        return;
    }
    let first = items(root, tags::OPTICAL_PATH_SEQUENCE).first();
    if !first.is_some_and(|path| has_bytes(path, tags::ICC_PROFILE)) {
        out.advise(
            &ICC_FIRST_OPTICAL_PATH,
            Subject::File(&facts.path),
            "the color VOLUME image has no ICC Profile (0028,2000) in Optical Path Sequence Item 1"
                .into(),
        );
    }
}

fn offset_table(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    let Some(element) = object.get(tags::PIXEL_DATA) else {
        return;
    };
    let Some(basic) = element.value().offset_table() else {
        return;
    };
    let frames = integer_value(object, tags::NUMBER_OF_FRAMES).unwrap_or(1);
    if frames > 1 && basic.is_empty() && object.get(tags::EXTENDED_OFFSET_TABLE).is_none() {
        out.advise(
            &HIGHDICOM_OFFSET_TABLE,
            Subject::File(&facts.path),
            format!("the {frames} encapsulated Frames have neither a Basic nor an Extended Offset Table"),
        );
    }
}

fn dciodvfy(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    let file = Subject::File(&facts.path);
    if facts.shows_label() && facts.pyramid_uid.is_some() {
        out.advise(
            &DCIODVFY_PYRAMID_UID_LABEL,
            file,
            format!(
                "Pyramid UID present in LABEL or OVERVIEW image: the {} image has Pyramid UID {}",
                facts.flavor.as_deref().unwrap_or_default(),
                shown(
                    tags::PYRAMID_UID,
                    facts.pyramid_uid.as_deref().unwrap_or_default()
                )
            ),
        );
    }
    if facts.flavor_is(&["LOCALIZER", "THUMBNAIL", "LABEL", "OVERVIEW"]) {
        let frames = integer_value(root, tags::NUMBER_OF_FRAMES);
        if frames != Some(1) {
            out.advise(
                &DCIODVFY_SINGLE_FRAME,
                file,
                format!(
                    "the {} image has Number of Frames {}",
                    facts.flavor.as_deref().unwrap_or_default(),
                    frames.map_or("absent".to_string(), |frames| frames.to_string())
                ),
            );
        }
    }
    let (Some((row_spacing, column_spacing)), Some((rows, columns))) =
        (facts.spacing, facts.matrix)
    else {
        return;
    };
    for (name, imaged, count, spacing, extent) in [
        (
            "ImagedVolumeWidth",
            facts.imaged_volume.0,
            columns,
            column_spacing,
            "TotalPixelMatrixColumns",
        ),
        (
            "ImagedVolumeHeight",
            facts.imaged_volume.1,
            rows,
            row_spacing,
            "TotalPixelMatrixRows",
        ),
    ] {
        let Some(imaged) = imaged.filter(|value| *value > 0.0) else {
            continue;
        };
        // dciodvfy compares only when both Imaged Volume Values are positive.
        if facts.imaged_volume.0.unwrap_or(0.0) <= 0.0
            || facts.imaged_volume.1.unwrap_or(0.0) <= 0.0
        {
            continue;
        }
        let computed = count as f64 * spacing;
        if (computed / imaged - 1.0).abs() > DCIODVFY_IMAGED_VOLUME_RELATIVE {
            out.advise(
                &DCIODVFY_IMAGED_VOLUME,
                file,
                format!(
                    "{name} inconsistent with {extent} and PixelSpacing: {imaged} mm, expected {computed} mm for {count} at {spacing} mm"
                ),
            );
        }
    }
}

/// Same-size VOLUME images that are not all members of one Concatenation.
fn split_layers(volumes: &[&Facts], subject: Subject<'_>, out: &mut Out) {
    let mut sizes: Vec<(u64, u64)> = volumes.iter().filter_map(|fact| fact.matrix).collect();
    sizes.sort_unstable();
    sizes.dedup();
    for size in sizes {
        let layer: Vec<&&Facts> = volumes
            .iter()
            .filter(|fact| fact.matrix == Some(size))
            .collect();
        if layer.len() < 2 {
            continue;
        }
        let concatenations: BTreeSet<Option<&str>> = layer
            .iter()
            .map(|fact| fact.concatenation_uid.as_deref())
            .collect();
        let one_concatenation = concatenations.len() == 1
            && concatenations.iter().all(Option::is_some)
            && layer.iter().all(|fact| fact.concatenation_source.is_some());
        if !one_concatenation {
            out.advise(
                &SPLIT_LAYER,
                subject,
                format!(
                    "{} VOLUME images have the {}x{} Total Pixel Matrix but are not members of one Concatenation",
                    layer.len(),
                    size.1,
                    size.0
                ),
            );
        }
    }
}

fn associated_images(members: &[&Facts], subject: Subject<'_>, out: &mut Out) {
    for flavor in ["LABEL", "OVERVIEW", "THUMBNAIL"] {
        let images: Vec<&&Facts> = members
            .iter()
            .filter(|fact| fact.flavor_is(&[flavor]))
            .collect();
        let instances: BTreeSet<Option<&str>> = images
            .iter()
            .map(|fact| fact.sop_instance_uid.as_deref())
            .collect();
        if instances.len() > 1 {
            out.advise(
                &OPENSLIDE_ASSOCIATED,
                subject,
                format!(
                    "Slide contains unexpected associated image: {} {flavor} images",
                    instances.len()
                ),
            );
        }
        for image in images
            .iter()
            .filter(|fact| fact.concatenation_uid.is_some())
        {
            out.advise(
                &OPENSLIDE_ASSOCIATED,
                Subject::File(&image.path),
                format!("Found associated image as a concatenation member: the {flavor} image"),
            );
        }
    }
}

fn near(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6 * left.abs().max(right.abs()).max(1.0)
}

/// The finest level's instances agree on Imaged Volume and lens power.
fn base_consistency(volumes: &[&Facts], subject: Subject<'_>, out: &mut Out) {
    let Some(largest) = volumes.iter().filter_map(|fact| fact.area()).max() else {
        return;
    };
    let base: Vec<&&Facts> = volumes
        .iter()
        .filter(|fact| fact.area() == Some(largest))
        .collect();
    let disagree = |value: &dyn Fn(&Facts) -> Option<f64>| {
        let found: Vec<f64> = base.iter().filter_map(|fact| value(fact)).collect();
        found.windows(2).any(|pair| !near(pair[0], pair[1]))
    };
    if disagree(&|fact| fact.imaged_volume.0) || disagree(&|fact| fact.imaged_volume.1) {
        out.advise(
            &ORTHANC_BASE_CONSISTENCY,
            subject,
            "Inconsistency of imaged volume width/height in series: the instances of the finest level disagree".into(),
        );
    }
    if disagree(&|fact| fact.objective_lens_power) {
        out.advise(
            &ORTHANC_BASE_CONSISTENCY,
            subject,
            "Inconsistency of objective lens power in series: the instances of the finest level disagree".into(),
        );
    }
}

fn wsidicom(volumes: &[&Facts], subject: Subject<'_>, out: &mut Out) {
    let tiles = distinct(volumes, |fact| {
        fact.tile.map(|(rows, columns)| format!("{columns}x{rows}"))
    });
    if tiles.len() > 1 {
        out.advise(
            &WSIDICOM_TILE_SIZE,
            subject,
            format!(
                "the VOLUME layers have tile sizes {}",
                tiles.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
    }
    let Some(base) = volumes.iter().max_by_key(|fact| fact.area()) else {
        return;
    };
    for fact in volumes {
        if let (Some((_, base_spacing)), Some((_, spacing))) = (base.spacing, fact.spacing) {
            let scale = (spacing / base_spacing).log2();
            if (scale - scale.round()).abs() > WSIDICOM_SCALE_LOG2 {
                out.advise(
                    &WSIDICOM_SCALE,
                    subject,
                    format!(
                        "{} is downsampled {:.3} times from the base layer, not close to a power of two",
                        fact.path.display(),
                        spacing / base_spacing
                    ),
                );
            }
        }
        let origins = base
            .origin
            .zip(fact.origin)
            .and_then(|(base, own)| Some(((own.x? - base.x?), (own.y? - base.y?))));
        if let Some((dx, dy)) = origins {
            let distance = dx.hypot(dy);
            if distance > WSIDICOM_ORIGIN_MM {
                out.advise(
                    &WSIDICOM_ORIGIN,
                    subject,
                    format!(
                        "the Total Pixel Matrix Origin of {} is {distance:.4} mm from the base layer's",
                        fact.path.display()
                    ),
                );
            }
        }
    }
}

fn associated_frame_of_reference(members: &[&Facts], volumes: &[&Facts], out: &mut Out) {
    if !volumes
        .iter()
        .any(|fact| fact.frame_of_reference_uid.is_some())
    {
        return;
    }
    for image in members
        .iter()
        .filter(|fact| fact.shows_label() && fact.frame_of_reference_uid.is_none())
    {
        out.advise(
            &ASSOCIATED_FRAME_OF_REFERENCE,
            Subject::File(&image.path),
            format!(
                "the {} image has no Frame of Reference UID, but shares the Series of a pyramid that has one",
                image.flavor.as_deref().unwrap_or_default()
            ),
        );
    }
}
