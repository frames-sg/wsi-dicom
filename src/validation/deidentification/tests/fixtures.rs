//! The rule over exported VL WSI pyramids that a reference de-identifier
//! protects with the Basic Profile and the Options each case declares,
//! applying PS3.15 Table E.1-1 as written: each conformant combination
//! passes per file and over the set, and each breach fails, through the
//! same validation path users run.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use crate::validation::deidentification::DEIDENTIFICATION_RULE;
use crate::validation::iod::deidentification::{
    confidentiality_attribute, ConfidentialityOption as O, DeidentificationAction as A,
    BASIC_PROFILE,
};
use crate::validation::{
    validate_dicom_path_with_runner, CommandOutcome, ValidationCheck, ValidationCommandRunner,
    ValidationOptions, ValidationStatus,
};

/// External validators are not run here; non-strict validation skips them.
struct NoTools;

impl ValidationCommandRunner for NoTools {
    fn find_command(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    fn run(
        &self,
        program: &Path,
        _args: &[OsString],
        _timeout: Duration,
        _max_output_bytes: usize,
    ) -> Result<CommandOutcome, std::io::Error> {
        Err(std::io::Error::other(format!(
            "{} is not run by these tests",
            program.display()
        )))
    }
}

/// A two-instance pyramid: the instance exported from an 8 by 8 synthetic
/// RGB slide in 2-pixel tiles, and a copy that is another SOP Instance of
/// the same Pyramid UID and Series, as the series-set tests build.
fn exported_pyramid(root: &Path) -> Vec<PathBuf> {
    let source = root.join("source.dcm");
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(8, 8).expect("pixels");
    crate::synthetic_source::write_rgb_source_dicom(
        &source,
        "1.2.826.0.1.3680043.10.999.301",
        "1.2.826.0.1.3680043.10.999.302",
        8,
        8,
        pixels,
    )
    .expect("source");
    let first = crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: root.join("original"),
        options: crate::ExportOptions {
            tile_size: 2,
            transfer_syntax: crate::TransferSyntax::Jpeg2000Lossless,
            encode_backend: crate::EncodeBackendPreference::CpuOnly,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .expect("export")
    .instances
    .remove(0)
    .path;
    let mut copy = dicom_object::open_file(&first).expect("exported file");
    let uid = "1.2.826.0.1.3680043.10.999.777";
    copy.put(text(tags::SOP_INSTANCE_UID, VR::UI, uid));
    copy.meta_mut().media_storage_sop_instance_uid = uid.to_string();
    copy.put(text(tags::INSTANCE_NUMBER, VR::IS, "2"));
    let second = root.join("original").join("copy.dcm");
    copy.write_to_file(&second).expect("write copy");
    for path in [&first, &second] {
        let object = dicom_object::open_file(path).expect("exported file");
        assert!(object.element(tags::PYRAMID_UID).is_ok(), "no Pyramid UID");
    }
    vec![first, second]
}

/// A de-identifier applying Table E.1-1 for `options`, replacing UIDs
/// consistently across the instances it protects.
struct Deidentifier<'a> {
    options: &'a [O],
    uids: BTreeMap<String, String>,
    /// Attributes left as they are, to break the Profile on purpose.
    skipped: &'a [Tag],
}

impl Deidentifier<'_> {
    fn action(&self, tag: Tag) -> Option<A> {
        let row = confidentiality_attribute(tag)?;
        Some(
            row.options
                .iter()
                .find(|(option, _)| self.options.contains(option))
                .map_or(row.basic, |(_, action)| *action),
        )
    }

    fn protect(&mut self, object: &mut DefaultDicomObject) {
        self.data_set(object);
        let codes = std::iter::once(BASIC_PROFILE)
            .chain(self.options.iter().map(|option| option.code()))
            .map(|code| {
                InMemDicomObject::from_element_iter([
                    text(tags::CODE_VALUE, VR::SH, code.value),
                    text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, code.scheme),
                    text(tags::CODE_MEANING, VR::LO, code.meaning),
                ])
            })
            .collect::<Vec<_>>();
        object.put(text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "YES"));
        object.put(DataElement::new(
            tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(codes),
        ));
        let temporal = if self
            .options
            .contains(&O::RetainLongitudinalTemporalInformationFullDates)
        {
            "UNMODIFIED"
        } else if self
            .options
            .contains(&O::RetainLongitudinalTemporalInformationModifiedDates)
        {
            "MODIFIED"
        } else {
            "REMOVED"
        };
        object.put(text(
            tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
            VR::CS,
            temporal,
        ));
        if self.options.contains(&O::CleanPixelData) {
            object.put(text(tags::BURNED_IN_ANNOTATION, VR::CS, "NO"));
        }
        if self.options.contains(&O::CleanRecognizableVisualFeatures) {
            object.put(text(tags::RECOGNIZABLE_VISUAL_FEATURES, VR::CS, "NO"));
        }
        let instance = object
            .element(tags::SOP_INSTANCE_UID)
            .ok()
            .and_then(|element| element.to_str().ok())
            .map(|uid| uid.trim_end_matches(['\0', ' ']).to_string());
        if let Some(instance) = instance {
            object.update_meta(|meta| meta.media_storage_sop_instance_uid = instance);
        }
    }

    fn data_set(&mut self, data_set: &mut InMemDicomObject) {
        let present: Vec<Tag> = data_set
            .iter()
            .map(|element| element.header().tag)
            .collect();
        for tag in present {
            if self.skipped.contains(&tag) {
                continue;
            }
            if tag.group() % 2 == 1 {
                if !self.options.contains(&O::RetainSafePrivate) {
                    data_set.remove_element(tag);
                }
                continue;
            }
            let Ok(element) = data_set.take_element(tag) else {
                continue;
            };
            if let Some(element) = self.element(element) {
                data_set.put(element);
            }
        }
    }

    /// The element as its action leaves it, if kept.
    fn element(&mut self, element: InMemElement) -> Option<InMemElement> {
        let tag = element.header().tag;
        let vr = element.vr();
        let element = match self.action(tag) {
            Some(A::X | A::XZ | A::XD | A::XZD | A::XZUStar) => return None,
            Some(A::Z) if vr != VR::SQ => DataElement::new(tag, vr, PrimitiveValue::Empty),
            Some(A::D | A::ZD) => dummy(element),
            Some(A::U) => self.replaced_uid(element),
            _ => element,
        };
        if vr != VR::SQ {
            return Some(element);
        }
        let items = element.into_value().into_items().unwrap_or_default();
        let items: Vec<InMemDicomObject> = items
            .into_iter()
            .map(|mut item| {
                self.data_set(&mut item);
                item
            })
            .collect();
        Some(DataElement::new(tag, VR::SQ, DataSetSequence::from(items)))
    }

    fn replaced_uid(&mut self, element: InMemElement) -> InMemElement {
        let tag = element.header().tag;
        let Ok(original) = element.to_str() else {
            return element;
        };
        let original = original.trim_end_matches(['\0', ' ']).to_string();
        let next = self.uids.len() + 1;
        let replaced = self
            .uids
            .entry(original)
            .or_insert_with(|| format!("2.25.{}", 900_000 + next))
            .clone();
        text(tag, VR::UI, &replaced)
    }
}

/// A non-zero length dummy Value consistent with the VR; Sequences and
/// binary Values keep theirs, which are not zero length.
fn dummy(element: InMemElement) -> InMemElement {
    let tag = element.header().tag;
    let vr = element.vr();
    let value = match vr {
        VR::DA => "19000101",
        VR::TM => "000000",
        VR::DT => "19000101000000",
        VR::PN => "DUMMY^PATIENT",
        VR::AS => "000Y",
        VR::DS | VR::IS => "0",
        VR::LO | VR::SH | VR::ST | VR::LT | VR::UT | VR::UC | VR::CS => "DUMMY",
        _ => return element,
    };
    text(tag, vr, value)
}

fn text(tag: Tag, vr: VR, value: &str) -> InMemElement {
    DataElement::new(tag, vr, PrimitiveValue::from(value))
}

/// Writes each of `originals`, protected by `deidentifier` and then edited
/// by `edit` (given its index), to `directory`.
fn protect_into(
    originals: &[PathBuf],
    directory: &Path,
    deidentifier: &mut Deidentifier<'_>,
    edit: impl Fn(usize, &mut DefaultDicomObject),
) {
    std::fs::create_dir_all(directory).expect("directory");
    for (index, original) in originals.iter().enumerate() {
        let mut object = dicom_object::open_file(original).expect("exported file");
        deidentifier.protect(&mut object);
        edit(index, &mut object);
        object
            .write_to_file(directory.join(format!("level-{index}.dcm")))
            .expect("write");
    }
}

/// The de-identification checks of validating `directory`: per file, then
/// over the set.
fn deidentification_checks(directory: &Path) -> Vec<ValidationCheck> {
    validate_dicom_path_with_runner(directory, &ValidationOptions::default(), &NoTools)
        .expect("validation")
        .checks
        .into_iter()
        .filter(|check| check.name == DEIDENTIFICATION_RULE)
        .collect()
}

fn protected_checks(
    options: &[O],
    skipped: &[Tag],
    edit: impl Fn(usize, &mut DefaultDicomObject),
) -> Vec<ValidationCheck> {
    let root = tempfile::tempdir().expect("tempdir");
    let originals = exported_pyramid(root.path());
    let directory = root.path().join("protected");
    let mut deidentifier = Deidentifier {
        options,
        uids: BTreeMap::new(),
        skipped,
    };
    protect_into(&originals, &directory, &mut deidentifier, edit);
    let checks = deidentification_checks(&directory);
    assert_eq!(
        checks.len(),
        originals.len() + 1,
        "a check per file and one over the set: {checks:#?}"
    );
    checks
}

fn failures(checks: &[ValidationCheck]) -> Vec<&str> {
    checks
        .iter()
        .filter(|check| check.status == ValidationStatus::Failed)
        .map(|check| check.message.as_str())
        .collect()
}

#[test]
fn exported_slides_claiming_no_deidentification_pass_saying_so() {
    let root = tempfile::tempdir().expect("tempdir");
    let originals = exported_pyramid(root.path());
    let checks = deidentification_checks(&root.path().join("original"));
    assert_eq!(checks.len(), originals.len(), "no set check: {checks:#?}");
    for check in checks {
        assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
        assert!(check.message.contains("no de-identification is claimed"));
    }
}

#[test]
fn exported_pyramids_deidentified_with_each_option_combination_pass() {
    let combinations: [&[O]; 4] = [
        &[],
        &[
            O::CleanPixelData,
            O::CleanRecognizableVisualFeatures,
            O::CleanDescriptors,
            O::RetainLongitudinalTemporalInformationModifiedDates,
            O::RetainPatientCharacteristics,
            O::RetainSafePrivate,
        ],
        &[
            O::RetainLongitudinalTemporalInformationFullDates,
            O::RetainUIDs,
            O::RetainDeviceIdentity,
            O::RetainInstitutionIdentity,
        ],
        &[
            O::CleanStructuredContent,
            O::CleanDescriptors,
            O::CleanGraphics,
        ],
    ];
    for options in combinations {
        let checks = protected_checks(options, &[], |_, _| {});
        assert!(
            failures(&checks).is_empty(),
            "{options:?}: {:#?}",
            failures(&checks)
        );
    }
}

#[test]
fn a_pyramid_level_with_its_own_dummy_frame_of_reference_fails_over_the_set() {
    let checks = protected_checks(&[], &[], |index, object| {
        if index == 1 {
            object.put(text(tags::FRAME_OF_REFERENCE_UID, VR::UI, "2.25.123456789"));
        }
    });
    let failed = failures(&checks);
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert!(
        failed[0].contains("disagree on Frame of Reference UID (0020,0052)"),
        "{}",
        failed[0]
    );
}

/// The Options declared, the edit breaking them, and what the failure says.
type Breach = (
    &'static [O],
    fn(usize, &mut DefaultDicomObject),
    &'static str,
);

#[test]
fn breaches_of_the_declared_options_fail_per_file() {
    let cases: [Breach; 3] = [
        (
            &[O::CleanPixelData],
            |_, object| {
                object.put(text(tags::BURNED_IN_ANNOTATION, VR::CS, "YES"));
            },
            "Burned In Annotation (0028,0301) is \"YES\"",
        ),
        (
            &[],
            |_, object| {
                object.put(DataElement::new(
                    tags::OTHER_PATIENT_I_DS_SEQUENCE,
                    VR::SQ,
                    DataSetSequence::from(vec![InMemDicomObject::from_element_iter([text(
                        tags::PATIENT_ID,
                        VR::LO,
                        "MRN-1",
                    )])]),
                ));
            },
            "OtherPatientIDsSequence (0010,1002) is present; Table E.1-1 action X",
        ),
        (
            &[O::RetainLongitudinalTemporalInformationFullDates],
            |_, object| {
                object.put(text(
                    tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
                    VR::CS,
                    "REMOVED",
                ));
            },
            "the Value UNMODIFIED",
        ),
    ];
    for (options, edit, needle) in cases {
        let checks = protected_checks(options, &[], edit);
        let failed = failures(&checks);
        assert!(!failed.is_empty(), "{needle}: no failure");
        assert!(
            failed.iter().all(|message| message.contains(needle)),
            "{needle}: {failed:#?}"
        );
    }
}

#[test]
fn an_instance_uid_kept_beside_its_original_fails_over_the_set() {
    let root = tempfile::tempdir().expect("tempdir");
    let originals = exported_pyramid(root.path());
    let directory = root.path().join("protected");
    let mut deidentifier = Deidentifier {
        options: &[],
        uids: BTreeMap::new(),
        skipped: &[tags::SOP_INSTANCE_UID],
    };
    protect_into(&originals[..1], &directory, &mut deidentifier, |_, _| {});
    std::fs::copy(&originals[1], directory.join("original.dcm")).expect("copy");
    // The original of the protected level, still identified.
    std::fs::copy(&originals[0], directory.join("original-0.dcm")).expect("copy");
    let checks = deidentification_checks(&directory);
    let failed = failures(&checks);
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert!(
        failed[0].contains("so action U did not replace it"),
        "{}",
        failed[0]
    );
}

/// The original Patient's Name of the exported pyramid, and a dummy of its
/// length that a protected File may carry (Table E.1-1 action Z).
const ORIGINAL_NAME: &str = "RESEARCH^PLACEHOLDER";
const DUMMY_NAME: &str = "DUMMYNAME^XXXXXXXXXX";

/// A File-set root holding the protected pyramid as AB/L0 and AB/L1, its
/// DICOMDIR made from the protected Files by dcmmkdir (General Purpose DVD
/// with JPEG 2000, inventing the Type 1 Keys the protection emptied).
fn protected_file_set(root: &Path) -> PathBuf {
    let originals = exported_pyramid(root);
    let file_set = root.join("ROOT");
    let members = file_set.join("AB");
    std::fs::create_dir_all(&members).expect("File-set directory");
    let mut deidentifier = Deidentifier {
        options: &[],
        uids: BTreeMap::new(),
        skipped: &[],
    };
    let mut names = Vec::new();
    for (index, original) in originals.iter().enumerate() {
        let mut object = dicom_object::open_file(original).expect("exported file");
        deidentifier.protect(&mut object);
        object.put(text(tags::PATIENT_NAME, VR::PN, DUMMY_NAME));
        let name = format!("L{index}");
        object.write_to_file(members.join(&name)).expect("write");
        names.push(format!("AB/{name}"));
    }
    let output = Command::new("dcmmkdir")
        .args(["-Pd2", "+I", "+D"])
        .arg(file_set.join("DICOMDIR"))
        .arg("+id")
        .arg(&file_set)
        .args(&names)
        .output()
        .expect("these tests need dcmtk dcmmkdir on PATH");
    assert!(
        output.status.success(),
        "dcmmkdir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    file_set
}

/// The de-identification check over the set validating `file_set` yields.
fn file_set_check(file_set: &Path) -> ValidationCheck {
    let checks = deidentification_checks(file_set);
    let mut over_set: Vec<ValidationCheck> = checks
        .into_iter()
        .filter(|check| check.path.is_none())
        .collect();
    assert_eq!(over_set.len(), 1, "one set check: {over_set:#?}");
    over_set.remove(0)
}

#[test]
fn a_dicomdir_made_from_the_deidentified_files_passes() {
    let root = tempfile::tempdir().expect("tempdir");
    let file_set = protected_file_set(root.path());
    let check = file_set_check(&file_set);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
}

#[test]
fn a_dicomdir_record_keeping_the_original_patients_name_fails() {
    let root = tempfile::tempdir().expect("tempdir");
    let file_set = protected_file_set(root.path());
    let dicomdir = file_set.join("DICOMDIR");
    let mut bytes = std::fs::read(&dicomdir).expect("DICOMDIR");
    let at: Vec<usize> = bytes
        .windows(DUMMY_NAME.len())
        .enumerate()
        .filter(|(_, window)| *window == DUMMY_NAME.as_bytes())
        .map(|(at, _)| at)
        .collect();
    assert_eq!(at.len(), 1, "one PATIENT record holds the name");
    bytes[at[0]..at[0] + ORIGINAL_NAME.len()].copy_from_slice(ORIGINAL_NAME.as_bytes());
    std::fs::write(&dicomdir, bytes).expect("patched DICOMDIR");
    let check = file_set_check(&file_set);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains("PATIENT record at byte")
            && check.message.contains(
                "PatientName (0010,0010) is not the Value any of the 2 de-identified Files"
            ),
        "{}",
        check.message
    );
    // The record's Value may be the original identity: never echoed.
    assert!(!check.message.contains(ORIGINAL_NAME), "{}", check.message);
}

#[test]
fn a_dicomdir_whose_records_cannot_be_read_is_named_not_skipped() {
    let root = tempfile::tempdir().expect("tempdir");
    let file_set = protected_file_set(root.path());
    std::fs::write(file_set.join("DICOMDIR"), b"not a DICOMDIR").expect("DICOMDIR");
    let check = file_set_check(&file_set);
    assert!(
        check
            .message
            .contains("its records cannot be read, so it is not held to PS3.15 E.1.1"),
        "{}",
        check.message
    );
}

/// A Comprehensive SR Content Item naming a person: `concept` (a DCM code
/// and its meaning) with Person Name (0040,A123) `name`.
fn person_item(concept: (&str, &str), name: &str) -> InMemDicomObject {
    InMemDicomObject::from_element_iter([
        text(tags::RELATIONSHIP_TYPE, VR::CS, "HAS OBS CONTEXT"),
        text(tags::VALUE_TYPE, VR::CS, "PNAME"),
        DataElement::new(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![InMemDicomObject::from_element_iter([
                text(tags::CODE_VALUE, VR::SH, concept.0),
                text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "DCM"),
                text(tags::CODE_MEANING, VR::LO, concept.1),
            ])]),
        ),
        text(tags::PERSON_NAME, VR::PN, name),
    ])
}

/// The de-identification check of a Comprehensive SR document protected
/// with the Basic Profile and `options`, whose root holds `items`,
/// validated as a file.
fn report_check(options: &[O], items: Vec<InMemDicomObject>) -> ValidationCheck {
    let uid = "2.25.8001";
    let code = |value: &str, meaning: &str| {
        InMemDicomObject::from_element_iter([
            text(tags::CODE_VALUE, VR::SH, value),
            text(tags::CODING_SCHEME_DESIGNATOR, VR::SH, "DCM"),
            text(tags::CODE_MEANING, VR::LO, meaning),
        ])
    };
    let methods = std::iter::once(BASIC_PROFILE)
        .chain(options.iter().map(|option| option.code()))
        .map(|method| code(method.value, method.meaning))
        .collect::<Vec<_>>();
    let report = InMemDicomObject::from_element_iter([
        text(tags::SOP_CLASS_UID, VR::UI, uids::COMPREHENSIVE_SR_STORAGE),
        text(tags::SOP_INSTANCE_UID, VR::UI, uid),
        text(tags::STUDY_INSTANCE_UID, VR::UI, "2.25.8002"),
        text(tags::SERIES_INSTANCE_UID, VR::UI, "2.25.8003"),
        text(tags::MODALITY, VR::CS, "SR"),
        DataElement::new(tags::PATIENT_NAME, VR::PN, PrimitiveValue::Empty),
        text(tags::PATIENT_ID, VR::LO, "PSEUDO-1"),
        text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "YES"),
        DataElement::new(
            tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(methods),
        ),
        text(
            tags::LONGITUDINAL_TEMPORAL_INFORMATION_MODIFIED,
            VR::CS,
            "REMOVED",
        ),
        text(tags::VALUE_TYPE, VR::CS, "CONTAINER"),
        DataElement::new(
            tags::CONCEPT_NAME_CODE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![code("126000", "Imaging Measurement Report")]),
        ),
        text(tags::CONTINUITY_OF_CONTENT, VR::CS, "SEPARATE"),
        DataElement::new(tags::CONTENT_SEQUENCE, VR::SQ, DataSetSequence::from(items)),
    ])
    .with_meta(
        dicom_object::FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(uids::COMPREHENSIVE_SR_STORAGE)
            .media_storage_sop_instance_uid(uid)
            .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
    )
    .expect("file meta");
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("report.dcm");
    report.write_to_file(&path).expect("write report");
    let mut checks: Vec<ValidationCheck> = deidentification_checks(root.path())
        .into_iter()
        .filter(|check| check.path.as_deref() == Some(path.as_path()))
        .collect();
    assert_eq!(
        checks.len(),
        1,
        "the rule runs on the SR document: {checks:#?}"
    );
    checks.remove(0)
}

/// SR documents are held to Table E.1-1 at every depth and, under the Clean
/// Structured Content Option, to Table E.3.4-1 on their Content Items.
#[test]
fn a_deidentified_report_passes_unless_a_content_item_leaks_a_person() {
    // Table E.3.4-1: Person Observer Name D, which a dummy satisfies.
    let observer = person_item(("121008", "Person Observer Name"), "DUMMY^OBSERVER");
    let check = report_check(&[O::CleanStructuredContent], vec![observer.clone()]);
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    // Table E.3.4-1: Fellow X, removed under Clean Structured Content.
    let fellow = person_item(("121088", "Fellow"), "JONES^ALICE");
    let check = report_check(&[O::CleanStructuredContent], vec![observer, fellow]);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(
            "ContentSequence (0040,A730)[2] > Content Item (121088, DCM, \"Fellow\") PNAME is present; Table E.3.4-1 action X requires its removal"
        ),
        "{}",
        check.message
    );
}

/// E.3.1 "is intended to apply to the Pixel Data (7FE0,0010) Attribute that
/// occurs in the top level Data Set of an Image Storage SOP Instance": an SR
/// document declaring the pixel Options needs neither Burned In Annotation
/// nor Recognizable Visual Features, and is told they do not apply.
#[test]
fn a_report_declaring_the_pixel_options_passes_without_their_attributes() {
    let observer = person_item(("121008", "Person Observer Name"), "DUMMY^OBSERVER");
    let check = report_check(
        &[O::CleanPixelData, O::CleanRecognizableVisualFeatures],
        vec![observer],
    );
    assert_eq!(check.status, ValidationStatus::Passed, "{}", check.message);
    for needle in [
        "so its Burned In Annotation (0028,0301) NO does not apply (E.3.1: \"This Option is intended to apply to the Pixel Data (7FE0,0010) Attribute that occurs in the top level Data Set of an Image Storage SOP Instance\")",
        "so its Recognizable Visual Features (0028,0302) NO does not apply",
    ] {
        assert!(check.message.contains(needle), "{needle}: {}", check.message);
    }
}
