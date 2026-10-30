//! `interop`: each advisory appears for the configuration its tool rejects
//! or warns about, citing the tool.

use dicom_core::value::PixelFragmentSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};

use super::fixture::{self, item, sq, text, Flavor, Image};
use super::*;

const P: &[ConventionProfile] = &[ConventionProfile::Interop];

#[track_caller]
fn advises(name: &str, scan: &Scan, advisories: &[&str]) -> ValidationReport {
    assert_outcome(name, scan, P, &[], advisories)
}

fn origin(x: &str) -> dicom_object::mem::InMemElement {
    sq(
        tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE,
        vec![item(vec![
            text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, x),
            text(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "10"),
        ])],
    )
}

#[test]
fn viewer_pyramid_requirements() {
    let report = advises(
        "interop-dmv-frame",
        &Scan::conformant(90).edit(VOLUME_2, |object| {
            object.put(text(
                tags::FRAME_OF_REFERENCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.8.90.99",
            ));
        }),
        &["convention-interop-dmv-frame-of-reference"],
    );
    assert!(
        advisory_message(&report, "convention-interop-dmv-frame-of-reference")
            .contains("src/pyramid.js:111-115")
    );
    advises(
        "interop-dmv-container",
        &Scan::conformant(91).edit(VOLUME_3, |object| {
            object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "SLIDE-2"));
        }),
        &["convention-interop-dmv-container"],
    );
    advises(
        "interop-dmv-channels",
        &Scan::conformant(92).edit(VOLUME_3, |object| {
            object.put(fixture::long(tags::NUMBER_OF_OPTICAL_PATHS, 2));
        }),
        &["convention-interop-dmv-channels"],
    );
    advises(
        "interop-dmv-focal-planes",
        &Scan::conformant(93).edit(VOLUME_1, |object| {
            object.put(text(tags::EXTENDED_DEPTH_OF_FIELD, VR::CS, "YES"));
            object.put(fixture::unsigned(tags::NUMBER_OF_FOCAL_PLANES, 5));
        }),
        &["convention-interop-dmv-focal-planes"],
    );
}

#[test]
fn a_split_layer_and_a_second_label_break_viewers_and_openslide() {
    let mut split = Scan::conformant(94);
    let copy = Image::volume(6, 16, 0.001).object(&split.slide);
    split = split.with("volume-2-copy", copy);
    advises(
        "interop-split-layer",
        &split,
        &["convention-interop-split-layer"],
    );

    let mut labels = Scan::conformant(95);
    let second = Image::associated(7, Flavor::Label).object(&labels.slide);
    labels = labels.with("label-2", second);
    advises(
        "interop-second-label",
        &labels,
        &["convention-interop-openslide-associated-images"],
    );
}

#[test]
fn the_icc_profile_is_read_from_the_first_optical_path() {
    let scan = Scan::conformant(96).edit(VOLUME_1, |object| {
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).expect("paths");
        paths.items_mut().expect("items")[0].remove_element(tags::ICC_PROFILE);
        object.put(paths);
    });
    advises(
        "interop-no-icc",
        &scan,
        &["convention-interop-icc-first-optical-path"],
    );
}

#[test]
fn transfer_syntaxes_and_pixel_formats_outside_the_readers() {
    let mut implicit = Scan::conformant(97);
    let object = Image {
        syntax: uids::IMPLICIT_VR_LITTLE_ENDIAN,
        ..Image::associated(4, Flavor::Label)
    }
    .object(&implicit.slide);
    implicit = implicit.with(LABEL, object);
    let report = advises(
        "interop-implicit-label",
        &implicit,
        &[
            "convention-interop-openslide-transfer-syntax",
            "convention-interop-slim-transfer-syntax",
        ],
    );
    assert!(
        advisory_message(&report, "convention-interop-openslide-transfer-syntax")
            .contains("Unsupported transfer syntax 1.2.840.10008.1.2")
    );
    advises(
        "interop-planar",
        &Scan::conformant(98).edit(OVERVIEW, |object| {
            object.put(fixture::unsigned(tags::PLANAR_CONFIGURATION, 1));
        }),
        &["convention-interop-openslide-pixel-format"],
    );
    advises(
        "interop-ybr-full",
        &Scan::conformant(99).edit(VOLUME_3, |object| {
            object.put(text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "YBR_FULL"));
        }),
        &["convention-interop-openslide-photometric"],
    );
}

#[test]
fn orthanc_requires_modality_sm_and_a_consistent_base_layer() {
    advises(
        "interop-modality",
        &Scan::conformant(100).edit(VOLUME_2, |object| {
            object.put(text(tags::MODALITY, VR::CS, "OT"));
        }),
        &["convention-interop-orthanc-modality"],
    );
    // A base layer concatenated in two members that disagree on the lens.
    let slide = Scan::conformant(101);
    let concatenation = "1.2.826.0.1.3680043.10.999.8.101.9";
    let source = slide.slide.instance_uid(1);
    let member = |object: &mut DefaultDicomObject, number: u16, power: &str| {
        object.put(text(tags::CONCATENATION_UID, VR::UI, concatenation));
        object.put(text(
            tags::SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
            VR::UI,
            &source,
        ));
        object.put(fixture::unsigned(tags::IN_CONCATENATION_NUMBER, number));
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).expect("paths");
        paths.items_mut().expect("items")[0].put(text(tags::OBJECTIVE_LENS_POWER, VR::DS, power));
        object.put(paths);
    };
    let second = {
        let mut object = Image::volume(8, 32, 0.0005).object(&slide.slide);
        member(&mut object, 2, "40");
        object
    };
    let scan = slide
        .edit(VOLUME_1, |object| member(object, 1, "20"))
        .with("volume-1-member-2", second);
    let report = advises(
        "interop-lens-power",
        &scan,
        &["convention-interop-orthanc-base-consistency"],
    );
    assert!(
        advisory_message(&report, "convention-interop-orthanc-base-consistency")
            .contains("objective lens power")
    );
}

#[test]
fn orthanc_drops_misplaced_sparse_tiles() {
    let scan = Scan::conformant(102).edit(VOLUME_2, |object| {
        let frame = |column: i32, row: i32| {
            item(vec![sq(
                tags::PLANE_POSITION_SLIDE_SEQUENCE,
                vec![item(vec![
                    DataElement::new(
                        tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                        VR::SL,
                        PrimitiveValue::from(column),
                    ),
                    DataElement::new(
                        tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                        VR::SL,
                        PrimitiveValue::from(row),
                    ),
                ])],
            )])
        };
        object.put(text(
            tags::DIMENSION_ORGANIZATION_TYPE,
            VR::CS,
            "TILED_SPARSE",
        ));
        object.put(sq(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![frame(1, 1), frame(5, 1), frame(1, 9), frame(9, 9)],
        ));
    });
    let report = advises(
        "interop-sparse-tiles",
        &scan,
        &["convention-interop-orthanc-sparse-tiles"],
    );
    assert!(
        advisory_message(&report, "convention-interop-orthanc-sparse-tiles")
            .contains("1 Frame(s) with unexpected tile location")
    );
}

#[test]
fn large_sequences_and_missing_index_fields() {
    advises(
        "interop-large-sequence",
        &Scan::conformant(103).edit(VOLUME_3, |object| {
            object.put(sq(
                tags::ACQUISITION_CONTEXT_SEQUENCE,
                vec![fixture::content(
                    "TEXT",
                    fixture::code("DCM", "121106", "Comment"),
                    DataElement::new(
                        tags::TEXT_VALUE,
                        VR::UT,
                        PrimitiveValue::from("x".repeat(1024 * 1024)),
                    ),
                )],
            ));
        }),
        &["convention-interop-metadata-sequence-size"],
    );
    advises(
        "interop-no-lens-power",
        &Scan::conformant(104).edit(VOLUME_1, |object| {
            let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).expect("paths");
            paths.items_mut().expect("items")[0].remove_element(tags::OBJECTIVE_LENS_POWER);
            object.put(paths);
        }),
        &["convention-interop-idc-index"],
    );
    advises(
        "interop-no-offset-table",
        &Scan::conformant(105).edit(VOLUME_1, |object| {
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
        }),
        &["convention-interop-highdicom-offset-table"],
    );
}

#[test]
fn wsidicom_level_requirements() {
    let mut tiles = Scan::conformant(106);
    let object = Image {
        tile: 4,
        ..Image::volume(2, 16, 0.001)
    }
    .object(&tiles.slide);
    tiles = tiles.with(VOLUME_2, object);
    advises(
        "interop-tile-size",
        &tiles,
        &["convention-interop-wsidicom-tile-size"],
    );

    let mut scale = Scan::conformant(107);
    let object = Image::volume(2, 11, 0.0015).object(&scale.slide);
    scale = scale.with(VOLUME_2, object);
    let report = advises(
        "interop-scale",
        &scale,
        &["convention-interop-wsidicom-scale"],
    );
    assert!(advisory_message(&report, "convention-interop-wsidicom-scale").contains("3.000"));

    advises(
        "interop-origin",
        &Scan::conformant(108).edit(VOLUME_3, |object| {
            object.put(origin("20.05"));
        }),
        &["convention-interop-wsidicom-origin"],
    );
}

#[test]
fn dciodvfy_base_warnings() {
    advises(
        "interop-label-pyramid",
        &Scan::conformant(109).edit(LABEL, |object| {
            object.put(text(
                tags::PYRAMID_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.8.109.4",
            ));
        }),
        &["convention-interop-dciodvfy-pyramid-uid-label"],
    );
    advises(
        "interop-overview-frames",
        &Scan::conformant(110).edit(OVERVIEW, |object| {
            object.put(text(tags::NUMBER_OF_FRAMES, VR::IS, "2"));
        }),
        &["convention-interop-dciodvfy-single-frame"],
    );
    let report = advises(
        "interop-imaged-volume",
        &Scan::conformant(111).edit(VOLUME_1, |object| {
            object.put(DataElement::new(
                tags::IMAGED_VOLUME_WIDTH,
                VR::FL,
                PrimitiveValue::from(0.02f32),
            ));
        }),
        &["convention-interop-dciodvfy-imaged-volume"],
    );
    assert!(
        advisory_message(&report, "convention-interop-dciodvfy-imaged-volume")
            .contains("ImagedVolumeWidth inconsistent with TotalPixelMatrixColumns")
    );
}

#[test]
fn conflicts_with_normative_text_are_advisories() {
    advises(
        "interop-label-frame",
        &Scan::conformant(112).edit(LABEL, |object| {
            object.remove_element(tags::FRAME_OF_REFERENCE_UID);
            object.remove_element(tags::POSITION_REFERENCE_INDICATOR);
        }),
        &["convention-interop-associated-frame-of-reference"],
    );
    advises(
        "interop-xyb",
        &Scan::conformant(113).edit(VOLUME_2, |object| {
            object.put(text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "XYB"));
        }),
        &[
            "convention-interop-xyb-photometric",
            "convention-interop-openslide-photometric",
        ],
    );
}
