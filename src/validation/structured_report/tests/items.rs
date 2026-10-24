//! The Values of the Content Item Macros by Value Type (C.17-5, C.18), in
//! documents and through `content_item_violations`, which DICOMDIR record
//! Content Sequences are held to.

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::fixtures::*;
use super::{assert_fails, assert_passes};
use crate::validation::structured_report::content_item_violations;

/// Puts `element` into `item`, discarding the element it replaces.
fn drop_put(item: &mut InMemDicomObject, element: InMemElement) {
    item.put(element);
}

/// The report with the Measured Value Sequence Item of its NUM edited.
fn measured(edit: impl FnOnce(&mut InMemDicomObject)) -> InMemDicomObject {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    edit_first(&mut object, &AREA, tags::MEASURED_VALUE_SEQUENCE, edit);
    object
}

#[test]
fn text_values_hold_no_format_control_characters() {
    for (value, name) in [
        ("A\ttab", "HT (U+0009) at character 2"),
        ("Form\u{c}feed", "FF (U+000C)"),
        ("Line\nfeed", "LF without a preceding CR"),
        ("Carriage\rreturn", "CR without a following LF"),
    ] {
        let mut object = key_object_selection();
        put_at(&mut object, &[0], text(tags::TEXT_VALUE, VR::UT, value));
        assert_fails(
            object,
            &format!("Content Sequence[1] > Text Value (0040,A160) holds {name}"),
        );
    }
    // CR LF line breaks (the fixture's), spaces and code extensions pass.
    let mut object = key_object_selection();
    put_at(
        &mut object,
        &[0],
        text(tags::TEXT_VALUE, VR::UT, "A \u{1b}$B text"),
    );
    assert_passes(object);
}

#[test]
fn each_measured_value_attribute_holds_a_single_value() {
    for (element, name) in [
        (
            text(tags::NUMERIC_VALUE, VR::DS, "600\\601"),
            "Numeric Value (0040,A30A) has 2 Values",
        ),
        (
            DataElement::new(
                tags::FLOATING_POINT_VALUE,
                VR::FD,
                PrimitiveValue::F64([600.0, 601.0].into_iter().collect()),
            ),
            "Floating Point Value (0040,A161) has 2 Values",
        ),
        (
            DataElement::new(
                tags::RATIONAL_NUMERATOR_VALUE,
                VR::SL,
                PrimitiveValue::I32([600, 1].into_iter().collect()),
            ),
            "Rational Numerator Value (0040,A162) has 2 Values",
        ),
        (
            DataElement::new(
                tags::RATIONAL_DENOMINATOR_VALUE,
                VR::UL,
                PrimitiveValue::U32([1, 1].into_iter().collect()),
            ),
            "Rational Denominator Value (0040,A163) has 2 Values",
        ),
    ] {
        let object = measured(|value| drop_put(value, element));
        assert_fails(
            object,
            &format!("{name}; only a single Value shall be present (PS3.3 2026d Table C.18.1-1)"),
        );
    }
}

fn floating(value: f64) -> InMemElement {
    DataElement::new(
        tags::FLOATING_POINT_VALUE,
        VR::FD,
        PrimitiveValue::F64([value].into_iter().collect()),
    )
}

fn rational(numerator: i32, denominator: u32) -> [InMemElement; 2] {
    [
        DataElement::new(
            tags::RATIONAL_NUMERATOR_VALUE,
            VR::SL,
            PrimitiveValue::I32([numerator].into_iter().collect()),
        ),
        DataElement::new(
            tags::RATIONAL_DENOMINATOR_VALUE,
            VR::UL,
            PrimitiveValue::U32([denominator].into_iter().collect()),
        ),
    ]
}

#[test]
fn measured_values_represent_integers_or_real_numbers() {
    assert_passes(measured(|value| drop_put(value, floating(600.4))));
    assert_passes(measured(|value| {
        value.put(text(tags::NUMERIC_VALUE, VR::DS, "0.3333"));
        value.put(floating(1.0 / 3.0));
        rational(1, 3).into_iter().for_each(|element| {
            value.put(element);
        });
    }));
    assert_fails(
        measured(|value| drop_put(value, floating(f64::NAN))),
        "Floating Point Value (0040,A161) is NaN, which is not an integer or real number",
    );
    assert_fails(
        measured(|value| drop_put(value, floating(612.0))),
        "Floating Point Value (0040,A161) 612 is not the floating point representation of Numeric Value (0040,A30A) 600",
    );
    assert_fails(
        measured(|value| {
            rational(1, 0).into_iter().for_each(|element| {
                value.put(element);
            })
        }),
        "Rational Denominator Value (0040,A163) is 0",
    );
    assert_fails(
        measured(|value| {
            rational(1203, 2).into_iter().for_each(|element| {
                value.put(element);
            })
        }),
        "1203/2 is not a rational representation of Numeric Value (0040,A30A) 600",
    );
}

/// The KOS with the Referenced SOP Sequence Item of its IMAGE edited.
fn referenced(edit: impl FnOnce(&mut InMemDicomObject)) -> InMemDicomObject {
    let mut object = key_object_selection();
    edit_first(&mut object, &[1], tags::REFERENCED_SOP_SEQUENCE, edit);
    object
}

#[test]
fn referenced_frame_numbers_count_from_one() {
    assert_passes(referenced(|sop| {
        drop_put(sop, text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "1\\2"))
    }));
    assert_fails(
        referenced(|sop| drop_put(sop, text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "0"))),
        "Referenced Frame Number (0008,1160) has Value 0; the first Frame shall be denoted as Frame number 1 (PS3.3 2026d Table C.18.4-1)",
    );
}

#[test]
fn an_icon_image_is_at_most_128_by_128() {
    let icon = |rows: &str, columns: &str| {
        sq(
            tags::ICON_IMAGE_SEQUENCE,
            vec![item(vec![
                text(tags::ROWS, VR::US, rows),
                text(tags::COLUMNS, VR::US, columns),
            ])],
        )
    };
    assert_passes(referenced(|sop| drop_put(sop, icon("128", "64"))));
    assert_fails(
        referenced(|sop| drop_put(sop, icon("256", "64"))),
        "Icon Image Sequence (0088,0200) is 256 rows by 64 columns; the Icon Image shall be no greater than 128 rows by 128 columns",
    );
}

/// The report with its SCOORD's Graphic Type and Graphic Data replaced.
fn outline(graphic_type: &str, values: &[f32]) -> InMemDicomObject {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    put_at(
        &mut object,
        &REGION,
        text(tags::GRAPHIC_TYPE, VR::CS, graphic_type),
    );
    put_at(&mut object, &REGION, floats(tags::GRAPHIC_DATA, values));
    object
}

#[test]
fn graphic_data_has_the_points_of_its_graphic_type() {
    assert_passes(outline("POINT", &[5.0, 5.0]));
    assert_passes(outline("CIRCLE", &[5.0, 5.0, 8.0, 5.0]));
    assert_passes(outline(
        "ELLIPSE",
        &[1.0, 5.0, 9.0, 5.0, 5.0, 3.0, 5.0, 7.0],
    ));
    assert_fails(
        outline("POINT", &[5.0, 5.0, 6.0, 6.0]),
        "Graphic Data (0070,0022) has 2 (column,row) pairs for Graphic Type (0070,0023) POINT, which is defined by exactly 1 (PS3.3 2026d C.18.6.1.2)",
    );
    assert_fails(
        outline("POLYLINE", &[5.0, 5.0]),
        "which is defined by at least 2",
    );
    assert_fails(
        outline("ELLIPSE", &[1.0, 5.0, 9.0, 5.0]),
        "which is defined by exactly 4",
    );
    assert_fails(
        outline("POLYLINE", &[5.0, 5.0, 6.0]),
        "Graphic Data (0070,0022) has 3 Values, which are not (column,row) pairs",
    );
}

#[test]
fn graphic_data_is_at_least_0_0() {
    assert_fails(
        outline("POLYLINE", &[-1.0, 5.0, 6.0, 6.0]),
        "Graphic Data (0070,0022) has Value -1; the values must be within the range 0\\0 to Columns\\Rows (PS3.3 2026d Table C.18.6-1)",
    );
}

/// A Comprehensive 3D report whose group holds an SCOORD3D of
/// `graphic_type` instead of the SCOORD (TID 1410 Row 7b).
fn volume(graphic_type: &str, values: &[f32]) -> InMemDicomObject {
    let mut object = measurement_report(COMPREHENSIVE_3D_SR);
    let replacement = content(
        "CONTAINS",
        "SCOORD3D",
        Some(code("DCM", "111030", "Image Region")),
        vec![
            text(
                tags::REFERENCED_FRAME_OF_REFERENCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.60.40",
            ),
            text(tags::GRAPHIC_TYPE, VR::CS, graphic_type),
            floats(tags::GRAPHIC_DATA, values),
        ],
    );
    edit_at(&mut object, &REGION, |item| *item = replacement);
    object
}

#[test]
fn a_polygon_closes_and_is_coplanar() {
    let square = [
        0.0, 0.0, 1.0, 10.0, 0.0, 1.0, 10.0, 10.0, 1.0, 0.0, 10.0, 1.0, 0.0, 0.0, 1.0,
    ];
    assert_passes(volume("POLYGON", &square));
    let mut open = square;
    open[14] = 2.0;
    assert_fails(
        volume("POLYGON", &open),
        "Graphic Data (0070,0022) of a POLYGON starts at [0.0, 0.0, 1.0] and ends at [0.0, 0.0, 2.0]; the first and last vertices shall be the same (PS3.3 2026d C.18.9.1.2)",
    );
    let mut bent = square;
    bent[8] = 4.0;
    assert_fails(
        volume("POLYGON", &bent),
        "off the plane of the others; the points shall be coplanar (PS3.3 2026d C.18.9.1.2)",
    );
}

#[test]
fn graphic_data_3d_has_the_triplets_of_its_graphic_type() {
    assert_fails(
        volume("ELLIPSOID", &[0.0; 15]),
        "Graphic Data (0070,0022) has 5 (x,y,z) triplets for Graphic Type (0070,0023) ELLIPSOID, which is defined by exactly 6",
    );
    assert_fails(
        volume("POINT", &[0.0; 4]),
        "has 4 Values, which are not (x,y,z) triplets",
    );
}

/// The report with a TCOORD of `range` and `values` sample positions,
/// selected from the slide image.
fn timed(range: &str, positions: &str) -> InMemDicomObject {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut object,
        &GROUP,
        content(
            "CONTAINS",
            "TCOORD",
            Some(code("DCM", "111030", "Image Region")),
            vec![
                text(tags::TEMPORAL_RANGE_TYPE, VR::CS, range),
                text(tags::REFERENCED_TIME_OFFSETS, VR::DS, positions),
                sq(tags::CONTENT_SEQUENCE, vec![image("SELECTED FROM")]),
            ],
        ),
    );
    object
}

#[test]
fn temporal_points_follow_the_temporal_range_type() {
    assert_passes(timed("SEGMENT", "1\\2"));
    assert_passes(timed("MULTISEGMENT", "1\\2\\3\\4"));
    assert_fails(
        timed("SEGMENT", "1\\2\\3"),
        "Referenced Time Offsets (0040,A138) has 3 Values for Temporal Range Type (0040,A130) SEGMENT, which is defined by exactly 2 temporal points (PS3.3 2026d C.18.7.1.1)",
    );
    assert_fails(timed("MULTISEGMENT", "1\\2\\3"), "two per segment");
}

#[test]
fn sample_positions_count_from_one() {
    let mut object = measurement_report(COMPREHENSIVE_SR);
    push_child(
        &mut object,
        &GROUP,
        content(
            "CONTAINS",
            "TCOORD",
            None,
            vec![
                text(tags::TEMPORAL_RANGE_TYPE, VR::CS, "POINT"),
                text(tags::REFERENCED_SAMPLE_POSITIONS, VR::UL, "0"),
                sq(tags::CONTENT_SEQUENCE, vec![image("SELECTED FROM")]),
            ],
        ),
    );
    assert_fails(object, "Referenced Sample Positions (0040,A132) has Value 0; the position of the first sample is 1");
}

/// A TABLE Content Item of 2 rows and 2 columns, with these Items.
fn table(
    rows: Vec<InMemDicomObject>,
    columns: Vec<InMemDicomObject>,
    cells: Vec<InMemDicomObject>,
) -> InMemDicomObject {
    content(
        "HAS CONCEPT MOD",
        "TABLE",
        Some(code("DCM", "130520", "Transformation Matrix")),
        vec![sq(
            tags::TABULATED_VALUES_SEQUENCE,
            vec![item(vec![
                text(tags::NUMBER_OF_TABLE_ROWS, VR::UL, "2"),
                text(tags::NUMBER_OF_TABLE_COLUMNS, VR::UL, "2"),
                sq(tags::TABLE_ROW_DEFINITION_SEQUENCE, rows),
                sq(tags::TABLE_COLUMN_DEFINITION_SEQUENCE, columns),
                sq(tags::CELL_VALUES_SEQUENCE, cells),
            ])],
        )],
    )
}

fn definition(number: dicom_core::Tag, value: &str) -> InMemDicomObject {
    item(vec![
        text(number, VR::UL, value),
        sq(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            vec![code("DCM", "113734", "X-Ray Tube Current")],
        ),
    ])
}

fn cell(row: Option<&str>, column: Option<&str>, values: &str) -> InMemDicomObject {
    let mut elements = vec![
        text(tags::SELECTOR_ATTRIBUTE_VR, VR::CS, "IS"),
        text(tags::SELECTOR_IS_VALUE, VR::IS, values),
    ];
    elements.extend(row.map(|row| text(tags::TABLE_ROW_NUMBER, VR::UL, row)));
    elements.extend(column.map(|column| text(tags::TABLE_COLUMN_NUMBER, VR::UL, column)));
    item(elements)
}

/// The TABLE Value Type is in no A.35 IOD; its Macro binds wherever a
/// TABLE Content Item is, as in a DICOMDIR record's Content Sequence.
#[test]
fn table_rows_columns_and_cells_are_sorted_and_within_the_table() {
    let rows = || {
        vec![
            definition(tags::TABLE_ROW_NUMBER, "1"),
            definition(tags::TABLE_ROW_NUMBER, "2"),
        ]
    };
    let columns = || {
        vec![
            definition(tags::TABLE_COLUMN_NUMBER, "1"),
            definition(tags::TABLE_COLUMN_NUMBER, "2"),
        ]
    };
    let cells = || {
        vec![
            cell(Some("1"), Some("1"), "1"),
            cell(Some("1"), Some("2"), "0"),
            cell(Some("2"), None, "0\\1"),
        ]
    };
    assert_eq!(
        content_item_violations(&[table(rows(), columns(), cells())]),
        Vec::<String>::new()
    );

    let violations = |item: InMemDicomObject| content_item_violations(&[item]).join("; ");
    let unsorted = violations(table(
        rows().into_iter().rev().collect(),
        columns(),
        cells(),
    ));
    assert!(unsorted.contains("Table Row Definition Sequence (0040,A806) has Table Row Number (0040,A804) Values [2, 1]; Items of this Sequence shall be sorted by Table Row Number (0040,A804) (PS3.3 2026d Table C.18.10-1)"), "{unsorted}");
    let unsorted = violations(table(
        rows(),
        columns().into_iter().rev().collect(),
        cells(),
    ));
    assert!(
        unsorted.contains("shall be sorted by Table Column Number (0040,A805)"),
        "{unsorted}"
    );
    let unsorted = violations(table(
        rows(),
        columns(),
        cells().into_iter().rev().collect(),
    ));
    assert!(unsorted.contains("follows a later cell; Items of this Sequence shall be in row-major order, then sorted by column"), "{unsorted}");
    let outside = violations(table(
        rows(),
        columns(),
        vec![cell(Some("3"), Some("1"), "1")],
    ));
    assert!(
        outside.contains("names row 3 and column 1 of a table of 2 rows and 2 columns"),
        "{outside}"
    );
    let short = violations(table(rows(), columns(), vec![cell(Some("1"), None, "1")]));
    assert!(
        short.contains("has 1 Selector IS Values for all of row 1, which has 2 cells"),
        "{short}"
    );
    let numbered = violations(table(
        vec![definition(tags::TABLE_ROW_NUMBER, "5")],
        columns(),
        cells(),
    ));
    assert!(
        numbered.contains("has Table Row Number (0040,A804) 5, outside the table's 1 to 2"),
        "{numbered}"
    );
}

#[test]
fn record_content_items_are_held_to_the_macros_of_their_value_types() {
    let language = content(
        "HAS CONCEPT MOD",
        "CODE",
        Some(code(
            "DCM",
            "121049",
            "Language of Content Item and Descendants",
        )),
        vec![sq(
            tags::CONCEPT_CODE_SEQUENCE,
            vec![code("RFC5646", "en-US", "English (United States)")],
        )],
    );
    let comment = |value: &str| {
        content(
            "HAS CONCEPT MOD",
            "TEXT",
            Some(code("DCM", "121106", "Comment")),
            vec![text(tags::TEXT_VALUE, VR::UT, value)],
        )
    };
    assert!(content_item_violations(&[language.clone(), comment("fine")]).is_empty());
    let violations = content_item_violations(&[language, comment("a\tb")]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("Content Sequence[2] > Text Value (0040,A160) holds HT"),
        "{violations:?}"
    );
    // SCOORD Targets bind in a record too; a by-reference Target names the
    // referenced document's tree, which the record does not hold.
    let region = content(
        "HAS CONCEPT MOD",
        "SCOORD",
        None,
        vec![
            text(tags::GRAPHIC_TYPE, VR::CS, "POINT"),
            floats(tags::GRAPHIC_DATA, &[1.0, 1.0]),
        ],
    );
    let violations = content_item_violations(&[region]);
    assert!(
        violations.iter().any(|violation| violation
            .contains("is an SCOORD Content Item without a SELECTED FROM relationship")),
        "{violations:?}"
    );
}

#[test]
fn record_violations_are_bounded() {
    let tabbed: Vec<InMemDicomObject> = (0..60)
        .map(|_| {
            content(
                "HAS CONCEPT MOD",
                "TEXT",
                None,
                vec![text(tags::TEXT_VALUE, VR::UT, "\t")],
            )
        })
        .collect();
    let violations = content_item_violations(&tabbed);
    assert_eq!(violations.len(), 51);
    assert_eq!(violations[50], "and 10 more Content Item violations");
}
