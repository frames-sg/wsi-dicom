//! `wg26`: each broken variant fails exactly the checks of the requirement
//! it breaks; where dciodvfy's WG26SP2025 profile and the connectathon
//! document state the same requirement, both fail.

use dicom_core::VR;
use dicom_dictionary_std::{tags, uids};

use super::fixture::{self, code, item, sq, text, Image};
use super::*;
use crate::validation::conventions::specimen::SPECIMEN_PROCESSING;

const P: &[ConventionProfile] = &[ConventionProfile::Wg26];

fn image_in(scan: &Scan, image: Image) -> DefaultDicomObject {
    image.object(&scan.slide)
}

#[test]
fn a_tiled_sparse_image_fails_both_tiled_full_checks() {
    let scan = Scan::conformant(10).edit(VOLUME_1, |object| {
        object.put(text(
            tags::DIMENSION_ORGANIZATION_TYPE,
            VR::CS,
            "TILED_SPARSE",
        ));
    });
    let report = assert_outcome(
        "wg26-tiled-sparse",
        &scan,
        P,
        &[
            "convention-wg26-sp2025-dimension-organization-type",
            "convention-wg26-tiled-full",
        ],
        &[],
    );
    let message = failure_message(
        &report,
        "convention-wg26-sp2025-dimension-organization-type",
    );
    assert!(
        message.contains("libsrc/standard/module/vl.tpl:701"),
        "{message}"
    );
}

#[test]
fn an_absent_dimension_organization_or_a_per_frame_sequence_fails_the_connectathon_check() {
    let absent = Scan::conformant(11).edit(VOLUME_1, |object| {
        object.remove_element(tags::DIMENSION_ORGANIZATION_TYPE);
    });
    assert_outcome(
        "wg26-no-organization",
        &absent,
        P,
        &["convention-wg26-tiled-full"],
        &[],
    );
    let per_frame = Scan::conformant(12).edit(VOLUME_3, |object| {
        object.put(sq(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![item(Vec::new())],
        ));
    });
    let report = assert_outcome(
        "wg26-per-frame",
        &per_frame,
        P,
        &["convention-wg26-tiled-full"],
        &[],
    );
    let message = failure_message(&report, "convention-wg26-tiled-full");
    assert!(message.contains("requirement 8, 8.a and 8.b"), "{message}");
    assert!(message.contains("(5200,9230) is present"), "{message}");
}

#[test]
fn an_absent_focal_plane_count_fails_the_profile_check() {
    let scan = Scan::conformant(13).edit(VOLUME_2, |object| {
        object.remove_element(tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES);
    });
    assert_outcome(
        "wg26-no-focal-planes",
        &scan,
        P,
        &["convention-wg26-sp2025-focal-planes"],
        &[],
    );
}

#[test]
fn layers_at_different_depths_fail_the_single_z_plane_check() {
    let scan = Scan::conformant(14).edit(VOLUME_2, |object| {
        object.put(sq(
            tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
            vec![item(vec![
                text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "20"),
                text(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "10"),
                text(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "2.5"),
            ])],
        ));
    });
    let report = assert_outcome(
        "wg26-two-depths",
        &scan,
        P,
        &["convention-wg26-single-z-plane"],
        &[],
    );
    assert!(failure_message(&report, "convention-wg26-single-z-plane").contains("2.5"));
}

#[test]
fn missing_acquisition_and_pyramid_uids_are_profile_warnings() {
    let scan = Scan::conformant(15)
        .edit(LABEL, |object| {
            object.remove_element(tags::ACQUISITION_UID);
        })
        .edit(VOLUME_3, |object| {
            object.remove_element(tags::PYRAMID_UID);
        });
    let report = assert_outcome(
        "wg26-uid-warnings",
        &scan,
        P,
        &[],
        &[
            "convention-wg26-sp2025-acquisition-uid",
            "convention-wg26-sp2025-pyramid-uid",
        ],
    );
    assert!(
        advisory_message(&report, "convention-wg26-sp2025-acquisition-uid")
            .contains("AcquisitionUID missing")
    );
}

#[test]
fn a_concatenation_fails_both_concatenation_checks() {
    let scan = Scan::conformant(16).edit(VOLUME_3, |object| {
        object.put(text(
            tags::CONCATENATION_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.8.16.9",
        ));
    });
    assert_outcome(
        "wg26-concatenation",
        &scan,
        P,
        &[
            "convention-wg26-sp2025-concatenation-uid",
            "convention-wg26-no-concatenation",
        ],
        &[],
    );
}

#[test]
fn a_layer_split_over_two_instances_fails_the_connectathon_concatenation_check() {
    let mut scan = Scan::conformant(17);
    let copy = image_in(&scan, Image::volume(6, 16, 0.001));
    scan = scan.with("volume-2-copy", copy);
    let report = assert_outcome(
        "wg26-split-layer",
        &scan,
        P,
        &["convention-wg26-no-concatenation"],
        &[],
    );
    assert!(failure_message(&report, "convention-wg26-no-concatenation")
        .contains("16x16 layer is split"));
}

#[test]
fn specimen_description_items_need_the_profile_sequences() {
    let without_type = Scan::conformant(18).edit_images(|object| {
        edit_specimen(object, |specimen| {
            specimen.remove_element(tags::SPECIMEN_TYPE_CODE_SEQUENCE);
        });
    });
    let report = assert_outcome(
        "wg26-no-specimen-type",
        &without_type,
        P,
        &["convention-wg26-sp2025-specimen-type"],
        &[],
    );
    // The SNOMED CT check cannot tell the scheme of a code that is absent.
    assert_eq!(
        names_with(&report, ValidationStatus::Skipped),
        ["convention-wg26-specimen-type-sct".to_string()].into()
    );

    let empty_step = Scan::conformant(19).edit_images(|object| {
        edit_specimen(object, |specimen| {
            let mut steps = specimen
                .take(tags::SPECIMEN_PREPARATION_SEQUENCE)
                .expect("preparation");
            steps.items_mut().expect("items").push(item(vec![sq(
                tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                Vec::new(),
            )]));
            specimen.put(steps);
        });
    });
    assert_outcome(
        "wg26-empty-step",
        &empty_step,
        P,
        &["convention-wg26-sp2025-specimen-preparation"],
        &[],
    );

    let no_anatomy = Scan::conformant(20).edit(LABEL, |object| {
        edit_specimen(object, |specimen| {
            specimen.remove_element(tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE);
        });
    });
    assert_outcome(
        "wg26-no-anatomy",
        &no_anatomy,
        P,
        &["convention-wg26-sp2025-primary-anatomic-structure"],
        &[],
    );
}

#[test]
fn an_empty_preparation_sequence_fails_both_preparation_checks() {
    let scan = Scan::conformant(21).edit(OVERVIEW, |object| {
        edit_specimen(object, |specimen| {
            specimen.put(sq(tags::SPECIMEN_PREPARATION_SEQUENCE, Vec::new()));
        });
    });
    assert_outcome(
        "wg26-empty-preparation",
        &scan,
        P,
        &[
            "convention-wg26-sp2025-specimen-preparation",
            "convention-wg26-specimen-preparation",
        ],
        &[],
    );
}

#[test]
fn text_entries_do_not_describe_fixation_embedding_or_staining() {
    let scan = Scan::conformant(22).edit(VOLUME_1, |object| {
        edit_specimen(object, |specimen| {
            let mut steps = specimen
                .take(tags::SPECIMEN_PREPARATION_SEQUENCE)
                .expect("preparation");
            // The fixation step names its fixative in text.
            steps.items_mut().expect("items")[1] = fixture::step(
                code("SCT", SPECIMEN_PROCESSING.1, "Specimen processing"),
                vec![fixture::content(
                    "TEXT",
                    code("SCT", "430864009", "Tissue Fixative"),
                    text(tags::TEXT_VALUE, VR::UT, "Formalin"),
                )],
            );
            specimen.put(steps);
        });
    });
    let report = assert_outcome(
        "wg26-text-fixation",
        &scan,
        P,
        &["convention-wg26-specimen-preparation"],
        &[],
    );
    assert!(
        failure_message(&report, "convention-wg26-specimen-preparation")
            .contains("no coded fixation")
    );
}

#[test]
fn an_hl7_specimen_type_fails_the_snomed_check() {
    let scan = Scan::conformant(23).edit_images(|object| {
        edit_specimen(object, |specimen| {
            specimen.put(sq(
                tags::SPECIMEN_TYPE_CODE_SEQUENCE,
                vec![code("HL70487", "TISS", "Tissue")],
            ));
        });
    });
    let report = assert_outcome(
        "wg26-hl7-specimen-type",
        &scan,
        P,
        &["convention-wg26-specimen-type-sct"],
        &[],
    );
    assert!(failure_message(&report, "convention-wg26-specimen-type-sct").contains("HL70487"));
}

#[test]
fn a_series_with_two_slides_or_scans_fails_the_one_series_check() {
    let two_slides = Scan::conformant(24).edit(VOLUME_3, |object| {
        object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "SLIDE-2"));
    });
    let report = assert_outcome(
        "wg26-two-slides",
        &two_slides,
        P,
        &["convention-wg26-one-series-per-scan"],
        &[],
    );
    let message = failure_message(&report, "convention-wg26-one-series-per-scan");
    assert!(
        message.contains("mixes 2 values of Container Identifier (0040,0512) ((value withheld"),
        "{message}"
    );
    assert!(!message.contains("SLIDE-2"), "{message}");

    // One scan split over two Series.
    let first = Scan::conformant(25).without(ANNOTATION).without(COMPLETION);
    let acquisition = first.slide.acquisition.clone();
    let second = Scan::conformant(26)
        .without(ANNOTATION)
        .without(COMPLETION)
        .edit_images(|object| {
            object.put(text(tags::ACQUISITION_UID, VR::UI, &acquisition));
        });
    let split = first.and(second);
    assert_outcome(
        "wg26-split-scan",
        &split,
        P,
        &["convention-wg26-one-series-per-scan"],
        &[],
    );
}

#[test]
fn a_series_without_a_label_image_or_barcode_fails() {
    let unlabelled = Scan::conformant(27).without(LABEL).without(OVERVIEW);
    let report = assert_outcome(
        "wg26-no-label",
        &unlabelled,
        P,
        &["convention-wg26-label-image"],
        &[],
    );
    assert_eq!(
        names_with(&report, ValidationStatus::Skipped),
        ["convention-wg26-barcode-value".to_string()].into()
    );
    let no_barcode = Scan::conformant(28).edit(LABEL, |object| {
        object.put(text(tags::BARCODE_VALUE, VR::LT, ""));
    });
    assert_outcome(
        "wg26-no-barcode",
        &no_barcode,
        P,
        &["convention-wg26-barcode-value"],
        &[],
    );
}

#[test]
fn a_fluorescence_layer_is_not_true_color_brightfield() {
    let scan = Scan::conformant(29).edit(VOLUME_2, |object| {
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).expect("paths");
        paths.items_mut().expect("items")[0].put(sq(
            tags::ILLUMINATION_TYPE_CODE_SEQUENCE,
            vec![code("DCM", "111743", "Epifluorescence illumination")],
        ));
        object.put(paths);
    });
    let report = assert_outcome(
        "wg26-fluorescence",
        &scan,
        P,
        &["convention-wg26-true-color-brightfield"],
        &[],
    );
    assert!(failure_message(&report, "convention-wg26-true-color-brightfield").contains("111743"));
}

#[test]
fn only_the_top_layer_and_associated_images_may_be_uncompressed() {
    let uncompressed = |instance, matrix, spacing| Image {
        syntax: uids::EXPLICIT_VR_LITTLE_ENDIAN,
        ..Image::volume(instance, matrix, spacing)
    };
    let mut apex = Scan::conformant(30);
    let object = image_in(&apex, uncompressed(3, 8, 0.002));
    apex = apex.with(VOLUME_3, object);
    assert_outcome("wg26-uncompressed-apex", &apex, P, &[], &[]);

    let mut base = Scan::conformant(31);
    let object = image_in(&base, uncompressed(1, 32, 0.0005));
    base = base.with(VOLUME_1, object);
    let report = assert_outcome(
        "wg26-uncompressed-base",
        &base,
        P,
        &["convention-wg26-transfer-syntax"],
        &[],
    );
    assert!(failure_message(&report, "convention-wg26-transfer-syntax")
        .contains("only the top layer, LABEL and OVERVIEW images may be uncompressed"));

    let mut lossless = Scan::conformant(32);
    let object = image_in(
        &lossless,
        Image {
            syntax: uids::JPEG2000_LOSSLESS,
            ..Image::volume(1, 32, 0.0005)
        },
    );
    lossless = lossless.with(VOLUME_1, object);
    assert_outcome(
        "wg26-lossless-base",
        &lossless,
        P,
        &[],
        &["convention-wg26-lossy-layers"],
    );
}

#[test]
fn a_pyramid_needs_two_layers_and_is_expected_to_have_three() {
    let single = Scan::conformant(33).without(VOLUME_2).without(VOLUME_3);
    assert_outcome(
        "wg26-single-layer",
        &single,
        P,
        &["convention-wg26-pyramid"],
        &[],
    );
    let two = Scan::conformant(34).without(VOLUME_3);
    assert_outcome(
        "wg26-two-layers",
        &two,
        P,
        &[],
        &["convention-wg26-pyramid-layers"],
    );
}

#[test]
fn the_icc_profile_belongs_in_the_single_optical_path() {
    let scan = Scan::conformant(35).edit(VOLUME_1, |object| {
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).expect("paths");
        paths.items_mut().expect("items")[0].remove_element(tags::ICC_PROFILE);
        object.put(paths);
    });
    assert_outcome(
        "wg26-no-icc",
        &scan,
        P,
        &["convention-wg26-icc-single-optical-path"],
        &[],
    );
}

#[test]
fn annotations_are_two_dimensional_and_share_the_patient() {
    let three_d = Scan::conformant(36).edit(ANNOTATION, |object| {
        object.put(text(tags::ANNOTATION_COORDINATE_TYPE, VR::CS, "3D"));
    });
    assert_outcome(
        "wg26-3d-annotation",
        &three_d,
        P,
        &["convention-wg26-sp2025-annotation-coordinate-type"],
        &[],
    );
    let other_patient = Scan::conformant(37).edit(ANNOTATION, |object| {
        object.put(text(tags::PATIENT_ID, VR::LO, "SOMEONE-ELSE"));
    });
    let report = assert_outcome(
        "wg26-annotation-patient",
        &other_patient,
        P,
        &["convention-wg26-annotation-identity"],
        &[],
    );
    let message = failure_message(&report, "convention-wg26-annotation-identity");
    assert!(
        message.contains("Patient ID is (value withheld: identifying; 12 characters)"),
        "{message}"
    );
    assert!(!message.contains("SOMEONE-ELSE"), "{message}");
    let other_study = Scan::conformant(38).edit(ANNOTATION, |object| {
        object.put(text(
            tags::STUDY_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.8.38.99",
        ));
        object.put(text(tags::MANUFACTURER, VR::LO, "Fixture Scanner Co"));
        object.put(text(tags::MANUFACTURER_MODEL_NAME, VR::LO, "F1"));
    });
    assert_outcome(
        "wg26-annotation-study",
        &other_study,
        P,
        &[],
        &[
            "convention-wg26-annotation-study",
            "convention-wg26-annotation-manufacturer",
        ],
    );
    let unresolved = Scan::conformant(39).edit(ANNOTATION, |object| {
        object.remove_element(tags::REFERENCED_SERIES_SEQUENCE);
        object.put(sq(
            tags::REFERENCED_IMAGE_SEQUENCE,
            vec![item(vec![
                text(
                    tags::REFERENCED_SOP_CLASS_UID,
                    VR::UI,
                    crate::VL_WSI_SOP_CLASS_UID,
                ),
                text(
                    tags::REFERENCED_SOP_INSTANCE_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.8.39.98",
                ),
            ])],
        ));
    });
    let report = assert_outcome("wg26-annotation-unresolved", &unresolved, P, &[], &[]);
    assert_eq!(
        names_with(&report, ValidationStatus::Skipped),
        ["convention-wg26-annotation-identity".to_string()].into()
    );
}

#[test]
fn a_label_image_needs_no_frame_of_reference_for_wg26() {
    let scan = Scan::conformant(40).edit(LABEL, |object| {
        object.remove_element(tags::FRAME_OF_REFERENCE_UID);
        object.remove_element(tags::POSITION_REFERENCE_INDICATOR);
    });
    assert_outcome("wg26-label-without-frame", &scan, P, &[], &[]);
}

/// Applies `edit` to every Specimen Description Item of `object`.
pub(super) fn edit_specimen(
    object: &mut DefaultDicomObject,
    edit: impl Fn(&mut dicom_object::InMemDicomObject),
) {
    let mut specimens = object
        .take(tags::SPECIMEN_DESCRIPTION_SEQUENCE)
        .expect("specimen description");
    for specimen in specimens.items_mut().expect("items").iter_mut() {
        edit(specimen);
    }
    object.put(specimens);
}
