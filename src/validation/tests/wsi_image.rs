//! The VL WSI Image constraints the generated tables do not express, and the
//! Dimension Index rules, each failed by an edited export.

use super::*;

const WSI_IMAGE: &str = "intrinsic-wsi-dicom-wsi-image";
const DIMENSIONS: &str = "intrinsic-wsi-dicom-dimension-order";

/// The Plane Position (Slide) Item of Per-Frame Item `frame`.
fn plane_position(
    object: &mut InMemDicomObject,
    frame: usize,
    edit: impl FnOnce(&mut InMemDicomObject),
) {
    edit_items(
        object,
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        |frames| {
            first_item(
                &mut frames[frame],
                tags::PLANE_POSITION_SLIDE_SEQUENCE,
                edit,
            );
        },
    );
}

#[test]
fn exported_wsi_passes_the_wsi_image_and_dimension_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "grid", 4, 4, 2);
    for rule in [WSI_IMAGE, DIMENSIONS] {
        let (status, message) = rule_result(&path, rule);
        assert_eq!(status, ValidationStatus::Passed, "{rule}: {message}");
    }
}

#[test]
fn wsi_image_constraints_fail_their_violations() {
    let cases: [(&str, Edit, &str); 13] = [
        ("image-type-values", |object| {
            object.put(DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                PrimitiveValue::Strs(["ORIGINAL", "PRIMARY", "VOLUME"].map(String::from).into_iter().collect()),
            ));
        }, "Image Type (0008,0008) has 3 Values; exactly four are required"),
        ("derived-pixel-empty", |object| {
            object.put(DataElement::new(
                tags::IMAGE_TYPE,
                VR::CS,
                PrimitiveValue::Strs(["ORIGINAL", "PRIMARY", "VOLUME", ""].map(String::from).into_iter().collect()),
            ));
        }, "Value 4 (Derived Pixel) is empty"),
        ("frame-type-values", |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                first_item(shared, tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE, |item| {
                    item.put(DataElement::new(
                        tags::FRAME_TYPE,
                        VR::CS,
                        PrimitiveValue::Strs(
                            ["ORIGINAL", "PRIMARY", "VOLUME", "NONE", "EXTRA"].map(String::from).into_iter().collect(),
                        ),
                    ));
                });
            });
        }, "Frame Type (0008,9007) has 5 Values"),
        ("specimen-label", |object| {
            object.put(text(tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, "YES"));
        }, "Specimen Label in Image (0048,0010) is YES; it shall be NO for Image Type Value 3 VOLUME"),
        ("tiles-overlap", |object| {
            object.put(text(tags::TILES_OVERLAP, VR::CS, "SOME"));
        }, "Tiles Overlap (0048,0304) is SOME; it shall be NONE"),
        ("volume-depth", |object| {
            object.put(DataElement::new(tags::IMAGED_VOLUME_DEPTH, VR::FL, PrimitiveValue::from(0.0f32)));
        }, "Imaged Volume Depth (0048,0003) is 0"),
        ("reference-indicator", |object| {
            object.put(text(tags::POSITION_REFERENCE_INDICATOR, VR::LO, "STAGE"));
        }, "Position Reference Indicator (0020,1040) is \"STAGE\""),
        ("duplicate-optical-path", |object| {
            let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).unwrap();
            let duplicate = paths.items().unwrap()[0].clone();
            paths.items_mut().unwrap().push(duplicate);
            object.put(paths);
        }, "Optical Path Identifier (0048,0106) \"0\" repeats"),
        ("unknown-optical-path", |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                first_item(shared, tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE, |item| {
                    item.put(text(tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "9"));
                });
            });
        }, "Optical Path Identifier \"9\" names no Optical Path Sequence Item"),
        ("lossy-derivation", |object| {
            let mut code = InMemDicomObject::new_empty();
            code.put(text(tags::CODE_VALUE, VR::SH, "113040"));
            code.put(text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "DCM"));
            code.put(text(tags::CODE_MEANING, VR::LO, "Lossy Compression"));
            let mut derivation = InMemDicomObject::new_empty();
            derivation.put(DataElement::new(tags::DERIVATION_CODE_SEQUENCE, VR::SQ, DataSetSequence::from(vec![code])));
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(DataElement::new(tags::DERIVATION_IMAGE_SEQUENCE, VR::SQ, DataSetSequence::from(vec![derivation])));
            });
        }, "Image Type Value 1 is ORIGINAL; a compressed version of another image shall be DERIVED"),
        ("self-source", |object| {
            let own = object.element(tags::SOP_INSTANCE_UID).unwrap().to_str().unwrap().to_string();
            let mut source = InMemDicomObject::new_empty();
            source.put(text(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, &own));
            let mut derivation = InMemDicomObject::new_empty();
            derivation.put(DataElement::new(tags::SOURCE_IMAGE_SEQUENCE, VR::SQ, DataSetSequence::from(vec![source])));
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(DataElement::new(tags::DERIVATION_IMAGE_SEQUENCE, VR::SQ, DataSetSequence::from(vec![derivation])));
            });
        }, "names this SOP Instance as its own source"),
        ("pixel-spacing", |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                first_item(shared, tags::PIXEL_MEASURES_SEQUENCE, |item| {
                    item.put(text(tags::PIXEL_SPACING, VR::DS, "0\\0.0005"));
                });
            });
        }, "Pixel Spacing (0028,0030) Value 1 is 0; pixel spacing shall be positive"),
        ("slice-spacing", |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                first_item(shared, tags::PIXEL_MEASURES_SEQUENCE, |item| {
                    item.put(text(tags::SPACING_BETWEEN_SLICES, VR::DS, "-1"));
                });
            });
        }, "Spacing Between Slices (0018,0088) is -1; it shall not be negative"),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, edit, expected) in cases {
        let path = edited_wsi(tmp.path(), name, edit);
        let (status, message) = rule_result(&path, WSI_IMAGE);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}

/// The one-row exception of PS3.3 10.7.1.3 is judged on the Total Pixel
/// Matrix, not on the Frame.
#[test]
fn zero_spacing_is_allowed_only_along_a_single_row_of_the_total_pixel_matrix() {
    let zero_row_spacing: Edit = |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            first_item(shared, tags::PIXEL_MEASURES_SEQUENCE, |item| {
                item.put(text(tags::PIXEL_SPACING, VR::DS, "0\\0.0005"));
            });
        });
    };
    // Each export gets its own directory: sources of one Series in one
    // directory are read as levels of one pyramid.
    let (tiles, image) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let one_row_tiles = export_valid_color_wsi_grid_for_validation(tiles.path(), "tiles", 4, 4, 1);
    let (status, message) = rule_result(&edited(one_row_tiles, zero_row_spacing), WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(
        message.contains("Pixel Spacing (0028,0030) Value 1 is 0"),
        "{message}"
    );
    let one_row_image = export_valid_color_wsi_grid_for_validation(image.path(), "image", 4, 1, 1);
    let (status, message) = rule_result(&edited(one_row_image, zero_row_spacing), WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
}

#[test]
fn frames_off_the_regular_tiling_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_grid_for_validation(tmp.path(), "tiling", 4, 4, 2);
    let path = edited(path, |object| {
        plane_position(object, 1, |position| {
            position.put(DataElement::new(
                tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
                VR::SL,
                PrimitiveValue::from(4i32),
            ));
        });
    });
    let (status, message) = rule_result(&path, WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(message.contains("column position 4 is off the tiling of the other Frames, whose column positions are 1 modulo 2"), "{message}");
    // The same move breaks the Dimension Index: the moved Frame keeps the
    // column index of column 3, and a pixel position is compared exactly.
    let (status, message) = rule_result(&path, DIMENSIONS);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(
        message.contains("have different Values, 4 and 3, for (0048,021E)"),
        "{message}"
    );
}

#[test]
fn dimension_index_rules_fail_their_violations() {
    let cases: [(&str, Edit, &str); 4] = [
        ("frame-content-pointer", |object| {
            first_item(object, tags::DIMENSION_INDEX_SEQUENCE, |item| {
                item.put(DataElement::new(tags::DIMENSION_INDEX_POINTER, VR::AT, PrimitiveValue::Tags([tags::FRAME_CONTENT_SEQUENCE].into_iter().collect())));
            });
        }, "shall not name Frame Content Sequence or Dimension Index Values"),
        ("group-pointer-on-group", |object| {
            first_item(object, tags::DIMENSION_INDEX_SEQUENCE, |item| {
                item.put(DataElement::new(tags::DIMENSION_INDEX_POINTER, VR::AT, PrimitiveValue::Tags([tags::PLANE_POSITION_SLIDE_SEQUENCE].into_iter().collect())));
            });
        }, "Functional Group Pointer is present although the Dimension Index Pointer (0048,021A) names a Functional Group Sequence"),
        ("index-count", |object| {
            edit_items(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, |frames| {
                first_item(&mut frames[0], tags::FRAME_CONTENT_SEQUENCE, |content| {
                    content.put(DataElement::new(tags::DIMENSION_INDEX_VALUES, VR::UL, PrimitiveValue::from(1u32)));
                });
            });
        }, "1 Frame(s) have Dimension Index Values whose count is not the 2 Items"),
        ("absent-attribute", |object| {
            first_item(object, tags::DIMENSION_INDEX_SEQUENCE, |item| {
                item.put(DataElement::new(tags::DIMENSION_INDEX_POINTER, VR::AT, PrimitiveValue::Tags([tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM].into_iter().collect())));
                item.put(DataElement::new(tags::FUNCTIONAL_GROUP_POINTER, VR::AT, PrimitiveValue::Tags([tags::PIXEL_MEASURES_SEQUENCE].into_iter().collect())));
            });
        }, "Dimension Index Pointer (0040,074A) names an Attribute no Frame has"),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (name, edit, expected) in cases {
        let path = export_valid_color_wsi_grid_for_validation(tmp.path(), name, 4, 4, 2);
        let path = edited(path, edit);
        let (status, message) = rule_result(&path, DIMENSIONS);
        assert_eq!(status, ValidationStatus::Failed, "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }
}

/// A 4 x 4 slide in 2 x 2 tiles at a row spacing of 0.0005 mm and a column
/// spacing of 0.00025 mm, as the exporter writes it.
fn anisotropic_export(root: &Path) -> PathBuf {
    let source = root.join("anisotropic-source.dcm");
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(4, 4).unwrap();
    crate::synthetic_source::write_rgb_source_dicom(
        &source,
        "1.2.826.0.1.3680043.10.999.311",
        "1.2.826.0.1.3680043.10.999.312",
        4,
        4,
        pixels,
    )
    .unwrap();
    // Row spacing 0.0005 mm, column spacing 0.00025 mm (PS3.3 10.7.1.3).
    let source = edited(source, |object| {
        object.put(text(tags::PIXEL_SPACING, VR::DS, "0.0005\\0.00025"));
    });
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: root.join("anisotropic-out"),
        options: crate::ExportOptions {
            tile_size: 2,
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: crate::EncodeBackendPreference::CpuOnly,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
    .instances
    .remove(0)
    .path
}

#[test]
fn frame_offsets_follow_the_column_and_row_spacing_along_the_orientation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = anisotropic_export(tmp.path());
    // The exporter places Frame 2 (Column Position 3) 2 x 0.00025 mm along +X.
    let object = dicom_object::open_file(&path).unwrap();
    let frames = object
        .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    let position = frames[1]
        .element(tags::PLANE_POSITION_SLIDE_SEQUENCE)
        .unwrap()
        .items()
        .unwrap()[0]
        .clone();
    assert_eq!(
        position
            .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
            .unwrap()
            .to_int::<i64>()
            .unwrap(),
        3
    );
    let x = position
        .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
        .unwrap()
        .to_float64()
        .unwrap();
    assert!((x - 0.0005).abs() < 1e-9, "{x}");
    let (status, message) = rule_result(&path, WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Passed, "{message}");

    // The row spacing applied along X puts Frame 2 two columns too far.
    let swapped = edited(path, |object| {
        plane_position(object, 1, |position| {
            position.put(text(
                tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                VR::DS,
                "0.001",
            ));
        });
    });
    let (status, message) = rule_result(&swapped, WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(
        message.contains("Per-Frame Functional Groups Sequence[2] > Plane Position (Slide) Sequence > X and Y Offset in Slide Coordinate System are 0.001\\0 mm, but the Frame's top left pixel at Column Position 3 and Row Position 1 lies at 0.0005\\0 mm")
            && message.contains("1 Frame(s) lie more than half a pixel"),
        "{message}"
    );
}

#[test]
fn rotated_frame_offsets_pass_when_rotated_with_the_orientation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = anisotropic_export(tmp.path());
    // Rotate the slide a quarter turn: rows run along +Y and columns along
    // -X, so a Frame c columns and r rows from the origin lies at
    // (-r x row spacing, c x column spacing).
    let rotated = edited(path, |object| {
        object.put(text(
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "0\\1\\0\\-1\\0\\0",
        ));
        let mut frames = object
            .take(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .unwrap();
        for frame in frames.items_mut().unwrap() {
            first_item(frame, tags::PLANE_POSITION_SLIDE_SEQUENCE, |position| {
                let at = |tag| position.element(tag).unwrap().to_int::<i64>().unwrap() as f64 - 1.0;
                let (column, row) = (
                    at(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
                    at(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX),
                );
                position.put(text(
                    tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                    VR::DS,
                    &format!("{}", -row * 0.0005),
                ));
                position.put(text(
                    tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
                    VR::DS,
                    &format!("{}", column * 0.00025),
                ));
            });
        }
        object.put(frames);
    });
    let (status, message) = rule_result(&rotated, WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Passed, "{message}");
    // Left unrotated, the offsets no longer follow the orientation.
    let unrotated = edited(rotated, |object| {
        object.put(text(
            tags::IMAGE_ORIENTATION_SLIDE,
            VR::DS,
            "1\\0\\0\\0\\1\\0",
        ));
    });
    let (status, message) = rule_result(&unrotated, WSI_IMAGE);
    assert_eq!(status, ValidationStatus::Failed, "{message}");
    assert!(message.contains("lie more than half a pixel"), "{message}");
}
