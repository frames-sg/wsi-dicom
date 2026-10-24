//! The intrinsic rules that hold VL WSI objects to statements their IODs
//! share also hold the objects of the other pathology-slide IODs: each valid
//! fixture passes every one of them, and breaking a representative statement
//! of each rule fails it in each IOD family.

use dicom_core::value::{PixelFragmentSequence, Value};
use dicom_dictionary_std::uids;

use super::*;
use crate::validation::wsi_conformance::rule_iods;

/// The rules extended from VL WSI to the IODs they apply to.
const SHARED_RULES: [&str; 10] = [
    "intrinsic-wsi-dicom-icc-profile",
    "intrinsic-wsi-dicom-lossy-history",
    "intrinsic-wsi-dicom-dimension-order",
    "intrinsic-wsi-dicom-identity-set",
    "intrinsic-wsi-dicom-image-metadata",
    "intrinsic-wsi-dicom-wsi-image",
    "intrinsic-wsi-dicom-attribute-relations",
    "intrinsic-wsi-dicom-pixel-attributes",
    "intrinsic-wsi-dicom-codestream",
    "intrinsic-pixel-structure",
];
/// The set rules that compare the objects of every IOD.
const SET_RULES: [&str; 3] = [
    "intrinsic-wsi-dicom-identity-set",
    "intrinsic-wsi-dicom-series-set",
    "intrinsic-wsi-dicom-specimen-uid-set",
];
const IMAGE_METADATA: &str = "intrinsic-wsi-dicom-image-metadata";
const CODESTREAM: &str = "intrinsic-wsi-dicom-codestream";
const RELATIONS: &str = "intrinsic-wsi-dicom-attribute-relations";
const PIXEL_ATTRIBUTES: &str = "intrinsic-wsi-dicom-pixel-attributes";
const ICC: &str = "intrinsic-wsi-dicom-icc-profile";
const DIMENSION: &str = "intrinsic-wsi-dicom-dimension-order";
const LOSSY: &str = "intrinsic-wsi-dicom-lossy-history";
const WSI_IMAGE: &str = "intrinsic-wsi-dicom-wsi-image";
const SERIES_SET: &str = "intrinsic-wsi-dicom-series-set";
const SPECIMEN_SET: &str = "intrinsic-wsi-dicom-specimen-uid-set";
const IDENTITY_SET: &str = "intrinsic-wsi-dicom-identity-set";

/// The PS3.3 section of each fixture's IOD.
fn iod_of(name: &str) -> &'static str {
    match name {
        "annotations" => "A.87",
        "segmentation" => "A.51",
        "parametric-map" => "A.75",
        other => panic!("no fixture {other}"),
    }
}

/// Writes `object` as `name`.dcm in Transfer Syntax `syntax`.
fn write_in(dir: &Path, name: &str, object: InMemDicomObject, syntax: &str) -> PathBuf {
    let path = dir.join(format!("{name}.dcm"));
    object
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(syntax))
        .expect("file meta")
        .write_to_file(&path)
        .expect("write fixture");
    path
}

/// Fixture `name` edited by `edit`, without writing it.
fn edited(name: &str, edit: impl FnOnce(&mut InMemDicomObject)) -> InMemDicomObject {
    let mut object = fixture(name);
    edit(&mut object);
    object
}

fn put_text(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(text(tag, vr, value));
}

fn put_us(object: &mut InMemDicomObject, tag: Tag, value: u16) {
    object.put(us(tag, &[value]));
}

/// Replaces the pixel element with one encapsulated Fragment.
fn encapsulate(object: &mut InMemDicomObject, fragment: Vec<u8>) {
    object.remove_element(tags::FLOAT_PIXEL_DATA);
    object.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        Value::PixelSequence(PixelFragmentSequence::new(
            Vec::<u32>::new(),
            vec![fragment],
        )),
    ));
}

/// An RLE Frame of a 2 x 2 image of one byte per sample whose header
/// declares `segments` segments, each a literal run of two bytes per row.
fn rle_frame(segments: u32) -> Vec<u8> {
    let segment = [0x01, 0x10, 0x20, 0x01, 0x30, 0x40];
    let mut frame = segments.to_le_bytes().to_vec();
    for index in 0..15u32 {
        let offset = if index < segments {
            64 + index * segment.len() as u32
        } else {
            0
        };
        frame.extend_from_slice(&offset.to_le_bytes());
    }
    for _ in 0..segments {
        frame.extend_from_slice(&segment);
    }
    frame
}

/// A Parametric Map of 16-bit integer Pixel Data instead of float.
fn integer_parametric_map() -> InMemDicomObject {
    edited("parametric-map", |object| {
        object.remove_element(tags::FLOAT_PIXEL_DATA);
        put_us(object, tags::BITS_ALLOCATED, 16);
        put_us(object, tags::BITS_STORED, 16);
        put_us(object, tags::HIGH_BIT, 15);
        put_us(object, tags::PIXEL_REPRESENTATION, 0);
        object.put(ob(tags::PIXEL_DATA, &[0; 8]));
    })
}

fn invalid_icc_profile() -> Vec<u8> {
    let mut profile = crate::icc::synthetic_srgb_icc_profile().unwrap();
    profile[12..16].copy_from_slice(b"mntr");
    profile
}

#[track_caller]
fn assert_fails_with(path: &Path, rule: &str, expected: &str) {
    assert_iod_failure(path, rule, expected);
}

#[test]
fn every_shared_rule_passes_every_valid_fixture_and_runs_where_the_catalog_says() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, build) in FIXTURES {
        let path = write(tmp.path(), name, build(), |_| {});
        let report = iod_report(&path, ValidationProfile::General);
        let iod = iod_of(name);
        for rule in SHARED_RULES {
            let checks: Vec<&ValidationCheck> = report
                .checks
                .iter()
                .filter(|check| check.name == rule)
                .collect();
            assert_eq!(
                !checks.is_empty(),
                rule_iods(rule).contains(&iod),
                "{name}: {rule} runs where rule_iods says"
            );
            for check in checks {
                assert_ne!(
                    check.status,
                    ValidationStatus::Failed,
                    "{name} {rule}: {}",
                    check.message
                );
            }
        }
    }
    // Each fixture as a set of its own: each set rule compares it and passes.
    for (name, build) in FIXTURES {
        let set = tmp.path().join(format!("{name}-set"));
        std::fs::create_dir(&set).unwrap();
        write(&set, name, build(), |_| {});
        let report = iod_report(&set, ValidationProfile::General);
        for rule in SET_RULES {
            let check = report
                .checks
                .iter()
                .find(|check| check.name == rule && check.path.is_none())
                .unwrap_or_else(|| panic!("{name}: missing set check {rule}"));
            assert_eq!(
                check.status,
                ValidationStatus::Passed,
                "{name} {rule}: {}",
                check.message
            );
        }
    }
}

/// One edit of a fixture, the rule it breaks and the message expected.
type Case = (
    &'static str,
    &'static str,
    fn(&mut InMemDicomObject),
    &'static str,
);

#[test]
fn breaking_a_shared_statement_fails_its_rule_in_each_iod_family() {
    let cases: [Case; 20] = [
        // PS3.5 8.1.1.
        ("segmentation", IMAGE_METADATA, |object| put_us(object, tags::BITS_ALLOCATED, 12),
            "Bits Allocated (0028,0100) is 12; it shall be either 1 or a multiple of 8"),
        ("parametric-map", IMAGE_METADATA, |object| put_us(object, tags::PIXEL_REPRESENTATION, 2),
            "Pixel Representation (0028,0103) is 2"),
        // PS3.3 Table C.7-5a, Table 10-3 and C.7-9.
        ("annotations", RELATIONS, |object| {
            put_text(object, tags::OPERATORS_NAME, VR::PN, "One^A\\Two^B");
            object.put(sq(tags::OPERATOR_IDENTIFICATION_SEQUENCE, vec![
                item(vec![]), item(vec![]), item(vec![]),
            ]));
        }, "their number and order shall correspond"),
        ("segmentation", RELATIONS, |object| {
            object.put(sq(tags::REFERENCED_IMAGE_SEQUENCE, vec![item(vec![
                text(tags::REFERENCED_FRAME_NUMBER, VR::IS, "0"),
            ])]));
        }, "Referenced Frame Number (0008,1160) 0 is below 1"),
        ("parametric-map", RELATIONS, |object| {
            put_text(object, tags::LATERALITY, VR::CS, "R");
            put_text(object, tags::IMAGE_LATERALITY, VR::CS, "L");
        }, "Image Laterality (0020,0062) is L but Laterality (0020,0060) is R"),
        // PS3.3 C.7.5.1.1.2 and C.7.6.1.1.6.
        ("segmentation", PIXEL_ATTRIBUTES, |object| put_us(object, tags::PIXEL_PADDING_VALUE, 5),
            "lies outside the stored sample range 0..=1"),
        ("parametric-map", PIXEL_ATTRIBUTES, |object| {
            object.put(sq(tags::ICON_IMAGE_SEQUENCE, vec![item(vec![us(tags::SAMPLES_PER_PIXEL, &[3])])]));
        }, "Icon Images have a single sample per pixel"),
        // ICC Profiles of every IOD follow C.11.15.1.1.
        ("annotations", ICC, |object| {
            object.put(ob(tags::ICC_PROFILE, &invalid_icc_profile()));
        }, "the top-level Data Set has an invalid ICC Profile"),
        ("segmentation", ICC, |object| {
            object.put(ob(tags::ICC_PROFILE, &invalid_icc_profile()));
        }, "input device ('scnr')"),
        ("parametric-map", ICC, |object| {
            object.put(ob(tags::ICC_PROFILE, &invalid_icc_profile()));
        }, "input device ('scnr')"),
        // PS3.3 C.7.6.17.1 and C.7.6.16.2.2.
        ("segmentation", DIMENSION, |object| {
            first_item(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, |frame| {
                first_item(frame, tags::FRAME_CONTENT_SEQUENCE, |content| {
                    content.put(ul(tags::DIMENSION_INDEX_VALUES, &[0]));
                });
            });
        }, "have a Dimension Index Value of 0"),
        ("parametric-map", DIMENSION, |object| {
            edit_items(object, tags::DIMENSION_INDEX_SEQUENCE, |items| {
                items[0].put(at(tags::DIMENSION_INDEX_POINTER, &[tags::FRAME_CONTENT_SEQUENCE]));
            });
        }, "shall not name Frame Content Sequence or Dimension Index Values"),
        // PS3.3 C.7.6.1.1.5.1 and C.7.6.1.1.5.2.
        ("segmentation", LOSSY, |object| {
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION_METHOD, VR::CS, "ISO_10918_1\\ISO_10918_1");
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, "10");
        }, "lossy method and ratio histories have different value multiplicities"),
        ("parametric-map", LOSSY, |object| {
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION_METHOD, VR::CS, "ISO_10918_1");
            put_text(object, tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, "0");
        }, "must contain positive finite values"),
        // PS3.3 C.7.4.1.1.2, 10.7.1.3, C.7.6.16-2 and C.7.6.1.1.5.
        ("annotations", WSI_IMAGE, |object| {
            put_text(object, tags::POSITION_REFERENCE_INDICATOR, VR::LO, "XY");
        }, "a slide-related Frame of Reference is identified by SLIDE_CORNER"),
        ("segmentation", WSI_IMAGE, |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(sq(tags::PIXEL_MEASURES_SEQUENCE, vec![item(vec![
                    text(tags::SPACING_BETWEEN_SLICES, VR::DS, "-1"),
                ])]));
            });
        }, "Spacing Between Slices (0018,0088) is -1; it shall not be negative"),
        ("segmentation", WSI_IMAGE, |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(sq(tags::PIXEL_MEASURES_SEQUENCE, vec![item(vec![
                    text(tags::PIXEL_SPACING, VR::DS, "0\\0.5"),
                ])]));
            });
        }, "Pixel Spacing (0028,0030) Value 1 is 0; pixel spacing shall be positive"),
        ("parametric-map", WSI_IMAGE, |object| {
            object.put(sq(tags::DERIVATION_CODE_SEQUENCE, vec![code("DCM", "113040", "Lossy Compression")]));
        }, "declares (113040, DCM, \"Lossy Compression\") but Lossy Image Compression (0028,2110) is not 01"),
        ("parametric-map", WSI_IMAGE, |object| {
            let own = object.element(tags::SOP_INSTANCE_UID).unwrap().to_str().unwrap().to_string();
            object.put(sq(tags::SOURCE_IMAGE_SEQUENCE, vec![item(vec![
                text(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, &own),
            ])]));
        }, "names this SOP Instance as its own source"),
        ("annotations", WSI_IMAGE, |object| {
            put_text(object, tags::POSITION_REFERENCE_INDICATOR, VR::LO, "PATIENT");
        }, "is \"PATIENT\"; a slide-related Frame of Reference"),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (index, (name, rule, edit, expected)) in cases.into_iter().enumerate() {
        let path = broken(tmp.path(), name, &format!("rule-{index}"), edit);
        assert_fails_with(&path, rule, expected);
    }
}

#[test]
fn a_parametric_map_palette_has_sixteen_bit_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let path = broken(tmp.path(), "parametric-map", "palette", |object| {
        for (descriptor, data) in [
            (
                tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            ),
            (
                tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            ),
            (
                tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            ),
        ] {
            object.put(us(descriptor, &[256, 0, 8]));
            object.put(ob(data, &[0; 256]));
        }
    });
    assert_fails_with(
        &path,
        PIXEL_ATTRIBUTES,
        "a Palette Color Lookup Table Module in an Image IOD has 16-bit entries",
    );
}

#[test]
fn an_rle_header_out_of_step_with_the_samples_fails_the_codestream_rule_in_each_image_family() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, object, segments, bits) in [
        ("segmentation", fixture("segmentation"), 2, 1u32),
        ("parametric-map", integer_parametric_map(), 1, 16),
    ] {
        let mut object = object;
        encapsulate(&mut object, rle_frame(segments));
        let path = write_in(
            tmp.path(),
            &format!("{name}-rle"),
            object,
            uids::RLE_LOSSLESS,
        );
        let expected = bits.div_ceil(8);
        assert_fails_with(
            &path,
            CODESTREAM,
            &format!("RLE Header declares {segments} RLE Segments but Samples per Pixel (0028,0002) 1 with Bits Allocated (0028,0100) {bits} makes {expected} Byte Segments"),
        );
    }
}

#[test]
fn a_lossy_transfer_syntax_makes_a_segmentation_or_parametric_map_lossy() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, object) in [
        ("segmentation", fixture("segmentation")),
        ("parametric-map", integer_parametric_map()),
    ] {
        let mut object = object;
        encapsulate(&mut object, vec![0xFF, 0xD8, 0xFF, 0xD9]);
        let path = write_in(
            tmp.path(),
            &format!("{name}-jpeg"),
            object,
            uids::JPEG_BASELINE8_BIT,
        );
        assert_fails_with(
            &path,
            LOSSY,
            "Lossy Image Compression is 00, but the Frames are encoded in a JPEG lossy (DCT) Transfer Syntax",
        );
    }
}

#[test]
fn a_motion_syntax_fixes_the_attributes_of_each_image_iod() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, syntax, expected) in [
        // MPEG2 MP@ML admits single component data of 8 bits, at most 576
        // Rows (Table 8.2.5-1 and Table 8-1).
        (
            "segmentation",
            uids::MPEG2MPML,
            "Rows 600 (at most 576, Table 8-1)",
        ),
        (
            "segmentation",
            uids::MPEG2MPML,
            "Bits Allocated 1 (shall be 8)",
        ),
        // H.264 requires three samples, which a Segmentation never has.
        (
            "segmentation",
            uids::MPEG4HP41,
            "Samples per Pixel 1 (shall be 3)",
        ),
        // HEVC Main 10 requires Bits Allocated 16, Bits Stored 10, High Bit 9.
        (
            "parametric-map",
            uids::HEVCM10P51,
            "Bits Stored 16 (shall be 10)",
        ),
    ] {
        let mut object = match name {
            "parametric-map" => integer_parametric_map(),
            _ => fixture(name),
        };
        put_us(&mut object, tags::ROWS, 600);
        encapsulate(&mut object, vec![0, 0, 1, 0xB3]);
        let path = write_in(
            tmp.path(),
            &format!("{name}-{}", syntax.replace('.', "-")),
            object,
            syntax,
        );
        assert_fails_with(&path, IMAGE_METADATA, expected);
    }
}

#[test]
fn a_segmentation_or_parametric_map_in_a_video_syntax_is_an_advisory() {
    let tmp = tempfile::tempdir().unwrap();
    // A FRACTIONAL Segmentation of 8-bit samples holds the attributes MPEG2
    // MP@ML fixes for single component data, so only the advisory remains.
    let mut segmentation = edited("segmentation", |object| {
        put_text(object, tags::SEGMENTATION_TYPE, VR::CS, "FRACTIONAL");
        put_us(object, tags::BITS_ALLOCATED, 8);
        put_us(object, tags::BITS_STORED, 8);
        put_us(object, tags::HIGH_BIT, 7);
    });
    encapsulate(&mut segmentation, vec![0, 0, 1, 0xB3]);
    // A Parametric Map departs from the attributes of every video syntax,
    // and gets the advisory beside its failure.
    let mut parametric_map = integer_parametric_map();
    encapsulate(&mut parametric_map, vec![0, 0, 0, 1]);
    for (name, object, syntax, status, advice) in [
        (
            "segmentation",
            segmentation,
            uids::MPEG2MPML,
            ValidationStatus::Passed,
            "BINARY or LABELMAP Segmentation Instances should not be lossy compressed (PS3.3 {E} C.8.20.2.2)",
        ),
        (
            "parametric-map",
            parametric_map,
            uids::HEVCMP51,
            ValidationStatus::Failed,
            "it is not advisable to lossy compress a Parametric Map SOP Instance, since that will alter the quantitative values that are the intent of the object (PS3.3 {E} C.8.32.2.1)",
        ),
    ] {
        let path = write_in(tmp.path(), &format!("{name}-video"), object, syntax);
        let report = iod_report(&path, ValidationProfile::General);
        let check = report
            .checks
            .iter()
            .find(|check| check.name == IMAGE_METADATA)
            .unwrap_or_else(|| panic!("{name}: no {IMAGE_METADATA}"));
        let advice = advice.replace("{E}", crate::validation::DICOM_EDITION);
        assert_eq!(check.status, status, "{name}: {}", check.message);
        assert!(
            check.message.contains("advisory: Transfer Syntax")
                && check.message.contains("which is not validated: a pathology Segmentation or Parametric Map is not video")
                && check.message.contains(&advice),
            "{name}: {}",
            check.message
        );
        assert!(
            report
                .optional
                .iter()
                .any(|entry| entry.advisories.iter().any(|advisory| advisory.contains(&advice))),
            "{name}: {:?}",
            report.optional
        );
    }
}

/// Writes fixture `name` twice into `dir` as two objects of one set, the
/// second with a new SOP Instance UID and `edit` applied.
fn pair(dir: &Path, name: &str, edit: impl FnOnce(&mut InMemDicomObject)) {
    write(dir, &format!("{name}-1"), fixture(name), |_| {});
    let mut second = fixture(name);
    put_text(
        &mut second,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999.77.9",
    );
    edit(&mut second);
    write(dir, &format!("{name}-2"), second, |_| {});
}

fn set_check(dir: &Path, rule: &str) -> ValidationCheck {
    iod_report(dir, ValidationProfile::General)
        .checks
        .into_iter()
        .find(|check| check.name == rule && check.path.is_none())
        .unwrap_or_else(|| panic!("missing set check {rule}"))
}

#[track_caller]
fn assert_set_fails(dir: &Path, rule: &str, expected: &str) {
    let check = set_check(dir, rule);
    assert_eq!(
        check.status,
        ValidationStatus::Failed,
        "{rule}: {}",
        check.message
    );
    assert!(
        check.message.contains(expected),
        "{rule}: {}",
        check.message
    );
}

/// A fixture, the edit of its second copy, and the series-set failure the
/// pair causes.
type SeriesCase = (&'static str, fn(&mut InMemDicomObject), &'static str);

#[test]
fn the_series_relations_hold_in_each_iod_family() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: [SeriesCase; 4] = [
        (
            "parametric-map",
            |object| {
                put_text(
                    object,
                    tags::FRAME_OF_REFERENCE_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.77.5",
                );
                put_text(object, tags::INSTANCE_NUMBER, VR::IS, "2");
            },
            "each Series has a single one",
        ),
        (
            "segmentation",
            |object| {
                let series = object
                    .element(tags::SERIES_INSTANCE_UID)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                put_text(object, tags::SOP_INSTANCE_UID, VR::UI, &series);
                put_text(object, tags::INSTANCE_NUMBER, VR::IS, "2");
            },
            "UIDs identify one entity",
        ),
        (
            "annotations",
            |object| {
                put_text(
                    object,
                    tags::STUDY_INSTANCE_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.77.1",
                );
                put_text(object, tags::INSTANCE_NUMBER, VR::IS, "2");
            },
            "belongs to Studies",
        ),
        (
            "parametric-map",
            |object| {
                put_text(object, tags::INSTANCE_NUMBER, VR::IS, "1");
            },
            "Instance Number 1 is used by",
        ),
    ];
    for (index, (name, edit, expected)) in cases.into_iter().enumerate() {
        let dir = tmp.path().join(format!("set-{index}"));
        std::fs::create_dir(&dir).unwrap();
        pair(&dir, name, edit);
        assert_set_fails(&dir, SERIES_SET, expected);
    }
}

#[test]
fn a_specimen_uid_names_one_specimen_in_each_iod_family() {
    let tmp = tempfile::tempdir().unwrap();
    let specimen = |identifier: &str| {
        sq(
            tags::SPECIMEN_DESCRIPTION_SEQUENCE,
            vec![item(vec![
                text(tags::SPECIMEN_IDENTIFIER, VR::LO, identifier),
                text(
                    tags::SPECIMEN_UID,
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.77.2",
                ),
                sq(tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE, vec![]),
                sq(tags::SPECIMEN_PREPARATION_SEQUENCE, vec![]),
            ])],
        )
    };
    for name in ["annotations", "segmentation", "parametric-map"] {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        let mut first = fixture(name);
        first.put(specimen("SPECIMEN-A"));
        write(&dir, "first", first, |_| {});
        let mut second = fixture(name);
        put_text(
            &mut second,
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.77.9",
        );
        put_text(&mut second, tags::INSTANCE_NUMBER, VR::IS, "2");
        second.put(specimen("SPECIMEN-B"));
        write(&dir, "second", second, |_| {});
        assert_set_fails(
            &dir,
            SPECIMEN_SET,
            "maps to conflicting identifiers or issuers",
        );
    }
}

/// A derived image of `name` whose Source Image Sequence names a Parametric
/// Map, written with that source declaring "01".
fn derived_from_lossy_source(dir: &Path, name: &str) {
    let mut source = integer_parametric_map();
    put_text(
        &mut source,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        "1.2.826.0.1.3680043.10.999.77.7",
    );
    put_text(&mut source, tags::INSTANCE_NUMBER, VR::IS, "7");
    put_text(&mut source, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, "01");
    put_text(
        &mut source,
        tags::LOSSY_IMAGE_COMPRESSION_METHOD,
        VR::CS,
        "ISO_10918_1",
    );
    put_text(
        &mut source,
        tags::LOSSY_IMAGE_COMPRESSION_RATIO,
        VR::DS,
        "10",
    );
    write(dir, "source", source, |_| {});
    let mut derived = fixture(name);
    derived.put(sq(
        tags::SOURCE_IMAGE_SEQUENCE,
        vec![item(vec![
            text(
                tags::REFERENCED_SOP_CLASS_UID,
                VR::UI,
                "1.2.840.10008.5.1.4.1.1.30",
            ),
            text(
                tags::REFERENCED_SOP_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.77.7",
            ),
        ])],
    ));
    write(dir, "derived", derived, |_| {});
}

#[test]
fn a_segmentation_or_parametric_map_of_a_lossy_source_is_lossy() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, what, section) in [
        ("segmentation", "Segmentation", "C.8.20.2.2"),
        ("parametric-map", "Parametric Map", "C.8.32.2.1"),
    ] {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        derived_from_lossy_source(&dir, name);
        assert_set_fails(
            &dir,
            SERIES_SET,
            &format!("the Value shall be \"01\" for a {what} Instance whose source images are (PS3.3 {} {section})", crate::validation::DICOM_EDITION),
        );
    }
}

#[test]
fn files_of_one_sop_instance_are_copies_or_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let case = |name: &str, edit: fn(&mut InMemDicomObject)| {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        write(&dir, "first", fixture("segmentation"), |_| {});
        write(&dir, "second", fixture("segmentation"), edit);
        set_check(&dir, IDENTITY_SET)
    };
    let identical = case("identical", |_| {});
    assert_eq!(
        identical.status,
        ValidationStatus::Passed,
        "{}",
        identical.message
    );
    assert!(
        identical.message.contains("identical copies"),
        "{}",
        identical.message
    );

    let class = case("class", |object| {
        put_text(
            object,
            tags::SOP_CLASS_UID,
            VR::UI,
            "1.2.840.10008.5.1.4.1.1.66.7",
        );
    });
    assert_eq!(class.status, ValidationStatus::Failed, "{}", class.message);
    assert!(
        class.message.contains("they are of SOP Classes"),
        "{}",
        class.message
    );

    let pixels = case("pixels", |object| {
        object.put(ob(tags::PIXEL_DATA, &[6, 0]));
    });
    assert_eq!(
        pixels.status,
        ValidationStatus::Failed,
        "{}",
        pixels.message
    );
    assert!(
        pixels
            .message
            .contains("their Pixel Data differs in the same Transfer Syntax"),
        "{}",
        pixels.message
    );

    let coerced = case("coerced", |object| {
        put_text(object, tags::PATIENT_ID, VR::LO, "P2");
    });
    assert_eq!(
        coerced.status,
        ValidationStatus::Passed,
        "{}",
        coerced.message
    );
    assert!(
        coerced.message.contains("advisory") && coerced.message.contains("PatientID (0010,0020)"),
        "{}",
        coerced.message
    );
    let report = iod_report(&tmp.path().join("coerced"), ValidationProfile::General);
    assert!(report.optional.iter().any(|entry| entry
        .advisories
        .iter()
        .any(|advisory| advisory.contains("PS3.4"))));
}

#[test]
fn a_file_in_an_smpte_st2110_transfer_syntax_fails() {
    let tmp = tempfile::tempdir().unwrap();
    for syntax in [
        "1.2.840.10008.1.2.7.1",
        "1.2.840.10008.1.2.7.2",
        "1.2.840.10008.1.2.7.3",
    ] {
        let path = write_in(
            tmp.path(),
            &syntax.replace('.', "-"),
            fixture("segmentation"),
            syntax,
        );
        assert_fails_with(
            &path,
            "intrinsic-pixel-structure",
            "describes a DICOM-RTV Flow, carried outside any file, and defines no encoding of a Data Set",
        );
    }
}
