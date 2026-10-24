//! The set-level checks of the IOD families: a Segmentation and the images
//! it references (PS3.3 A.51.1 and A.51.5.1), annotations and the image they
//! reference (C.37.1.2.1.1), and a synchronized Parametric Map's Series
//! (C.7.4.2.1.1).

use super::*;
use crate::validation::image_semantics::{run_family_set_checks, FamilyCorpus};

fn object(name: &str, edit: impl FnOnce(&mut InMemDicomObject)) -> DefaultDicomObject {
    in_syntax(name, EXPLICIT_VR_LITTLE_ENDIAN, edit)
}

/// A 2 x 2 VL WSI image `uid` as the set checks read it, after `edit`.
fn image(uid: &str, edit: impl FnOnce(&mut InMemDicomObject)) -> DefaultDicomObject {
    let mut image = item(vec![
        text(tags::SOP_CLASS_UID, VR::UI, crate::VL_WSI_SOP_CLASS_UID),
        text(tags::SOP_INSTANCE_UID, VR::UI, uid),
        text(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.89",
        ),
        us(tags::ROWS, &[2]),
        us(tags::COLUMNS, &[2]),
    ]);
    edit(&mut image);
    image
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(EXPLICIT_VR_LITTLE_ENDIAN))
        .expect("file meta")
}

/// The image the segmentation fixture is derived from.
const SOURCE: &str = "1.2.826.0.1.3680043.10.999.7.3";
/// The image the annotation fixture references.
const ANNOTATED: &str = "1.2.826.0.1.3680043.10.999.7.31";
const SLIDE_FRAME: &str = "1.2.826.0.1.3680043.10.999.7.5";

/// The set checks of `objects`, each labelled with its index.
fn set_checks(objects: &[DefaultDicomObject]) -> Vec<ValidationCheck> {
    let mut corpus = FamilyCorpus::default();
    for (index, object) in objects.iter().enumerate() {
        corpus.observe(Path::new(&format!("object-{index}.dcm")), object);
    }
    run_family_set_checks(&corpus)
}

#[track_caller]
fn assert_set(
    objects: &[DefaultDicomObject],
    rule: &str,
    status: ValidationStatus,
    expected: &str,
) {
    let checks = set_checks(objects);
    let found: Vec<&ValidationCheck> = checks.iter().filter(|check| check.name == rule).collect();
    assert_eq!(found.len(), 1, "{rule}: {checks:?}");
    let check = found[0];
    assert_eq!(check.path, None, "a set check names no file");
    assert_eq!(check.status, status, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "message lacks {expected:?}: {}",
        check.message
    );
}

#[test]
fn a_set_without_the_iods_reports_no_family_set_check() {
    assert!(set_checks(&[image(SOURCE, |_| {}), object("parametric-map", |_| {})]).is_empty());
}

#[test]
fn a_segmentation_without_frame_of_reference_matches_its_image() {
    assert_set(
        &[object("segmentation", |_| {}), image(SOURCE, |_| {})],
        SEGMENTATION,
        ValidationStatus::Passed,
        "conformant",
    );
    assert_set(
        &[
            object("segmentation", |object| {
                object.put(us(tags::COLUMNS, &[3]));
            }),
            image(SOURCE, |_| {}),
        ],
        SEGMENTATION,
        ValidationStatus::Failed,
        "the Segmentation's Rows x Columns 2 x 3 differ from the image's 2 x 2; it shall have the image's spatial sampling and extent (PS3.3 2026d A.51.1 and A.51.5.1)",
    );
    let fine = image(SOURCE, |image| {
        image.put(text(tags::PIXEL_SPACING, VR::DS, "0.0005\\0.0005"));
    });
    let coarse = object("segmentation", |object| {
        object.put(text(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.81",
        ));
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            shared.put(sq(
                tags::PIXEL_MEASURES_SEQUENCE,
                vec![item(vec![text(
                    tags::PIXEL_SPACING,
                    VR::DS,
                    "0.001\\0.001",
                )])],
            ));
        });
    });
    assert_set(
        &[coarse, fine],
        SEGMENTATION,
        ValidationStatus::Failed,
        "Pixel Spacing [0.001, 0.001] differs from the image's [0.0005, 0.0005]",
    );
}

#[test]
fn a_segmentation_shares_the_frame_of_reference_of_its_image() {
    let slide = || {
        image(SOURCE, |image| {
            image.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, SLIDE_FRAME));
        })
    };
    let segmentation = |frame_of_reference: Option<&'static str>| {
        object("segmentation", move |object| {
            if let Some(uid) = frame_of_reference {
                object.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, uid));
            }
        })
    };
    assert_set(
        &[segmentation(Some(SLIDE_FRAME)), slide()],
        SEGMENTATION,
        ValidationStatus::Passed,
        "conformant",
    );
    assert_set(
        &[segmentation(None), slide()],
        SEGMENTATION,
        ValidationStatus::Failed,
        "whose Frame of Reference UID is 1.2.826.0.1.3680043.10.999.7.5, but the Segmentation's is absent; it shall have the same Frame of Reference (PS3.3 2026d A.51.1)",
    );
    assert_set(
        &[segmentation(Some(SLIDE_FRAME))],
        SEGMENTATION,
        ValidationStatus::Skipped,
        "the referenced Instance is not in the validated set",
    );
}

/// The image the annotation fixture references, rows along -Y and columns
/// along -X of the slide, after `edit`.
fn annotated(edit: impl FnOnce(&mut InMemDicomObject)) -> DefaultDicomObject {
    image(ANNOTATED, |image| {
        image.put(text(
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "0\\-1\\0\\-1\\0\\0",
        ));
        edit(image);
    })
}

fn polygon(points: &'static [f32]) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| {
        first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
            group.put(text(tags::GRAPHIC_TYPE, VR::CS, "POLYGON"));
            group.put(of(tags::POINT_COORDINATES_DATA, points));
            group.put(DataElement::new(
                tags::LONG_PRIMITIVE_POINT_INDEX_LIST,
                VR::OL,
                PrimitiveValue::U32([1].into_iter().collect()),
            ));
        });
    }
}

#[test]
fn an_annotated_image_is_no_concatenation_instance() {
    let annotations = object("annotations", |_| {});
    assert_set(
        &[annotations.clone(), annotated(|_| {})],
        BULK_ANNOTATIONS,
        ValidationStatus::Passed,
        "conformant",
    );
    assert_set(
        &[
            annotations.clone(),
            annotated(|image| {
                image.put(text(
                    tags::CONCATENATION_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.93",
                ));
            }),
        ],
        BULK_ANNOTATIONS,
        ValidationStatus::Failed,
        "an Instance of a Concatenation",
    );
    assert_set(
        &[annotations],
        BULK_ANNOTATIONS,
        ValidationStatus::Skipped,
        "the referenced Instance is not in the validated set",
    );
}

#[test]
fn image_relative_outlines_wind_clockwise_viewed_from_the_slide_top() {
    // With rows along -Y and columns along -X, (column, row) clockwise on the
    // screen is clockwise from the top of the slide.
    let clockwise: &'static [f32] = &[0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0];
    let counter: &'static [f32] = &[0.0, 0.0, 0.0, 4.0, 4.0, 4.0, 4.0, 0.0];
    assert_set(
        &[object("annotations", polygon(clockwise)), annotated(|_| {})],
        BULK_ANNOTATIONS,
        ValidationStatus::Passed,
        "conformant",
    );
    assert_set(
        &[object("annotations", polygon(counter)), annotated(|_| {})],
        BULK_ANNOTATIONS,
        ValidationStatus::Failed,
        "Annotation Group Sequence Item 1 has 1 POLYGON Annotation(s) winding counter-clockwise viewed from the top surface of the slide (Annotation 1 the first)",
    );
    // A mirrored image orientation reverses the winding.
    let mirrored = annotated(|image| {
        image.put(text(
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "0\\1\\0\\-1\\0\\0",
        ));
    });
    assert_set(
        &[object("annotations", polygon(counter)), mirrored],
        BULK_ANNOTATIONS,
        ValidationStatus::Passed,
        "conformant",
    );
    assert_set(
        &[
            object("annotations", polygon(counter)),
            image(ANNOTATED, |_| {}),
        ],
        BULK_ANNOTATIONS,
        ValidationStatus::Passed,
        "not examined: the winding of the 2D polylines and polygons",
    );
}

#[test]
fn a_synchronized_parametric_map_series_shares_one_frame_of_reference() {
    let synchronized = |instance: &'static str, frame: &'static str| {
        object("parametric-map", move |object| {
            object.put(text(
                tags::SYNCHRONIZATION_FRAME_OF_REFERENCE_UID,
                VR::UI,
                "1.2.840.10008.15.1.1",
            ));
            object.put(text(tags::SOP_INSTANCE_UID, VR::UI, instance));
            object.put(text(
                tags::SERIES_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.7.94",
            ));
            object.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, frame));
        })
    };
    let first = synchronized(
        "1.2.826.0.1.3680043.10.999.7.95",
        "1.2.826.0.1.3680043.10.999.7.96",
    );
    assert_set(
        std::slice::from_ref(&first),
        SYNCHRONIZATION,
        ValidationStatus::Skipped,
        "no other Instance of the Series is in the validated set",
    );
    let same = synchronized(
        "1.2.826.0.1.3680043.10.999.7.97",
        "1.2.826.0.1.3680043.10.999.7.96",
    );
    assert_set(
        &[first.clone(), same],
        SYNCHRONIZATION,
        ValidationStatus::Passed,
        "conformant",
    );
    let other = synchronized(
        "1.2.826.0.1.3680043.10.999.7.97",
        "1.2.826.0.1.3680043.10.999.7.98",
    );
    let third = synchronized(
        "1.2.826.0.1.3680043.10.999.7.99",
        "1.2.826.0.1.3680043.10.999.7.98",
    );
    // Each member is compared with the first synchronized map once, and the
    // disagreements are counted, not listed.
    assert_set(
        &[first, other, third],
        SYNCHRONIZATION,
        ValidationStatus::Failed,
        "object-0.dcm has a Synchronization Frame of Reference UID and Frame of Reference UID 1.2.826.0.1.3680043.10.999.7.96, but 2 other Instance(s) of its Series have another, object-1.dcm with 1.2.826.0.1.3680043.10.999.7.98; all SOP Instances in the Series shall share the same Frame of Reference (PS3.3 2026d C.7.4.2.1.1)",
    );
}
