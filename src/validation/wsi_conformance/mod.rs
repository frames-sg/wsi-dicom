mod corpus;
mod dimensions;
mod fields;
mod functional_groups;
mod geometry;
mod identity;
mod instance;
mod lossy_encoding;
mod pixel_attributes;
mod pixel_syntax;
mod relations;
mod series_set;
#[cfg(test)]
mod tests;
mod tiled_full;
mod wsi_image;

use std::path::{Path, PathBuf};

use dicom_object::DefaultDicomObject;

pub(super) use corpus::{
    run_clinical_identity_set_check, run_identity_set_check, run_pyramid_geometry_set_check,
    run_series_set_check, run_slide_coordinate_set_check, run_source_relationship_set_check,
    run_specimen_uid_set_check, WsiConformanceCorpus,
};

use geometry::{
    validate_dimension_rule, validate_slide_coordinate_system_rule, validate_tile_geometry_rule,
};
use identity::{
    specimen_identities, validate_file_meta_identity, validate_specimen_container_rule,
};
use instance::{
    validate_core_image_profile_rule, validate_icc_rule, validate_image_metadata_rule,
    validate_lossy_rule, validate_monochrome_rule, validate_optical_path_structure_rule,
};
use pixel_attributes::validate_pixel_attributes_rule;

use super::codestream::run_codestream_check;
use super::iod::{declares_vl_wsi, run_iod_checks, OptionalNotes};
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

/// The intrinsic checks of one object, with its optional conformance notes.
pub(super) fn run_intrinsic_wsi_conformance_checks(
    path: &Path,
    object: &DefaultDicomObject,
    profile: ValidationProfile,
    conformance: crate::ConformanceLevel,
    max_pixel_frame_bytes: usize,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let (iod_checks, notes) = run_iod_checks(path, object, conformance);
    if !declares_vl_wsi(object) {
        let mut checks = if profile == ValidationProfile::Core {
            vec![rule_check(
                path,
                CORE_IMAGE_PROFILE_RULE,
                concat!("DICOM PS3.3 ", dicom_edition!(), " A.32.8"),
                Err("core profile requires VL WSI SOP Class UID".into()),
            )]
        } else {
            Vec::new()
        };
        checks.extend(iod_checks);
        return (checks, notes);
    }

    let mut checks = vec![
        rule_check(
            path,
            ICC_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.11.15 and C.8.12.5"),
            validate_icc_rule(object),
        ),
        rule_check(
            path,
            MONOCHROME_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.4"),
            validate_monochrome_rule(object),
        ),
        rule_check(
            path,
            LOSSY_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.4 and C.7.6"),
            validate_lossy_rule(object),
        ),
        rule_check(
            path,
            SPECIMEN_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.22 and 10.14"),
            specimen_identities(object).map(|_| ()),
        ),
        rule_check(
            path,
            DIMENSION_RULE,
            concat!(
                "DICOM PS3.3 ",
                dicom_edition!(),
                " C.7.6.17.1 and C.8.12.6.1"
            ),
            validate_dimension_rule(object),
        ),
        rule_check(
            path,
            TILE_GEOMETRY_RULE,
            concat!(
                "DICOM PS3.3 ",
                dicom_edition!(),
                " C.7.6.16.2.1 and C.8.12.6.1"
            ),
            validate_tile_geometry_rule(object),
        ),
        rule_check(
            path,
            IDENTITY_SET_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.12.1"),
            validate_file_meta_identity(object),
        ),
        rule_check(
            path,
            IMAGE_METADATA_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.3"),
            validate_image_metadata_rule(object),
        ),
        rule_check(
            path,
            WSI_IMAGE_RULE,
            concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12"),
            wsi_image::validate_wsi_image_rule(object),
        ),
        rule_check(
            path,
            RELATIONS_RULE,
            concat!(
                "DICOM PS3.3 ",
                dicom_edition!(),
                " 8.6, 10.17, C.7, C.8.12.5 and C.12"
            ),
            relations::validate_relations_rule(object),
        ),
        rule_check(
            path,
            PIXEL_ATTRIBUTES_RULE,
            concat!(
                "DICOM PS3.3 ",
                dicom_edition!(),
                " C.7.5.1.1.2, C.7.6.1.1.6, C.7.6.3.1 and C.7.9"
            ),
            validate_pixel_attributes_rule(object),
        ),
    ];
    checks.push(run_codestream_check(path, object, max_pixel_frame_bytes));
    checks.extend(iod_checks);
    if profile == ValidationProfile::Core {
        checks.extend([
            rule_check(
                path,
                SPECIMEN_CONTAINER_RULE,
                concat!("DICOM PS3.3 ", dicom_edition!(), " C.7.6.22"),
                validate_specimen_container_rule(object),
            ),
            rule_check(
                path,
                SLIDE_COORDINATE_RULE,
                concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.14"),
                validate_slide_coordinate_system_rule(object),
            ),
            rule_check(
                path,
                OPTICAL_PATH_STRUCTURE_RULE,
                concat!("DICOM PS3.3 ", dicom_edition!(), " C.8.12.5"),
                validate_optical_path_structure_rule(object),
            ),
            rule_check(
                path,
                CORE_IMAGE_PROFILE_RULE,
                concat!("DICOM PS3.3 ", dicom_edition!(), " A.32.8 and C.8.12.4"),
                validate_core_image_profile_rule(object),
            ),
        ]);
    }
    (checks, notes)
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
