//! `ihe-dpia`: each Appendix B rule a file or set shows fails on its broken
//! variant, and the profile's expectations and conflicts are advisories.

use dicom_core::VR;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::fixture::{self, code, item, sq, text};
use super::wg26::edit_specimen;
use super::*;

const P: &[ConventionProfile] = &[ConventionProfile::IheDpia];

/// Applies `edit` to the Specimen Preparation Sequence Items of every image.
fn edit_steps(scan: Scan, edit: impl Fn(&mut Vec<InMemDicomObject>)) -> Scan {
    scan.edit_images(|object| {
        edit_specimen(object, |specimen| {
            let mut steps = specimen
                .take(tags::SPECIMEN_PREPARATION_SEQUENCE)
                .expect("preparation");
            let mut items: Vec<InMemDicomObject> = steps.items().expect("items").to_vec();
            edit(&mut items);
            steps = sq(tags::SPECIMEN_PREPARATION_SEQUENCE, items);
            specimen.put(steps);
        });
    })
}

/// Applies `edit` to the Content Items of Specimen Preparation Sequence
/// Item `step`.
fn edit_content(scan: Scan, step: usize, edit: impl Fn(&mut Vec<InMemDicomObject>)) -> Scan {
    edit_steps(scan, |steps| {
        let mut content: Vec<InMemDicomObject> = steps[step]
            .get(tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
            .and_then(|element| element.items())
            .expect("content items")
            .to_vec();
        edit(&mut content);
        steps[step].put(sq(
            tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
            content,
        ));
    })
}

#[test]
fn request_attributes_repeat_the_accession_and_study() {
    let other_accession = Scan::conformant(50).edit(VOLUME_2, |object| {
        object.put(sq(
            tags::REQUEST_ATTRIBUTES_SEQUENCE,
            vec![item(vec![text(tags::ACCESSION_NUMBER, VR::SH, "A9")])],
        ));
    });
    let report = assert_outcome(
        "dpia-request-accession",
        &other_accession,
        P,
        &["convention-ihe-dpia-request-attributes"],
        &[],
    );
    let message = failure_message(&report, "convention-ihe-dpia-request-attributes");
    assert!(message.contains("\"A9\""), "{message}");
    assert!(message.contains("Study Instance UID"), "{message}");
    assert!(message.contains("Issuer of Accession Number"), "{message}");
    let absent = Scan::conformant(51).edit(LABEL, |object| {
        object.remove_element(tags::REQUEST_ATTRIBUTES_SEQUENCE);
    });
    assert_outcome(
        "dpia-request-absent",
        &absent,
        P,
        &["convention-ihe-dpia-request-attributes"],
        &[],
    );
}

#[test]
fn order_numbers_in_the_request_attributes_are_standard_extended() {
    let scan = Scan::conformant(52).edit(VOLUME_1, |object| {
        let mut requests = object
            .take(tags::REQUEST_ATTRIBUTES_SEQUENCE)
            .expect("request attributes");
        requests.items_mut().expect("items")[0].put(text(
            tags::PLACER_ORDER_NUMBER_IMAGING_SERVICE_REQUEST,
            VR::LO,
            "ORDER-1",
        ));
        object.put(requests);
    });
    let report = assert_outcome(
        "dpia-order-numbers",
        &scan,
        P,
        &[],
        &["convention-ihe-dpia-order-numbers-standard-extended"],
    );
    // DICOM conformance is decided by the DICOM checks alone.
    let presence: Vec<&crate::validation::ValidationCheck> = report
        .checks
        .iter()
        .filter(|check| check.name == "intrinsic-wsi-dicom-iod-attribute-presence")
        .filter(|check| {
            check
                .path
                .as_ref()
                .is_some_and(|path| path.ends_with("volume-1.dcm"))
        })
        .collect();
    assert_eq!(presence.len(), 1);
    assert_eq!(
        presence[0].status,
        ValidationStatus::Passed,
        "{}",
        presence[0].message
    );
}

#[test]
fn each_preparation_step_of_appendix_b_is_required() {
    let no_date = edit_content(Scan::conformant(53), 0, |content| {
        content.remove(3);
    });
    let report = assert_outcome(
        "dpia-no-collection-date",
        &no_date,
        P,
        &["convention-ihe-dpia-collection-step"],
        &[],
    );
    assert!(failure_message(&report, "convention-ihe-dpia-collection-step").contains("111702"));
    let no_fixation = edit_steps(Scan::conformant(54), |steps| {
        steps.remove(1);
    });
    assert_outcome(
        "dpia-no-fixation",
        &no_fixation,
        P,
        &["convention-ihe-dpia-fixation-step"],
        &[],
    );
    let no_embedding = edit_steps(Scan::conformant(55), |steps| {
        steps.remove(2);
    });
    assert_outcome(
        "dpia-no-embedding",
        &no_embedding,
        P,
        &["convention-ihe-dpia-embedding-step"],
        &[],
    );
    // The staining steps are recorded as generic processing.
    let no_staining = edit_steps(Scan::conformant(56), |steps| {
        for step in [3, 4] {
            let mut content: Vec<InMemDicomObject> = steps[step]
                .get(tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
                .and_then(|element| element.items())
                .expect("content items")
                .to_vec();
            content[2] = fixture::coded(
                code("DCM", "111701", "Processing type"),
                code("SCT", "9265001", "Specimen processing"),
            );
            steps[step].put(sq(
                tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                content,
            ));
        }
    });
    assert_outcome(
        "dpia-no-staining",
        &no_staining,
        P,
        &["convention-ihe-dpia-staining-step"],
        &[],
    );
}

#[test]
fn every_step_names_the_specimen_and_its_issuer() {
    let other = edit_content(Scan::conformant(57), 2, |content| {
        content[0] = fixture::content(
            "TEXT",
            code("DCM", "121041", "Specimen Identifier"),
            text(tags::TEXT_VALUE, VR::UT, "SPECIMEN-9"),
        );
        content.remove(1);
    });
    let report = assert_outcome(
        "dpia-step-identifier",
        &other,
        P,
        &["convention-ihe-dpia-step-specimen-identifier"],
        &[],
    );
    let message = failure_message(&report, "convention-ihe-dpia-step-specimen-identifier");
    assert!(
        message.contains("SPECIMEN-9") && message.contains("111724"),
        "{message}"
    );
}

#[test]
fn a_clean_structured_content_object_need_not_name_the_specimen_in_steps() {
    let scan = edit_steps(Scan::conformant(58), |steps| {
        for step in steps.iter_mut() {
            let content: Vec<InMemDicomObject> = step
                .get(tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE)
                .and_then(|element| element.items())
                .expect("content items")[2..]
                .to_vec();
            step.put(sq(
                tags::SPECIMEN_PREPARATION_STEP_CONTENT_ITEM_SEQUENCE,
                content,
            ));
        }
    })
    .edit_images(|object| {
        object.put(text(tags::PATIENT_IDENTITY_REMOVED, VR::CS, "YES"));
        object.put(sq(
            tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
            vec![
                code("DCM", "113100", "Basic Application Confidentiality Profile"),
                code("DCM", "113104", "Clean Structured Content Option"),
            ],
        ));
    });
    let report = assert_outcome(
        "dpia-deidentified-steps",
        &scan,
        P,
        &[],
        &["convention-ihe-dpia-deidentified-step-identifiers"],
    );
    assert!(names_with(&report, ValidationStatus::Skipped)
        .contains("convention-ihe-dpia-step-specimen-identifier"));
}

#[test]
fn steps_newest_first_are_out_of_chronological_order() {
    let scan = edit_steps(Scan::conformant(59), |steps| steps.reverse());
    let report = assert_outcome(
        "dpia-newest-first",
        &scan,
        P,
        &["convention-ihe-dpia-step-order"],
        &["convention-ihe-dpia-preparation-order-conflict"],
    );
    assert!(failure_message(&report, "convention-ihe-dpia-step-order").contains("C.7.6.22.1.3"));
}

#[test]
fn the_specimen_type_is_coded_and_hl7_codes_are_a_conflict() {
    let absent = Scan::conformant(60).edit_images(|object| {
        edit_specimen(object, |specimen| {
            specimen.remove_element(tags::SPECIMEN_TYPE_CODE_SEQUENCE);
        });
    });
    assert_outcome(
        "dpia-no-specimen-type",
        &absent,
        P,
        &["convention-ihe-dpia-specimen-type"],
        &[],
    );
    let hl7 = Scan::conformant(61).edit(VOLUME_1, |object| {
        edit_specimen(object, |specimen| {
            specimen.put(sq(
                tags::SPECIMEN_TYPE_CODE_SEQUENCE,
                vec![code("HL70487", "TISS", "Tissue")],
            ));
        });
    });
    assert_outcome(
        "dpia-hl7-specimen-type",
        &hl7,
        P,
        &[],
        &["convention-ihe-dpia-specimen-type-coding"],
    );
}

#[test]
fn patient_sex_comes_from_pid_8() {
    let scan = Scan::conformant(62).edit_images(|object| {
        object.put(text(tags::PATIENT_SEX, VR::CS, "O"));
    });
    assert_outcome(
        "dpia-patient-sex",
        &scan,
        P,
        &["convention-ihe-dpia-patient-sex"],
        &[],
    );
}

#[test]
fn a_study_maps_one_to_one_to_an_accession() {
    let first = Scan::conformant(63).without(ANNOTATION).without(COMPLETION);
    let study = first.slide.study.clone();
    // A second slide of the Study under another accession.
    let second = Scan::conformant(64)
        .without(ANNOTATION)
        .without(COMPLETION)
        .edit_images(|object| {
            object.put(text(tags::STUDY_INSTANCE_UID, VR::UI, &study));
            object.put(text(tags::ACCESSION_NUMBER, VR::SH, "A2"));
            object.put(sq(
                tags::REQUEST_ATTRIBUTES_SEQUENCE,
                vec![item(vec![
                    text(tags::ACCESSION_NUMBER, VR::SH, "A2"),
                    sq(
                        tags::ISSUER_OF_ACCESSION_NUMBER_SEQUENCE,
                        vec![item(vec![text(
                            tags::LOCAL_NAMESPACE_ENTITY_ID,
                            VR::UT,
                            "FIXTURE-LIS",
                        )])],
                    ),
                    text(tags::STUDY_INSTANCE_UID, VR::UI, &study),
                ])],
            ));
        });
    let report = assert_outcome(
        "dpia-accession-study",
        &first.and(second),
        P,
        &["convention-ihe-dpia-accession-study"],
        &[],
    );
    assert!(failure_message(&report, "convention-ihe-dpia-accession-study").contains("\"A2\""));
}

#[test]
fn label_and_container_expectations_are_advisories() {
    let scan = Scan::conformant(65)
        .edit(LABEL, |object| {
            object.put(text(tags::BARCODE_VALUE, VR::LT, "\\SLIDE-1\\"));
            object.put(text(tags::LABEL_TEXT, VR::UT, ""));
        })
        .edit(OVERVIEW, |object| {
            object.put(sq(tags::CONTAINER_TYPE_CODE_SEQUENCE, Vec::new()));
            let mut specimens = object
                .take(tags::SPECIMEN_DESCRIPTION_SEQUENCE)
                .expect("specimens");
            let control = specimens.items().expect("items")[0].clone();
            specimens.items_mut().expect("items").push(control);
            object.put(specimens);
        });
    let report = assert_outcome(
        "dpia-advisories",
        &scan,
        P,
        &[],
        &[
            "convention-ihe-dpia-barcode-container",
            "convention-ihe-dpia-container-type",
            "convention-ihe-dpia-label-text",
            "convention-ihe-dpia-single-specimen",
        ],
    );
    assert!(
        advisory_message(&report, "convention-ihe-dpia-barcode-container")
            .contains("Open Issue 32")
    );
}
