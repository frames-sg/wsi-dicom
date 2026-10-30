//! PS3.3 A.75 and C.8.32: the Parametric Map check.

use super::*;
use crate::validation::parametric_map::run_parametric_map_check;

/// Edits the Frame VOI LUT Item of the Shared Functional Groups.
fn voi(edit: impl FnOnce(&mut InMemDicomObject)) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            first_item(shared, tags::FRAME_VOILUT_SEQUENCE, edit);
        });
    }
}

fn shared(edit: impl FnOnce(&mut InMemDicomObject)) -> impl FnOnce(&mut InMemDicomObject) {
    move |object| first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, edit)
}

#[test]
fn the_float_parametric_map_passes() {
    assert_family_passes("parametric-map", PARAMETRIC_MAP, |_| {});
}

#[test]
fn the_window_width_bounds_follow_the_voi_lut_function() {
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "it shall be greater than or equal to 1 (PS3.3 2026d C.11.2.1.2.1)",
        voi(|item| {
            item.put(text(tags::WINDOW_WIDTH, VR::DS, "0.5"));
        }),
    );
    assert_family_passes(
        "parametric-map",
        PARAMETRIC_MAP,
        voi(|item| {
            item.put(text(tags::WINDOW_WIDTH, VR::DS, "0.5"));
            item.put(text(tags::VOILUT_FUNCTION, VR::CS, "SIGMOID"));
        }),
    );
    for (function, section) in [
        ("SIGMOID", "C.11.2.1.3.1"),
        ("LINEAR_EXACT", "C.11.2.1.3.2"),
    ] {
        assert_family_fails(
            "parametric-map",
            PARAMETRIC_MAP,
            &format!("it shall be greater than 0 (PS3.3 2026d {section})"),
            voi(move |item| {
                item.put(text(tags::WINDOW_WIDTH, VR::DS, "0"));
                item.put(text(tags::VOILUT_FUNCTION, VR::CS, function));
            }),
        );
    }
}

#[test]
fn window_centers_and_widths_are_pairs_for_monochrome_images() {
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "they are pairs and have the same number of Values",
        voi(|item| {
            item.put(text(tags::WINDOW_CENTER, VR::DS, "0.5\\2"));
        }),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "they are used only for MONOCHROME1 and MONOCHROME2 images",
        |object| {
            object.put(text(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "RGB"));
        },
    );
}

/// A VOI LUT of `entries` (the Descriptor's first Value) of `bits`, with
/// LUT Data of `bytes` bytes, encoded with `vr`.
fn lut(
    descriptor_vr: VR,
    entries: u16,
    bits: u16,
    bytes: usize,
) -> impl FnOnce(&mut InMemDicomObject) {
    voi(move |frame_voi| {
        let descriptor = match descriptor_vr {
            VR::SS => ss(tags::LUT_DESCRIPTOR, &[entries as i16, 0, bits as i16]),
            _ => us(tags::LUT_DESCRIPTOR, &[entries, 0, bits]),
        };
        frame_voi.put(sq(
            tags::VOILUT_SEQUENCE,
            vec![item(vec![descriptor, ob(tags::LUT_DATA, &vec![0; bytes])])],
        ));
    })
}

#[test]
fn a_voi_lut_descriptor_matches_its_data() {
    // Float input may be signed, so the Descriptor is SS.
    assert_family_passes("parametric-map", PARAMETRIC_MAP, lut(VR::SS, 4, 16, 8));
    assert_family_passes("parametric-map", PARAMETRIC_MAP, lut(VR::SS, 3, 8, 4));
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "the Value Representation of its second Value shall be SS",
        lut(VR::US, 4, 16, 8),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "in an Image IOD it shall be 8 or 16",
        lut(VR::SS, 4, 12, 8),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "it shall be stored as if with 8 bits allocated, 4 bytes",
        lut(VR::SS, 4, 8, 8),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "when the number of table entries is 2^16 it shall be 0",
        lut(VR::SS, 4096, 16, 131_072),
    );
    assert_family_passes(
        "parametric-map",
        PARAMETRIC_MAP,
        lut(VR::SS, 0, 16, 131_072),
    );
}

#[test]
fn image_and_frame_type_values_three_and_four_are_present() {
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Image Type (0008,0008) Value 4 (Derived Pixel Contrast) is absent or zero length",
        |object| {
            object.put(text(tags::IMAGE_TYPE, VR::CS, "DERIVED\\PRIMARY\\VOLUME"));
        },
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Value 3 (Image Flavor) is absent or zero length",
        |object| {
            object.put(text(tags::IMAGE_TYPE, VR::CS, "DERIVED\\PRIMARY\\\\NONE"));
        },
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Frame Type (0008,9007) Value 4 is absent or zero length",
        shared(|group| {
            first_item(
                group,
                tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE,
                |frame_type| {
                    frame_type.put(text(tags::FRAME_TYPE, VR::CS, "DERIVED\\PRIMARY\\VOLUME\\"));
                },
            );
        }),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Value 4 shall be NONE when Value 1 is ORIGINAL",
        |object| {
            object.put(text(
                tags::IMAGE_TYPE,
                VR::CS,
                "ORIGINAL\\PRIMARY\\VOLUME\\QUANTITY",
            ));
        },
    );
}

#[test]
fn image_type_value_three_is_no_mixed_summary() {
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Image Type (0008,0008) Value 3 (Image Flavor) is MIXED; it may not be MIXED",
        |object| {
            object.put(text(
                tags::IMAGE_TYPE,
                VR::CS,
                "DERIVED\\PRIMARY\\MIXED\\NONE",
            ));
        },
    );
    // A Frame Type takes no MIXED Value, Shared or Per-Frame.
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Shared Functional Groups Sequence[1] > Parametric Map Frame Type Sequence (0040,9092)[1] > Frame Type (0008,9007) Value 3 is MIXED; the Value MIXED is not allowed in a Parametric Map Frame Type (PS3.3 2026d Table C.8.32-3)",
        shared(|group| {
            first_item(group, tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE, |frame_type| {
                frame_type.put(text(tags::FRAME_TYPE, VR::CS, "DERIVED\\PRIMARY\\MIXED\\NONE"));
            });
        }),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Per-Frame Functional Groups Sequence[1] > Parametric Map Frame Type Sequence (0040,9092)[1] > Frame Type (0008,9007) Value 3 is MIXED",
        |object| {
            let mut frame_type = None;
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                frame_type = shared.take(tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE);
            });
            let mut frame_type = frame_type.expect("the fixture's Frame Type");
            frame_type.items_mut().expect("Items")[0]
                .put(text(tags::FRAME_TYPE, VR::CS, "DERIVED\\PRIMARY\\MIXED\\NONE"));
            first_item(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, |frame| {
                frame.put(frame_type);
            });
        },
    );
    // Frame Types may differ from the Image Type's summary Value 3.
    assert_family_passes(
        "parametric-map",
        PARAMETRIC_MAP,
        shared(|group| {
            first_item(
                group,
                tags::PARAMETRIC_MAP_FRAME_TYPE_SEQUENCE,
                |frame_type| {
                    frame_type.put(text(
                        tags::FRAME_TYPE,
                        VR::CS,
                        "DERIVED\\PRIMARY\\PERFUSION\\NONE",
                    ));
                },
            );
        }),
    );
}

#[test]
fn float_pixel_data_is_native_of_its_own_depth() {
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Bits Allocated (0028,0100) is 64 with Float Pixel Data (7FE0,0008); it shall be 32",
        |object| {
            object.put(us(tags::BITS_ALLOCATED, &[64]));
        },
    );
    assert_family_fails("parametric-map", PARAMETRIC_MAP, "Bits Stored (0028,0101) is present with Float Pixel Data (7FE0,0008); it shall not be present", |object| {
        object.put(us(tags::BITS_STORED, &[32]));
    });
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Float Pixel Data (7FE0,0008) has Value Representation OD; it shall be OF",
        |object| {
            object.put(DataElement::new(
                tags::FLOAT_PIXEL_DATA,
                VR::OD,
                PrimitiveValue::F64([0.0, 0.25].into_iter().collect()),
            ));
        },
    );
    assert_family_passes("parametric-map", PARAMETRIC_MAP, |object| {
        object.remove_element(tags::FLOAT_PIXEL_DATA);
        object.put(us(tags::BITS_ALLOCATED, &[64]));
        object.put(DataElement::new(
            tags::DOUBLE_FLOAT_PIXEL_DATA,
            VR::OD,
            PrimitiveValue::F64([0.0, 0.25, 0.5, 1.0].into_iter().collect()),
        ));
    });
    let object = in_syntax("parametric-map", "1.2.840.10008.1.2.5", |object| {
        object.put(DataElement::new(
            tags::FLOAT_PIXEL_DATA,
            VR::OF,
            Value::PixelSequence(PixelFragmentSequence::new(
                Vec::<u32>::new(),
                vec![vec![0; 16]],
            )),
        ));
    });
    assert_status(
        run_parametric_map_check(Path::new("pm"), &object, Some(1 << 20)),
        ValidationStatus::Failed,
        "in a Standard Transfer Syntax it shall be in a Native (uncompressed) Format",
    );
}

#[test]
fn frame_laterality_agrees_with_the_structure_modifiers() {
    let anatomy = |laterality: &'static str| {
        shared(move |group| {
            group.put(sq(
                tags::FRAME_ANATOMY_SEQUENCE,
                vec![item(vec![
                    sq(
                        tags::ANATOMIC_REGION_SEQUENCE,
                        vec![code("SCT", "45048000", "Neck")],
                    ),
                    sq(
                        tags::PRIMARY_ANATOMIC_STRUCTURE_SEQUENCE,
                        vec![item(vec![
                            text(tags::CODE_VALUE, VR::SH, "62413002"),
                            text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "SCT"),
                            text(tags::CODE_MEANING, VR::LO, "Thyroid lobe"),
                            sq(
                                tags::PRIMARY_ANATOMIC_STRUCTURE_MODIFIER_SEQUENCE,
                                vec![code("SCT", "7771000", "Left")],
                            ),
                        ])],
                    ),
                    text(tags::FRAME_LATERALITY, VR::CS, laterality),
                ])],
            ));
        })
    };
    assert_family_passes("parametric-map", PARAMETRIC_MAP, anatomy("L"));
    assert_family_fails("parametric-map", PARAMETRIC_MAP, "Frame Laterality (0020,9072) is R but a Primary Anatomic Structure Modifier Sequence (0008,2230) code means L", anatomy("R"));
}

#[test]
fn a_parametric_map_palette_uses_exactly_one_lookup_table_encoding() {
    assert_family_fails("parametric-map", PARAMETRIC_MAP, "requires exactly one of Red Palette Color Lookup Table Data (0028,1201) and Segmented Red Palette Color Lookup Table Data (0028,1221)", |object| {
        object.put(us(tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR, &[3, 0, 16]));
    });
}

#[test]
fn color_space_names_the_space_of_the_icc_profile() {
    assert_family_fails("parametric-map", PARAMETRIC_MAP, "are not those of DISPLAYP3; it shall be consistent with the ICC Profile (PS3.3 2026d Table C.8.32-2", |object| {
        object.put(ob(tags::ICC_PROFILE, &crate::icc::synthetic_srgb_icc_profile().unwrap()));
        object.put(text(tags::COLOR_SPACE, VR::CS, "DISPLAYP3"));
    });
}

#[test]
fn patient_orientation_and_the_patient_plane_agree() {
    let plane = |orientation: &'static str| {
        shared(move |group| {
            group.put(sq(
                tags::PLANE_ORIENTATION_SEQUENCE,
                vec![item(vec![text(
                    tags::IMAGE_ORIENTATION_PATIENT,
                    VR::DS,
                    orientation,
                )])],
            ));
        })
    };
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "they shall be orthogonal",
        plane("1\\0\\0\\1\\0\\0"),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "column direction cosine [0.0, 0.5, 0.0] has a squared length of 0.25; it shall be normal",
        plane("1\\0\\0\\0\\0.5\\0"),
    );
    assert_family_passes(
        "parametric-map",
        PARAMETRIC_MAP,
        plane("0.70710678118655\\0.70710678118655\\0\\-0.70710678118655\\0.70710678118655\\0"),
    );
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "they shall be provided as a pair",
        |object| {
            first_item(
                object,
                tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
                |frame| {
                    frame.remove_element(tags::PLANE_POSITION_SEQUENCE);
                },
            );
        },
    );
    let orientation = |value: &'static str| {
        move |object: &mut InMemDicomObject| {
            object.put(text(tags::PATIENT_ORIENTATION, VR::CS, value));
        }
    };
    // Rows run to the patient's left (+x) and columns to the posterior (+y).
    assert_family_passes("parametric-map", PARAMETRIC_MAP, orientation("L\\P"));
    assert_family_fails("parametric-map", PARAMETRIC_MAP, "Patient Orientation (0020,0020) Value 1 \"R\" abbreviation R names as principal a direction other than that", orientation("R\\P"));
    assert_family_fails(
        "parametric-map",
        PARAMETRIC_MAP,
        "Value 2 \"PH\" abbreviation H names a direction that is not one",
        orientation("L\\PH"),
    );
}
