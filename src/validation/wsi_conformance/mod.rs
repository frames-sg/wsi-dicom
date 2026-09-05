mod corpus;
mod fields;
mod functional_groups;
mod instance;

use std::path::Path;

use dicom_object::DefaultDicomObject;

pub(super) use corpus::{
    run_clinical_identity_set_check, run_identity_set_check, run_pyramid_geometry_set_check,
    run_slide_coordinate_set_check, run_source_relationship_set_check, run_specimen_uid_set_check,
    WsiConformanceCorpus,
};

use instance::{
    is_vl_wsi, specimen_identities, validate_core_image_profile_rule, validate_dimension_rule,
    validate_file_meta_identity, validate_icc_rule, validate_image_metadata_rule,
    validate_lossy_rule, validate_monochrome_rule, validate_optical_path_structure_rule,
    validate_slide_coordinate_system_rule, validate_specimen_container_rule,
    validate_tile_geometry_rule,
};

use super::{ValidationCheck, ValidationProfile, ValidationStatus};

const ICC_RULE: &str = "intrinsic-wsi-dicom-2026c-icc-profile";
const MONOCHROME_RULE: &str = "intrinsic-wsi-dicom-2026c-monochrome-presentation";
const LOSSY_RULE: &str = "intrinsic-wsi-dicom-2026c-lossy-history";
const SPECIMEN_RULE: &str = "intrinsic-wsi-dicom-2026c-specimen-identity";
const DIMENSION_RULE: &str = "intrinsic-wsi-dicom-2026c-dimension-order";
const TILE_GEOMETRY_RULE: &str = "intrinsic-wsi-dicom-2026c-tile-geometry";
const IDENTITY_SET_RULE: &str = "intrinsic-wsi-dicom-2026c-identity-set";
const IMAGE_METADATA_RULE: &str = "intrinsic-wsi-dicom-2026c-image-metadata";
const CORE_IMAGE_PROFILE_RULE: &str = "intrinsic-wsi-dicom-2026c-core-image-profile";
const SLIDE_COORDINATE_RULE: &str = "intrinsic-wsi-dicom-2026c-slide-coordinate-system";
const OPTICAL_PATH_STRUCTURE_RULE: &str = "intrinsic-wsi-dicom-2026c-optical-path-structure";
const SPECIMEN_CONTAINER_RULE: &str = "intrinsic-wsi-dicom-2026c-specimen-container";

pub(super) fn run_intrinsic_wsi_conformance_checks(
    path: &Path,
    object: &DefaultDicomObject,
    profile: ValidationProfile,
) -> Vec<ValidationCheck> {
    if !is_vl_wsi(object) {
        return if profile == ValidationProfile::Core2026c {
            vec![rule_check(
                path,
                CORE_IMAGE_PROFILE_RULE,
                "DICOM PS3.3 2026c A.32.8",
                Err("core profile requires VL WSI SOP Class UID".into()),
            )]
        } else {
            Vec::new()
        };
    }

    let mut checks = vec![
        rule_check(
            path,
            ICC_RULE,
            "DICOM PS3.3 2026c C.11.15 and C.8.12.5",
            validate_icc_rule(object),
        ),
        rule_check(
            path,
            MONOCHROME_RULE,
            "DICOM PS3.3 2026c C.8.12.4",
            validate_monochrome_rule(object),
        ),
        rule_check(
            path,
            LOSSY_RULE,
            "DICOM PS3.3 2026c C.8.12.4 and C.7.6",
            validate_lossy_rule(object),
        ),
        rule_check(
            path,
            SPECIMEN_RULE,
            "DICOM PS3.3 2026c C.7.6.22 and 10.14",
            specimen_identities(object).map(|_| ()),
        ),
        rule_check(
            path,
            DIMENSION_RULE,
            "DICOM PS3.3 2026c C.7.6.17.1 and C.8.12.6.1",
            validate_dimension_rule(object),
        ),
        rule_check(
            path,
            TILE_GEOMETRY_RULE,
            "DICOM PS3.3 2026c C.7.6.16.2.1 and C.8.12.6.1",
            validate_tile_geometry_rule(object),
        ),
        rule_check(
            path,
            IDENTITY_SET_RULE,
            "DICOM PS3.3 2026c C.12.1",
            validate_file_meta_identity(object),
        ),
        rule_check(
            path,
            IMAGE_METADATA_RULE,
            "DICOM PS3.3 2026c C.7.6.3",
            validate_image_metadata_rule(object),
        ),
    ];
    if profile == ValidationProfile::Core2026c {
        checks.extend([
            rule_check(
                path,
                SPECIMEN_CONTAINER_RULE,
                "DICOM PS3.3 2026c C.7.6.22",
                validate_specimen_container_rule(object),
            ),
            rule_check(
                path,
                SLIDE_COORDINATE_RULE,
                "DICOM PS3.3 2026c C.8.12.14",
                validate_slide_coordinate_system_rule(object),
            ),
            rule_check(
                path,
                OPTICAL_PATH_STRUCTURE_RULE,
                "DICOM PS3.3 2026c C.8.12.5",
                validate_optical_path_structure_rule(object),
            ),
            rule_check(
                path,
                CORE_IMAGE_PROFILE_RULE,
                "DICOM PS3.3 2026c A.32.8 and C.8.12.4",
                validate_core_image_profile_rule(object),
            ),
        ]);
    }
    checks
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
    ValidationCheck {
        execution: None,
        name: name.to_string(),
        path: (!path.as_os_str().is_empty()).then(|| path.to_path_buf()),
        status,
        command: Vec::new(),
        message,
        stdout: String::new(),
        stderr: String::new(),
    }
}
