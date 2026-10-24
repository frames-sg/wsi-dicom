//! The encoding check over files this crate exports, and its Attribute Value
//! rules over exported files edited through the parser. Byte-level
//! violations are built by hand in src/validation/encoding/tests.rs.

use super::*;
use crate::validation::ValidationCheck;

const ENCODING_RULE: &str = "intrinsic-wsi-dicom-encoding";

fn encoding_checks(path: &Path) -> Vec<ValidationCheck> {
    validate_dicom_path_with_runner(path, &ValidationOptions::default(), &FakeRunner::default())
        .unwrap()
        .checks
        .into_iter()
        .filter(|check| check.name == ENCODING_RULE)
        .collect()
}

fn encoding_check(path: &Path) -> ValidationCheck {
    let mut checks = encoding_checks(path);
    assert_eq!(checks.len(), 1, "{checks:?}");
    checks.remove(0)
}

#[track_caller]
pub(super) fn assert_encoding_failure(path: &Path, expected: &str) {
    let check = encoding_check(path);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(expected),
        "message lacks {expected:?}: {}",
        check.message
    );
}

#[test]
fn exported_files_pass_the_encoding_check() {
    let tmp = tempfile::tempdir().unwrap();
    let grid = export_valid_color_wsi_grid_for_validation(tmp.path(), "grid", 4, 4, 2);
    let check = encoding_check(&grid);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);

    // Every level and associated image of a JPEG Baseline export.
    let source = tmp.path().join("associated.svs");
    crate::test_support::write_aperio_svs_with_associated_images(&source, false);
    let output = tmp.path().join("associated-out");
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: output.clone(),
        options: crate::ExportOptions {
            tile_size: 32,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();
    let checks = encoding_checks(&output);
    assert!(checks.len() > 1, "{checks:?}");
    for check in checks {
        assert_eq!(check.status, ValidationStatus::Passed, "{check:?}");
    }
}

#[test]
fn files_the_parser_rejects_still_get_the_encoding_check() {
    let tmp = tempfile::tempdir().unwrap();
    let path = export_valid_color_wsi_for_validation(tmp.path(), "truncated");
    let bytes = std::fs::read(&path).unwrap();
    // Cut inside the Pixel Data fragments.
    std::fs::write(&path, &bytes[..bytes.len() - 7]).unwrap();
    let check = encoding_check(&path);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
}

#[test]
fn timezone_offset_from_utc_is_encoded_as_signed_hours_and_minutes() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, value, expected) in [
        (
            "leading",
            " +0100",
            "leading space characters shall not be present",
        ),
        ("negative-zero", "-0000", "-0000 shall not be used"),
        ("format", "0100", "it shall be encoded as \"&ZZXX\""),
    ] {
        let path = edited_wsi(tmp.path(), name, |object| {
            object.put(DataElement::new(
                tags::TIMEZONE_OFFSET_FROM_UTC,
                VR::SH,
                value,
            ));
        });
        assert_encoding_failure(&path, expected);
    }
    let path = edited_wsi(tmp.path(), "utc", |object| {
        object.put(DataElement::new(
            tags::TIMEZONE_OFFSET_FROM_UTC,
            VR::SH,
            "+0000",
        ));
    });
    assert_eq!(encoding_check(&path).status, ValidationStatus::Passed);
}

#[test]
fn signature_and_encryption_attributes_follow_their_encoding_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "signature", |object| {
        object.put(DataElement::new(
            tags::DIGITAL_SIGNATURES_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![DataElement::new(
                tags::DIGITAL_SIGNATURE_DATE_TIME,
                VR::DT,
                "20240101120000",
            )])]),
        ));
        object.put(DataElement::new(
            tags::MAC_PARAMETERS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![DataElement::new(
                tags::MAC_CALCULATION_TRANSFER_SYNTAX_UID,
                VR::UI,
                "1.2.840.10008.1.2",
            )])]),
        ));
        object.put(DataElement::new(
            tags::ENCRYPTED_ATTRIBUTES_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![DataElement::new(
                tags::ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID,
                VR::UI,
                "1.2.840.10008.1.2.2",
            )])]),
        ));
    });
    assert_encoding_failure(
        &path,
        "DigitalSignatureDateTime (0400,0105) (value withheld: identifying; 14 characters): no &ZZXX offset from UTC",
    );
    assert_encoding_failure(
        &path,
        "1.2.840.10008.1.2 is an Implicit VR Transfer Syntax; only Explicit VR Little Endian Transfer Syntaxes shall be used (PS3.3 Table C.12-6)",
    );
    assert_encoding_failure(
        &path,
        "1.2.840.10008.1.2.2 is a Big Endian Transfer Syntax; only Explicit VR Little Endian Transfer Syntaxes shall be used (PS3.3 Table C.12-1)",
    );
}

#[test]
fn private_data_element_characteristics_encode_counts_and_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let definition = |vr: &str, multiplicity: Vec<u32>, items: Vec<u32>| {
        let mut elements = vec![
            DataElement::new(
                tags::PRIVATE_DATA_ELEMENT_VALUE_MULTIPLICITY,
                VR::UL,
                PrimitiveValue::U32(multiplicity.into()),
            ),
            DataElement::new(tags::PRIVATE_DATA_ELEMENT_VALUE_REPRESENTATION, VR::CS, vr),
        ];
        if !items.is_empty() {
            elements.push(DataElement::new(
                tags::PRIVATE_DATA_ELEMENT_NUMBER_OF_ITEMS,
                VR::UL,
                PrimitiveValue::U32(items.into()),
            ));
        }
        item(elements)
    };
    let path = edited_wsi(tmp.path(), "private-characteristics", |object| {
        object.put(DataElement::new(
            tags::PRIVATE_DATA_ELEMENT_CHARACTERISTICS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![
                DataElement::new(
                    tags::IDENTIFYING_PRIVATE_ELEMENTS,
                    VR::US,
                    PrimitiveValue::U16(vec![0x1002, 0x1001].into()),
                ),
                DataElement::new(
                    tags::NONIDENTIFYING_PRIVATE_ELEMENTS,
                    VR::US,
                    PrimitiveValue::U16(vec![0x1003, 0x1003].into()),
                ),
                DataElement::new(
                    tags::PRIVATE_DATA_ELEMENT_DEFINITION_SEQUENCE,
                    VR::SQ,
                    DataSetSequence::from(vec![
                        definition("LO", vec![2, 2], Vec::new()),
                        definition("SQ", vec![1, 0], vec![3, 3]),
                    ]),
                ),
            ])]),
        ));
    });
    for expected in [
        "IdentifyingPrivateElements (0008,0306): Value 1001H follows 1002H",
        "NonidentifyingPrivateElements (0008,0304): Value 1003H follows 1003H",
        "[2, 2]: a fixed multiplicity shall be a single integer Value (PS3.3 C.12.1.1.7.1)",
        "[1, 0]: for a Private Data Element Value Representation of SQ the multiplicity shall be 1",
        "[3, 3]: a fixed number of Items shall be a single integer Value (PS3.3 C.12.1.1.7.2)",
    ] {
        assert_encoding_failure(&path, expected);
    }
    let conformant = edited_wsi(tmp.path(), "private-characteristics-ok", |object| {
        object.put(DataElement::new(
            tags::PRIVATE_DATA_ELEMENT_CHARACTERISTICS_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![DataElement::new(
                tags::PRIVATE_DATA_ELEMENT_DEFINITION_SEQUENCE,
                VR::SQ,
                DataSetSequence::from(vec![
                    definition("LO", vec![1, 3], Vec::new()),
                    definition("SQ", vec![1], vec![1, 0]),
                ]),
            )])]),
        ));
    });
    assert_eq!(encoding_check(&conformant).status, ValidationStatus::Passed);
}

#[test]
fn privately_defined_items_do_not_use_the_dicom_root() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "dicom-root", |object| {
        object.put(DataElement::new(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.840.10008.99.1",
        ));
    });
    assert_encoding_failure(
        &path,
        "SeriesInstanceUID (0020,000E) (value withheld: identifying; 18 characters): the root 1.2.840.10008 is reserved for DICOM defined items",
    );
    // A well-known Frame of Reference of PS3.6 Annex A is DICOM defined.
    let atlas = edited_wsi(tmp.path(), "atlas", |object| {
        object.put(DataElement::new(
            tags::FRAME_OF_REFERENCE_UID,
            VR::UI,
            "1.2.840.10008.1.4.1.1",
        ));
    });
    assert_eq!(encoding_check(&atlas).status, ValidationStatus::Passed);
}

#[test]
fn a_veterinary_patient_name_has_two_components() {
    let tmp = tempfile::tempdir().unwrap();
    let species = |code: &str| {
        DataElement::new(
            tags::PATIENT_SPECIES_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![
                DataElement::new(tags::CODE_VALUE, VR::SH, code),
                DataElement::new(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "SCT"),
                DataElement::new(tags::CODE_MEANING, VR::LO, "species"),
            ])]),
        )
    };
    let dog = edited_wsi(tmp.path(), "dog", |object| {
        object.put(species("448771007"));
        object.put(DataElement::new(
            tags::PATIENT_NAME,
            VR::PN,
            "Smith^Fluffy^Middle",
        ));
    });
    assert_encoding_failure(
        &dog,
        "PatientName (0010,0010) (value withheld: identifying; 19 characters): component group 1 of a veterinary Patient's Name has 3 components",
    );
    let human = edited_wsi(tmp.path(), "human", |object| {
        object.put(species("337915000"));
        object.put(DataElement::new(tags::PATIENT_NAME, VR::PN, "Doe^John^Q"));
    });
    assert_eq!(encoding_check(&human).status, ValidationStatus::Passed);
}

#[test]
fn lookup_table_descriptors_are_little_endian() {
    let tmp = tempfile::tempdir().unwrap();
    let path = edited_wsi(tmp.path(), "descriptor", |object| {
        object.put(DataElement::new(
            tags::ICON_IMAGE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![
                DataElement::new(
                    tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
                    VR::US,
                    PrimitiveValue::U16(vec![0x0001, 0, 0x1000].into()),
                ),
                DataElement::new(
                    tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    VR::OW,
                    // A discrete segment of two entries, then a linear one,
                    // byte-swapped.
                    PrimitiveValue::U16(
                        [0, 2, 10, 20, 1, 4, 255]
                            .map(u16::swap_bytes)
                            .to_vec()
                            .into(),
                    ),
                ),
                DataElement::new(
                    tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
                    VR::OW,
                    PrimitiveValue::U16(vec![0, 2, 10, 20, 1, 4, 255].into()),
                ),
            ])]),
        ));
    });
    assert_encoding_failure(
        &path,
        "RedPaletteColorLookupTableDescriptor (0028,1101): third Value 4096 is 16 byte-swapped, so the Descriptor is encoded Big Endian; it shall be encoded in Little Endian (PS3.5 A.4)",
    );
    assert_encoding_failure(
        &path,
        "SegmentedGreenPaletteColorLookupTableData (0028,1222): the segments parse only byte-swapped",
    );
    let check = encoding_check(&path);
    assert!(!check.message.contains("(0028,1223)"), "{}", check.message);
}

/// The status and message of check `rule` for the object at `path`.
fn named_check(path: &Path, rule: &str) -> ValidationCheck {
    validate_dicom_path_with_runner(path, &ValidationOptions::default(), &FakeRunner::default())
        .unwrap()
        .checks
        .into_iter()
        .find(|check| check.name == rule)
        .unwrap_or_else(|| panic!("missing check {rule}"))
}

#[test]
fn waveform_and_extended_lookup_data_keep_their_value_representations() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = |tag, vr| DataElement::new(tag, vr, PrimitiveValue::U8(vec![0; 8].into()));
    let path = edited_wsi(tmp.path(), "waveform", |object| {
        object.put(DataElement::new(
            tags::WAVEFORM_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![item(vec![
                DataElement::new(
                    tags::WAVEFORM_BITS_ALLOCATED,
                    VR::US,
                    PrimitiveValue::from(16u16),
                ),
                binary(tags::WAVEFORM_PADDING_VALUE, VR::OW),
                binary(tags::WAVEFORM_DATA, VR::OB),
            ])]),
        ));
        // Standard Type 3 Attributes beyond the IOD (Standard Extended SOP
        // Class), in the wrong VR.
        object.put(binary(tags::BLENDING_LOOKUP_TABLE_DATA, VR::OB));
        object.put(binary(tags::TRACK_POINT_INDEX_LIST, VR::OW));
    });
    assert_encoding_failure(
        &path,
        "WaveformSequence (5400,0100)[1] > WaveformData (5400,1010): VR OB with Waveform Bits Allocated (5400,1004) 16; Waveform Data shall be OW",
    );
    assert_encoding_failure(
        &path,
        "WaveformPaddingValue (5400,100A): VR OW where Waveform Data (5400,1010) is OB",
    );
    let vr_vm = named_check(&path, "intrinsic-wsi-dicom-iod-vr-vm");
    assert_eq!(vr_vm.status, ValidationStatus::Failed, "{}", vr_vm.message);
    for expected in [
        "BlendingLookupTableData (0028,1408): VR OB where PS3.6 specifies OW",
        "TrackPointIndexList (0066,0129): VR OW where PS3.6 specifies OL",
    ] {
        assert!(
            vr_vm.message.contains(expected),
            "{expected}: {}",
            vr_vm.message
        );
    }
}

#[test]
fn data_set_trailing_padding_is_accepted_at_any_level_and_its_value_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let padding = || {
        DataElement::new(
            tags::DATA_SET_TRAILING_PADDING,
            VR::OB,
            PrimitiveValue::U8(vec![0xDE, 0xAD, 0xBE, 0xEF].into()),
        )
    };
    let path = edited_wsi(tmp.path(), "padding", |object| {
        object.put(padding());
        let mut paths = object.take(tags::OPTICAL_PATH_SEQUENCE).unwrap();
        paths.items_mut().unwrap()[0].put(padding());
        object.put(paths);
    });
    for rule in [
        ENCODING_RULE,
        "intrinsic-wsi-dicom-iod-vr-vm",
        "intrinsic-wsi-dicom-iod-value-format",
        "intrinsic-pixel-structure",
    ] {
        let check = named_check(&path, rule);
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{rule}: {}",
            check.message
        );
    }
}
