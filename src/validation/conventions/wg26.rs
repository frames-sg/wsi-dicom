//! `wg26`: the DICOM WG-26 Digital Pathology Connectathon Technical
//! Requirements 2026 that objects and sets show, and the nine checks that
//! dicom3tools `dciodvfy -profile WG26SP2025` adds to the VL Whole Slide
//! Microscopy Image and Microscopy Bulk Simple Annotations IODs.

use std::collections::BTreeSet;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::facts::{describe_code, is_code, text, volume_layers, Facts, Kind, SetIndex};
use super::specimen::{specimens, EMBEDDING_MEDIUM, STAINING, TISSUE_FIXATIVE, USING_SUBSTANCE};
use super::{advisory, check, ConventionEntry, ConventionProfile, Out, Subject, Violations};
use crate::validation::iod::{integer_value, items};

const P: ConventionProfile = ConventionProfile::Wg26;

/// A line of the WG26SP2025 profile in dicom3tools 1.00.snapshot.20260927113651.
macro_rules! sp2025 {
    ($line:literal) => {
        concat!(
            "dicom3tools dciodvfy -profile WG26SP2025 (1.00.snapshot.20260927113651) libsrc/standard/",
            $line
        )
    };
}

/// A numbered requirement of the 2026 connectathon document.
macro_rules! connectathon {
    ($actor:literal, $number:literal) => {
        concat!(
            "DICOM WG-26 Digital Pathology Connectathon Technical Requirements 2026 (last update 2026/07/31), ",
            $actor,
            " requirement ",
            $number
        )
    };
}

pub(super) const SP_DIMENSION_ORGANIZATION: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-dimension-organization-type",
    sp2025!("module/vl.tpl:701"),
    "Dimension Organization Type (0020,9311), when present, is TILED_FULL (Type 3, StringEnumValues DimensionOrganizationTypeTILED_FULL).",
);
pub(super) const SP_FOCAL_PLANES: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-focal-planes",
    sp2025!("module/vl.tpl:702"),
    "Total Pixel Matrix Focal Planes (0048,0303) is present (Type 1) with Value 1.",
);
pub(super) const SP_ACQUISITION_UID: ConventionEntry = advisory(
    P,
    "convention-wg26-sp2025-acquisition-uid",
    sp2025!("module/vl.tpl:704"),
    "dciodvfy warns \"AcquisitionUID missing\" when Acquisition UID (0008,0017) is absent.",
);
pub(super) const SP_PYRAMID_UID: ConventionEntry = advisory(
    P,
    "convention-wg26-sp2025-pyramid-uid",
    sp2025!("module/vl.tpl:705"),
    "dciodvfy warns \"PyramidUID missing for Volume or Thumbnail ImageType\" when a VOLUME or THUMBNAIL image has no Pyramid UID (0008,0019).",
);
pub(super) const SP_CONCATENATION: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-concatenation-uid",
    sp2025!("module/vl.tpl:707"),
    "Concatenation UID (0020,9161) is absent (Type 1C, Condition Never).",
);
pub(super) const SP_SPECIMEN_TYPE: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-specimen-type",
    sp2025!("module/vl.tpl:708-711"),
    "Specimen Description Sequence (0040,0560) has at least one Item, and each has a Specimen Type Code Sequence (0040,059A) of exactly one Item (Type 1, VM 1, Baseline CID 8103).",
);
pub(super) const SP_SPECIMEN_PREPARATION: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-specimen-preparation",
    sp2025!("module/vl.tpl:712-716"),
    "In each Specimen Description Item, Specimen Preparation Sequence (0040,0610) has at least one Item, and each of them has at least one Specimen Preparation Step Content Item Sequence (0040,0612) Item (both Type 1, VM 1-n).",
);
pub(super) const SP_PRIMARY_ANATOMIC_STRUCTURE: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-primary-anatomic-structure",
    sp2025!("module/vl.tpl:717 and module/base.tpl:219-225"),
    "Each Specimen Description Item has a Primary Anatomic Structure Sequence (0008,2228) of at least one Item (Type 1, PrimaryAnatomicStructureMandatoryMacro).",
);
pub(super) const SP_ANNOTATION_COORDINATE_TYPE: ConventionEntry = check(
    P,
    "convention-wg26-sp2025-annotation-coordinate-type",
    sp2025!("module/vl.tpl:1615"),
    "A Microscopy Bulk Simple Annotations object's Annotation Coordinate Type (006A,0001) is 2D.",
);

pub(super) const ONE_SERIES_PER_SCAN: ConventionEntry = check(
    P,
    "convention-wg26-one-series-per-scan",
    connectathon!("Acquisition Modality", "3 and 3.a"),
    "One Series per slide scan: the instances of a Series share one Container Identifier, Barcode Value and Acquisition UID where present, and no Acquisition UID occurs in two Series.",
);
pub(super) const LABEL_IMAGE: ConventionEntry = check(
    P,
    "convention-wg26-label-image",
    connectathon!("Acquisition Modality", "4 and 4.a"),
    "Each slide Series includes an image with the slide label in its pixel data: a LABEL or OVERVIEW image.",
);
pub(super) const BARCODE_VALUE: ConventionEntry = check(
    P,
    "convention-wg26-barcode-value",
    connectathon!("Acquisition Modality", "4.b"),
    "A LABEL or OVERVIEW image of the Series stores the query key read from the label in Barcode Value (2200,0005).",
);
pub(super) const TRUE_COLOR_BRIGHTFIELD: ConventionEntry = check(
    P,
    "convention-wg26-true-color-brightfield",
    connectathon!("Acquisition Modality", "6"),
    "Only true color brightfield images: Samples per Pixel (0028,0002) is 3, and each Optical Path Sequence Item of a VOLUME or THUMBNAIL image has Illumination Type (111744, DCM, \"Brightfield illumination\") or (111741, DCM, \"Transmission illumination\").",
);
pub(super) const TRANSFER_SYNTAX: ConventionEntry = check(
    P,
    "convention-wg26-transfer-syntax",
    connectathon!("Acquisition Modality", "6.b"),
    "Only baseline JPEG (1.2.840.10008.1.2.4.50) and JPEG 2000 (1.2.840.10008.1.2.4.90, 1.2.840.10008.1.2.4.91) Transfer Syntaxes; the top layer of the pyramid and the THUMBNAIL, LABEL and OVERVIEW images may also be uncompressed (1.2.840.10008.1.2, 1.2.840.10008.1.2.1, 1.2.840.10008.1.2.1.98).",
);
pub(super) const LOSSY_LAYERS: ConventionEntry = advisory(
    P,
    "convention-wg26-lossy-layers",
    connectathon!("Acquisition Modality", "6.b"),
    "\"It is expected that only lossy compressed images will be sent for the high-resolution and intermediate resolution layers of the pyramid\": a VOLUME layer below the top in JPEG 2000 lossless (1.2.840.10008.1.2.4.90).",
);
pub(super) const PYRAMID: ConventionEntry = check(
    P,
    "convention-wg26-pyramid",
    connectathon!("Acquisition Modality", "7"),
    "A multi-resolution pyramid: at least two VOLUME layers (distinct Total Pixel Matrix sizes) in the Series.",
);
pub(super) const PYRAMID_LAYERS: ConventionEntry = advisory(
    P,
    "convention-wg26-pyramid-layers",
    connectathon!("Acquisition Modality", "7.a"),
    "\"multiple layers are expected (typically 3 to 5 ...)\": a pyramid of two VOLUME layers.",
);
pub(super) const TILED_FULL: ConventionEntry = check(
    P,
    "convention-wg26-tiled-full",
    connectathon!("Acquisition Modality", "8, 8.a and 8.b"),
    "Dimension Organization Type (0020,9311) is TILED_FULL and the Per-Frame Functional Groups Sequence (5200,9230) is absent.",
);
pub(super) const SINGLE_Z_PLANE: ConventionEntry = check(
    P,
    "convention-wg26-single-z-plane",
    connectathon!("Acquisition Modality", "9"),
    "Only single Z-plane images: no VOLUME or THUMBNAIL image of the Series has a Total Pixel Matrix Focal Planes (0048,0303) Value other than 1, and they share one Total Pixel Matrix Origin Z Offset in Slide Coordinate System (0040,074A). Its absence is reported by convention-wg26-sp2025-focal-planes.",
);
pub(super) const NO_CONCATENATION: ConventionEntry = check(
    P,
    "convention-wg26-no-concatenation",
    connectathon!("Acquisition Modality", "10 and 10.a"),
    "Concatenations are not used and each pyramid layer is entirely in one SOP Instance: no image of the Series is a Concatenation member, and no two VOLUME images have the same Total Pixel Matrix size.",
);
pub(super) const ICC_SINGLE_OPTICAL_PATH: ConventionEntry = check(
    P,
    "convention-wg26-icc-single-optical-path",
    connectathon!("Acquisition Modality", "11"),
    "The Optical Path Sequence (0048,0105) has a single Item, and it holds a non-empty ICC Profile (0028,2000).",
);
pub(super) const SPECIMEN_PREPARATION: ConventionEntry = check(
    P,
    "convention-wg26-specimen-preparation",
    connectathon!("Acquisition Modality", "13.a"),
    "Each Specimen Description Item's Specimen Preparation Sequence has coded (Value Type CODE) entries for the fixation (430864009, SCT, \"Tissue Fixative\"), the embedding (430863003, SCT, \"Embedding medium\") and, in a step of Processing type (127790008, SCT, \"Staining\"), the stain (424361007, SCT, \"Using substance\"), as IHE PaLM DPIA 1.4 Appendix B describes.",
);
pub(super) const SPECIMEN_TYPE_SCT: ConventionEntry = check(
    P,
    "convention-wg26-specimen-type-sct",
    connectathon!("Acquisition Modality", "13.a and Acquisition Manager requirement 5.c"),
    "Specimen Type Code Sequence (0040,059A) Items use SNOMED CT (Coding Scheme Designator SCT), e.g. (119376003, SCT, \"Tissue specimen\"), not HL7 Table 0487.",
);
pub(super) const ANNOTATION_IDENTITY: ConventionEntry = check(
    P,
    "convention-wg26-annotation-identity",
    connectathon!("Evidence Creator", "3"),
    "An annotation object has the Patient's Name and Patient ID of the images it references, and is in a different Series.",
);
pub(super) const ANNOTATION_STUDY: ConventionEntry = advisory(
    P,
    "convention-wg26-annotation-study",
    connectathon!("Evidence Creator", "3"),
    "Annotations \"should also be in the same Study\" as the images they annotate: an annotation object in another Study.",
);
pub(super) const ANNOTATION_MANUFACTURER: ConventionEntry = advisory(
    P,
    "convention-wg26-annotation-manufacturer",
    connectathon!("Evidence Creator", "3"),
    "\"the Manufacturer and ManufacturerModelName of the annotations should allow for this determination\": an annotation object with the Manufacturer and Manufacturer's Model Name of the scanner.",
);

pub(super) const ENTRIES: &[&ConventionEntry] = &[
    &SP_DIMENSION_ORGANIZATION,
    &SP_FOCAL_PLANES,
    &SP_ACQUISITION_UID,
    &SP_PYRAMID_UID,
    &SP_CONCATENATION,
    &SP_SPECIMEN_TYPE,
    &SP_SPECIMEN_PREPARATION,
    &SP_PRIMARY_ANATOMIC_STRUCTURE,
    &SP_ANNOTATION_COORDINATE_TYPE,
    &ONE_SERIES_PER_SCAN,
    &LABEL_IMAGE,
    &BARCODE_VALUE,
    &TRUE_COLOR_BRIGHTFIELD,
    &TRANSFER_SYNTAX,
    &LOSSY_LAYERS,
    &PYRAMID,
    &PYRAMID_LAYERS,
    &TILED_FULL,
    &SINGLE_Z_PLANE,
    &NO_CONCATENATION,
    &ICC_SINGLE_OPTICAL_PATH,
    &SPECIMEN_PREPARATION,
    &SPECIMEN_TYPE_SCT,
    &ANNOTATION_IDENTITY,
    &ANNOTATION_STUDY,
    &ANNOTATION_MANUFACTURER,
];

/// Transfer Syntaxes of uncompressed Pixel Data (PS3.5 A.1, A.2, A.4.11).
pub(super) const UNCOMPRESSED: &[&str] = &[
    uids::IMPLICIT_VR_LITTLE_ENDIAN,
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
];
const COMPRESSED: &[&str] = &[
    uids::JPEG_BASELINE8_BIT,
    uids::JPEG2000_LOSSLESS,
    uids::JPEG2000,
];
const JPEG_XL: &[&str] = &[
    uids::JPEGXL_LOSSLESS,
    uids::JPEGXLJPEG_RECOMPRESSION,
    uids::JPEGXL,
];

pub(super) fn file_checks(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    let root: &InMemDicomObject = object;
    let file = Subject::File(&facts.path);
    match facts.kind {
        Kind::Annotation => {
            out.check(
                &SP_ANNOTATION_COORDINATE_TYPE,
                file,
                annotation_coordinate_type(root),
            );
            return;
        }
        Kind::Wsi => {}
        _ => return,
    }
    out.check(
        &SP_DIMENSION_ORGANIZATION,
        file,
        dimension_organization(root),
    );
    out.check(&SP_FOCAL_PLANES, file, focal_planes(root));
    if facts.acquisition_uid.is_none() {
        out.advise(
            &SP_ACQUISITION_UID,
            file,
            "AcquisitionUID missing: the image has no Acquisition UID (0008,0017)".into(),
        );
    }
    if facts.is_layer() && facts.pyramid_uid.is_none() {
        out.advise(
            &SP_PYRAMID_UID,
            file,
            format!(
                "PyramidUID missing for Volume or Thumbnail ImageType: the {} image has no Pyramid UID (0008,0019)",
                facts.flavor.as_deref().unwrap_or_default()
            ),
        );
    }
    out.check(
        &SP_CONCATENATION,
        file,
        match &facts.concatenation_uid {
            None => Ok(()),
            Some(uid) => Err(format!(
                "Concatenation UID (0020,9161) is present ({uid}), but the profile's Condition for it is Never"
            )),
        },
    );
    out.check(&SP_SPECIMEN_TYPE, file, specimen_type(root));
    out.check(
        &SP_SPECIMEN_PREPARATION,
        file,
        specimen_preparation_present(root),
    );
    out.check(
        &SP_PRIMARY_ANATOMIC_STRUCTURE,
        file,
        primary_anatomic_structure(root),
    );
    out.check(
        &TRUE_COLOR_BRIGHTFIELD,
        file,
        true_color_brightfield(root, facts),
    );
    out.check(&TILED_FULL, file, tiled_full(root));
    out.check(
        &ICC_SINGLE_OPTICAL_PATH,
        file,
        icc_single_optical_path(root),
    );
    out.check(&SPECIMEN_PREPARATION, file, coded_preparation(root));
    match specimen_type_sct(root) {
        Some(result) => out.check(&SPECIMEN_TYPE_SCT, file, result),
        None => out.skip(
            &SPECIMEN_TYPE_SCT,
            file,
            "no Specimen Description Item has a Specimen Type Code Sequence Item; convention-wg26-sp2025-specimen-type reports its absence".into(),
        ),
    }
}

pub(super) fn set_checks(index: &SetIndex<'_>, facts: &[Facts], out: &mut Out) {
    for (position, series) in index.series.iter().enumerate() {
        let (uid, members) = (series.uid, &series.members);
        let subject = Subject::Series(uid);
        out.check(
            &ONE_SERIES_PER_SCAN,
            subject,
            one_series_per_scan(index, position),
        );
        let label_images: Vec<&&Facts> = members.iter().filter(|fact| fact.shows_label()).collect();
        out.check(
            &LABEL_IMAGE,
            subject,
            if label_images.is_empty() {
                Err("the Series has no LABEL or OVERVIEW image (Image Type Value 3)".into())
            } else {
                Ok(())
            },
        );
        if label_images.is_empty() {
            out.skip(
                &BARCODE_VALUE,
                subject,
                "the Series has no LABEL or OVERVIEW image to hold the Barcode Value; convention-wg26-label-image reports it".into(),
            );
        } else {
            out.check(
                &BARCODE_VALUE,
                subject,
                if label_images.iter().any(|fact| fact.barcode_value.is_some()) {
                    Ok(())
                } else {
                    Err(format!(
                        "none of the {} LABEL or OVERVIEW image(s) has a non-empty Barcode Value (2200,0005)",
                        label_images.len()
                    ))
                },
            );
        }
        transfer_syntaxes(members, out);
        pyramid(uid, members, out);
        out.check(&SINGLE_Z_PLANE, subject, single_z_plane(members));
        out.check(&NO_CONCATENATION, subject, no_concatenation(members));
    }
    for annotation in facts.iter().filter(|fact| fact.kind == Kind::Annotation) {
        annotation_identity(annotation, index, out);
    }
}

fn annotation_coordinate_type(root: &InMemDicomObject) -> Result<(), String> {
    match text(root, tags::ANNOTATION_COORDINATE_TYPE).as_deref() {
        Some("2D") => Ok(()),
        Some(other) => Err(format!(
            "Annotation Coordinate Type (006A,0001) is {other}, but only 2D coordinates are permitted"
        )),
        None => Err("Annotation Coordinate Type (006A,0001) is absent or empty, but must be 2D".into()),
    }
}

fn dimension_organization(root: &InMemDicomObject) -> Result<(), String> {
    match text(root, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() {
        None | Some("TILED_FULL") => Ok(()),
        Some(other) => Err(format!(
            "Dimension Organization Type (0020,9311) is {other}, but only TILED_FULL is permitted"
        )),
    }
}

fn focal_planes(root: &InMemDicomObject) -> Result<(), String> {
    match integer_value(root, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES) {
        Some(1) => Ok(()),
        Some(other) => Err(format!(
            "Total Pixel Matrix Focal Planes (0048,0303) is {other}, but must be 1"
        )),
        None => Err("Total Pixel Matrix Focal Planes (0048,0303) is absent or empty, but is Type 1 with Value 1".into()),
    }
}

fn specimen_type(root: &InMemDicomObject) -> Result<(), String> {
    let specimens = items(root, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
    if specimens.is_empty() {
        return Err("Specimen Description Sequence (0040,0560) has no Item, but is Type 1".into());
    }
    let mut found = Violations::default();
    for (index, specimen) in specimens.iter().enumerate() {
        let count = items(specimen, tags::SPECIMEN_TYPE_CODE_SEQUENCE).len();
        if count != 1 {
            found.push(|| format!(
                "Specimen Description Sequence item {} has {count} Specimen Type Code Sequence (0040,059A) Items, but must have exactly one",
                index + 1
            ));
        }
    }
    found.result()
}

fn specimen_preparation_present(root: &InMemDicomObject) -> Result<(), String> {
    let mut found = Violations::default();
    for specimen in specimens(root) {
        if specimen.steps.is_empty() {
            found.push(|| {
                format!(
                    "{} has no Specimen Preparation Sequence (0040,0610) Item, but it is Type 1",
                    specimen.describe()
                )
            });
        }
        for step in specimen.steps.iter().filter(|step| step.content.is_empty()) {
            found.push(|| format!(
                "{} {} has no Specimen Preparation Step Content Item Sequence (0040,0612) Item, but it is Type 1",
                specimen.describe(),
                step.describe()
            ));
        }
    }
    found.result()
}

fn primary_anatomic_structure(root: &InMemDicomObject) -> Result<(), String> {
    let mut found = Violations::default();
    for specimen in specimens(root).iter().filter(|specimen| {
        items(specimen.item, tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE).is_empty()
    }) {
        found.push(|| format!(
            "{} has no Primary Anatomic Structure Sequence (0008,2228) Item, but the profile makes it Type 1",
            specimen.describe()
        ));
    }
    found.result()
}

/// Brightfield is (111744, DCM) and transmitted light (111741, DCM), PS3.16
/// CID 8123.
fn brightfield(path: &InMemDicomObject) -> bool {
    items(path, tags::ILLUMINATION_TYPE_CODE_SEQUENCE)
        .iter()
        .any(|code| is_code(code, "DCM", "111744") || is_code(code, "DCM", "111741"))
}

fn true_color_brightfield(root: &InMemDicomObject, facts: &Facts) -> Result<(), String> {
    let mut found = Violations::default();
    let samples = integer_value(root, tags::SAMPLES_PER_PIXEL);
    if samples != Some(3) {
        found.push(|| {
            format!(
                "Samples per Pixel (0028,0002) is {}, but a true color image has 3",
                samples.map_or("absent".to_string(), |value| value.to_string())
            )
        });
    }
    // A label or overview photograph may be taken in reflected light.
    if facts.is_layer() {
        for (index, path) in items(root, tags::OPTICAL_PATH_SEQUENCE).iter().enumerate() {
            if !brightfield(path) {
                let illumination = items(path, tags::ILLUMINATION_TYPE_CODE_SEQUENCE)
                    .iter()
                    .map(describe_code)
                    .collect::<Vec<_>>();
                found.push(|| format!(
                    "Optical Path Sequence item {} has Illumination Type {}, not brightfield (111744, DCM) or transmission (111741, DCM) illumination",
                    index + 1,
                    if illumination.is_empty() {
                        "(none)".to_string()
                    } else {
                        illumination.join(", ")
                    }
                ));
            }
        }
    }
    found.result()
}

fn tiled_full(root: &InMemDicomObject) -> Result<(), String> {
    let mut found = Violations::default();
    match text(root, tags::DIMENSION_ORGANIZATION_TYPE).as_deref() {
        Some("TILED_FULL") => {}
        Some(other) => found.push(|| format!(
            "Dimension Organization Type (0020,9311) is {other}, but only TILED_FULL images may be sent"
        )),
        None => found.push(|| {
            "Dimension Organization Type (0020,9311) is absent, so the image is not TILED_FULL".into()
        }),
    }
    if root
        .get(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .is_some()
    {
        found.push(|| format!(
            "the Per-Frame Functional Groups Sequence (5200,9230) is present ({} Items), but shall not be",
            items(root, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE).len()
        ));
    }
    found.result()
}

pub(super) fn has_bytes(item: &InMemDicomObject, tag: dicom_core::Tag) -> bool {
    item.get(tag).is_some_and(|element| {
        element
            .value()
            .primitive()
            .is_some_and(|value| value.calculate_byte_len() > 0)
    })
}

fn icc_single_optical_path(root: &InMemDicomObject) -> Result<(), String> {
    let paths = items(root, tags::OPTICAL_PATH_SEQUENCE);
    match paths {
        [path] if has_bytes(path, tags::ICC_PROFILE) => Ok(()),
        [_] => Err("the single Optical Path Sequence (0048,0105) Item has no ICC Profile (0028,2000)".into()),
        _ => Err(format!(
            "the Optical Path Sequence (0048,0105) has {} Items, but an ICC Profile is expected in a single Item",
            paths.len()
        )),
    }
}

fn coded_preparation(root: &InMemDicomObject) -> Result<(), String> {
    let mut found = Violations::default();
    for specimen in specimens(root) {
        if specimen.steps.is_empty() {
            found.push(|| {
                format!(
                    "{} has an empty Specimen Preparation Sequence, but it shall not be empty",
                    specimen.describe()
                )
            });
            continue;
        }
        for (concept, what) in [
            (
                TISSUE_FIXATIVE,
                "fixation (430864009, SCT, \"Tissue Fixative\")",
            ),
            (
                EMBEDDING_MEDIUM,
                "embedding (430863003, SCT, \"Embedding medium\")",
            ),
        ] {
            if specimen.steps_coding(concept).is_empty() {
                found.push(|| format!("{} has no coded {what} Content Item", specimen.describe()));
            }
        }
        let stained = specimen
            .steps_of(STAINING)
            .iter()
            .any(|step| step.coded(USING_SUBSTANCE).is_ok());
        if !stained {
            found.push(|| format!(
                "{} has no staining step (Processing type (127790008, SCT, \"Staining\")) with a coded (424361007, SCT, \"Using substance\") Content Item",
                specimen.describe()
            ));
        }
    }
    found.result()
}

/// `None` when no Specimen Type Code Sequence Item is present.
fn specimen_type_sct(root: &InMemDicomObject) -> Option<Result<(), String>> {
    let mut seen = false;
    let mut found = Violations::default();
    for specimen in specimens(root) {
        for code in items(specimen.item, tags::SPECIMEN_TYPE_CODE_SEQUENCE) {
            seen = true;
            if text(code, tags::CODING_SCHEME_DESIGNATOR).as_deref() != Some("SCT") {
                found.push(|| format!(
                    "{} has Specimen Type {}, but SNOMED CT (SCT) codes such as (119376003, SCT, \"Tissue specimen\") shall be used",
                    specimen.describe(),
                    describe_code(code)
                ));
            }
        }
    }
    seen.then(|| found.result())
}

fn distinct<'a>(
    members: &[&'a Facts],
    value: impl Fn(&'a Facts) -> Option<&'a str>,
) -> BTreeSet<&'a str> {
    members.iter().filter_map(|fact| value(fact)).collect()
}

fn one_series_per_scan(index: &SetIndex<'_>, position: usize) -> Result<(), String> {
    let series = &index.series[position];
    let mut found = Violations::default();
    let mut mixed = |what: &str, values: &BTreeSet<&str>| {
        if values.len() > 1 {
            found.push(|| format!(
                "the Series mixes {} values of {what} ({}), so it holds more than one slide or scan",
                values.len(),
                values.iter().copied().collect::<Vec<_>>().join(", ")
            ));
        }
    };
    mixed(
        "Container Identifier (0040,0512)",
        &distinct(&series.members, |fact| fact.container_identifier.as_deref()),
    );
    mixed(
        "Barcode Value (2200,0005)",
        &distinct(&series.members, |fact| fact.barcode_value.as_deref()),
    );
    mixed("Acquisition UID (0008,0017)", &series.acquisitions);
    for (other, shared) in index.shared_acquisitions(position) {
        found.push(|| {
            format!(
            "Acquisition UID {} also occurs in Series {other}, so one scan is split over Series",
            shared.join(", ")
        )
        });
    }
    found.result()
}

/// The top layer: the lowest-resolution VOLUME or THUMBNAIL image.
fn apex<'a>(members: &[&'a Facts]) -> Option<&'a Facts> {
    members
        .iter()
        .copied()
        .filter(|fact| fact.is_layer())
        .filter(|fact| fact.area().is_some())
        .min_by_key(|fact| fact.area())
}

fn transfer_syntaxes(members: &[&Facts], out: &mut Out) {
    let top = apex(members);
    for fact in members {
        let file = Subject::File(&fact.path);
        let syntax = fact.transfer_syntax.as_str();
        let may_be_uncompressed =
            !fact.is_volume() || top.is_some_and(|top| top.area() == fact.area());
        let permitted =
            COMPRESSED.contains(&syntax) || (may_be_uncompressed && UNCOMPRESSED.contains(&syntax));
        let result = if permitted {
            Ok(())
        } else {
            let role = if may_be_uncompressed {
                "baseline JPEG, JPEG 2000 or uncompressed"
            } else {
                "baseline JPEG or JPEG 2000 (only the top layer, LABEL and OVERVIEW images may be uncompressed)"
            };
            let experiment = if JPEG_XL.contains(&syntax) {
                "; JPEG XL may be used only by prior arrangement between partners (requirement 6.c)"
            } else {
                ""
            };
            Err(format!(
                "the {} image is encoded in Transfer Syntax {syntax}, but must be {role}{experiment}",
                fact.flavor.as_deref().unwrap_or("WSI")
            ))
        };
        out.check(&TRANSFER_SYNTAX, file, result);
        if fact.is_volume() && !may_be_uncompressed && syntax == uids::JPEG2000_LOSSLESS {
            out.advise(
                &LOSSY_LAYERS,
                file,
                "a VOLUME layer below the top is JPEG 2000 lossless only (1.2.840.10008.1.2.4.90), where lossy compression is expected".into(),
            );
        }
    }
}

fn pyramid(uid: &str, members: &[&Facts], out: &mut Out) {
    let layers = volume_layers(members);
    let subject = Subject::Series(uid);
    let describe = || {
        layers
            .iter()
            .map(|(rows, columns)| format!("{columns}x{rows}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.check(
        &PYRAMID,
        subject,
        if layers.len() >= 2 {
            Ok(())
        } else {
            Err(format!(
                "the Series has {} VOLUME layer(s) ({}), but a multi-resolution pyramid is required",
                layers.len(),
                describe()
            ))
        },
    );
    if layers.len() == 2 {
        out.advise(
            &PYRAMID_LAYERS,
            subject,
            format!("the pyramid has two VOLUME layers ({})", describe()),
        );
    }
}

fn single_z_plane(members: &[&Facts]) -> Result<(), String> {
    let layers: Vec<&&Facts> = members.iter().filter(|fact| fact.is_layer()).collect();
    let mut found = Violations::default();
    for fact in &layers {
        if let Some(planes) = fact.focal_planes.filter(|&planes| planes != 1) {
            found.push(|| {
                format!(
                    "{} has Total Pixel Matrix Focal Planes {planes}",
                    fact.path.display()
                )
            });
        }
    }
    let offsets: BTreeSet<String> = layers
        .iter()
        .map(|fact| match fact.origin.and_then(|origin| origin.z) {
            Some(z) => format!("{z}"),
            None => "absent".into(),
        })
        .collect();
    if offsets.len() > 1 {
        found.push(|| format!(
            "the VOLUME and THUMBNAIL images have different Total Pixel Matrix Origin Z Offsets ({}), so the Series holds more than one Z-plane",
            offsets.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    found.result()
}

fn no_concatenation(members: &[&Facts]) -> Result<(), String> {
    let mut found = Violations::default();
    for fact in members
        .iter()
        .filter(|fact| fact.concatenation_uid.is_some() || fact.concatenation_member)
    {
        found.push(|| format!("{} is a Concatenation member", fact.path.display()));
    }
    // The first VOLUME image of each layer.
    let mut layers = std::collections::BTreeMap::<(u64, u64), &std::path::Path>::new();
    for fact in members.iter().filter(|fact| fact.is_volume()) {
        let Some(matrix) = fact.matrix else {
            continue;
        };
        match layers.get(&matrix) {
            Some(first) => found.push(|| {
                format!(
                    "the {}x{} layer is split over {} and {}",
                    matrix.1,
                    matrix.0,
                    first.display(),
                    fact.path.display()
                )
            }),
            None => {
                layers.insert(matrix, &fact.path);
            }
        }
    }
    found.result()
}

fn annotation_identity(annotation: &Facts, index: &SetIndex<'_>, out: &mut Out) {
    let file = Subject::File(&annotation.path);
    let images = index.referenced(&annotation.references);
    let Some(image) = images.first() else {
        out.skip(
            &ANNOTATION_IDENTITY,
            file,
            format!(
                "none of the {} image(s) the annotation references is in the validated set, so its identity cannot be compared",
                annotation.references.len()
            ),
        );
        return;
    };
    let mut found = Violations::default();
    for image in &images {
        for (what, own, theirs) in [
            (
                "Patient's Name",
                &annotation.patient.name,
                &image.patient.name,
            ),
            ("Patient ID", &annotation.patient.id, &image.patient.id),
        ] {
            if own != theirs {
                found.push(|| {
                    format!(
                        "{what} is {own:?}, but the referenced image {} has {theirs:?}",
                        image.path.display()
                    )
                });
            }
        }
        if annotation.series_uid.is_some() && annotation.series_uid == image.series_uid {
            found.push(|| {
                format!(
                    "the annotation is in the Series of the image {} it annotates",
                    image.path.display()
                )
            });
        }
    }
    out.check(&ANNOTATION_IDENTITY, file, found.result());
    if annotation.study_uid != image.study_uid {
        out.advise(
            &ANNOTATION_STUDY,
            file,
            format!(
                "the annotation is in Study {:?}, the image it annotates in Study {:?}",
                annotation.study_uid, image.study_uid
            ),
        );
    }
    if annotation.manufacturer.is_some()
        && (&annotation.manufacturer, &annotation.model) == (&image.manufacturer, &image.model)
    {
        out.advise(
            &ANNOTATION_MANUFACTURER,
            file,
            format!(
                "the annotation's Manufacturer {:?} and Manufacturer's Model Name {:?} are those of the scanner, so they do not identify the Evidence Creator",
                annotation.manufacturer, annotation.model
            ),
        );
    }
}
