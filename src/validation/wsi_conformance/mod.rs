mod copies;
mod corpus;
mod dimensions;
mod fields;
mod functional_groups;
mod geometry;
mod identity;
mod instance;
mod lossy_encoding;
mod nested;
mod pixel_attributes;
mod pixel_syntax;
mod relations;
mod series_set;
#[cfg(test)]
mod tests;
mod tiled_full;
mod wsi_image;

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::DefaultDicomObject;

pub(super) use corpus::{set_checks, Origin, SetResults, WsiConformanceCorpus};

use geometry::{
    validate_dimension_rule, validate_slide_coordinate_system_rule, validate_tile_geometry_rule,
};
use identity::{
    specimen_identities, validate_file_meta_identity, validate_specimen_container_rule,
};
use instance::{
    validate_core_image_profile_rule, validate_icc_rule, validate_image_metadata_rule,
    validate_lossy_rule, validate_monochrome_rule, validate_optical_path_structure_rule,
    video_syntax_advisory,
};
use nested::Nested;
use pixel_attributes::validate_pixel_attributes_rule;

use super::codestream::run_codestream_check;
use super::iod::{declares_vl_wsi, run_iod_checks, run_iod_checks_on, OptionalNotes, Unavailable};
use super::{intrinsic_check, ValidationCheck, ValidationProfile, ValidationStatus};
use crate::validation::iod::dicom_edition;

const ICC_RULE: &str = "intrinsic-wsi-dicom-icc-profile";
const MONOCHROME_RULE: &str = "intrinsic-wsi-dicom-monochrome-presentation";
const LOSSY_RULE: &str = "intrinsic-wsi-dicom-lossy-history";
const SPECIMEN_RULE: &str = "intrinsic-wsi-dicom-specimen-identity";
const DIMENSION_RULE: &str = "intrinsic-wsi-dicom-dimension-order";
const TILE_GEOMETRY_RULE: &str = "intrinsic-wsi-dicom-tile-geometry";
const IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-identity-set";
const IMAGE_METADATA_RULE: &str = "intrinsic-wsi-dicom-image-metadata";
const PIXEL_ATTRIBUTES_RULE: &str = "intrinsic-wsi-dicom-pixel-attributes";
const CORE_IMAGE_PROFILE_RULE: &str = "intrinsic-wsi-dicom-core-image-profile";
const SLIDE_COORDINATE_RULE: &str = "intrinsic-wsi-dicom-slide-coordinate-system";
const OPTICAL_PATH_STRUCTURE_RULE: &str = "intrinsic-wsi-dicom-optical-path-structure";
const SPECIMEN_CONTAINER_RULE: &str = "intrinsic-wsi-dicom-specimen-container";
const WSI_IMAGE_RULE: &str = "intrinsic-wsi-dicom-wsi-image";
const RELATIONS_RULE: &str = "intrinsic-wsi-dicom-attribute-relations";
const CODESTREAM_RULE: &str = "intrinsic-wsi-dicom-codestream";
const SERIES_SET_RULE: &str = series_set::SERIES_SET_RULE;
const SPECIMEN_SET_RULE: &str = corpus::SPECIMEN_SET_RULE;

/// The pathology-slide IODs, each named by the PS3.3 section defining it.
pub(in crate::validation) const VL_WSI: &str = "A.32.8";
const BULK_ANNOTATIONS: &str = "A.87";
const SEGMENTATION: &str = "A.51";
const PARAMETRIC_MAP: &str = "A.75";
/// The Key Object Selection Document and SR document IODs (PS3.3 A.35),
/// whose documents select or describe the pathology-slide objects. They hold
/// no Pixel Data, so no image or pixel rule runs on them.
const KEY_OBJECT_SELECTION: &str = "A.35.4";
const BASIC_TEXT_SR: &str = "A.35.1";
const ENHANCED_SR: &str = "A.35.2";
const COMPREHENSIVE_SR: &str = "A.35.3";
const COMPREHENSIVE_3D_SR: &str = "A.35.13";

/// Every pathology-slide IOD.
const EVERY_IOD: &[&str] = &[VL_WSI, BULK_ANNOTATIONS, SEGMENTATION, PARAMETRIC_MAP];
/// Every pathology-slide IOD and document IOD: the objects a SOP Instance
/// and its Series identify.
const EVERY_OBJECT: &[&str] = &[
    VL_WSI,
    BULK_ANNOTATIONS,
    SEGMENTATION,
    PARAMETRIC_MAP,
    KEY_OBJECT_SELECTION,
    BASIC_TEXT_SR,
    ENHANCED_SR,
    COMPREHENSIVE_SR,
    COMPREHENSIVE_3D_SR,
];
/// The IODs with Pixel Data: every one but Microscopy Bulk Simple
/// Annotations (Table A.87-1).
const IMAGE_IODS: &[&str] = &[VL_WSI, SEGMENTATION, PARAMETRIC_MAP];
/// The IODs that include the Multi-frame Dimension Module (C.7.6.17).
const DIMENSION_IODS: &[&str] = &[VL_WSI, SEGMENTATION, PARAMETRIC_MAP];
const VL_WSI_ONLY: &[&str] = &[VL_WSI];
/// The scope of a File-set's DICOMDIR, a Basic Directory IOD instance
/// (PS3.3 F.3), beside the pathology-slide IODs.
const FILE_SET: &str = "file-set";
/// The Media Storage Directory Storage SOP Class of a DICOMDIR.
const DICOMDIR_SOP_CLASS: &str = "1.2.840.10008.1.3.10";
/// Every IOD, with the DICOMDIR of a File-set.
const EVERY_FILE: &[&str] = &[
    VL_WSI,
    BULK_ANNOTATIONS,
    SEGMENTATION,
    PARAMETRIC_MAP,
    KEY_OBJECT_SELECTION,
    BASIC_TEXT_SR,
    ENHANCED_SR,
    COMPREHENSIVE_SR,
    COMPREHENSIVE_3D_SR,
    FILE_SET,
];
/// The files the per-file and IOD checks hold.
const IOD_CHECKED_FILES: &[&str] = EVERY_FILE;
/// The IODs that include the Synchronization Module (C.7.4.2).
const SYNCHRONIZATION_IODS: &[&str] = &[
    PARAMETRIC_MAP,
    ENHANCED_SR,
    COMPREHENSIVE_SR,
    COMPREHENSIVE_3D_SR,
];
/// The IODs with Pixel Data, with a DICOMDIR, whose Directory Records may
/// hold Icon Images (PS3.3 F.5).
const IMAGES_AND_ICONS: &[&str] = &[VL_WSI, SEGMENTATION, PARAMETRIC_MAP, FILE_SET];

/// The SOP Classes of each IOD (PS3.4 Table B.5-1).
const SOP_CLASSES: [(&str, &str); 10] = [
    ("1.2.840.10008.5.1.4.1.1.77.1.6", VL_WSI),
    ("1.2.840.10008.5.1.4.1.1.91.1", BULK_ANNOTATIONS),
    ("1.2.840.10008.5.1.4.1.1.66.4", SEGMENTATION),
    ("1.2.840.10008.5.1.4.1.1.66.7", SEGMENTATION),
    ("1.2.840.10008.5.1.4.1.1.30", PARAMETRIC_MAP),
    ("1.2.840.10008.5.1.4.1.1.88.59", KEY_OBJECT_SELECTION),
    ("1.2.840.10008.5.1.4.1.1.88.11", BASIC_TEXT_SR),
    ("1.2.840.10008.5.1.4.1.1.88.22", ENHANCED_SR),
    ("1.2.840.10008.5.1.4.1.1.88.33", COMPREHENSIVE_SR),
    ("1.2.840.10008.5.1.4.1.1.88.34", COMPREHENSIVE_3D_SR),
];

/// The pathology-slide IOD an object is held to: the one its Data Set's SOP
/// Class UID names, or else its File Meta Information's, as for the IOD
/// checks.
pub(in crate::validation) fn iod_of(object: &DefaultDicomObject) -> Option<&'static str> {
    let declared = object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .map(|uid| uid.trim_end_matches(['\0', ' ']).to_string());
    let meta = object
        .meta()
        .media_storage_sop_class_uid()
        .trim_end_matches(['\0', ' '])
        .to_string();
    [declared, Some(meta)]
        .into_iter()
        .flatten()
        .find_map(|uid| {
            if uid == DICOMDIR_SOP_CLASS {
                return Some(FILE_SET);
            }
            SOP_CLASSES
                .iter()
                .find(|(class, _)| *class == uid)
                .map(|(_, iod)| *iod)
        })
}

/// A check of one object: its name, the citation its message opens with,
/// the IODs whose objects it holds, and the check of an object of one of
/// them, which is told the object's IOD.
pub(super) struct Rule {
    name: &'static str,
    citation: &'static str,
    iods: &'static [&'static str],
    check: Check,
}

/// How a rule checks an object: alone, or with what one walk of its nested
/// Items found, which the rules holding nested content share.
#[derive(Clone, Copy)]
enum Check {
    Object(fn(&DefaultDicomObject, &'static str) -> Result<(), String>),
    Nested(fn(&DefaultDicomObject, &'static str, &Nested<'_>) -> Result<(), String>),
}

const fn rule(
    name: &'static str,
    citation: &'static str,
    iods: &'static [&'static str],
    check: fn(&DefaultDicomObject, &'static str) -> Result<(), String>,
) -> Rule {
    Rule {
        name,
        citation,
        iods,
        check: Check::Object(check),
    }
}

/// A rule of the nested Items `Nested` finds.
const fn nested_rule(
    name: &'static str,
    citation: &'static str,
    iods: &'static [&'static str],
    check: fn(&DefaultDicomObject, &'static str, &Nested<'_>) -> Result<(), String>,
) -> Rule {
    Rule {
        name,
        citation,
        iods,
        check: Check::Nested(check),
    }
}

/// The checks of one object in every profile, in report order. The
/// codestream check follows them; it reads the Frames, with a limit.
const RULES: &[Rule] = &[
    nested_rule(
        ICC_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.11.15 and C.8.12.5"),
        EVERY_FILE,
        validate_icc_rule,
    ),
    rule(
        MONOCHROME_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.4"),
        VL_WSI_ONLY,
        |object, _| validate_monochrome_rule(object),
    ),
    rule(
        LOSSY_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.4 and C.7.6"),
        IMAGE_IODS,
        validate_lossy_rule,
    ),
    rule(
        SPECIMEN_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.22 and 10.14"),
        VL_WSI_ONLY,
        |object, _| specimen_identities(object).map(|_| ()),
    ),
    rule(
        DIMENSION_RULE,
        concat!(
            "DICOM PS3.3 ",
            dicom_edition!(),
            " C.7.6.17.1 and C.8.12.6.1"
        ),
        DIMENSION_IODS,
        |object, _| validate_dimension_rule(object),
    ),
    rule(
        TILE_GEOMETRY_RULE,
        concat!(
            "DICOM PS3.3 ",
            dicom_edition!(),
            " C.7.6.16.2.1 and C.8.12.6.1"
        ),
        VL_WSI_ONLY,
        |object, _| validate_tile_geometry_rule(object),
    ),
    rule(
        IDENTITY_SET_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.12.1"),
        EVERY_OBJECT,
        |object, _| validate_file_meta_identity(object),
    ),
    rule(
        IMAGE_METADATA_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.3"),
        IMAGE_IODS,
        validate_image_metadata_rule,
    ),
    rule(
        WSI_IMAGE_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12"),
        EVERY_IOD,
        wsi_image::validate_wsi_image_rule,
    ),
    rule(
        RELATIONS_RULE,
        concat!(
            "DICOM PS3.3 ",
            dicom_edition!(),
            " 8.6, 10.17, C.7, C.8.12.5 and C.12"
        ),
        EVERY_FILE,
        |object, _| relations::validate_relations_rule(object),
    ),
    nested_rule(
        PIXEL_ATTRIBUTES_RULE,
        concat!(
            "DICOM PS3.3 ",
            dicom_edition!(),
            " C.7.5.1.1.2, C.7.6.1.1.6, C.7.6.3.1 and C.7.9"
        ),
        IMAGES_AND_ICONS,
        validate_pixel_attributes_rule,
    ),
];

/// The further checks of the core profile, which holds VL WSI objects alone.
const CORE_RULES: &[Rule] = &[
    rule(
        SPECIMEN_CONTAINER_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.22"),
        VL_WSI_ONLY,
        |object, _| validate_specimen_container_rule(object),
    ),
    rule(
        SLIDE_COORDINATE_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.14"),
        VL_WSI_ONLY,
        |object, _| validate_slide_coordinate_system_rule(object),
    ),
    rule(
        OPTICAL_PATH_STRUCTURE_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.5"),
        VL_WSI_ONLY,
        |object, _| validate_optical_path_structure_rule(object),
    ),
    rule(
        CORE_IMAGE_PROFILE_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " A.32.8 and C.8.12.4"),
        VL_WSI_ONLY,
        |object, _| validate_core_image_profile_rule(object),
    ),
];

/// The IODs whose objects the codestream check holds.
const CODESTREAM_IODS: &[&str] = IMAGE_IODS;

/// Checks that hold every file outside this dispatch, a DICOMDIR's
/// included: the per-file checks of `validate_dicom_path` and the IOD
/// checks.
/// The scope of a DICOMweb origin server and of the DICOM JSON it returns
/// (PS3.18), beside the pathology-slide IODs.
const DICOMWEB: &str = "dicomweb";
/// The checks of a DICOMweb origin server (`wsi-dicom dicomweb`) and of the
/// DICOM JSON Model (PS3.18 Annex F), which hold the "dicomweb" scope. A
/// DICOMweb test proves that a run against an origin server emits each.
pub(crate) const DICOMWEB_RULES: [&str; 35] = [
    "dicomweb-capabilities",
    "dicomweb-search-studies",
    "dicomweb-search-series",
    "dicomweb-search-instances",
    "dicomweb-search-paging",
    "dicomweb-search-matching",
    "dicomweb-search-parameters",
    "dicomweb-accept-required",
    "dicomweb-search-retrieve-url",
    "dicomweb-retrieve-metadata",
    "dicomweb-retrieve-instance-metadata",
    "dicomweb-retrieve-metadata-xml",
    "dicomweb-retrieve-bulkdata",
    "dicomweb-retrieve-frames",
    "dicomweb-retrieve-frames-uncompressed",
    "dicomweb-retrieve-frames-missing",
    "dicomweb-retrieve-rendered",
    "dicomweb-retrieve-thumbnail",
    "dicomweb-retrieve-instance",
    "dicomweb-store",
    "dicomweb-store-readback",
    "dicomweb-payload-headers",
    "dicomweb-content-location",
    "intrinsic-wsi-dicom-json-model",
    "intrinsic-wsi-dicom-json-bulkdata",
    "dicomweb-response-messages",
    "dicomweb-error-responses",
    "dicomweb-range-requests",
    "dicomweb-accept-negotiation",
    "dicomweb-character-sets",
    "dicomweb-transfer-syntaxes",
    "dicomweb-search-matching-parameters",
    "dicomweb-search-matching-attributes",
    "dicomweb-search-resources",
    "dicomweb-rendered-parameters",
];

const EVERY_IOD_ELSEWHERE: [&str; 11] = [
    "intrinsic-pixel-structure",
    "intrinsic-wsi-dicom-encoding",
    "intrinsic-wsi-dicom-digital-signatures",
    "intrinsic-wsi-dicom-iod-modules",
    "intrinsic-wsi-dicom-iod-attribute-presence",
    "intrinsic-wsi-dicom-iod-enumerated-values",
    "intrinsic-wsi-dicom-iod-vr-vm",
    "intrinsic-wsi-dicom-iod-value-format",
    "intrinsic-wsi-dicom-iod-sequence-items",
    "intrinsic-wsi-dicom-iod-value-sets",
    "intrinsic-wsi-dicom-file-meta",
];

/// The checks of the IOD families' own semantics, which their modules run
/// beside `RULES` (per object and over the set), with the IODs of each.
const FAMILY_RULES: [(&str, &[&str]); 5] = [
    ("intrinsic-wsi-dicom-bulk-annotations", &[BULK_ANNOTATIONS]),
    ("intrinsic-wsi-dicom-segmentation", &[SEGMENTATION]),
    ("intrinsic-wsi-dicom-parametric-map", &[PARAMETRIC_MAP]),
    (
        "intrinsic-wsi-dicom-structured-report",
        &[
            BASIC_TEXT_SR,
            ENHANCED_SR,
            COMPREHENSIVE_SR,
            COMPREHENSIVE_3D_SR,
        ],
    ),
    (
        "intrinsic-wsi-dicom-key-object-selection",
        &[KEY_OBJECT_SELECTION],
    ),
];

/// The family check holding the objects of `iod` to the semantics of its
/// own Modules, for a statement several families share.
pub(in crate::validation) fn family_rule(iod: &str) -> Option<&'static str> {
    FAMILY_RULES
        .iter()
        .find(|(_, iods)| iods.contains(&iod))
        .map(|(rule, _)| *rule)
}

/// Whether `rule` is the check of an IOD family's own semantics.
pub(in crate::validation) fn is_family_rule(rule: &str) -> bool {
    FAMILY_RULES.iter().any(|(name, _)| *name == rule)
}

/// The set checks whose facts the corpus gathers from every object, the
/// documents' included; the specimen check compares the pathology-slide
/// objects, and the other set checks VL WSI objects alone.
const EVERY_OBJECT_SET_RULES: [&str; 3] = [
    IDENTITY_SET_RULE,
    SERIES_SET_RULE,
    // Per object (object_checks) and over the set.
    "intrinsic-wsi-dicom-deidentification",
];

/// The IODs whose objects the intrinsic check `rule` holds: what the
/// requirement catalog reads, so a disposition names a check only where it
/// runs. A check this table does not name holds VL WSI objects alone.
pub(in crate::validation) fn rule_iods(rule: &str) -> &'static [&'static str] {
    if EVERY_IOD_ELSEWHERE.contains(&rule) {
        return IOD_CHECKED_FILES;
    }
    if DICOMWEB_RULES.contains(&rule) {
        return &[DICOMWEB];
    }
    if EVERY_OBJECT_SET_RULES.contains(&rule) {
        return EVERY_OBJECT;
    }
    if rule == SPECIMEN_SET_RULE {
        return EVERY_IOD;
    }
    if rule == CODESTREAM_RULE {
        return CODESTREAM_IODS;
    }
    // validate_dicom_path runs the File-set check once per File-set root.
    if rule == "intrinsic-wsi-dicom-file-set" {
        return &[FILE_SET];
    }
    // The IOD semantics check of the Synchronization Module, per object and
    // over the set.
    if rule == "intrinsic-wsi-dicom-synchronization" {
        return SYNCHRONIZATION_IODS;
    }
    if let Some((_, iods)) = FAMILY_RULES.iter().find(|(name, _)| *name == rule) {
        return iods;
    }
    RULES
        .iter()
        .chain(CORE_RULES)
        .find(|candidate| candidate.name == rule)
        .map_or(VL_WSI_ONLY, |found| found.iods)
}

/// The intrinsic checks of one file, with its optional conformance notes.
pub(super) fn run_intrinsic_wsi_conformance_checks(
    path: &Path,
    object: &DefaultDicomObject,
    profile: ValidationProfile,
    conformance: crate::ConformanceLevel,
    max_pixel_frame_bytes: usize,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let iod_checks = run_iod_checks(path, object, conformance);
    object_checks(
        path,
        object,
        &Unavailable::default(),
        iod_checks,
        profile,
        max_pixel_frame_bytes,
    )
}

/// The intrinsic checks of a parsed object whose results are labelled
/// `label`, a path or a document locator, without what `unavailable` says
/// its source cannot supply: the checks that need it are reported skipped
/// with why. No check reads `label`; the IOD checks read the File Meta
/// Information there unless `unavailable.file_meta` gives a reason.
pub(in crate::validation) fn run_intrinsic_wsi_conformance_checks_on(
    label: &Path,
    object: &DefaultDicomObject,
    unavailable: &Unavailable,
    profile: ValidationProfile,
    conformance: crate::ConformanceLevel,
    max_pixel_frame_bytes: usize,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let iod_checks = run_iod_checks_on(label, object, unavailable, conformance);
    object_checks(
        label,
        object,
        unavailable,
        iod_checks,
        profile,
        max_pixel_frame_bytes,
    )
}

/// The checks of one object around its IOD checks, `iod_checks`.
fn object_checks(
    label: &Path,
    object: &DefaultDicomObject,
    unavailable: &Unavailable,
    (iod_checks, mut notes): (Vec<ValidationCheck>, Option<OptionalNotes>),
    profile: ValidationProfile,
    max_pixel_frame_bytes: usize,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let Some(iod) = iod_of(object) else {
        // An object of another SOP Class: the IOD checks say why no IOD
        // rules apply.
        let mut checks = core_profile_scope(label, object, profile);
        checks.extend(iod_checks);
        return (checks, notes);
    };
    let mut checks = core_profile_scope(label, object, profile);
    let nested = OnceCell::new();
    for rule in RULES.iter().filter(|rule| rule.iods.contains(&iod)) {
        checks.push(run_rule(label, object, iod, rule, unavailable, &nested));
    }
    if let Some(advisory) = video_syntax_advisory(object, iod) {
        advise(&mut checks, IMAGE_METADATA_RULE, &advisory, notes.as_mut());
    }
    if CODESTREAM_IODS.contains(&iod) {
        checks.push(if unavailable.pixel_data {
            skipped(
                label,
                CODESTREAM_RULE,
                &format!(
                    "DICOM PS3.5 {} 8.2 and A.4: needs the encoded Frames of Pixel Data (7FE0,0010), which the source does not hold",
                    crate::validation::iod::EDITION
                ),
            )
        } else {
            run_codestream_check(label, object, max_pixel_frame_bytes)
        });
    }
    let held = (!unavailable.pixel_data).then_some(max_pixel_frame_bytes);
    checks.extend(super::bulk_annotations::run_bulk_annotations_check(
        label, object,
    ));
    checks.extend(super::segmentation::run_segmentation_check(
        label, object, held,
    ));
    checks.extend(super::parametric_map::run_parametric_map_check(
        label, object, held,
    ));
    checks.extend(super::structured_report::run_structured_report_check(
        label, object,
    ));
    checks.extend(super::structured_report::run_key_object_selection_check(
        label, object,
    ));
    checks.extend(super::image_semantics::run_synchronization_check(
        label, object,
    ));
    checks.push(super::deidentification::run_deidentification_check(
        label, object,
    ));
    checks.extend(iod_checks);
    if profile == ValidationProfile::Core {
        for rule in CORE_RULES.iter().filter(|rule| rule.iods.contains(&iod)) {
            checks.push(run_rule(label, object, iod, rule, unavailable, &nested));
        }
    }
    (checks, notes)
}

/// The core profile holds objects whose Data Set declares the VL WSI SOP
/// Class alone; another object fails it.
fn core_profile_scope(
    label: &Path,
    object: &DefaultDicomObject,
    profile: ValidationProfile,
) -> Vec<ValidationCheck> {
    if profile != ValidationProfile::Core || declares_vl_wsi(object) {
        return Vec::new();
    }
    vec![rule_check(
        label,
        CORE_IMAGE_PROFILE_RULE,
        concat!("DICOM PS3.3 ", dicom_edition!(), " A.32.8"),
        Err("core profile requires VL WSI SOP Class UID".into()),
    )]
}

/// One rule's check of an object of `iod`. The identity check compares the
/// File Meta Information with the Data Set, so it is skipped without it.
/// The check of `rule`; `nested` is the one walk of the object's nested
/// Items, taken when a rule first needs it.
fn run_rule<'a>(
    label: &Path,
    object: &'a DefaultDicomObject,
    iod: &'static str,
    rule: &Rule,
    unavailable: &Unavailable,
    nested: &OnceCell<Nested<'a>>,
) -> ValidationCheck {
    match (&unavailable.file_meta, rule.name) {
        (Some(reason), IDENTITY_SET_RULE) => skipped(
            label,
            rule.name,
            &format!(
                "{}: compares the File Meta Information with the Data Set: {reason}",
                rule.citation
            ),
        ),
        _ => {
            let result = match rule.check {
                Check::Object(check) => check(object, iod),
                Check::Nested(check) => {
                    check(object, iod, nested.get_or_init(|| Nested::of(object)))
                }
            };
            rule_check(label, rule.name, rule.citation, result)
        }
    }
}

/// Adds `advisory` to the message of check `name`, which stays as it is
/// otherwise, and to the object's optional conformance where it is reported.
fn advise(
    checks: &mut [ValidationCheck],
    name: &str,
    advisory: &str,
    notes: Option<&mut OptionalNotes>,
) {
    if let Some(check) = checks.iter_mut().find(|check| check.name == name) {
        check.message.push_str(&format!("; advisory: {advisory}"));
    }
    if let Some(notes) = notes {
        notes.advisories.push(advisory.to_string());
    }
}

fn skipped(label: &Path, name: &str, message: &str) -> ValidationCheck {
    intrinsic_check(
        name,
        rule_path(label),
        ValidationStatus::Skipped,
        message.to_string(),
    )
}

fn rule_check(
    path: &Path,
    name: &str,
    citation: &str,
    result: Result<(), String>,
) -> ValidationCheck {
    let (status, message) = match result {
        Ok(()) => (ValidationStatus::Passed, format!("{citation}: conformant")),
        Err(reason) => (ValidationStatus::Failed, format!("{citation}: {reason}")),
    };
    intrinsic_check(name, rule_path(path), status, message)
}

/// Violations shown in a rule's message; the rest are counted.
const MAX_SHOWN: usize = 20;

/// A rule's violations as its result: the first `MAX_SHOWN`, then how many more.
pub(super) fn violation_result(violations: Vec<String>) -> Result<(), String> {
    if violations.is_empty() {
        return Ok(());
    }
    let hidden = violations.len().saturating_sub(MAX_SHOWN);
    let mut message = violations[..violations.len() - hidden].join("; ");
    if hidden > 0 {
        message.push_str(&format!("; and {hidden} more"));
    }
    Err(message)
}

/// The checked file; an empty path marks a check of the whole set.
pub(super) fn rule_path(path: &Path) -> Option<PathBuf> {
    (!path.as_os_str().is_empty()).then(|| path.to_path_buf())
}
