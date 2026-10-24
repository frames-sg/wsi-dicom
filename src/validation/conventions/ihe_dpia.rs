//! `ihe-dpia`: the IHE PaLM Digital Pathology Workflow - Image Acquisition
//! (DPIA) Rev. 1.4 Trial Implementation rules that a VL WSI object or set
//! shows: the Appendix B mapping of the LAB-80 message to DICOM Attributes
//! and the profile's identification assumptions. The rest of Appendix B
//! compares an object with the HL7 message that drove it.

use std::collections::{BTreeMap, BTreeSet};

use dicom_dictionary_std::tags;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::facts::{describe_code, is_code, text, Facts, Kind};
use super::specimen::{
    specimens, Specimen, DATETIME_OF_PROCESSING, EMBEDDING_MEDIUM, ISSUER_OF_SPECIMEN_IDENTIFIER,
    SPECIMEN_COLLECTION, SPECIMEN_IDENTIFIER, SPECIMEN_PROCESSING, STAINING, TISSUE_FIXATIVE,
    USING_SUBSTANCE,
};
use super::{advisory, check, ConventionEntry, ConventionProfile, Out, Subject, Violations};
use crate::validation::disclosure::{difference, shown, shown_all, shown_in, shown_or};
use crate::validation::iod::{dicom_edition, items};

const P: ConventionProfile = ConventionProfile::IheDpia;

/// A part of the DPIA Rev. 1.4 supplement.
macro_rules! dpia {
    ($part:literal) => {
        concat!(
            "IHE PaLM DPIA Rev. 1.4 (Trial Implementation, 2026-03-06) ",
            $part
        )
    };
}

pub(super) const REQUEST_ATTRIBUTES: ConventionEntry = check(
    P,
    "convention-ihe-dpia-request-attributes",
    dpia!("Appendix B Table B.1-1, Request Attributes Sequence rows"),
    "The Request Attributes Sequence (0040,0275) has Items, and each one's Accession Number (0008,0050), Issuer of Accession Number Sequence (0008,0051) and Study Instance UID (0020,000D) are those of the top level (\"Same as in top level dataset\").",
);
pub(super) const SPECIMEN_TYPE: ConventionEntry = check(
    P,
    "convention-ihe-dpia-specimen-type",
    dpia!("Appendix B Table B.1-1, Specimen Type Code Sequence row (SPM-4)"),
    "Each Specimen Description Item has a Specimen Type Code Sequence (0040,059A) Item, mapped from SPM-4.",
);
pub(super) const COLLECTION_STEP: ConventionEntry = check(
    P,
    "convention-ihe-dpia-collection-step",
    dpia!("Appendix B Table B.2-1 item 1"),
    "Each Specimen Description Item has a collection step: Processing type (111701, DCM) (17636008, SCT, \"Specimen collection\") with a DateTime of processing (111702, DCM) from SPM-17 and a coded (17636008, SCT, \"Specimen Collection\") procedure from SPM-7.",
);
pub(super) const FIXATION_STEP: ConventionEntry = check(
    P,
    "convention-ihe-dpia-fixation-step",
    dpia!("Appendix B Table B.2-1 item 2"),
    "Each Specimen Description Item has a fixation step: Processing type (9265001, SCT, \"Specimen Processing\") with a coded (430864009, SCT, \"Tissue Fixative\").",
);
pub(super) const EMBEDDING_STEP: ConventionEntry = check(
    P,
    "convention-ihe-dpia-embedding-step",
    dpia!("Appendix B Table B.2-1 item 3"),
    "Each Specimen Description Item has an embedding step: Processing type (9265001, SCT, \"Specimen Processing\") with a coded (430863003, SCT, \"Embedding medium\").",
);
pub(super) const STAINING_STEP: ConventionEntry = check(
    P,
    "convention-ihe-dpia-staining-step",
    dpia!("Appendix B Table B.2-1 items 4 to n"),
    "Each Specimen Description Item has at least one staining step: Processing type (127790008, SCT, \"Staining\") with a coded (424361007, SCT, \"Using substance\") from OBX-5 of LN 8026-7 \"Stain method\".",
);
pub(super) const STEP_IDENTIFIER: ConventionEntry = check(
    P,
    "convention-ihe-dpia-step-specimen-identifier",
    dpia!("Appendix B Table B.2-1 items 1 to n"),
    "Every preparation step has (121041, DCM, \"Specimen Identifier\") equal to the Item's Specimen Identifier (SPM-2.1) and, when the Issuer of the Specimen Identifier Sequence names one, (111724, DCM, \"Issuer of Specimen Identifier\") equal to it (SPM-2.2).",
);
pub(super) const STEP_ORDER: ConventionEntry = check(
    P,
    "convention-ihe-dpia-step-order",
    concat!(
        "IHE PaLM DPIA Rev. 1.4 (Trial Implementation, 2026-03-06) Appendix B Table B.2-1 item numbers, and DICOM PS3.3 ",
        dicom_edition!(),
        " C.7.6.22.1.3"
    ),
    "The steps are in ascending chronological order: collection, then fixation, then embedding, then staining.",
);
pub(super) const PATIENT_SEX: ConventionEntry = check(
    P,
    "convention-ihe-dpia-patient-sex",
    dpia!("Appendix B Table B.1-1 Patient's Sex row (PID-8) and Table A.4-5"),
    "Patient's Sex (0010,0040) is F, M or empty: PID-8 holds only F, M or U (Unknown), and U has no DICOM Value.",
);
pub(super) const ACCESSION_STUDY: ConventionEntry = check(
    P,
    "convention-ihe-dpia-accession-study",
    dpia!("Closed Issues 14A and 14B, and Table A.2-4b"),
    "\"The Study Instance UID will map 1:1 with the case accession number\": across the set, each Study Instance UID has one Accession Number and each Accession Number one Study Instance UID.",
);
pub(super) const BARCODE_CONTAINER: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-barcode-container",
    dpia!("Appendix B Table B.1-1 Barcode Value row, Example column"),
    "Barcode Value (2200,0005) \"Will have same value as Container Identifier (0040,0512), since used as query key in LAB-81 QPD-3\": a Barcode Value that differs from the Container Identifier.",
);
pub(super) const CONTAINER_TYPE: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-container-type",
    dpia!("Appendix B Table B.1-1 Container Type Code Sequence row (SAC-47, SPM-27)"),
    "Container Type Code Sequence (0040,0518) mapped from SAC-47, example (433466003, SCT, \"Microscope slide\"): absent, empty or another code.",
);
pub(super) const LABEL_TEXT: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-label-text",
    dpia!("Appendix B Table B.1-1 Label Text row"),
    "Label Text (2200,0002) is \"Populated by scanner by scanning slide label and performing OCR\": a LABEL image without Label Text.",
);
pub(super) const SINGLE_SPECIMEN: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-single-specimen",
    dpia!("Closed Issue 7"),
    "\"only one patient's sample for diagnostic interpretation placed on a given glass slide ... Positive control tissue may optionally be present\": more than one Specimen Description Item.",
);
pub(super) const ORDER_NUMBERS_EXTENDED: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-order-numbers-standard-extended",
    concat!(
        "IHE PaLM DPIA Rev. 1.4 Appendix B Table B.1-1 order number rows, and DICOM PS3.3 ",
        dicom_edition!(),
        " Table 10-9"
    ),
    "Conflict: DPIA maps ORC-2/ORC-3 to Placer and Filler Order Numbers (0040,2016)/(0040,2017) and their Identifier Sequences (0040,0026)/(0040,0027) inside the Request Attributes Sequence (\"Not actually in Request Attributes Sequence - optional extension\"). The Request Attributes Macro defines none of them, so the object is a Standard Extended SOP Instance; the DICOM checks decide its conformance and ihe-dpia does not require them.",
);
pub(super) const SPECIMEN_TYPE_CODING: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-specimen-type-coding",
    concat!(
        "IHE PaLM DPIA Rev. 1.4 Table A.5-4 (SPM-4, HL7 Table 0487) against DICOM PS3.16 ",
        dicom_edition!(),
        " CID 8103"
    ),
    "Conflict: DPIA's SPM-4 Specimen Type takes HL7 Table 0487 codes, but the DICOM Specimen Type Code Sequence's Baseline CID 8103 \"Anatomic Pathology Specimen Type\" is SNOMED CT, as the WG-26 2026 connectathon requires. ihe-dpia accepts any coded Specimen Type and reports codes of another scheme here.",
);
pub(super) const PREPARATION_ORDER_CONFLICT: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-preparation-order-conflict",
    concat!(
        "DICOM PS3.17 ",
        dicom_edition!(),
        " NN.6.2 against PS3.3 ",
        dicom_edition!(),
        " C.7.6.22.1.3"
    ),
    "Conflict: PS3.17 NN.6.2 (informative) says its example \"first describes the most recent processing of the slide (staining), then goes back\", but PS3.3 C.7.6.22.1.3 (normative) requires \"ascending chronological order\". The validator follows PS3.3 and reports steps that run newest first here.",
);
pub(super) const DEIDENTIFIED_STEPS: ConventionEntry = advisory(
    P,
    "convention-ihe-dpia-deidentified-step-identifiers",
    concat!(
        "DICOM PS3.15 ",
        dicom_edition!(),
        " Table E.3.4-1 against PS3.16 ",
        dicom_edition!(),
        " TID 8001 and IHE PaLM DPIA Rev. 1.4 Table B.2-1"
    ),
    "Conflict: the Clean Structured Content Option removes (121041, DCM, \"Specimen Identifier\") and (111724, DCM, \"Issuer of Specimen Identifier\") Content Items, which TID 8001 and DPIA Table B.2-1 put in every step. For an object that declares Patient Identity Removed YES with (113104, DCM, \"Clean Structured Content Option\"), PS3.15 is followed and convention-ihe-dpia-step-specimen-identifier is skipped.",
);

pub(super) const ENTRIES: &[&ConventionEntry] = &[
    &REQUEST_ATTRIBUTES,
    &SPECIMEN_TYPE,
    &COLLECTION_STEP,
    &FIXATION_STEP,
    &EMBEDDING_STEP,
    &STAINING_STEP,
    &STEP_IDENTIFIER,
    &STEP_ORDER,
    &PATIENT_SEX,
    &ACCESSION_STUDY,
    &BARCODE_CONTAINER,
    &CONTAINER_TYPE,
    &LABEL_TEXT,
    &SINGLE_SPECIMEN,
    &ORDER_NUMBERS_EXTENDED,
    &SPECIMEN_TYPE_CODING,
    &PREPARATION_ORDER_CONFLICT,
    &DEIDENTIFIED_STEPS,
];

/// The order-number Attributes DPIA puts in the Request Attributes Sequence.
const ORDER_NUMBERS: [(dicom_core::Tag, &str); 4] = [
    (
        tags::PLACER_ORDER_NUMBER_IMAGING_SERVICE_REQUEST,
        "Placer Order Number / Imaging Service Request (0040,2016)",
    ),
    (
        tags::ORDER_PLACER_IDENTIFIER_SEQUENCE,
        "Order Placer Identifier Sequence (0040,0026)",
    ),
    (
        tags::FILLER_ORDER_NUMBER_IMAGING_SERVICE_REQUEST,
        "Filler Order Number / Imaging Service Request (0040,2017)",
    ),
    (
        tags::ORDER_FILLER_IDENTIFIER_SEQUENCE,
        "Order Filler Identifier Sequence (0040,0027)",
    ),
];

pub(super) fn file_checks(object: &DefaultDicomObject, facts: &Facts, out: &mut Out) {
    if facts.kind != Kind::Wsi {
        return;
    }
    let root: &InMemDicomObject = object;
    let file = Subject::File(&facts.path);
    out.check(&REQUEST_ATTRIBUTES, file, request_attributes(root));
    order_numbers(root, facts, out);
    out.check(&PATIENT_SEX, file, patient_sex(facts));
    let specimens = specimens(root);
    out.check(&SPECIMEN_TYPE, file, specimen_type(&specimens));
    specimen_type_coding(&specimens, facts, out);
    out.check(&COLLECTION_STEP, file, each(&specimens, collection_step));
    out.check(
        &FIXATION_STEP,
        file,
        each(&specimens, |specimen| {
            processing_step(specimen, SPECIMEN_PROCESSING, TISSUE_FIXATIVE, "fixation")
        }),
    );
    out.check(
        &EMBEDDING_STEP,
        file,
        each(&specimens, |specimen| {
            processing_step(specimen, SPECIMEN_PROCESSING, EMBEDDING_MEDIUM, "embedding")
        }),
    );
    out.check(
        &STAINING_STEP,
        file,
        each(&specimens, |specimen| {
            processing_step(specimen, STAINING, USING_SUBSTANCE, "staining")
        }),
    );
    if clean_structured_content(root) {
        out.skip(
            &STEP_IDENTIFIER,
            file,
            "the object declares Patient Identity Removed YES with the Clean Structured Content Option (113104, DCM), which removes (121041) and (111724) Content Items (PS3.15 Table E.3.4-1)".into(),
        );
        out.advise(
            &DEIDENTIFIED_STEPS,
            file,
            "the step Specimen Identifier and Issuer Content Items are not required of this de-identified object".into(),
        );
    } else {
        out.check(&STEP_IDENTIFIER, file, each(&specimens, step_identifiers));
    }
    let (order, newest_first) = step_order(&specimens);
    out.check(&STEP_ORDER, file, order);
    if newest_first {
        out.advise(
            &PREPARATION_ORDER_CONFLICT,
            file,
            "the Specimen Preparation Sequence lists staining before the earlier steps, as PS3.17 NN.6.2 describes; PS3.3 C.7.6.22.1.3 requires ascending chronological order".into(),
        );
    }
    label_advisories(root, facts, out);
    if specimens.len() > 1 {
        out.advise(
            &SINGLE_SPECIMEN,
            file,
            format!(
                "the Specimen Description Sequence has {} Items; DPIA assumes one patient sample per slide, with optional positive control tissue",
                specimens.len()
            ),
        );
    }
}

pub(super) fn set_checks(facts: &[Facts], out: &mut Out) {
    let mut accessions = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut studies = BTreeMap::<&str, BTreeSet<&str>>::new();
    for fact in facts.iter().filter(|fact| fact.is_wsi()) {
        let Some(study) = fact.study_uid.as_deref() else {
            continue;
        };
        let accession = fact.accession.as_deref().unwrap_or("");
        studies.entry(study).or_default().insert(accession);
        accessions.entry(accession).or_default().insert(study);
    }
    if studies.is_empty() {
        return;
    }
    let mut found = Violations::default();
    for (study, numbers) in &studies {
        if numbers.len() > 1 {
            found.push(|| {
                format!(
                    "Study Instance UID {} has {} Accession Numbers, {}",
                    shown(tags::STUDY_INSTANCE_UID, study),
                    numbers.len(),
                    shown_all(tags::ACCESSION_NUMBER, numbers.iter().copied(), " and ")
                )
            });
        }
    }
    for (accession, uids) in &accessions {
        if uids.len() > 1 {
            found.push(|| {
                format!(
                    "Accession Number {} has Study Instance UIDs {}",
                    shown(tags::ACCESSION_NUMBER, accession),
                    shown_all(tags::STUDY_INSTANCE_UID, uids.iter().copied(), ", ")
                )
            });
        }
    }
    out.check(&ACCESSION_STUDY, Subject::Set, found.result());
}

/// The first result of `rule` per Specimen Description Item, joined.
fn each(
    specimens: &[Specimen<'_>],
    rule: impl Fn(&Specimen<'_>) -> Result<(), String>,
) -> Result<(), String> {
    if specimens.is_empty() {
        return Err("the Specimen Description Sequence (0040,0560) has no Item".into());
    }
    let mut found = Violations::default();
    for specimen in specimens {
        if let Err(message) = rule(specimen) {
            found.push(|| message);
        }
    }
    found.result()
}

/// The Issuer of Accession Number Sequence's first Item, as compared.
fn issuer(object: &InMemDicomObject) -> Option<(Option<String>, Option<String>, Option<String>)> {
    items(object, tags::ISSUER_OF_ACCESSION_NUMBER_SEQUENCE)
        .first()
        .map(|item| {
            (
                text(item, tags::LOCAL_NAMESPACE_ENTITY_ID),
                text(item, tags::UNIVERSAL_ENTITY_ID),
                text(item, tags::UNIVERSAL_ENTITY_ID_TYPE),
            )
        })
}

fn request_attributes(root: &InMemDicomObject) -> Result<(), String> {
    let requests = items(root, tags::REQUEST_ATTRIBUTES_SEQUENCE);
    if requests.is_empty() {
        return Err("the Request Attributes Sequence (0040,0275) is absent or empty, but Appendix B maps the accession and Study Instance UID into it".into());
    }
    let mut found = Violations::default();
    for (index, request) in requests.iter().enumerate() {
        for (tag, what) in [
            (tags::ACCESSION_NUMBER, "Accession Number (0008,0050)"),
            (tags::STUDY_INSTANCE_UID, "Study Instance UID (0020,000D)"),
        ] {
            let (own, top) = (text(request, tag), text(root, tag));
            if own != top {
                found.push(|| {
                    format!(
                    "Request Attributes Sequence item {} has {what} {}, but the top level has {}{}",
                    index + 1,
                    shown_or(tag, own.as_deref(), "absent"),
                    shown_or(tag, top.as_deref(), "absent"),
                    own.as_deref()
                        .zip(top.as_deref())
                        .map(|(own, top)| difference(tag, own, top).to_string())
                        .unwrap_or_default()
                )
                });
            }
        }
        if issuer(request) != issuer(root) {
            found.push(|| format!(
                "Request Attributes Sequence item {} has an Issuer of Accession Number Sequence (0008,0051) that differs from the top level's",
                index + 1
            ));
        }
    }
    found.result()
}

fn order_numbers(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    let present: Vec<&str> = items(root, tags::REQUEST_ATTRIBUTES_SEQUENCE)
        .iter()
        .flat_map(|request| {
            ORDER_NUMBERS
                .iter()
                .filter(|(tag, _)| request.get(*tag).is_some())
                .map(|(_, name)| *name)
        })
        .collect();
    if !present.is_empty() {
        out.advise(
            &ORDER_NUMBERS_EXTENDED,
            Subject::File(&facts.path),
            format!(
                "the Request Attributes Sequence holds {}, which the Request Attributes Macro does not define",
                present.join(", ")
            ),
        );
    }
}

fn patient_sex(facts: &Facts) -> Result<(), String> {
    match facts.patient.sex.as_deref() {
        None | Some("F") | Some("M") => Ok(()),
        Some(other) => Err(format!(
            "Patient's Sex (0010,0040) is {}, but PID-8 holds only F, M or U (Unknown, which maps to an empty Value)",
            shown(tags::PATIENT_SEX, other)
        )),
    }
}

fn specimen_type(specimens: &[Specimen<'_>]) -> Result<(), String> {
    each(specimens, |specimen| {
        if items(specimen.item, tags::SPECIMEN_TYPE_CODE_SEQUENCE).is_empty() {
            Err(format!(
                "{} has no Specimen Type Code Sequence (0040,059A) Item for SPM-4",
                specimen.describe()
            ))
        } else {
            Ok(())
        }
    })
}

fn specimen_type_coding(specimens: &[Specimen<'_>], facts: &Facts, out: &mut Out) {
    for specimen in specimens {
        for code in items(specimen.item, tags::SPECIMEN_TYPE_CODE_SEQUENCE) {
            if text(code, tags::CODING_SCHEME_DESIGNATOR).as_deref() != Some("SCT") {
                out.advise(
                    &SPECIMEN_TYPE_CODING,
                    Subject::File(&facts.path),
                    format!(
                        "{} has Specimen Type {}, which is not a SNOMED CT code of CID 8103",
                        specimen.describe(),
                        describe_code(code)
                    ),
                );
            }
        }
    }
}

fn collection_step(specimen: &Specimen<'_>) -> Result<(), String> {
    let steps = specimen.steps_of(SPECIMEN_COLLECTION);
    let Some(step) = steps.first() else {
        return Err(format!(
            "{} has no step of Processing type (17636008, SCT, \"Specimen collection\")",
            specimen.describe()
        ));
    };
    let mut missing = Vec::new();
    if step
        .find(DATETIME_OF_PROCESSING)
        .and_then(|item| item.datetime.as_ref())
        .is_none()
    {
        missing.push("no DATETIME (111702, DCM, \"DateTime of processing\") Content Item");
    }
    if step.coded(SPECIMEN_COLLECTION).is_err() {
        missing.push("no coded (17636008, SCT, \"Specimen Collection\") procedure");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} {} has {}",
            specimen.describe(),
            step.describe(),
            missing.join(" and ")
        ))
    }
}

/// A step of Processing type `processing` with the CODE Content Item
/// `concept`.
fn processing_step(
    specimen: &Specimen<'_>,
    processing: (&str, &str),
    concept: (&str, &str),
    what: &str,
) -> Result<(), String> {
    let steps = specimen.steps_of(processing);
    if steps.iter().any(|step| step.coded(concept).is_ok()) {
        return Ok(());
    }
    Err(format!(
        "{} has no {what} step: none of Processing type ({}, {}) has a coded ({}, {}) Content Item",
        specimen.describe(),
        processing.1,
        processing.0,
        concept.1,
        concept.0
    ))
}

fn step_identifiers(specimen: &Specimen<'_>) -> Result<(), String> {
    let mut found = Violations::default();
    for step in &specimen.steps {
        let identifier = step
            .find(SPECIMEN_IDENTIFIER)
            .and_then(|item| item.text.clone());
        if identifier.is_none() || identifier != specimen.identifier {
            found.push(|| format!(
                "{} {} has (121041, DCM, \"Specimen Identifier\") {}, but the Specimen Identifier is {}",
                specimen.describe(),
                step.describe(),
                shown_or(tags::TEXT_VALUE, identifier.as_deref(), "absent"),
                shown_or(tags::SPECIMEN_IDENTIFIER, specimen.identifier.as_deref(), "absent")
            ));
        }
        if specimen.issuer.is_none() {
            continue;
        }
        let issuer = step
            .find(ISSUER_OF_SPECIMEN_IDENTIFIER)
            .and_then(|item| item.text.clone());
        if issuer != specimen.issuer {
            let named = specimen.issuer.as_deref().map_or_else(
                || "none".to_string(),
                |named| {
                    shown_in(
                        tags::ISSUER_OF_THE_SPECIMEN_IDENTIFIER_SEQUENCE,
                        tags::LOCAL_NAMESPACE_ENTITY_ID,
                        named,
                    )
                    .to_string()
                },
            );
            found.push(|| format!(
                "{} {} has (111724, DCM, \"Issuer of Specimen Identifier\") {}, but the Issuer of the Specimen Identifier Sequence names {named}",
                specimen.describe(),
                step.describe(),
                shown_or(tags::TEXT_VALUE, issuer.as_deref(), "absent")
            ));
        }
    }
    found.result()
}

/// The rank of a step in DPIA's order, if it is one of its steps.
fn rank(step: &super::specimen::Step<'_>) -> Option<u8> {
    if step.processing_is(SPECIMEN_COLLECTION) {
        Some(1)
    } else if step.processing_is(SPECIMEN_PROCESSING) && step.coded(TISSUE_FIXATIVE).is_ok() {
        Some(2)
    } else if step.processing_is(SPECIMEN_PROCESSING) && step.coded(EMBEDDING_MEDIUM).is_ok() {
        Some(3)
    } else if step.processing_is(STAINING) {
        Some(4)
    } else {
        None
    }
}

/// The order result, and whether a staining step precedes an earlier step,
/// as PS3.17 NN.6.2 describes.
fn step_order(specimens: &[Specimen<'_>]) -> (Result<(), String>, bool) {
    let mut found = Violations::default();
    let mut newest_first = false;
    for specimen in specimens {
        let ranked: Vec<(u8, &super::specimen::Step<'_>)> = specimen
            .steps
            .iter()
            .filter_map(|step| rank(step).map(|rank| (rank, step)))
            .collect();
        for pair in ranked.windows(2) {
            let ((earlier, first), (later, second)) = (pair[0], pair[1]);
            if later < earlier {
                newest_first |= earlier == 4;
                found.push(|| {
                    format!(
                        "{} lists {} before {}, out of chronological order",
                        specimen.describe(),
                        first.describe(),
                        second.describe()
                    )
                });
            }
        }
    }
    (found.result(), newest_first)
}

/// Patient Identity Removed YES with the Clean Structured Content Option.
fn clean_structured_content(root: &InMemDicomObject) -> bool {
    text(root, tags::PATIENT_IDENTITY_REMOVED).as_deref() == Some("YES")
        && items(root, tags::DEIDENTIFICATION_METHOD_CODE_SEQUENCE)
            .iter()
            .any(|code| is_code(code, "DCM", "113104"))
}

fn label_advisories(root: &InMemDicomObject, facts: &Facts, out: &mut Out) {
    let file = Subject::File(&facts.path);
    if let (Some(barcode), Some(container)) = (&facts.barcode_value, &facts.container_identifier) {
        if barcode != container {
            out.advise(
                &BARCODE_CONTAINER,
                file,
                format!(
                    "Barcode Value {} differs from Container Identifier {}{}; DPIA Open Issue 32 notes barcodes may add delimiter characters to the slide ID",
                    shown(tags::BARCODE_VALUE, barcode),
                    shown(tags::CONTAINER_IDENTIFIER, container),
                    difference(tags::CONTAINER_IDENTIFIER, barcode, container)
                ),
            );
        }
    }
    let container_types = items(root, tags::CONTAINER_TYPE_CODE_SEQUENCE);
    if !container_types
        .iter()
        .any(|code| is_code(code, "SCT", "433466003"))
    {
        out.advise(
            &CONTAINER_TYPE,
            file,
            match container_types.first() {
                Some(code) => format!(
                    "Container Type Code Sequence holds {}, not (433466003, SCT, \"Microscope slide\")",
                    describe_code(code)
                ),
                None => "the Container Type Code Sequence (0040,0518) is absent or empty".into(),
            },
        );
    }
    if facts.flavor_is(&["LABEL"]) && text(root, tags::LABEL_TEXT).is_none() {
        out.advise(
            &LABEL_TEXT,
            file,
            "the LABEL image has no Label Text (2200,0002) read from the slide label".into(),
        );
    }
}
