//! The pathology-slide IODs besides VL Whole Slide Microscopy: an object of
//! each IOD, built here, passes every IOD check, and breaking one Attribute
//! fails the check of each family. The same objects, written by
//! `write_pathology_fixtures`, have no dciodvfy errors and no validate_iods
//! errors where those validators know the IOD.

use super::*;

mod rules;

const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
/// Directory `write_pathology_fixtures` writes the objects to.
const FIXTURE_DIR_VARIABLE: &str = "WSI_DICOM_IOD_FIXTURES";

fn us(tag: Tag, values: &[u16]) -> InMemElement {
    DataElement::new(
        tag,
        VR::US,
        PrimitiveValue::U16(values.iter().copied().collect()),
    )
}

fn ul(tag: Tag, values: &[u32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::UL,
        PrimitiveValue::U32(values.iter().copied().collect()),
    )
}

fn sl(tag: Tag, values: &[i32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::SL,
        PrimitiveValue::I32(values.iter().copied().collect()),
    )
}

fn fl(tag: Tag, values: &[f32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::FL,
        PrimitiveValue::F32(values.iter().copied().collect()),
    )
}

fn fd(tag: Tag, values: &[f64]) -> InMemElement {
    DataElement::new(
        tag,
        VR::FD,
        PrimitiveValue::F64(values.iter().copied().collect()),
    )
}

fn of(tag: Tag, values: &[f32]) -> InMemElement {
    DataElement::new(
        tag,
        VR::OF,
        PrimitiveValue::F32(values.iter().copied().collect()),
    )
}

fn ob(tag: Tag, values: &[u8]) -> InMemElement {
    DataElement::new(
        tag,
        VR::OB,
        PrimitiveValue::U8(values.iter().copied().collect()),
    )
}

fn ss(tag: Tag, values: &[i16]) -> InMemElement {
    DataElement::new(
        tag,
        VR::SS,
        PrimitiveValue::I16(values.iter().copied().collect()),
    )
}

fn at(tag: Tag, values: &[Tag]) -> InMemElement {
    DataElement::new(
        tag,
        VR::AT,
        PrimitiveValue::Tags(values.iter().copied().collect()),
    )
}

/// A fixture's file name and the function building its Data Set.
type Fixture = (&'static str, fn() -> InMemDicomObject);

/// The fixtures by file name.
const FIXTURES: [Fixture; 3] = [
    ("annotations", annotations),
    ("segmentation", segmentation),
    ("parametric-map", parametric_map),
];

/// Objects of the microscopy IODs that are not pathology-slide IODs: VL
/// Microscopic Image (A.32.2), VL Slide-Coordinates Microscopic Image
/// (A.32.3), Confocal Microscopy Image (A.90.1) and Confocal Microscopy Tiled
/// Pyramidal Image (A.90.2).
const OTHER_MICROSCOPY: [Fixture; 4] = [
    ("vl-microscopic", vl_microscopic),
    ("vl-slide-coordinates", vl_slide_coordinates),
    ("confocal", confocal),
    ("confocal-tiled", confocal_tiled),
];

/// Writes `object` as `name`.dcm, its File Meta Information from its SOP
/// Class and Instance UIDs, and then applies `edit` to the Data Set.
fn write(
    dir: &Path,
    name: &str,
    object: InMemDicomObject,
    edit: impl FnOnce(&mut InMemDicomObject),
) -> PathBuf {
    let mut file = object
        .with_meta(FileMetaTableBuilder::new().transfer_syntax(EXPLICIT_VR_LITTLE_ENDIAN))
        .expect("file meta");
    edit(&mut file);
    let path = dir.join(format!("{name}.dcm"));
    file.write_to_file(&path).expect("write fixture");
    path
}

#[track_caller]
fn assert_every_iod_check_passes(path: &Path) {
    let report = iod_report(path, ValidationProfile::General);
    for name in IOD_CHECKS {
        let check = named(&report, name);
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{} {name}: {}",
            path.display(),
            check.message
        );
    }
}

#[test]
fn an_object_of_each_pathology_slide_iod_passes_every_iod_check() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, fixture) in FIXTURES {
        assert_every_iod_check_passes(&write(tmp.path(), name, fixture(), |_| {}));
    }
}

/// Writes the fixtures for the external validators:
/// `WSI_DICOM_IOD_FIXTURES=<dir> cargo test write_pathology_fixtures -- --ignored`.
#[test]
#[ignore = "writes the fixtures to $WSI_DICOM_IOD_FIXTURES for dciodvfy and validate_iods"]
fn write_pathology_fixtures() {
    let dir = std::env::var_os(FIXTURE_DIR_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {FIXTURE_DIR_VARIABLE} to the output directory"));
    std::fs::create_dir_all(&dir).unwrap();
    write_fixtures(&dir);
}

/// Writes every fixture into `dir`, so that each IOD's checks run there.
pub(super) fn write_fixtures(dir: &Path) {
    for (name, fixture) in FIXTURES {
        write(dir, name, fixture(), |_| {});
    }
}

/// One broken Attribute and the failure it causes.
struct Breakage {
    check: &'static str,
    expected: &'static str,
    edit: fn(&mut InMemDicomObject),
}

/// Breakages that apply to an object of every pathology-slide IOD.
const COMMON_BREAKAGES: [Breakage; 7] = [
    Breakage {
        check: "intrinsic-wsi-dicom-iod-modules",
        expected: "Patient Module (PS3.3 C.7.1.1, Usage M): required by Table",
        edit: |object| {
            for tag in [
                tags::PATIENT_NAME,
                tags::PATIENT_ID,
                tags::PATIENT_BIRTH_DATE,
                tags::PATIENT_SEX,
            ] {
                object.remove_element(tag);
            }
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-iod-attribute-presence",
        expected: "Study Instance UID (0020,000D): Type 1 missing",
        edit: |object| {
            object.remove_element(tags::STUDY_INSTANCE_UID);
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-iod-enumerated-values",
        expected: "Patient's Sex (0010,0040) \"X\" is not an Enumerated Value [M, F, O]",
        edit: |object| {
            object.put(text(tags::PATIENT_SEX, VR::CS, "X"));
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-iod-vr-vm",
        expected: "(0010,0040)",
        edit: |object| {
            object.put(text(tags::PATIENT_SEX, VR::CS, "O\\M"));
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-iod-value-format",
        expected: "(0008,0020)",
        edit: |object| {
            object.put(text(tags::STUDY_DATE, VR::DA, "2026-01-01"));
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-iod-sequence-items",
        expected: "Patient Species Code Sequence (0010,2202): 2 Items where exactly 1 is required",
        edit: |object| {
            let human = || code("SCT", "337915000", "Homo sapiens");
            object.put(sq(
                tags::PATIENT_SPECIES_CODE_SEQUENCE,
                vec![human(), human()],
            ));
        },
    },
    Breakage {
        check: "intrinsic-wsi-dicom-file-meta",
        expected: "does not match SOP Instance UID (0008,0018)",
        edit: |object| {
            object.put(text(
                tags::SOP_INSTANCE_UID,
                VR::UI,
                "1.2.826.0.1.3680043.10.999.7.999",
            ));
        },
    },
];

#[test]
fn breaking_an_attribute_of_each_family_fails_its_check_in_every_iod() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, fixture) in FIXTURES {
        for (index, breakage) in COMMON_BREAKAGES.iter().enumerate() {
            let path = write(
                tmp.path(),
                &format!("{name}-{index}"),
                fixture(),
                breakage.edit,
            );
            assert_iod_failure(&path, breakage.check, breakage.expected);
        }
    }
}

/// A Value Set departure of each IOD's own Code Sequences.
fn value_set_breakage(name: &str) -> Breakage {
    let check = "intrinsic-wsi-dicom-iod-value-sets";
    match name {
        "segmentation" => Breakage {
            check,
            expected: "which PS3.3 A.51.5.1 requires",
            edit: |object| {
                first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                    first_item(shared, tags::DERIVATION_IMAGE_SEQUENCE, |derivation| {
                        derivation.put(sq(
                            tags::DERIVATION_CODE_SEQUENCE,
                            vec![code("DCM", "113075", "Image Processing")],
                        ));
                    });
                });
            },
        },
        _ => Breakage {
            check,
            expected: "is not in Defined CID 5000",
            edit: |object| {
                object.put(sq(
                    tags::ALTERNATE_CONTENT_DESCRIPTION_SEQUENCE,
                    vec![item(vec![
                        text(tags::CONTENT_DESCRIPTION, VR::LO, "alternate"),
                        sq(
                            tags::LANGUAGE_CODE_SEQUENCE,
                            vec![code("99LOCAL", "xx", "local language")],
                        ),
                    ])],
                ));
            },
        },
    }
}

#[test]
fn a_departure_from_a_defined_value_set_fails_in_every_iod() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, fixture) in FIXTURES {
        let breakage = value_set_breakage(name);
        let path = write(
            tmp.path(),
            &format!("{name}-value-set"),
            fixture(),
            breakage.edit,
        );
        assert_iod_failure(&path, breakage.check, breakage.expected);
    }
}

/// The fixture called `name`.
fn fixture(name: &str) -> InMemDicomObject {
    FIXTURES
        .iter()
        .find(|(fixture, _)| *fixture == name)
        .map(|(_, build)| build())
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// Writes fixture `name` edited by `edit`, as `name`-`case`.dcm.
fn broken(dir: &Path, name: &str, case: &str, edit: impl FnOnce(&mut InMemDicomObject)) -> PathBuf {
    write(dir, &format!("{name}-{case}"), fixture(name), edit)
}

const MODULES: &str = "intrinsic-wsi-dicom-iod-modules";
const PRESENCE: &str = "intrinsic-wsi-dicom-iod-attribute-presence";
const ENUMERATED: &str = "intrinsic-wsi-dicom-iod-enumerated-values";

#[test]
fn shared_functional_group_placement_follows_each_iod_table() {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["segmentation", "parametric-map"] {
        let path = broken(tmp.path(), name, "shared-frame-content", |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(sq(tags::FRAME_CONTENT_SEQUENCE, vec![item(vec![])]));
            });
        });
        assert_iod_failure(
            &path,
            MODULES,
            "shall not be used as a Shared Functional Group",
        );
    }
    // Table A.75-2 (A.75.5): "U - May not be used as a Per-Frame Functional
    // Group" and "U - May not be used as a Shared Functional Group".
    let converted = |tag| sq(tag, vec![item(vec![])]);
    let path = broken(
        tmp.path(),
        "parametric-map",
        "per-frame-unassigned-shared",
        |object| {
            first_item(
                object,
                tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
                |frame| {
                    frame.put(converted(
                        tags::UNASSIGNED_SHARED_CONVERTED_ATTRIBUTES_SEQUENCE,
                    ));
                },
            );
        },
    );
    assert_iod_failure(&path, MODULES, "Unassigned Shared Converted Attributes");
    assert_iod_failure(
        &path,
        MODULES,
        "shall be used as a Shared Functional Group but is in 1 Per-Frame Item(s)",
    );
    let path = broken(
        tmp.path(),
        "parametric-map",
        "shared-unassigned-per-frame",
        |object| {
            first_item(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, |shared| {
                shared.put(converted(
                    tags::UNASSIGNED_PER_FRAME_CONVERTED_ATTRIBUTES_SEQUENCE,
                ));
            });
        },
    );
    assert_iod_failure(&path, MODULES, "Unassigned Per-Frame Converted Attributes");
}

/// An Overlay Plane in group `group` without Overlay Data (60xx,3000).
fn overlay(object: &mut InMemDicomObject, group: u16) {
    object.put(us(Tag(group, 0x0010), &[2]));
    object.put(us(Tag(group, 0x0011), &[2]));
    object.put(text(Tag(group, 0x0040), VR::CS, "G"));
    object.put(ss(Tag(group, 0x0050), &[1, 1]));
    object.put(us(Tag(group, 0x0100), &[1]));
    object.put(us(Tag(group, 0x0102), &[0]));
}

#[test]
fn iod_content_constraints_forbid_modules() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, iod, section) in [
        ("segmentation", "Segmentation", "A.51.4"),
        ("parametric-map", "Parametric Map", "A.75.4"),
    ] {
        let path = broken(tmp.path(), name, "window", |object| {
            object.put(text(tags::WINDOW_CENTER, VR::DS, "0.5"));
            object.put(text(tags::WINDOW_WIDTH, VR::DS, "1"));
        });
        assert_iod_failure(
            &path,
            MODULES,
            &format!("VOI LUT Module (PS3.3 C.11.2): present ((0028,1050), (0028,1051)) although PS3.3 {section} says it shall not be present in the {iod} IOD"),
        );
        let path = broken(tmp.path(), name, "overlay", |object| {
            overlay(object, 0x6002)
        });
        assert_iod_failure(
            &path,
            MODULES,
            &format!("Overlay Plane Module (PS3.3 C.9.2): present ((6002,0010), (6002,0011), (6002,0040), (6002,0050) and 2 more) although PS3.3 {section}"),
        );
    }
}

#[test]
fn a_parametric_map_has_exactly_one_pixel_module() {
    let tmp = tempfile::tempdir().unwrap();
    let path = broken(
        tmp.path(),
        "parametric-map",
        "two-pixel-modules",
        |object| {
            object.put(us(tags::PIXEL_REPRESENTATION, &[0]));
            object.put(ob(tags::PIXEL_DATA, &[0; 8]));
        },
    );
    assert_iod_failure(
        &path,
        MODULES,
        "the Image Pixel, Floating Point Image Pixel Modules are all present; PS3.3 A.75.4 permits only one of them",
    );
    let path = broken(tmp.path(), "parametric-map", "no-pixel-module", |object| {
        object.remove_element(tags::FLOAT_PIXEL_DATA);
    });
    assert_iod_failure(
        &path,
        MODULES,
        "none of the Image Pixel, Floating Point Image Pixel, Double Floating Point Image Pixel Modules is present; PS3.3 A.75.4 requires one of them",
    );
}

#[test]
fn segmentation_constraints_bind_padding_types_and_sop_classes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = broken(tmp.path(), "segmentation", "padding", |object| {
        object.put(us(tags::PIXEL_PADDING_VALUE, &[0]));
        object.put(us(tags::PIXEL_PADDING_RANGE_LIMIT, &[0]));
    });
    assert_iod_failure(
        &path,
        PRESENCE,
        "Pixel Padding Range Limit (0028,0121): Type 1C present although its condition is not met",
    );
    assert_iod_failure(
        &path,
        PRESENCE,
        "Pixel Padding Value (0028,0120): Type 1C present although its condition is not met",
    );
    let path = broken(tmp.path(), "segmentation", "fractional", |object| {
        object.put(text(tags::SEGMENTATION_TYPE, VR::CS, "FRACTIONAL"));
    });
    assert_iod_failure(
        &path,
        PRESENCE,
        "Segmentation Fractional Type (0062,0010): Type 1C missing",
    );
    let mut label_map = fixture("segmentation");
    label_map.put(text(
        tags::SOP_CLASS_UID,
        VR::UI,
        "1.2.840.10008.5.1.4.1.1.66.7",
    ));
    let path = write(tmp.path(), "segmentation-label-map", label_map, |_| {});
    assert_iod_failure(
        &path,
        ENUMERATED,
        "Segmentation Type (0062,0001) \"BINARY\" is not an Enumerated Value [LABELMAP]",
    );
}

#[test]
fn an_object_of_another_sop_class_says_no_iod_rules_apply() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("secondary-capture.dcm");
    write_primitive_pixel_dicom(&path, EXPLICIT_VR_LITTLE_ENDIAN, &[1]);
    let report = iod_report(&path, ValidationProfile::General);
    let check = named(&report, MODULES);
    assert_eq!(check.status, ValidationStatus::Skipped, "{}", check.message);
    assert!(
        check.message.contains(
            "SOP Class UID \"1.2.840.10008.5.1.4.1.1.7\" (Secondary Capture Image Storage) is not a SOP Class of the pathology-slide IODs"
        ) && check.message.ends_with("no IOD rules apply"),
        "{}",
        check.message
    );
    for name in &IOD_CHECKS[1..] {
        assert!(
            report.checks.iter().all(|check| check.name != *name),
            "{name} ran on a Secondary Capture object"
        );
    }
}

#[test]
fn the_other_microscopy_iods_get_no_iod_rules() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, fixture) in OTHER_MICROSCOPY {
        let path = write(tmp.path(), name, fixture(), |_| {});
        let report = iod_report(&path, ValidationProfile::General);
        let check = named(&report, MODULES);
        assert_eq!(
            check.status,
            ValidationStatus::Skipped,
            "{name}: {}",
            check.message
        );
        assert!(
            check
                .message
                .contains("is not a SOP Class of the pathology-slide IODs")
                && check.message.ends_with("no IOD rules apply"),
            "{name}: {}",
            check.message
        );
        for other in &IOD_CHECKS[1..] {
            assert!(
                report.checks.iter().all(|check| check.name != *other),
                "{other} ran on a {name} object"
            );
        }
    }
}
#[test]
fn a_parsed_object_is_checked_without_its_file_meta_or_pixel_values() {
    use crate::validation::iod::{run_iod_checks_on, Unavailable};

    let tmp = tempfile::tempdir().unwrap();
    // Parsed as a reader would hold it, with multi-valued strings split.
    let mut file =
        dicom_object::open_file(write(tmp.path(), "parsed", fixture("segmentation"), |_| {}))
            .unwrap();
    // An odd-length stand-in, as for Pixel Data given only by a BulkDataURI.
    file.put(ob(tags::PIXEL_DATA, &[0; 7]));
    // Nothing exists at the label; only the File Meta check would read it.
    let label = Path::new("no-such-document.json#/0");
    let reason = "Metadata includes no File Meta Information";
    let run = |pixel_data: bool| {
        let unavailable = Unavailable {
            file_meta: Some(reason.to_string()),
            pixel_data,
        };
        run_iod_checks_on(
            label,
            &file,
            &unavailable,
            crate::ConformanceLevel::Required,
        )
        .0
    };
    let checks = run(true);
    for name in IOD_CHECKS {
        let check = checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let expected = if name == "intrinsic-wsi-dicom-file-meta" {
            ValidationStatus::Skipped
        } else {
            ValidationStatus::Passed
        };
        assert_eq!(check.status, expected, "{name}: {}", check.message);
    }
    let file_meta = checks
        .iter()
        .find(|check| check.name == "intrinsic-wsi-dicom-file-meta")
        .unwrap();
    assert!(file_meta.message.ends_with(reason), "{}", file_meta.message);
    // Without the stand-in declared, its odd Value Length is a finding.
    let format = run(false)
        .into_iter()
        .find(|check| check.name == "intrinsic-wsi-dicom-iod-value-format")
        .unwrap();
    assert_eq!(
        format.status,
        ValidationStatus::Failed,
        "{}",
        format.message
    );
    assert!(
        format.message.contains("odd Value Length 7"),
        "{}",
        format.message
    );
}

/// A VL Microscopic Image (PS3.3 A.32.2) of one 2 x 2 MONOCHROME2 frame.
fn vl_microscopic() -> InMemDicomObject {
    item(vec![
        text(Tag(0x0008, 0x0008), VR::CS, "ORIGINAL\\PRIMARY"), // ImageType
        text(
            Tag(0x0008, 0x0016),
            VR::UI,
            "1.2.840.10008.5.1.4.1.1.77.1.2",
        ), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.3",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"),          // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"),          // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),            // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),            // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),                  // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "GM"),                // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),            // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),                  // ReferringPhysicianName
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"),      // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),                // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),                  // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),                 // PatientSex
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.13",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),                // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),                 // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),                 // InstanceNumber
        text(Tag(0x0020, 0x0020), VR::CS, ""),                  // PatientOrientation
        text(Tag(0x0020, 0x0060), VR::CS, ""),                  // Laterality
        us(Tag(0x0028, 0x0002), &[1]),                          // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"),       // PhotometricInterpretation
        us(Tag(0x0028, 0x0010), &[2]),                          // Rows
        us(Tag(0x0028, 0x0011), &[2]),                          // Columns
        us(Tag(0x0028, 0x0100), &[8]),                          // BitsAllocated
        us(Tag(0x0028, 0x0101), &[8]),                          // BitsStored
        us(Tag(0x0028, 0x0102), &[7]),                          // HighBit
        us(Tag(0x0028, 0x0103), &[0]),                          // PixelRepresentation
        text(Tag(0x0028, 0x2110), VR::CS, "00"),                // LossyImageCompression
        sq(Tag(0x0040, 0x0555), vec![]),                        // AcquisitionContextSequence
        ob(Tag(0x7FE0, 0x0010), &[0, 1, 2, 3]),                 // PixelData
    ])
}

/// A VL Slide-Coordinates Microscopic Image (PS3.3 A.32.3) of a slide specimen.
fn vl_slide_coordinates() -> InMemDicomObject {
    item(vec![
        text(Tag(0x0008, 0x0008), VR::CS, "ORIGINAL\\PRIMARY"), // ImageType
        text(
            Tag(0x0008, 0x0016),
            VR::UI,
            "1.2.840.10008.5.1.4.1.1.77.1.3",
        ), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.4",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"),          // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"),          // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),            // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),            // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),                  // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "SM"),                // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),            // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),                  // ReferringPhysicianName
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"),      // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),                // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),                  // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),                 // PatientSex
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.14",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),                // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),                 // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),                 // InstanceNumber
        text(Tag(0x0020, 0x0020), VR::CS, ""),                  // PatientOrientation
        text(
            Tag(0x0020, 0x0052),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.5",
        ), // FrameOfReferenceUID
        text(Tag(0x0020, 0x1040), VR::LO, "SLIDE_CORNER"),      // PositionReferenceIndicator
        us(Tag(0x0028, 0x0002), &[1]),                          // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"),       // PhotometricInterpretation
        us(Tag(0x0028, 0x0010), &[2]),                          // Rows
        us(Tag(0x0028, 0x0011), &[2]),                          // Columns
        us(Tag(0x0028, 0x0100), &[8]),                          // BitsAllocated
        us(Tag(0x0028, 0x0101), &[8]),                          // BitsStored
        us(Tag(0x0028, 0x0102), &[7]),                          // HighBit
        us(Tag(0x0028, 0x0103), &[0]),                          // PixelRepresentation
        text(Tag(0x0028, 0x2110), VR::CS, "00"),                // LossyImageCompression
        text(Tag(0x0040, 0x0512), VR::LO, "SLIDE-1"),           // ContainerIdentifier
        sq(Tag(0x0040, 0x0513), vec![]), // IssuerOfTheContainerIdentifierSequence
        sq(
            Tag(0x0040, 0x0518), // ContainerTypeCodeSequence
            vec![item(vec![
                text(Tag(0x0008, 0x0100), VR::SH, "433466003"), // CodeValue
                text(Tag(0x0008, 0x0102), VR::SH, "SCT"),       // CodingSchemeDesignator
                text(Tag(0x0008, 0x0104), VR::LO, "Microscope slide"), // CodeMeaning
            ])],
        ),
        sq(Tag(0x0040, 0x0555), vec![]), // AcquisitionContextSequence
        sq(
            Tag(0x0040, 0x0560), // SpecimenDescriptionSequence
            vec![item(vec![
                text(Tag(0x0040, 0x0551), VR::LO, "SPECIMEN-1"), // SpecimenIdentifier
                text(
                    Tag(0x0040, 0x0554),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.2",
                ), // SpecimenUID
                sq(Tag(0x0040, 0x0562), vec![]), // IssuerOfTheSpecimenIdentifierSequence
                sq(Tag(0x0040, 0x0610), vec![]), // SpecimenPreparationSequence
            ])],
        ),
        sq(
            Tag(0x0040, 0x071A), // ImageCenterPointCoordinatesSequence
            vec![item(vec![
                text(Tag(0x0040, 0x072A), VR::DS, "10"), // XOffsetInSlideCoordinateSystem
                text(Tag(0x0040, 0x073A), VR::DS, "20"), // YOffsetInSlideCoordinateSystem
                text(Tag(0x0040, 0x074A), VR::DS, "0"),  // ZOffsetInSlideCoordinateSystem
            ])],
        ),
        ob(Tag(0x7FE0, 0x0010), &[0, 1, 2, 3]), // PixelData
    ])
}

/// An in vivo Confocal Microscopy Image (PS3.3 A.90.1) of one frame.
fn confocal() -> InMemDicomObject {
    item(vec![
        text(
            Tag(0x0008, 0x0008),
            VR::CS,
            "ORIGINAL\\PRIMARY\\NONTILED\\NONE",
        ), // ImageType
        text(
            Tag(0x0008, 0x0016),
            VR::UI,
            "1.2.840.10008.5.1.4.1.1.77.1.8",
        ), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.20",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"), // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"), // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),   // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),   // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),         // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "CFM"),      // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),   // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),         // ReferringPhysicianName
        text(Tag(0x0008, 0x1090), VR::LO, "Fixture"),  // ManufacturerModelName
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"), // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),       // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),         // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),        // PatientSex
        text(Tag(0x0018, 0x1000), VR::LO, "1"),        // DeviceSerialNumber
        text(Tag(0x0018, 0x1020), VR::LO, "1"),        // SoftwareVersions
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.30",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),       // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),        // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),        // InstanceNumber
        text(Tag(0x0020, 0x0020), VR::CS, ""),         // PatientOrientation
        text(
            Tag(0x0020, 0x0052),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.21",
        ), // FrameOfReferenceUID
        text(Tag(0x0020, 0x1040), VR::LO, ""),         // PositionReferenceIndicator
        sq(
            Tag(0x0020, 0x9221), // DimensionOrganizationSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
            ])],
        ),
        sq(
            Tag(0x0020, 0x9222), // DimensionIndexSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
                at(Tag(0x0020, 0x9165), &[Tag(0x0048, 0x0106)]), // DimensionIndexPointer
                at(Tag(0x0020, 0x9167), &[Tag(0x0048, 0x0207)]), // FunctionalGroupPointer
            ])],
        ),
        us(Tag(0x0028, 0x0002), &[1]), // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"), // PhotometricInterpretation
        text(Tag(0x0028, 0x0008), VR::IS, "1"), // NumberOfFrames
        us(Tag(0x0028, 0x0010), &[2]), // Rows
        us(Tag(0x0028, 0x0011), &[2]), // Columns
        us(Tag(0x0028, 0x0100), &[8]), // BitsAllocated
        us(Tag(0x0028, 0x0101), &[8]), // BitsStored
        us(Tag(0x0028, 0x0102), &[7]), // HighBit
        us(Tag(0x0028, 0x0103), &[0]), // PixelRepresentation
        text(Tag(0x0028, 0x2110), VR::CS, "00"), // LossyImageCompression
        sq(Tag(0x0040, 0x0555), vec![]), // AcquisitionContextSequence
        sq(
            Tag(0x0048, 0x0105), // OpticalPathSequence
            vec![item(vec![
                sq(
                    Tag(0x0022, 0x0016), // IlluminationTypeCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "111742"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "DCM"),    // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Reflection illumination"), // CodeMeaning
                    ])],
                ),
                fl(Tag(0x0022, 0x0055), &[830.0]), // IlluminationWaveLength
                text(Tag(0x0048, 0x0106), VR::SH, "1"), // OpticalPathIdentifier
            ])],
        ),
        text(Tag(0x0048, 0x0114), VR::CS, "REFLECTANCE"), // ConfocalMode
        text(Tag(0x0048, 0x0115), VR::CS, "INVIVO"),      // TissueLocation
        sq(
            Tag(0x5200, 0x9229), // SharedFunctionalGroupsSequence
            vec![item(vec![
                sq(
                    Tag(0x0020, 0x9071), // FrameAnatomySequence
                    vec![item(vec![
                        sq(
                            Tag(0x0008, 0x2218), // AnatomicRegionSequence
                            vec![item(vec![
                                text(Tag(0x0008, 0x0100), VR::SH, "10200004"), // CodeValue
                                text(Tag(0x0008, 0x0102), VR::SH, "SCT"), // CodingSchemeDesignator
                                text(Tag(0x0008, 0x0104), VR::LO, "Liver"), // CodeMeaning
                            ])],
                        ),
                        text(Tag(0x0020, 0x9072), VR::CS, "U"), // FrameLaterality
                    ])],
                ),
                sq(
                    Tag(0x0028, 0x9110), // PixelMeasuresSequence
                    vec![item(vec![
                        text(Tag(0x0018, 0x0050), VR::DS, "0.005"), // SliceThickness
                        text(Tag(0x0028, 0x0030), VR::DS, "0.001\\0.001"), // PixelSpacing
                    ])],
                ),
                sq(
                    Tag(0x0048, 0x0116), // ConfocalMicroscopyImageFrameTypeSequence
                    vec![item(vec![
                        text(
                            Tag(0x0008, 0x9007),
                            VR::CS,
                            "ORIGINAL\\PRIMARY\\NONTILED\\NONE",
                        ), // FrameType
                    ])],
                ),
                sq(
                    Tag(0x0048, 0x0207), // OpticalPathIdentificationSequence
                    vec![item(vec![
                        text(Tag(0x0048, 0x0106), VR::SH, "1"), // OpticalPathIdentifier
                    ])],
                ),
            ])],
        ),
        sq(
            Tag(0x5200, 0x9230), // PerFrameFunctionalGroupsSequence
            vec![item(vec![sq(
                Tag(0x0020, 0x9111), // FrameContentSequence
                vec![item(vec![
                    text(Tag(0x0018, 0x9074), VR::DT, "20260101120000"), // FrameAcquisitionDateTime
                    text(Tag(0x0018, 0x9151), VR::DT, "20260101120000"), // FrameReferenceDateTime
                    fd(Tag(0x0018, 0x9220), &[1.0]),                     // FrameAcquisitionDuration
                    ul(Tag(0x0020, 0x9157), &[1]),                       // DimensionIndexValues
                ])],
            )])],
        ),
        ob(Tag(0x7FE0, 0x0010), &[0, 1, 2, 3]), // PixelData
    ])
}

/// Microscopy Bulk Simple Annotations (PS3.3 A.87) of one 2D point on a VL WSI image.
fn annotations() -> InMemDicomObject {
    item(vec![
        text(Tag(0x0008, 0x0016), VR::UI, "1.2.840.10008.5.1.4.1.1.91.1"), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.30",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"),                     // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"),                     // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),                       // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),                       // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),                             // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "ANN"),                          // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),                       // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),                             // ReferringPhysicianName
        text(Tag(0x0008, 0x1090), VR::LO, "Fixture"),                      // ManufacturerModelName
        sq(
            Tag(0x0008, 0x1115), // ReferencedSeriesSequence
            vec![item(vec![
                sq(
                    Tag(0x0008, 0x114A), // ReferencedInstanceSequence
                    vec![item(vec![
                        text(
                            Tag(0x0008, 0x1150),
                            VR::UI,
                            "1.2.840.10008.5.1.4.1.1.77.1.6",
                        ), // ReferencedSOPClassUID
                        text(
                            Tag(0x0008, 0x1155),
                            VR::UI,
                            "1.2.826.0.1.3680043.10.999.7.31",
                        ), // ReferencedSOPInstanceUID
                    ])],
                ),
                text(
                    Tag(0x0020, 0x000E),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.33",
                ), // SeriesInstanceUID
            ])],
        ),
        sq(
            Tag(0x0008, 0x1140), // ReferencedImageSequence
            vec![item(vec![
                text(
                    Tag(0x0008, 0x1150),
                    VR::UI,
                    "1.2.840.10008.5.1.4.1.1.77.1.6",
                ), // ReferencedSOPClassUID
                text(
                    Tag(0x0008, 0x1155),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.31",
                ), // ReferencedSOPInstanceUID
            ])],
        ),
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"), // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),           // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),             // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),            // PatientSex
        text(Tag(0x0018, 0x1000), VR::LO, "1"),            // DeviceSerialNumber
        text(Tag(0x0018, 0x1020), VR::LO, "1"),            // SoftwareVersions
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.40",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),           // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),            // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),            // InstanceNumber
        text(Tag(0x0020, 0x0060), VR::CS, ""),             // Laterality
        text(Tag(0x0048, 0x0301), VR::CS, "VOLUME"),       // PixelOriginInterpretation
        text(Tag(0x006A, 0x0001), VR::CS, "2D"),           // AnnotationCoordinateType
        sq(
            Tag(0x006A, 0x0002), // AnnotationGroupSequence
            vec![item(vec![
                us(Tag(0x0040, 0xA180), &[1]),          // AnnotationGroupNumber
                of(Tag(0x0066, 0x0016), &[10.0, 20.0]), // PointCoordinatesData
                text(
                    Tag(0x006A, 0x0003),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.32",
                ), // AnnotationGroupUID
                text(Tag(0x006A, 0x0005), VR::LO, "nuclei"), // AnnotationGroupLabel
                text(Tag(0x006A, 0x0007), VR::CS, "MANUAL"), // AnnotationGroupGenerationType
                sq(
                    Tag(0x006A, 0x0009), // AnnotationPropertyCategoryCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "91723000"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "SCT"),      // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Anatomical structure"), // CodeMeaning
                    ])],
                ),
                sq(
                    Tag(0x006A, 0x000A), // AnnotationPropertyTypeCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "84640000"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "SCT"),      // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Nucleus"),  // CodeMeaning
                    ])],
                ),
                ul(Tag(0x006A, 0x000C), &[1]), // NumberOfAnnotations
                text(Tag(0x006A, 0x000D), VR::CS, "YES"), // AnnotationAppliesToAllOpticalPaths
                text(Tag(0x0070, 0x0023), VR::CS, "POINT"), // GraphicType
            ])],
        ),
        text(Tag(0x0070, 0x0080), VR::CS, "NUCLEI"), // ContentLabel
        text(Tag(0x0070, 0x0081), VR::LO, ""),       // ContentDescription
        text(Tag(0x0070, 0x0084), VR::PN, ""),       // ContentCreatorName
    ])
}

/// A BINARY Segmentation (PS3.3 A.51) of one frame derived from a VL Microscopic Image.
fn segmentation() -> InMemDicomObject {
    item(vec![
        text(Tag(0x0008, 0x0008), VR::CS, "DERIVED\\PRIMARY"), // ImageType
        text(Tag(0x0008, 0x0016), VR::UI, "1.2.840.10008.5.1.4.1.1.66.4"), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.40",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"),         // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"),         // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),           // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),           // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),                 // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "SEG"),              // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),           // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),                 // ReferringPhysicianName
        text(Tag(0x0008, 0x1090), VR::LO, "Fixture"),          // ManufacturerModelName
        sq(
            Tag(0x0008, 0x1115), // ReferencedSeriesSequence
            vec![item(vec![
                sq(
                    Tag(0x0008, 0x114A), // ReferencedInstanceSequence
                    vec![item(vec![
                        text(
                            Tag(0x0008, 0x1150),
                            VR::UI,
                            "1.2.840.10008.5.1.4.1.1.77.1.2",
                        ), // ReferencedSOPClassUID
                        text(
                            Tag(0x0008, 0x1155),
                            VR::UI,
                            "1.2.826.0.1.3680043.10.999.7.3",
                        ), // ReferencedSOPInstanceUID
                    ])],
                ),
                text(
                    Tag(0x0020, 0x000E),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.13",
                ), // SeriesInstanceUID
            ])],
        ),
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"), // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),           // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),             // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),            // PatientSex
        text(Tag(0x0018, 0x1000), VR::LO, "1"),            // DeviceSerialNumber
        text(Tag(0x0018, 0x1020), VR::LO, "1"),            // SoftwareVersions
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.50",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),           // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),            // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),            // InstanceNumber
        text(Tag(0x0020, 0x0020), VR::CS, ""),             // PatientOrientation
        sq(
            Tag(0x0020, 0x9221), // DimensionOrganizationSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
            ])],
        ),
        sq(
            Tag(0x0020, 0x9222), // DimensionIndexSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
                at(Tag(0x0020, 0x9165), &[Tag(0x0062, 0x000B)]), // DimensionIndexPointer
                at(Tag(0x0020, 0x9167), &[Tag(0x0062, 0x000A)]), // FunctionalGroupPointer
            ])],
        ),
        us(Tag(0x0028, 0x0002), &[1]), // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"), // PhotometricInterpretation
        text(Tag(0x0028, 0x0008), VR::IS, "1"), // NumberOfFrames
        us(Tag(0x0028, 0x0010), &[2]), // Rows
        us(Tag(0x0028, 0x0011), &[2]), // Columns
        us(Tag(0x0028, 0x0100), &[1]), // BitsAllocated
        us(Tag(0x0028, 0x0101), &[1]), // BitsStored
        us(Tag(0x0028, 0x0102), &[0]), // HighBit
        us(Tag(0x0028, 0x0103), &[0]), // PixelRepresentation
        text(Tag(0x0028, 0x2110), VR::CS, "00"), // LossyImageCompression
        text(Tag(0x0062, 0x0001), VR::CS, "BINARY"), // SegmentationType
        sq(
            Tag(0x0062, 0x0002), // SegmentSequence
            vec![item(vec![
                sq(
                    Tag(0x0062, 0x0003), // SegmentedPropertyCategoryCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "91723000"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "SCT"),      // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Anatomical structure"), // CodeMeaning
                    ])],
                ),
                us(Tag(0x0062, 0x0004), &[1]), // SegmentNumber
                text(Tag(0x0062, 0x0005), VR::LO, "tissue"), // SegmentLabel
                text(Tag(0x0062, 0x0008), VR::CS, "MANUAL"), // SegmentAlgorithmType
                sq(
                    Tag(0x0062, 0x000F), // SegmentedPropertyTypeCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "85756007"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "SCT"),      // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Tissue"),   // CodeMeaning
                    ])],
                ),
            ])],
        ),
        text(Tag(0x0070, 0x0080), VR::CS, "TISSUE"), // ContentLabel
        text(Tag(0x0070, 0x0081), VR::LO, ""),       // ContentDescription
        text(Tag(0x0070, 0x0084), VR::PN, ""),       // ContentCreatorName
        sq(
            Tag(0x5200, 0x9229), // SharedFunctionalGroupsSequence
            vec![item(vec![sq(
                Tag(0x0008, 0x9124), // DerivationImageSequence
                vec![item(vec![
                    sq(
                        Tag(0x0008, 0x2112), // SourceImageSequence
                        vec![item(vec![
                            text(
                                Tag(0x0008, 0x1150),
                                VR::UI,
                                "1.2.840.10008.5.1.4.1.1.77.1.2",
                            ), // ReferencedSOPClassUID
                            text(
                                Tag(0x0008, 0x1155),
                                VR::UI,
                                "1.2.826.0.1.3680043.10.999.7.3",
                            ), // ReferencedSOPInstanceUID
                            sq(
                                Tag(0x0040, 0xA170), // PurposeOfReferenceCodeSequence
                                vec![item(vec![
                                    text(Tag(0x0008, 0x0100), VR::SH, "121322"), // CodeValue
                                    text(Tag(0x0008, 0x0102), VR::SH, "DCM"), // CodingSchemeDesignator
                                    text(
                                        Tag(0x0008, 0x0104),
                                        VR::LO,
                                        "Source Image for Image Processing Operation",
                                    ), // CodeMeaning
                                ])],
                            ),
                        ])],
                    ),
                    sq(
                        Tag(0x0008, 0x9215), // DerivationCodeSequence
                        vec![item(vec![
                            text(Tag(0x0008, 0x0100), VR::SH, "113076"), // CodeValue
                            text(Tag(0x0008, 0x0102), VR::SH, "DCM"),    // CodingSchemeDesignator
                            text(Tag(0x0008, 0x0104), VR::LO, "Segmentation"), // CodeMeaning
                        ])],
                    ),
                ])],
            )])],
        ),
        sq(
            Tag(0x5200, 0x9230), // PerFrameFunctionalGroupsSequence
            vec![item(vec![
                sq(
                    Tag(0x0020, 0x9111), // FrameContentSequence
                    vec![item(vec![
                        ul(Tag(0x0020, 0x9157), &[1]), // DimensionIndexValues
                    ])],
                ),
                sq(
                    Tag(0x0062, 0x000A), // SegmentIdentificationSequence
                    vec![item(vec![
                        us(Tag(0x0062, 0x000B), &[1]), // ReferencedSegmentNumber
                    ])],
                ),
            ])],
        ),
        ob(Tag(0x7FE0, 0x0010), &[9, 0]), // PixelData
    ])
}

/// A TILED_FULL Confocal Microscopy Tiled Pyramidal Image (PS3.3 A.90.2) of a slide.
fn confocal_tiled() -> InMemDicomObject {
    item(vec![
        text(
            Tag(0x0008, 0x0008),
            VR::CS,
            "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
        ), // ImageType
        text(
            Tag(0x0008, 0x0016),
            VR::UI,
            "1.2.840.10008.5.1.4.1.1.77.1.9",
        ), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.50",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"), // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"), // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),   // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),   // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),         // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "CFM"),      // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),   // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),         // ReferringPhysicianName
        text(Tag(0x0008, 0x1090), VR::LO, "Fixture"),  // ManufacturerModelName
        text(Tag(0x0008, 0x9206), VR::CS, "VOLUME"),   // VolumetricProperties
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"), // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),       // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),         // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),        // PatientSex
        text(Tag(0x0018, 0x1000), VR::LO, "1"),        // DeviceSerialNumber
        text(Tag(0x0018, 0x1020), VR::LO, "1"),        // SoftwareVersions
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.60",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),       // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),        // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),        // InstanceNumber
        text(
            Tag(0x0020, 0x0052),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.51",
        ), // FrameOfReferenceUID
        text(Tag(0x0020, 0x1040), VR::LO, "SLIDE_CORNER"), // PositionReferenceIndicator
        sq(
            Tag(0x0020, 0x9221), // DimensionOrganizationSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.52",
                ), // DimensionOrganizationUID
            ])],
        ),
        text(Tag(0x0020, 0x9311), VR::CS, "TILED_FULL"), // DimensionOrganizationType
        us(Tag(0x0028, 0x0002), &[1]),                   // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"), // PhotometricInterpretation
        text(Tag(0x0028, 0x0008), VR::IS, "1"),          // NumberOfFrames
        us(Tag(0x0028, 0x0010), &[2]),                   // Rows
        us(Tag(0x0028, 0x0011), &[2]),                   // Columns
        us(Tag(0x0028, 0x0100), &[8]),                   // BitsAllocated
        us(Tag(0x0028, 0x0101), &[8]),                   // BitsStored
        us(Tag(0x0028, 0x0102), &[7]),                   // HighBit
        us(Tag(0x0028, 0x0103), &[0]),                   // PixelRepresentation
        text(Tag(0x0028, 0x2110), VR::CS, "00"),         // LossyImageCompression
        text(Tag(0x0040, 0x0512), VR::LO, "SLIDE-1"),    // ContainerIdentifier
        sq(Tag(0x0040, 0x0513), vec![]),                 // IssuerOfTheContainerIdentifierSequence
        sq(
            Tag(0x0040, 0x0518), // ContainerTypeCodeSequence
            vec![item(vec![
                text(Tag(0x0008, 0x0100), VR::SH, "433466003"), // CodeValue
                text(Tag(0x0008, 0x0102), VR::SH, "SCT"),       // CodingSchemeDesignator
                text(Tag(0x0008, 0x0104), VR::LO, "Microscope slide"), // CodeMeaning
            ])],
        ),
        sq(Tag(0x0040, 0x0555), vec![]), // AcquisitionContextSequence
        sq(
            Tag(0x0040, 0x0560), // SpecimenDescriptionSequence
            vec![item(vec![
                text(Tag(0x0040, 0x0551), VR::LO, "SPECIMEN-1"), // SpecimenIdentifier
                text(
                    Tag(0x0040, 0x0554),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.2",
                ), // SpecimenUID
                sq(Tag(0x0040, 0x0562), vec![]), // IssuerOfTheSpecimenIdentifierSequence
                sq(Tag(0x0040, 0x0610), vec![]), // SpecimenPreparationSequence
            ])],
        ),
        fl(Tag(0x0048, 0x0001), &[0.002]), // ImagedVolumeWidth
        fl(Tag(0x0048, 0x0002), &[0.002]), // ImagedVolumeHeight
        fl(Tag(0x0048, 0x0003), &[0.005]), // ImagedVolumeDepth
        ul(Tag(0x0048, 0x0006), &[2]),     // TotalPixelMatrixColumns
        ul(Tag(0x0048, 0x0007), &[2]),     // TotalPixelMatrixRows
        sq(
            Tag(0x0048, 0x0008), // TotalPixelMatrixOriginSequence
            vec![item(vec![
                text(Tag(0x0040, 0x072A), VR::DS, "10"), // XOffsetInSlideCoordinateSystem
                text(Tag(0x0040, 0x073A), VR::DS, "20"), // YOffsetInSlideCoordinateSystem
            ])],
        ),
        text(Tag(0x0048, 0x0102), VR::DS, "0\\-1\\0\\-1\\0\\0"), // ImageOrientationSlide
        sq(
            Tag(0x0048, 0x0105), // OpticalPathSequence
            vec![item(vec![
                sq(
                    Tag(0x0022, 0x0016), // IlluminationTypeCodeSequence
                    vec![item(vec![
                        text(Tag(0x0008, 0x0100), VR::SH, "111742"), // CodeValue
                        text(Tag(0x0008, 0x0102), VR::SH, "DCM"),    // CodingSchemeDesignator
                        text(Tag(0x0008, 0x0104), VR::LO, "Reflection illumination"), // CodeMeaning
                    ])],
                ),
                fl(Tag(0x0022, 0x0055), &[830.0]), // IlluminationWaveLength
                text(Tag(0x0048, 0x0106), VR::SH, "1"), // OpticalPathIdentifier
            ])],
        ),
        text(Tag(0x0048, 0x0114), VR::CS, "REFLECTANCE"), // ConfocalMode
        text(Tag(0x0048, 0x0115), VR::CS, "EXVIVO"),      // TissueLocation
        ul(Tag(0x0048, 0x0302), &[1]),                    // NumberOfOpticalPaths
        ul(Tag(0x0048, 0x0303), &[1]),                    // TotalPixelMatrixFocalPlanes
        sq(
            Tag(0x5200, 0x9229), // SharedFunctionalGroupsSequence
            vec![item(vec![
                sq(
                    Tag(0x0020, 0x9071), // FrameAnatomySequence
                    vec![item(vec![
                        sq(
                            Tag(0x0008, 0x2218), // AnatomicRegionSequence
                            vec![item(vec![
                                text(Tag(0x0008, 0x0100), VR::SH, "10200004"), // CodeValue
                                text(Tag(0x0008, 0x0102), VR::SH, "SCT"), // CodingSchemeDesignator
                                text(Tag(0x0008, 0x0104), VR::LO, "Liver"), // CodeMeaning
                            ])],
                        ),
                        text(Tag(0x0020, 0x9072), VR::CS, "U"), // FrameLaterality
                    ])],
                ),
                sq(
                    Tag(0x0028, 0x9110), // PixelMeasuresSequence
                    vec![item(vec![
                        text(Tag(0x0018, 0x0050), VR::DS, "0.005"), // SliceThickness
                        text(Tag(0x0028, 0x0030), VR::DS, "0.001\\0.001"), // PixelSpacing
                    ])],
                ),
                sq(
                    Tag(0x0048, 0x0116), // ConfocalMicroscopyImageFrameTypeSequence
                    vec![item(vec![
                        text(
                            Tag(0x0008, 0x9007),
                            VR::CS,
                            "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
                        ), // FrameType
                    ])],
                ),
            ])],
        ),
        sq(
            Tag(0x5200, 0x9230), // PerFrameFunctionalGroupsSequence
            vec![item(vec![sq(
                Tag(0x0048, 0x021A), // PlanePositionSlideSequence
                vec![item(vec![
                    text(Tag(0x0040, 0x072A), VR::DS, "10"), // XOffsetInSlideCoordinateSystem
                    text(Tag(0x0040, 0x073A), VR::DS, "20"), // YOffsetInSlideCoordinateSystem
                    text(Tag(0x0040, 0x074A), VR::DS, "0"),  // ZOffsetInSlideCoordinateSystem
                    sl(Tag(0x0048, 0x021E), &[1]), // ColumnPositionInTotalImagePixelMatrix
                    sl(Tag(0x0048, 0x021F), &[1]), // RowPositionInTotalImagePixelMatrix
                ])],
            )])],
        ),
        ob(Tag(0x7FE0, 0x0010), &[0, 1, 2, 3]), // PixelData
    ])
}

/// A 32-bit float Parametric Map (PS3.3 A.75) of one frame in a patient Frame of Reference.
fn parametric_map() -> InMemDicomObject {
    item(vec![
        text(
            Tag(0x0008, 0x0008),
            VR::CS,
            "DERIVED\\PRIMARY\\VOLUME\\NONE",
        ), // ImageType
        text(Tag(0x0008, 0x0016), VR::UI, "1.2.840.10008.5.1.4.1.1.30"), // SOPClassUID
        text(
            Tag(0x0008, 0x0018),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.60",
        ), // SOPInstanceUID
        text(Tag(0x0008, 0x0020), VR::DA, "20260101"),                   // StudyDate
        text(Tag(0x0008, 0x0023), VR::DA, "20260101"),                   // ContentDate
        text(Tag(0x0008, 0x0030), VR::TM, "120000"),                     // StudyTime
        text(Tag(0x0008, 0x0033), VR::TM, "120000"),                     // ContentTime
        text(Tag(0x0008, 0x0050), VR::SH, ""),                           // AccessionNumber
        text(Tag(0x0008, 0x0060), VR::CS, "SM"),                         // Modality
        text(Tag(0x0008, 0x0070), VR::LO, "Frames"),                     // Manufacturer
        text(Tag(0x0008, 0x0090), VR::PN, ""),                           // ReferringPhysicianName
        text(Tag(0x0008, 0x1090), VR::LO, "Fixture"),                    // ManufacturerModelName
        text(Tag(0x0010, 0x0010), VR::PN, "Test^Patient"),               // PatientName
        text(Tag(0x0010, 0x0020), VR::LO, "P1"),                         // PatientID
        text(Tag(0x0010, 0x0030), VR::DA, ""),                           // PatientBirthDate
        text(Tag(0x0010, 0x0040), VR::CS, "O"),                          // PatientSex
        text(Tag(0x0018, 0x1000), VR::LO, "1"),                          // DeviceSerialNumber
        text(Tag(0x0018, 0x1020), VR::LO, "1"),                          // SoftwareVersions
        text(Tag(0x0018, 0x9004), VR::CS, "RESEARCH"),                   // ContentQualification
        text(
            Tag(0x0020, 0x000D),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.1",
        ), // StudyInstanceUID
        text(
            Tag(0x0020, 0x000E),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.70",
        ), // SeriesInstanceUID
        text(Tag(0x0020, 0x0010), VR::SH, "S1"),                         // StudyID
        text(Tag(0x0020, 0x0011), VR::IS, "1"),                          // SeriesNumber
        text(Tag(0x0020, 0x0013), VR::IS, "1"),                          // InstanceNumber
        text(
            Tag(0x0020, 0x0052),
            VR::UI,
            "1.2.826.0.1.3680043.10.999.7.61",
        ), // FrameOfReferenceUID
        text(Tag(0x0020, 0x0060), VR::CS, ""),                           // Laterality
        text(Tag(0x0020, 0x1040), VR::LO, ""), // PositionReferenceIndicator
        sq(
            Tag(0x0020, 0x9221), // DimensionOrganizationSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
            ])],
        ),
        sq(
            Tag(0x0020, 0x9222), // DimensionIndexSequence
            vec![item(vec![
                text(
                    Tag(0x0020, 0x9164),
                    VR::UI,
                    "1.2.826.0.1.3680043.10.999.7.6",
                ), // DimensionOrganizationUID
                at(Tag(0x0020, 0x9165), &[Tag(0x0020, 0x0032)]), // DimensionIndexPointer
                at(Tag(0x0020, 0x9167), &[Tag(0x0020, 0x9113)]), // FunctionalGroupPointer
            ])],
        ),
        us(Tag(0x0028, 0x0002), &[1]), // SamplesPerPixel
        text(Tag(0x0028, 0x0004), VR::CS, "MONOCHROME2"), // PhotometricInterpretation
        text(Tag(0x0028, 0x0008), VR::IS, "1"), // NumberOfFrames
        us(Tag(0x0028, 0x0010), &[2]), // Rows
        us(Tag(0x0028, 0x0011), &[2]), // Columns
        us(Tag(0x0028, 0x0100), &[32]), // BitsAllocated
        text(Tag(0x0028, 0x0301), VR::CS, "NO"), // BurnedInAnnotation
        text(Tag(0x0028, 0x0302), VR::CS, "NO"), // RecognizableVisualFeatures
        text(Tag(0x0028, 0x2110), VR::CS, "00"), // LossyImageCompression
        sq(Tag(0x0040, 0x0555), vec![]), // AcquisitionContextSequence
        text(Tag(0x0070, 0x0080), VR::CS, "DENSITY"), // ContentLabel
        text(Tag(0x0070, 0x0081), VR::LO, ""), // ContentDescription
        text(Tag(0x0070, 0x0084), VR::PN, ""), // ContentCreatorName
        text(Tag(0x2050, 0x0020), VR::CS, "IDENTITY"), // PresentationLUTShape
        sq(
            Tag(0x5200, 0x9229), // SharedFunctionalGroupsSequence
            vec![item(vec![
                sq(
                    Tag(0x0020, 0x9116), // PlaneOrientationSequence
                    vec![item(vec![
                        text(Tag(0x0020, 0x0037), VR::DS, "1\\0\\0\\0\\1\\0"), // ImageOrientationPatient
                    ])],
                ),
                sq(
                    Tag(0x0028, 0x9110), // PixelMeasuresSequence
                    vec![item(vec![
                        text(Tag(0x0018, 0x0050), VR::DS, "0.005"), // SliceThickness
                        text(Tag(0x0028, 0x0030), VR::DS, "0.001\\0.001"), // PixelSpacing
                    ])],
                ),
                sq(
                    Tag(0x0028, 0x9132), // FrameVOILUTSequence
                    vec![item(vec![
                        text(Tag(0x0028, 0x1050), VR::DS, "0.5"), // WindowCenter
                        text(Tag(0x0028, 0x1051), VR::DS, "1"),   // WindowWidth
                    ])],
                ),
                sq(
                    Tag(0x0028, 0x9145), // PixelValueTransformationSequence
                    vec![item(vec![
                        text(Tag(0x0028, 0x1052), VR::DS, "0"),  // RescaleIntercept
                        text(Tag(0x0028, 0x1053), VR::DS, "1"),  // RescaleSlope
                        text(Tag(0x0028, 0x1054), VR::LO, "US"), // RescaleType
                    ])],
                ),
                sq(
                    Tag(0x0040, 0x9092), // ParametricMapFrameTypeSequence
                    vec![item(vec![
                        text(
                            Tag(0x0008, 0x9007),
                            VR::CS,
                            "DERIVED\\PRIMARY\\VOLUME\\NONE",
                        ), // FrameType
                    ])],
                ),
                sq(
                    Tag(0x0040, 0x9096), // RealWorldValueMappingSequence
                    vec![item(vec![
                        text(Tag(0x0028, 0x3003), VR::LO, "density"), // LUTExplanation
                        sq(
                            Tag(0x0040, 0x08EA), // MeasurementUnitsCodeSequence
                            vec![item(vec![
                                text(Tag(0x0008, 0x0100), VR::SH, "1"),        // CodeValue
                                text(Tag(0x0008, 0x0102), VR::SH, "UCUM"), // CodingSchemeDesignator
                                text(Tag(0x0008, 0x0104), VR::LO, "no units"), // CodeMeaning
                            ])],
                        ),
                        text(Tag(0x0040, 0x9210), VR::SH, "DENSITY"), // LUTLabel
                        fd(Tag(0x0040, 0x9213), &[1.0]), // DoubleFloatRealWorldValueLastValueMapped
                        fd(Tag(0x0040, 0x9214), &[0.0]), // DoubleFloatRealWorldValueFirstValueMapped
                        fd(Tag(0x0040, 0x9224), &[0.0]), // RealWorldValueIntercept
                        fd(Tag(0x0040, 0x9225), &[1.0]), // RealWorldValueSlope
                    ])],
                ),
            ])],
        ),
        sq(
            Tag(0x5200, 0x9230), // PerFrameFunctionalGroupsSequence
            vec![item(vec![
                sq(
                    Tag(0x0020, 0x9111), // FrameContentSequence
                    vec![item(vec![
                        ul(Tag(0x0020, 0x9157), &[1]), // DimensionIndexValues
                    ])],
                ),
                sq(
                    Tag(0x0020, 0x9113), // PlanePositionSequence
                    vec![item(vec![
                        text(Tag(0x0020, 0x0032), VR::DS, "0\\0\\0"), // ImagePositionPatient
                    ])],
                ),
            ])],
        ),
        of(Tag(0x7FE0, 0x0008), &[0.0, 0.25, 0.5, 1.0]), // FloatPixelData
    ])
}

// The checks of each IOD family, on these fixtures.
mod families;
