//! `ihe-dpia-draft` and `ihe-dpia-draft-jpeg`: the CP-LAB-279 option rules
//! and the CP-LAB-281 completion document, each failed by one variant.

use dicom_core::value::PixelFragmentSequence;
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};

use super::fixture::{item, sq, text, Image};
use super::*;

const P: &[ConventionProfile] = &[ConventionProfile::IheDpiaDraft];
const JPEG: &[ConventionProfile] = &[ConventionProfile::IheDpiaDraftJpeg];
/// The conflict note every completion KOS of the edition carries.
pub(super) const KOS_NOTE: &str = "convention-ihe-dpia-draft-completion-specimen-standard-extended";

#[test]
fn a_monochrome_image_is_outside_the_true_color_option() {
    let scan = Scan::conformant(70).edit(VOLUME_3, |object| {
        object.put(text(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            "MONOCHROME2",
        ));
    });
    assert_outcome(
        "draft-monochrome",
        &scan,
        P,
        &["convention-ihe-dpia-draft-true-color"],
        &[KOS_NOTE],
    );
}

#[test]
fn the_option_requires_a_pyramid_tiled_full_and_one_z_plane() {
    let single = Scan::conformant(71).without(VOLUME_2).without(VOLUME_3);
    assert_outcome(
        "draft-single-layer",
        &single,
        P,
        &["convention-ihe-dpia-draft-pyramid"],
        &[KOS_NOTE],
    );
    let per_frame = Scan::conformant(72).edit(VOLUME_2, |object| {
        object.put(sq(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![item(Vec::new())],
        ));
    });
    assert_outcome(
        "draft-per-frame",
        &per_frame,
        P,
        &["convention-ihe-dpia-draft-tiled-full"],
        &[KOS_NOTE],
    );
    // Unlike the connectathon check, the CP requires the focal plane count.
    let no_planes = Scan::conformant(73).edit(VOLUME_2, |object| {
        object.remove_element(tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES);
    });
    let report = assert_outcome(
        "draft-no-focal-planes",
        &no_planes,
        P,
        &["convention-ihe-dpia-draft-single-z-plane"],
        &[KOS_NOTE],
    );
    assert!(
        failure_message(&report, "convention-ihe-dpia-draft-single-z-plane")
            .contains("Total Pixel Matrix Focal Planes absent")
    );
    let two_pyramids = Scan::conformant(74).edit(VOLUME_3, |object| {
        object.put(text(
            tags::PYRAMID_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.8.74.99",
        ));
    });
    assert_outcome(
        "draft-two-pyramids",
        &two_pyramids,
        P,
        &[
            "convention-ihe-dpia-draft-single-z-plane",
            "convention-ihe-dpia-draft-pyramid-identity",
        ],
        &[KOS_NOTE],
    );
}

#[test]
fn pyramid_images_share_the_frame_of_reference() {
    let scan = Scan::conformant(75).edit(VOLUME_2, |object| {
        object.put(text(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.8.75.99",
        ));
    });
    assert_outcome(
        "draft-frame-of-reference",
        &scan,
        P,
        &["convention-ihe-dpia-draft-pyramid-identity"],
        &[KOS_NOTE],
    );
}

#[test]
fn one_scan_has_one_acquisition_uid() {
    let scan = Scan::conformant(76).edit(OVERVIEW, |object| {
        object.put(text(
            tags::ACQUISITION_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.8.76.99",
        ));
    });
    assert_outcome(
        "draft-two-acquisitions",
        &scan,
        P,
        &["convention-ihe-dpia-draft-one-series-per-scan"],
        &[KOS_NOTE],
    );
}

#[test]
fn concatenation_members_are_not_permitted() {
    let scan = Scan::conformant(77).edit(VOLUME_1, |object| {
        object.put(DataElement::new(
            tags::CONCATENATION_FRAME_OFFSET_NUMBER,
            VR::UL,
            dicom_core::PrimitiveValue::from(0u32),
        ));
    });
    assert_outcome(
        "draft-concatenation",
        &scan,
        P,
        &["convention-ihe-dpia-draft-no-concatenation"],
        &[KOS_NOTE],
    );
}

#[test]
fn encapsulated_pixel_data_needs_an_offset_table() {
    let scan = Scan::conformant(78).edit(VOLUME_1, |object| {
        let fragments = object
            .element(tags::PIXEL_DATA)
            .expect("Pixel Data")
            .value()
            .fragments()
            .expect("fragments")
            .to_vec();
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PixelFragmentSequence::new(Vec::<u32>::new(), fragments),
        ));
    });
    let report = assert_outcome(
        "draft-no-offset-table",
        &scan,
        P,
        &["convention-ihe-dpia-draft-offset-table"],
        &[KOS_NOTE],
    );
    assert!(
        failure_message(&report, "convention-ihe-dpia-draft-offset-table")
            .contains("draft (CP under ballot)")
    );
}

#[test]
fn the_jpeg_option_requires_baseline_jpeg_layers() {
    let mut scan = Scan::conformant(79);
    let lossless = Image {
        syntax: uids::JPEG2000_LOSSLESS,
        ..Image::volume(3, 8, 0.002)
    }
    .object(&scan.slide);
    scan = scan.with(VOLUME_3, lossless);
    let report = assert_outcome(
        "draft-jpeg-lossless-layer",
        &scan,
        JPEG,
        &["convention-ihe-dpia-draft-jpeg-baseline"],
        &[],
    );
    assert!(
        failure_message(&report, "convention-ihe-dpia-draft-jpeg-baseline")
            .contains(uids::JPEG2000_LOSSLESS)
    );
    // The True Color option alone does not constrain the Transfer Syntax.
    assert_outcome("draft-lossless-layer", &scan, P, &[], &[KOS_NOTE]);
}

#[test]
fn completion_checks_are_skipped_without_a_completion_document() {
    let scan = Scan::conformant(80).without(COMPLETION);
    let report = assert_outcome("draft-no-completion", &scan, P, &[], &[]);
    assert_eq!(
        names_with(&report, ValidationStatus::Skipped),
        [
            "convention-ihe-dpia-draft-completion-identity",
            "convention-ihe-dpia-draft-completion-references",
            "convention-ihe-dpia-draft-completion-series",
            "convention-ihe-dpia-draft-completion-whole-images",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
}

#[test]
fn the_completion_document_lists_whole_images_of_the_scan() {
    let selected = Scan::conformant(81).edit(COMPLETION, |object| {
        let mut content = object.take(tags::CONTENT_SEQUENCE).expect("content");
        let image = &mut content.items_mut().expect("items")[0];
        let mut references = image
            .take(tags::REFERENCED_SOP_SEQUENCE)
            .expect("references");
        references.items_mut().expect("items")[0].put(text(
            tags::REFERENCED_FRAME_NUMBER,
            VR::IS,
            "1",
        ));
        image.put(references);
        object.put(content);
    });
    assert_outcome(
        "draft-completion-frames",
        &selected,
        P,
        &["convention-ihe-dpia-draft-completion-whole-images"],
        &[KOS_NOTE],
    );

    let mut incomplete = Scan::conformant(82);
    let images = fixture::images();
    let partial = fixture::completion(&incomplete.slide, &images[..4]);
    incomplete = incomplete.with(COMPLETION, partial);
    let report = assert_outcome(
        "draft-completion-incomplete",
        &incomplete,
        P,
        &["convention-ihe-dpia-draft-completion-references"],
        &[KOS_NOTE],
    );
    assert!(
        failure_message(&report, "convention-ihe-dpia-draft-completion-references")
            .contains("overview.dcm")
    );
}

#[test]
fn the_completion_document_is_a_new_series_with_the_images_identity() {
    let series = Scan::conformant(83);
    let image_series = series.slide.series.clone();
    let same_series = series.edit(COMPLETION, |object| {
        object.put(text(tags::SERIES_INSTANCE_UID, VR::UI, &image_series));
    });
    assert_outcome(
        "draft-completion-series",
        &same_series,
        P,
        &["convention-ihe-dpia-draft-completion-series"],
        &[KOS_NOTE],
    );
    let other_patient = Scan::conformant(84).edit(COMPLETION, |object| {
        object.put(text(tags::PATIENT_ID, VR::LO, "SOMEONE-ELSE"));
        object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "SLIDE-9"));
    });
    let report = assert_outcome(
        "draft-completion-identity",
        &other_patient,
        P,
        &["convention-ihe-dpia-draft-completion-identity"],
        &[KOS_NOTE],
    );
    let message = failure_message(&report, "convention-ihe-dpia-draft-completion-identity");
    // The differing Patient ID and Container Identifier are named, their
    // Values withheld (PS3.15 Table E.1-1).
    assert!(
        message.contains("Patient ID is (value withheld: identifying; 12 characters) in the KOS")
            && message
                .contains("Container Identifier is (value withheld: identifying; 7 characters)"),
        "{message}"
    );
    assert!(
        !message.contains("SOMEONE-ELSE") && !message.contains("SLIDE-9"),
        "{message}"
    );
    // Without Specimen Attributes the KOS still passes; the conflict is noted.
    let no_specimen = Scan::conformant(85).edit(COMPLETION, |object| {
        object.remove_element(tags::CONTAINER_IDENTIFIER);
        object.remove_element(tags::SPECIMEN_DESCRIPTION_SEQUENCE);
    });
    let report = assert_outcome(
        "draft-completion-no-specimen",
        &no_specimen,
        P,
        &[],
        &[KOS_NOTE],
    );
    assert!(advisory_message(&report, KOS_NOTE).contains("has no Specimen Attributes"));
}
