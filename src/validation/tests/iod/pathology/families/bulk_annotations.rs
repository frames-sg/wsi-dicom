//! PS3.3 A.87 and C.37.1.2: the Microscopy Bulk Simple Annotations check.

use super::*;

fn ol(tag: Tag, values: &[u32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::OL,
        PrimitiveValue::U32(values.iter().copied().collect()),
    )
}

/// The fixture's Annotation Group made `graphic` Annotations of `points`.
fn shapes(
    graphic: &'static str,
    points: &'static [f32],
    annotations: u32,
    indices: Option<&'static [u32]>,
) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| {
        first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
            group.put(text(tags::GRAPHIC_TYPE, VR::CS, graphic));
            group.put(of(tags::POINT_COORDINATES_DATA, points));
            group.put(ul(tags::NUMBER_OF_ANNOTATIONS, &[annotations]));
            match indices {
                Some(indices) => {
                    group.put(ol(tags::LONG_PRIMITIVE_POINT_INDEX_LIST, indices));
                }
                None => {
                    group.remove_element(tags::LONG_PRIMITIVE_POINT_INDEX_LIST);
                }
            }
        });
    }
}

/// `edit` after making the coordinates 3D in a slide Frame of Reference.
fn three_d(edit: impl FnOnce(&mut InMemDicomObject)) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| {
        object.put(text(tags::ANNOTATION_COORDINATE_TYPE, VR::CS, "3D"));
        object.remove_element(tags::PIXEL_ORIGIN_INTERPRETATION);
        object.put(text(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.34",
        ));
        object.put(text(
            tags::POSITION_REFERENCE_INDICATOR,
            VR::LO,
            "SLIDE_CORNER",
        ));
        first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
            group.put(text(tags::ANNOTATION_APPLIES_TO_ALL_Z_PLANES, VR::CS, "NO"));
        });
        edit(object);
    }
}

const SQUARE: &[f32] = &[0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0];

#[test]
fn the_annotation_fixture_and_each_well_formed_shape_pass() {
    assert_family_passes("annotations", BULK_ANNOTATIONS, |_| {});
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        shapes("POINT", &[1.0, 2.0, 3.0, 4.0], 2, None),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        shapes(
            "POLYLINE",
            &[0.0, 0.0, 5.0, 0.0, 10.0, 10.0, 12.0, 12.0],
            2,
            Some(&[1, 5]),
        ),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        shapes("POLYGON", SQUARE, 1, Some(&[1])),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        shapes(
            "ELLIPSE",
            &[0.0, 5.0, 10.0, 5.0, 5.0, 3.0, 5.0, 7.0],
            1,
            None,
        ),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        shapes(
            "RECTANGLE",
            &[0.0, 0.0, 8.0, 0.0, 8.0, 4.0, 0.0, 4.0],
            1,
            None,
        ),
    );
}

#[test]
fn image_relative_coordinates_reference_one_image_or_frame() {
    assert_family_fails("annotations", BULK_ANNOTATIONS, "Pixel Origin Interpretation (0048,0301) is FRAME, but Referenced Image Sequence (0008,1140) does not specify a single Frame", |object| {
        object.put(text(tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, "FRAME"));
    });
    assert_family_passes("annotations", BULK_ANNOTATIONS, |object| {
        object.put(text(tags::PIXEL_ORIGIN_INTERPRETATION, VR::CS, "FRAME"));
        first_item(object, tags::REFERENCED_IMAGE_SEQUENCE, |image| {
            image.put(text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "1"));
        });
    });
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "without a subset of Frames",
        |object| {
            first_item(object, tags::REFERENCED_IMAGE_SEQUENCE, |image| {
                image.put(text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "1"));
            });
        },
    );
}

#[test]
fn annotation_group_numbers_count_from_one() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "numbers start at 1 and increase monotonically by 1",
        |object| {
            first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
                group.put(us(tags::ANNOTATION_GROUP_NUMBER, &[2]));
            });
        },
    );
}

#[test]
fn coordinates_are_whole_tuples_with_a_common_z_factored_out() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "not a whole number of (X, Y) tuples",
        shapes("POINT", &[1.0, 2.0, 3.0], 1, None),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "a common Z shall be factored out into Common Z Coordinate Value (006A,0010)",
        three_d(shapes("POINT", &[1.0, 2.0, 0.1, 3.0, 4.0, 0.1], 2, None)),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        three_d(shapes("POINT", &[1.0, 2.0, 0.1, 3.0, 4.0, 0.2], 2, None)),
    );
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        three_d(|object| {
            shapes("POINT", &[1.0, 2.0, 3.0, 4.0], 2, None)(object);
            first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
                group.put(fd(tags::COMMON_Z_COORDINATE_VALUE, &[0.1]));
            });
        }),
    );
}

#[test]
fn points_ellipses_and_rectangles_are_counted_without_an_index_list() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "for POINT they shall be the same",
        shapes("POINT", &[1.0, 2.0], 2, None),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "there is no need for a separate index of each Annotation",
        shapes("POINT", &[1.0, 2.0], 1, Some(&[1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "one quarter of the number of coordinate tuples",
        shapes("ELLIPSE", &[0.0, 5.0, 10.0, 5.0, 5.0, 3.0], 1, None),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the index of each Annotation can be computed",
        shapes(
            "RECTANGLE",
            &[0.0, 0.0, 8.0, 0.0, 8.0, 4.0, 0.0, 4.0],
            1,
            Some(&[1]),
        ),
    );
}

#[test]
fn the_index_list_starts_at_one_and_increases_at_tuple_starts() {
    let lines: &[f32] = &[0.0, 0.0, 5.0, 0.0, 10.0, 10.0, 12.0, 12.0];
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "it shall contain Number of Annotations Values",
        shapes("POLYLINE", lines, 2, Some(&[1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "starts at 3; the index of the first Value of the first coordinate tuple shall be 1",
        shapes("POLYLINE", lines, 2, Some(&[3, 5])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "are not strictly increasing",
        shapes("POLYLINE", lines, 2, Some(&[1, 1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "Value 4 is not the index of the first Value of a coordinate tuple",
        shapes("POLYLINE", lines, 2, Some(&[1, 4])),
    );
}

#[test]
fn polygons_are_simple_and_implicitly_closed() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the first and last points of the POLYGON are the same",
        shapes(
            "POLYGON",
            &[0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 0.0],
            1,
            Some(&[1]),
        ),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "line segments 1 and 3 cross or overlap",
        shapes(
            "POLYGON",
            &[0.0, 0.0, 4.0, 4.0, 4.0, 0.0, 0.0, 4.0],
            1,
            Some(&[1]),
        ),
    );
    // A keyhole cut to an inner square runs one edge back along another.
    let keyhole: &[f32] = &[
        0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 5.0, 10.0, 5.0, 6.0, 6.0, 6.0, 6.0, 4.0, 4.0, 4.0, 4.0,
        6.0, 5.0, 6.0, 5.0, 10.0, 0.0, 10.0,
    ];
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "cross or overlap; the line segments shall not cross and shall not contain holes",
        shapes("POLYGON", keyhole, 1, Some(&[1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "a POLYGON of 2 points",
        shapes("POLYGON", &[0.0, 0.0, 4.0, 0.0], 1, Some(&[1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "cross or overlap",
        shapes("POLYLINE", &[0.0, 0.0, 4.0, 0.0, 2.0, 0.0], 1, Some(&[1])),
    );
}

#[test]
fn ellipses_and_rectangles_have_their_shape() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the axes are not perpendicular",
        shapes(
            "ELLIPSE",
            &[0.0, 5.0, 10.0, 5.0, 4.0, 3.0, 6.0, 7.0],
            1,
            None,
        ),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the first axis is shorter than the second",
        shapes(
            "ELLIPSE",
            &[4.0, 5.0, 6.0, 5.0, 5.0, 0.0, 5.0, 10.0],
            1,
            None,
        ),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the axes do not share their midpoint",
        shapes(
            "ELLIPSE",
            &[0.0, 5.0, 10.0, 5.0, 2.0, 3.0, 2.0, 7.0],
            1,
            None,
        ),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "are not four corners of a rectangle",
        shapes(
            "RECTANGLE",
            &[0.0, 0.0, 8.0, 0.0, 9.0, 4.0, 0.0, 4.0],
            1,
            None,
        ),
    );
}

/// A coplanar quadrilateral, clockwise viewed from the top of the slide.
const CLOCKWISE: &[f32] = &[0.0, 0.0, 0.1, 0.0, 1.0, 0.2, 1.0, 1.0, 0.3, 1.0, 0.0, 0.2];
const COUNTER_CLOCKWISE: &[f32] = &[1.0, 0.0, 0.2, 1.0, 1.0, 0.3, 0.0, 1.0, 0.2, 0.0, 0.0, 0.1];

#[test]
fn three_dimensional_outlines_are_coplanar_and_clockwise() {
    assert_family_passes(
        "annotations",
        BULK_ANNOTATIONS,
        three_d(shapes("POLYGON", CLOCKWISE, 1, Some(&[1]))),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the POLYGON's vertices wind counter-clockwise viewed from the top surface of the slide",
        three_d(shapes("POLYGON", COUNTER_CLOCKWISE, 1, Some(&[1]))),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "(PS3.3 2026d Table C.37.1-2)",
        three_d(shapes("RECTANGLE", COUNTER_CLOCKWISE, 1, None)),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "the points shall be coplanar",
        three_d(shapes(
            "POLYGON",
            &[0.0, 0.0, 0.1, 0.0, 1.0, 0.2, 1.0, 1.0, 0.3, 1.0, 0.0, 0.9],
            1,
            Some(&[1]),
        )),
    );
}

/// The fixture's Annotation Group with one Measurement of `values`, indexed
/// by `indices`.
fn measured(
    values: &'static [f32],
    indices: Option<&'static [u32]>,
) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| {
        first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
            let mut stored = item(vec![of(tags::FLOATING_POINT_VALUES, values)]);
            if let Some(indices) = indices {
                stored.put(ol(tags::ANNOTATION_INDEX_LIST, indices));
            }
            group.put(sq(
                tags::MEASUREMENTS_SEQUENCE,
                vec![item(vec![
                    sq(
                        tags::CONCEPT_NAME_CODE_SEQUENCE,
                        vec![code("SCT", "42798000", "Area")],
                    ),
                    sq(
                        tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
                        vec![code("UCUM", "um2", "square micrometer")],
                    ),
                    sq(tags::MEASUREMENT_VALUES_SEQUENCE, vec![stored]),
                ])],
            ));
        });
    }
}

#[test]
fn measurement_values_follow_the_annotations_they_cover() {
    let two_polygons: &'static [f32] = &[
        0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 10.0, 10.0, 14.0, 10.0, 12.0, 14.0,
    ];
    let polygons = || shapes("POLYGON", two_polygons, 2, Some(&[1, 7]));
    let with = |values, indices| {
        move |object: &mut InMemDicomObject| {
            polygons()(object);
            measured(values, indices)(object);
        }
    };
    assert_family_passes("annotations", BULK_ANNOTATIONS, with(&[16.0, 8.0], None));
    assert_family_passes("annotations", BULK_ANNOTATIONS, with(&[8.0], Some(&[2])));
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "has 3 Values, but Long Primitive Point Index List (0066,0040) has 2 Annotations",
        with(&[1.0, 2.0, 3.0], None),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "has 2 Values, but Annotation Index List (006A,0011) has 1 indices",
        with(&[1.0, 2.0], Some(&[1])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "Value 3 references no Annotation of the 2 in the group",
        with(&[1.0], Some(&[3])),
    );
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "has 2 Values, but Number of Annotations (006A,000C) is 1",
        measured(&[1.0, 2.0], None),
    );
}

#[test]
fn color_space_names_the_space_of_the_icc_profile() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "are not those of DISPLAYP3",
        |object| {
            object.put(ob(
                tags::ICC_PROFILE,
                &crate::icc::synthetic_srgb_icc_profile().unwrap(),
            ));
            object.put(text(tags::COLOR_SPACE, VR::CS, "DISPLAYP3"));
        },
    );
}

#[test]
fn many_violations_are_counted_and_the_first_shown() {
    assert_family_fails(
        "annotations",
        BULK_ANNOTATIONS,
        "25 violations: Annotation Group Sequence[1] > Annotation Group Number (0040,A180) is 0",
        |object| {
            let group = object
                .get(tags::ANNOTATION_GROUP_SEQUENCE)
                .and_then(|groups| groups.items())
                .expect("Annotation Group Items")[0]
                .clone();
            let mut misnumbered = group;
            misnumbered.put(us(tags::ANNOTATION_GROUP_NUMBER, &[0]));
            object.put(sq(tags::ANNOTATION_GROUP_SEQUENCE, vec![misnumbered; 25]));
        },
    );
    assert_family_fails("annotations", BULK_ANNOTATIONS, "; and 5 more", |object| {
        first_item(object, tags::ANNOTATION_GROUP_SEQUENCE, |group| {
            group.put(us(tags::ANNOTATION_GROUP_NUMBER, &[0]));
        });
        let group = object
            .get(tags::ANNOTATION_GROUP_SEQUENCE)
            .and_then(|groups| groups.items())
            .expect("Annotation Group Items")[0]
            .clone();
        object.put(sq(tags::ANNOTATION_GROUP_SEQUENCE, vec![group; 25]));
    });
}
