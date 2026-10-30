//! No message shows the Value of an identifying Attribute (PS3.15 Table
//! E.1-1, `disclosure`): the convention checks and advisories that compare
//! identities, and the whole report of a scan whose every identifying Value
//! is one sentinel string, read as files and as DICOM JSON.

use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::fixture::{item, sq, text};
use super::*;
use crate::dicomweb::tests::fixture::to_json;
use crate::validation::disclosure::Nesting;
use crate::validation::{validate_dicom_json_path, DicomJsonOptions};

const PROFILES: [ConventionProfile; 5] = [
    ConventionProfile::Wg26,
    ConventionProfile::IheDpia,
    ConventionProfile::IheDpiaDraft,
    ConventionProfile::IheDpiaDraftJpeg,
    ConventionProfile::Interop,
];

/// The Value Representations a sentinel string is planted in.
const TEXT_VRS: [VR; 16] = [
    VR::AE,
    VR::AS,
    VR::CS,
    VR::DA,
    VR::DS,
    VR::DT,
    VR::IS,
    VR::LO,
    VR::LT,
    VR::PN,
    VR::SH,
    VR::ST,
    VR::TM,
    VR::UC,
    VR::UR,
    VR::UT,
];

/// Every message of the convention report: checks and advisories.
fn convention_messages(report: &ValidationReport) -> Vec<String> {
    report
        .conventions
        .checks
        .iter()
        .map(|check| check.message.clone())
        .chain(
            report
                .conventions
                .advisories
                .iter()
                .map(|advisory| advisory.message.clone()),
        )
        .collect()
}

#[test]
fn convention_messages_withhold_identifying_values() {
    let scan = Scan::conformant(130)
        // An annotation and a completion KOS of another patient and slide.
        .edit(ANNOTATION, |object| {
            object.put(text(tags::PATIENT_ID, VR::LO, "MRN12345"));
            object.put(text(tags::PATIENT_NAME, VR::PN, "DOE^JANE"));
        })
        .edit(COMPLETION, |object| {
            object.put(text(tags::PATIENT_ID, VR::LO, "MRN12345"));
            object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "DOE^JANE"));
        })
        // A Series of two slides, a barcode of another, and a Request
        // Attributes Item of another accession.
        .edit(VOLUME_3, |object| {
            object.put(text(tags::CONTAINER_IDENTIFIER, VR::LO, "DOE^JANE"));
        })
        .edit(LABEL, |object| {
            object.put(text(tags::BARCODE_VALUE, VR::LT, "MRN12345"));
        })
        .edit(VOLUME_2, |object| {
            object.put(sq(
                tags::REQUEST_ATTRIBUTES_SEQUENCE,
                vec![item(vec![text(tags::ACCESSION_NUMBER, VR::SH, "MRN12345")])],
            ));
        });
    let dir = FixtureDir::new("withheld-conventions");
    let report = report(&scan.write(&dir.path), &PROFILES);
    let failed = names_with(&report, ValidationStatus::Failed);
    for check in [
        "convention-wg26-annotation-identity",
        "convention-wg26-one-series-per-scan",
        "convention-ihe-dpia-request-attributes",
        "convention-ihe-dpia-draft-completion-identity",
    ] {
        assert!(
            failed.contains(check),
            "{check} did not fail: {}",
            describe(&report)
        );
    }
    let messages = convention_messages(&report);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("(value withheld: identifying; 8 characters)")),
        "{messages:#?}"
    );
    for message in &messages {
        for planted in ["MRN12345", "DOE^JANE"] {
            assert!(!message.contains(planted), "{planted} in {message}");
        }
    }
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains("MRN12345") && !json.contains("DOE^JANE"),
        "{json}"
    );
}

/// The sentinel planted in every identifying Value, and a variant of it
/// planted in the annotation and the KOS so the identities they share with
/// the images differ.
const SENTINEL: &str = "QZXSENTINELQZX";
const VARIANT: &str = "QZXSENTINELQZY";
/// What both share; no report may hold it.
const SENTINEL_STEM: &str = "QZXSENTINEL";

/// Puts `sentinel` in each text Value of `object`, a Data Set whose Values
/// `nesting` withholds, that messages withhold; returns how many.
fn plant(object: &mut InMemDicomObject, nesting: Nesting, sentinel: &str) -> usize {
    let tags: Vec<Tag> = object.iter().map(|element| element.header().tag).collect();
    let mut planted = 0;
    for tag in tags {
        let Some(element) = object.get(tag) else {
            continue;
        };
        let vr = element.vr();
        if element.items().is_some() {
            let mut element = object.take(tag).expect("the Sequence");
            if let Some(items) = element.items_mut() {
                for item in items.iter_mut() {
                    let nested = nesting.item(tag, item);
                    planted += plant(item, nested, sentinel);
                }
            }
            object.put(element);
        } else if TEXT_VRS.contains(&vr) && nesting.withholds(tag) {
            object.put(DataElement::new(tag, vr, PrimitiveValue::from(sentinel)));
            planted += 1;
        }
    }
    planted
}

/// The scan as one DICOM JSON document, its ICC Profiles and Pixel Data as
/// BulkDataURIs, written to `dir/scan.json`.
fn json_document(scan: &Scan, dir: &Path) -> PathBuf {
    let data_sets: Vec<String> = scan
        .objects
        .iter()
        .map(|(_, object)| {
            let json = to_json(object, &|tag, top| match tag {
                tags::ICC_PROFILE => Some("bulk/icc".to_string()),
                tags::PIXEL_DATA if top => Some("bulk/pixeldata".to_string()),
                _ => None,
            });
            json[1..json.len() - 1].to_string()
        })
        .collect();
    let path = dir.join("scan.json");
    std::fs::write(&path, format!("[{}]", data_sets.join(","))).expect("write document");
    path
}

/// The conformant scan with the sentinel in every identifying Value.
fn sentinel_scan() -> Scan {
    let mut scan = Scan::conformant(131);
    let mut planted = 0;
    for (name, object) in &mut scan.objects {
        let sentinel = if [ANNOTATION, COMPLETION].contains(&name.as_str()) {
            VARIANT
        } else {
            SENTINEL
        };
        planted += plant(object, Nesting::default(), sentinel);
    }
    // Patient's Name, ID, Birth Date and Sex, Study Date and Time,
    // Accession Number, Study ID, Container and Specimen Identifiers and
    // more, in seven objects.
    assert!(planted > 70, "{planted} Values planted");
    scan
}

#[test]
fn no_report_shows_a_value_planted_in_every_identifying_attribute() {
    let scan = sentinel_scan();
    let dir = FixtureDir::new("withheld-sentinel");
    let report = report(&scan.write(&dir.path), &PROFILES);
    let json = serde_json::to_string(&report).unwrap();
    // The sentinel breaks dates, numbers and Enumerated Values, and the
    // variant the identities the annotation and KOS share with the images,
    // so messages reach the withheld Values.
    assert!(
        report.has_failures() && json.contains("(value withheld: identifying; 14 characters)"),
        "{json}"
    );
    assert!(!json.contains(SENTINEL_STEM), "{json}");

    let mut options = DicomJsonOptions::default();
    options.validation.conventions = PROFILES.into_iter().collect();
    let document = json_document(&scan, &dir.path);
    let json_report = validate_dicom_json_path(&document, &options).expect("JSON report");
    let json = serde_json::to_string(&json_report).unwrap();
    assert!(json.contains("(value withheld: identifying"), "{json}");
    assert!(!json.contains(SENTINEL_STEM), "{json}");
}
