//! PS3.3 A.51 and C.8.20: the Segmentation check.

use super::*;
use crate::validation::segmentation::run_segmentation_check;

const LIMIT: Option<usize> = Some(1 << 20);

/// The fixture as a 2 x 2 FRACTIONAL Segmentation of `pixels`, Maximum
/// Fractional Value 100.
fn fractional(object: &mut InMemDicomObject, pixels: &[u8]) {
    object.put(text(tags::SEGMENTATION_TYPE, VR::CS, "FRACTIONAL"));
    object.put(text(
        tags::SEGMENTATION_FRACTIONAL_TYPE,
        VR::CS,
        "PROBABILITY",
    ));
    object.put(us(tags::MAXIMUM_FRACTIONAL_VALUE, &[100]));
    eight_bits(object);
    object.put(ob(tags::PIXEL_DATA, pixels));
}

fn eight_bits(object: &mut InMemDicomObject) {
    object.put(us(tags::BITS_ALLOCATED, &[8]));
    object.put(us(tags::BITS_STORED, &[8]));
    object.put(us(tags::HIGH_BIT, &[7]));
}

fn segment(number: u16) -> InMemDicomObject {
    item(vec![
        sq(
            tags::SEGMENTED_PROPERTY_CATEGORY_CODE_SEQUENCE,
            vec![code("SCT", "91723000", "Anatomical structure")],
        ),
        us(tags::SEGMENT_NUMBER, &[number]),
        text(tags::SEGMENT_LABEL, VR::LO, "tissue"),
        text(tags::SEGMENT_ALGORITHM_TYPE, VR::CS, "MANUAL"),
        sq(
            tags::SEGMENTED_PROPERTY_TYPE_CODE_SEQUENCE,
            vec![code("SCT", "85756007", "Tissue")],
        ),
    ])
}

/// The fixture as a LABELMAP Segmentation of Segments 1 and 2.
fn label_map(object: &mut InMemDicomObject, pixels: &[u8]) {
    object.put(text(
        tags::SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.66.7",
    ));
    object.put(text(tags::SEGMENTATION_TYPE, VR::CS, "LABELMAP"));
    eight_bits(object);
    object.put(sq(tags::SEGMENT_SEQUENCE, vec![segment(1), segment(2)]));
    first_item(
        object,
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        |frame| {
            frame.remove_element(tags::SEGMENT_IDENTIFICATION_SEQUENCE);
        },
    );
    object.put(ob(tags::PIXEL_DATA, pixels));
}

#[test]
fn a_binary_fractional_and_label_map_segmentation_pass() {
    assert_family_passes("segmentation", SEGMENTATION, |_| {});
    assert_family_passes("segmentation", SEGMENTATION, |object| {
        fractional(object, &[0, 50, 100, 100])
    });
    assert_family_passes("segmentation", SEGMENTATION, |object| {
        label_map(object, &[1, 2, 2, 1])
    });
}

#[test]
fn image_type_has_no_value_beyond_two() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "no other Values shall be present",
        |object| {
            object.put(text(tags::IMAGE_TYPE, VR::CS, "DERIVED\\PRIMARY\\VOLUME"));
        },
    );
}

#[test]
fn segment_numbers_are_unique_and_count_from_one() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "shall be unique within the Segmentation Instance",
        |object| {
            object.put(sq(tags::SEGMENT_SEQUENCE, vec![segment(1), segment(1)]));
        },
    );
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "shall start at 1 and increase monotonically by 1",
        |object| {
            object.put(sq(tags::SEGMENT_SEQUENCE, vec![segment(1), segment(3)]));
        },
    );
    // A label map may number its Segments freely.
    assert_family_passes("segmentation", SEGMENTATION, |object| {
        label_map(object, &[5, 7, 7, 5]);
        object.put(sq(tags::SEGMENT_SEQUENCE, vec![segment(7), segment(5)]));
    });
}

#[test]
fn a_frame_references_one_existing_segment() {
    let referencing = |numbers: &'static [u16]| {
        move |object: &mut InMemDicomObject| {
            first_item(
                object,
                tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
                |frame| {
                    first_item(
                        frame,
                        tags::SEGMENT_IDENTIFICATION_SEQUENCE,
                        |identification| {
                            identification.put(us(tags::REFERENCED_SEGMENT_NUMBER, numbers));
                        },
                    );
                },
            );
        }
    };
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "shall not be multi-valued",
        referencing(&[1, 1]),
    );
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "identifies no Segment Number",
        referencing(&[2]),
    );
}

#[test]
fn a_label_map_does_not_overlap_and_describes_every_value() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "shall be NO when Segmentation Type is LABELMAP",
        |object| {
            label_map(object, &[1, 2, 2, 1]);
            object.put(text(tags::SEGMENTS_OVERLAP, VR::CS, "YES"));
        },
    );
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "holds the values {0, 3}",
        |object| {
            label_map(object, &[1, 0, 3, 1]);
        },
    );
}

#[test]
fn segments_are_located_in_the_frame_of_reference_or_derived_pixel_by_pixel() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "the Segment shall be specified within that coordinate system",
        |object| {
            object.put(text(
                tags::FRAME_OF_REFERENCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.7.80",
            ));
        },
    );
    assert_family_passes("segmentation", SEGMENTATION, |object| {
        object.put(text(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.80",
        ));
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            shared.put(sq(
                tags::PIXEL_MEASURES_SEQUENCE,
                vec![item(vec![
                    text(tags::SLICE_THICKNESS, VR::DS, "0.005"),
                    text(tags::PIXEL_SPACING, VR::DS, "0.001\\0.001"),
                ])],
            ));
        });
        first_item(
            object,
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            |frame| {
                frame.put(sq(
                    tags::PLANE_POSITION_SLIDE_SEQUENCE,
                    vec![item(vec![
                        text(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "10"),
                        text(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "20"),
                        text(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM, VR::DS, "0"),
                        sl(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX, &[1]),
                        sl(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX, &[1]),
                    ])],
                ));
            },
        );
    });
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "each pixel shall correspond to a pixel of a referenced image",
        |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                first_item(shared, tags::DERIVATION_IMAGE_SEQUENCE, |derivation| {
                    derivation.put(sq(tags::SOURCE_IMAGE_SEQUENCE, vec![]));
                });
            });
        },
    );
}

#[test]
fn binary_pixels_are_packed_eight_to_a_byte_across_frames() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "packed 8 to a byte they occupy 1 bytes, padded to 2",
        |object| {
            object.put(ob(tags::PIXEL_DATA, &[1, 0, 1, 0]));
        },
    );
    // Three 3 x 3 Frames: 27 bits in 4 bytes, not 2 bytes per Frame.
    let frames = |object: &mut InMemDicomObject, bytes: &[u8]| {
        object.put(us(tags::ROWS, &[3]));
        object.put(us(tags::COLUMNS, &[3]));
        object.put(text(tags::NUMBER_OF_FRAMES, VR::IS, "3"));
        let frame = object
            .get(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .and_then(|frames| frames.items())
            .expect("per-frame Items")[0]
            .clone();
        object.put(sq(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![frame.clone(), frame.clone(), frame],
        ));
        object.put(ob(tags::PIXEL_DATA, bytes));
    };
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "as if each Frame were padded to a whole byte",
        |object| {
            frames(object, &[0; 6]);
        },
    );
    assert_family_passes("segmentation", SEGMENTATION, |object| {
        frames(object, &[0; 4])
    });
}

#[test]
fn fractional_values_do_not_exceed_the_maximum() {
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "1 Pixel Data (7FE0,0010) values exceed Maximum Fractional Value (0062,000E) 100, the largest 200",
        |object| fractional(object, &[0, 50, 100, 200]),
    );
}

/// An RLE Frame of one segment holding `pixels` as one literal run.
fn rle_frame(pixels: &[u8]) -> Vec<u8> {
    let mut frame = vec![0u8; 64];
    frame[..4].copy_from_slice(&1u32.to_le_bytes());
    frame[4..8].copy_from_slice(&64u32.to_le_bytes());
    frame.push(u8::try_from(pixels.len() - 1).unwrap());
    frame.extend_from_slice(pixels);
    if frame.len() % 2 == 1 {
        // A no-operation byte keeps the segment even.
        frame.push(0x80);
    }
    frame
}

fn deflated(pixels: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(pixels).unwrap();
    let mut frame = encoder.finish().unwrap();
    if frame.len() % 2 == 1 {
        frame.push(0);
    }
    frame
}

fn jpeg_2000(pixels: &[u8]) -> Vec<u8> {
    let samples = j2k::J2kLosslessSamples::new(pixels, 2, 2, 1, 8, false).unwrap();
    let options = j2k::J2kLosslessEncodeOptions::default().with_cpu_only_backend();
    let mut frame = j2k::encode_j2k_lossless(samples, &options)
        .unwrap()
        .codestream;
    if frame.len() % 2 == 1 {
        frame.push(0);
    }
    frame
}

#[test]
fn encapsulated_fractional_values_are_decoded() {
    type Encoder = fn(&[u8]) -> Vec<u8>;
    let encodings: [(&str, Encoder); 3] = [
        ("1.2.840.10008.1.2.5", rle_frame),
        ("1.2.840.10008.1.2.8.1", deflated),
        ("1.2.840.10008.1.2.4.90", jpeg_2000),
    ];
    for (syntax, encode) in encodings {
        for (pixels, status, expected) in [
            ([0, 50, 100, 100], ValidationStatus::Passed, "conformant"),
            (
                [0, 50, 100, 101],
                ValidationStatus::Failed,
                "the largest 101",
            ),
        ] {
            let object = in_syntax("segmentation", syntax, |object| {
                fractional(object, &pixels);
                object.put(encapsulated(vec![encode(&pixels)]));
            });
            assert_status(
                run_segmentation_check(Path::new("seg"), &object, LIMIT),
                status,
                expected,
            );
        }
    }
}

#[test]
fn values_a_validator_cannot_decode_are_reported_unexamined() {
    let object = in_syntax("segmentation", "1.2.840.10008.1.2.4.80", |object| {
        fractional(object, &[0, 50, 100, 200]);
        object.put(encapsulated(vec![vec![0xFF, 0xD8, 0xFF, 0xD9]]));
    });
    assert_status(
        run_segmentation_check(Path::new("seg"), &object, LIMIT),
        ValidationStatus::Passed,
        "not examined: Pixel Data values against Maximum Fractional Value (0062,000E) (Table C.8.20-2): Frame 1: this validator decodes no Frames of Transfer Syntax 1.2.840.10008.1.2.4.80",
    );
    // A source holding stand-ins for the Pixel Data says so.
    let object = in_syntax("segmentation", EXPLICIT_VR_LITTLE_ENDIAN, |object| {
        fractional(object, &[0, 50, 100, 200]);
    });
    assert_status(
        run_segmentation_check(Path::new("seg"), &object, None),
        ValidationStatus::Passed,
        "not examined: the packing and values of Pixel Data (7FE0,0010), which the source does not hold",
    );
}

#[test]
fn a_segmentation_palette_is_lookup_table_data_of_8_or_16_bits() {
    let palette = |segmented: bool, bits: u16| {
        move |object: &mut InMemDicomObject| {
            label_map(object, &[1, 2, 2, 1]);
            object.put(text(
                tags::PHOTOMETRIC_INTERPRETATION,
                VR::CS,
                "PALETTE COLOR",
            ));
            object.put(ob(
                tags::ICC_PROFILE,
                &crate::icc::synthetic_srgb_icc_profile().unwrap(),
            ));
            for (descriptor, data, segmented_data) in [
                (
                    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                ),
                (
                    tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                ),
                (
                    tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                ),
            ] {
                object.put(us(descriptor, &[3, 0, bits]));
                object.put(us(data, &[0, 0, 0]));
                if segmented {
                    object.put(us(segmented_data, &[0, 3, 0, 0, 0]));
                }
            }
        }
    };
    assert_family_passes("segmentation", SEGMENTATION, palette(false, 16));
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "a Segmentation encodes it in Red Palette Color Lookup Table Data",
        palette(true, 16),
    );
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "the number of bits for each entry shall be 8 or 16",
        palette(false, 12),
    );
}

#[test]
fn color_space_names_the_space_of_the_icc_profile() {
    let profile = |space: &'static str| {
        move |object: &mut InMemDicomObject| {
            object.put(ob(
                tags::ICC_PROFILE,
                &crate::icc::synthetic_srgb_icc_profile().unwrap(),
            ));
            object.put(text(tags::COLOR_SPACE, VR::CS, space));
        }
    };
    assert_family_passes("segmentation", SEGMENTATION, profile("SRGB"));
    assert_family_fails(
        "segmentation",
        SEGMENTATION,
        "are not those of DISPLAYP3",
        profile("DISPLAYP3"),
    );
}

#[test]
fn a_patient_plane_has_orthonormal_direction_cosines() {
    assert_family_fails("segmentation", SEGMENTATION, "shall be normal", |object| {
        first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
            shared.put(sq(
                tags::PLANE_ORIENTATION_SEQUENCE,
                vec![item(vec![text(
                    tags::IMAGE_ORIENTATION_PATIENT,
                    VR::DS,
                    "2\\0\\0\\0\\1\\0",
                )])],
            ));
            shared.put(sq(
                tags::PLANE_POSITION_SEQUENCE,
                vec![item(vec![text(
                    tags::IMAGE_POSITION_PATIENT,
                    VR::DS,
                    "0\\0\\0",
                )])],
            ));
        });
    });
}
