//! The Search Transaction beyond the Study, Series and Instance searches
//! (PS3.18 10.6): the Search Query Parameters of Table 8.3.4-1 and the
//! three matching parameters (8.3.4.2, 8.3.4.5, 8.3.4.6), the Required
//! Matching Attributes of Table 10.6.1-5 one at a time, and the Mandatory
//! Search resources of Tables 10.3-2 and 10.6.1-1.

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::capabilities;
use super::client::{Exchange, Request};
use super::report::Verdict;
use super::response::{sec, text};
use super::search::{
    return_keys, search_results, warning_agent, ABSENT_UID, INSTANCE_RETURN_KEYS, JSON,
    SERIES_RETURN_KEYS, STUDY_RETURN_KEYS,
};
use super::url::encode_query_value;
use super::{DicomWebTransaction, Session, Target};

const PARAMETERS_NAME: &str = "dicomweb-search-matching-parameters";
const PARAMETERS_SECTIONS: &str = "8.3.4 Table 8.3.4-1, 8.3.4.2, 8.3.4.5, 8.3.4.6 and 10.6.1.2";
const ATTRIBUTES_NAME: &str = "dicomweb-search-matching-attributes";
const ATTRIBUTES_SECTIONS: &str =
    "10.6.1.2.3 Table 10.6.1-5, 10.6.3.3.1, 10.6.3.3.2 and 10.6.3.3.3";
const RESOURCES_NAME: &str = "dicomweb-search-resources";
const RESOURCES_SECTIONS: &str =
    "10.2, 10.3 Table 10.3-2, 10.6.1.1 Table 10.6.1-1, 10.6.1.2.3 Table 10.6.1-4, 10.6.3.3.2 and 10.6.3.3.3";
/// Appended to a Person Name, it is no case, accent or position variant.
const NAME_SUFFIX: &str = "XQZ";
/// A value no Attribute of the slide's Study holds.
const NO_MATCH: &str = "WSIDICOMNOMATCH";

/// Study Level Attributes beyond the Table 10.6.3-3 return keys, which an
/// includefield asks for (PS3.4 C.6.2.1.2).
const STUDY_OPTIONAL_KEYS: [(Tag, &str); 8] = [
    (tags::STUDY_DESCRIPTION, "StudyDescription"),
    (tags::ISSUER_OF_PATIENT_ID, "IssuerOfPatientID"),
    (
        tags::NAME_OF_PHYSICIANS_READING_STUDY,
        "NameOfPhysiciansReadingStudy",
    ),
    (tags::PATIENT_AGE, "PatientAge"),
    (tags::PATIENT_SIZE, "PatientSize"),
    (tags::PATIENT_WEIGHT, "PatientWeight"),
    (
        tags::ADMITTING_DIAGNOSES_DESCRIPTION,
        "AdmittingDiagnosesDescription",
    ),
    (tags::PATIENT_COMMENTS, "PatientComments"),
];

/// Runs the Search Query Parameter, matching Attribute and resource checks;
/// `held` is the target's Metadata, whose Study Attributes an includefield
/// asks for.
pub(super) fn run(session: &mut Session<'_>, target: &Target, held: Option<&InMemDicomObject>) {
    let names = [
        (PARAMETERS_NAME, PARAMETERS_SECTIONS),
        (ATTRIBUTES_NAME, ATTRIBUTES_SECTIONS),
        (RESOURCES_NAME, RESOURCES_SECTIONS),
    ];
    let reason = if session.skipped(DicomWebTransaction::Search) {
        Some("skipped by request (--skip search)")
    } else if target.study.is_none() {
        Some("no Study to search: none was given and none was found")
    } else {
        None
    };
    if let Some(reason) = reason {
        for (name, sections) in names {
            session.not_run(name, sections, reason);
        }
        return;
    }
    matching_parameters(session, target, held);
    matching_attributes(session, target);
    resources(session, target);
}

/// One search: its results, or `None` with the reason recorded.
fn search(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    path: &str,
    query: &str,
) -> Option<(Exchange, Vec<InMemDicomObject>)> {
    let url = session.url(path, Some(query));
    let exchange = session.send(verdict, &Request::get(url, JSON))?;
    let results = search_results(verdict, &exchange)?;
    Some((exchange, results))
}

/// The results' values of `key`.
fn uids(results: &[InMemDicomObject], key: Tag) -> Vec<String> {
    results
        .iter()
        .filter_map(|result| text(result, key))
        .collect()
}

// --- the Search Query Parameters (Table 8.3.4-1) --------------------------

/// A matching parameter of 8.3.4.2, 8.3.4.5 or 8.3.4.6.
struct Flag {
    name: &'static str,
    section: &'static str,
    /// The text of the 299 Warning that it is not supported.
    unsupported: &'static str,
}

const FUZZY: Flag = Flag {
    name: "fuzzymatching",
    section: "8.3.4.2",
    unsupported: "The fuzzymatching parameter is not supported",
};
const EMPTY: Flag = Flag {
    name: "emptyvaluematching",
    section: "8.3.4.5",
    unsupported: "The emptyvaluematching parameter is not supported",
};
const MULTIPLE: Flag = Flag {
    name: "multiplevaluematching",
    section: "8.3.4.6",
    unsupported: "The multiplevaluematching parameter is not supported",
};

/// Whether the response says the flag is not supported: a 299 Warning of
/// 8.3.4.2, 8.3.4.5 or 8.3.4.6, held to its form.
fn warns_unsupported(verdict: &mut Verdict, exchange: &Exchange, flag: &Flag) -> bool {
    let line = exchange.request_line();
    let warning = exchange
        .headers
        .iter()
        .find(|(name, value)| name == "warning" && value.to_ascii_lowercase().contains(flag.name));
    let Some((_, value)) = warning else {
        return false;
    };
    warning_agent(verdict, &line, value);
    if !value.trim_start().starts_with("299") || !value.contains(flag.unsupported) {
        verdict.fail(format!(
            "{line}: Warning {value:?} is not \"299 <service>: {}. ...\" ({})",
            flag.unsupported,
            sec(flag.section)
        ));
    }
    true
}

/// An origin server that does not warn that the flag is unsupported
/// performs the matching, which it documents in the Retrieve Capabilities
/// response; one that documents nothing and warns nothing breaks one of the
/// two.
fn claimed(session: &Session<'_>, verdict: &mut Verdict, line: &str, flag: &Flag) {
    match capabilities::documents(session, flag.name) {
        Some(true) => {}
        Some(false) => verdict.fail(format!(
            "{line}: no Warning says {} is not supported, so it was performed, yet the Capabilities Description does not document it; an unsupported value of \"true\" brings the 299 Warning, and a supported one is documented ({})",
            flag.name,
            sec(flag.section)
        )),
        None => verdict.note(format!(
            "no Capabilities Description to tell whether the {} performed is documented",
            flag.name
        )),
    }
}

/// Table 8.3.4-1 (all Mandatory for the origin server), then each matching
/// parameter "false" and "true" on a value whose literal and extended
/// matching differ.
fn matching_parameters(
    session: &mut Session<'_>,
    target: &Target,
    held: Option<&InMemDicomObject>,
) {
    let mut verdict = Verdict::default();
    let study = target.study.clone().unwrap_or_default();
    includefields(session, &mut verdict, &study, held);
    let query = format!(
        "StudyInstanceUID={study}&includefield=00081030&limit=1&offset=0&fuzzymatching=false&emptyvaluematching=false&multiplevaluematching=false"
    );
    if let Some((exchange, results)) = search(session, &mut verdict, "/studies", &query) {
        if uids(&results, tags::STUDY_INSTANCE_UID) != [study.clone()] {
            verdict.fail(format!(
                "{} returned {} results; every Search Query Parameter of Table 8.3.4-1 is supported, and these match the one Study ({})",
                exchange.request_line(),
                results.len(),
                sec("8.3.4 Table 8.3.4-1 and 10.6.1.2")
            ));
        }
    }
    let study_result = target.study_result.as_ref();
    match study_result.and_then(person_name) {
        Some((keyword, name)) => fuzzy(session, &mut verdict, &study, keyword, &name),
        None => verdict.note("the Study has no Person Name to match fuzzily"),
    }
    match study_result.and_then(empty_attribute) {
        Some(keyword) => empty_value(session, &mut verdict, &study, keyword),
        None => verdict.note("the Study result has no empty Attribute to match with \"\""),
    }
    match study_result.and_then(|result| text(result, tags::MODALITIES_IN_STUDY)) {
        Some(modalities) if !modalities.is_empty() => {
            multiple_value(session, &mut verdict, &study, &modalities);
        }
        _ => verdict.note("the Study result has no Modalities in Study to match"),
    }
    session.push(verdict.finish(
        PARAMETERS_NAME,
        sec(PARAMETERS_SECTIONS),
        "the Search Query Parameters are supported, and each matching parameter is performed as documented or warned unsupported and not performed",
    ));
}

/// 8.3.4.3: includefield names "the Attributes that should be included in
/// the response", and "all" means "that all available attributes of the
/// object should be included": a Study Level Attribute the Instance holds is
/// asked for by Tag and by "all". Recommendations, so advisories.
fn includefields(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    study: &str,
    held: Option<&InMemDicomObject>,
) {
    let Some((tag, keyword)) = held.and_then(|object| {
        STUDY_OPTIONAL_KEYS
            .into_iter()
            .find(|(tag, _)| text(object, *tag).is_some_and(|value| !value.is_empty()))
    }) else {
        verdict.note("the Instance Metadata holds no Study Level Attribute beyond the return keys to ask for as an includefield");
        return;
    };
    for (include, reference) in [
        (
            format!("{:04X}{:04X}", tag.group(), tag.element()),
            "8.3.4.3: includefield specifies \"the Attributes that should be included in the response\"",
        ),
        (
            "all".to_string(),
            "8.3.4.3: \"all\" means \"that all available attributes of the object should be included in the response\"",
        ),
    ] {
        let query = format!("StudyInstanceUID={study}&includefield={include}");
        let Some((exchange, results)) = search(session, verdict, "/studies", &query) else {
            continue;
        };
        if results.iter().any(|result| result.get(tag).is_none()) {
            verdict.advise(format!(
                "{}: the Study result lacks {keyword} {}, which the Study's Instance holds; {} ({})",
                exchange.request_line(),
                super::response::tag_text(tag),
                "an includefield Attribute should be included",
                sec(reference)
            ));
        }
    }
}

/// The Study's Patient's Name, else its Referring Physician's Name: the
/// Alphabetic component group.
fn person_name(result: &InMemDicomObject) -> Option<(&'static str, String)> {
    [
        (tags::PATIENT_NAME, "PatientName"),
        (tags::REFERRING_PHYSICIAN_NAME, "ReferringPhysicianName"),
    ]
    .into_iter()
    .find_map(|(tag, keyword)| {
        let alphabetic = text(result, tag)?.split('=').next()?.trim().to_string();
        (!alphabetic.is_empty()).then_some((keyword, alphabetic))
    })
}

/// 8.3.4.2: with "false" the search is literal, so a name with letters
/// added does not match; with "true" it is fuzzy, or warned unsupported and
/// literal.
fn fuzzy(session: &mut Session<'_>, verdict: &mut Verdict, study: &str, keyword: &str, name: &str) {
    let variant = encode_query_value(&format!("{name}{NAME_SUFFIX}"));
    for value in ["false", "true"] {
        let query = format!("StudyInstanceUID={study}&{keyword}={variant}&fuzzymatching={value}");
        let Some((exchange, results)) = search(session, verdict, "/studies", &query) else {
            continue;
        };
        let line = exchange.request_line();
        let literal = value == "false" || warns_unsupported(verdict, &exchange, &FUZZY);
        if literal && !results.is_empty() {
            verdict.fail(format!(
                "{line} returned the Study, whose {keyword} is {name:?}; {}, so the search is performed without Fuzzy Matching and {name:?} with {NAME_SUFFIX:?} added does not match ({})",
                if value == "false" { "fuzzymatching=false" } else { "fuzzymatching is warned unsupported" },
                sec("8.3.4.2 and PS3.4 C.2.2.2.1.1")
            ));
        } else if !literal {
            claimed(session, verdict, &line, &FUZZY);
            verdict.note(format!(
                "{line}: Fuzzy Matching is performed; what it matches is implementation specific (PS3.4 C.2.2.2.1.1)"
            ));
        }
    }
}

/// A Study Level Attribute of Table 10.6.1-5 the result holds empty, of a
/// VR for which Empty Value Matching is defined (PS3.4 C.2.2.2.7).
fn empty_attribute(result: &InMemDicomObject) -> Option<&'static str> {
    [
        (tags::ACCESSION_NUMBER, "AccessionNumber"),
        (tags::REFERRING_PHYSICIAN_NAME, "ReferringPhysicianName"),
        (tags::STUDY_ID, "StudyID"),
        (tags::PATIENT_ID, "PatientID"),
        (tags::PATIENT_NAME, "PatientName"),
    ]
    .into_iter()
    .find(|(tag, _)| {
        result.get(*tag).is_some() && text(result, *tag).unwrap_or_default().is_empty()
    })
    .map(|(_, keyword)| keyword)
}

/// 8.3.4.5: `""` is a literal value without Empty Value Matching, which an
/// empty Attribute does not match (PS3.4 C.2.2.2.1); with it, the empty
/// Attribute matches (C.2.2.2.7).
fn empty_value(session: &mut Session<'_>, verdict: &mut Verdict, study: &str, keyword: &str) {
    for value in ["false", "true"] {
        let query = format!("StudyInstanceUID={study}&{keyword}=%22%22&emptyvaluematching={value}");
        let Some((exchange, results)) = search(session, verdict, "/studies", &query) else {
            continue;
        };
        let line = exchange.request_line();
        let literal = value == "false" || warns_unsupported(verdict, &exchange, &EMPTY);
        if literal && !results.is_empty() {
            verdict.fail(format!(
                "{line} returned the Study, whose {keyword} is empty; {}, so \"\" is matched literally and the empty Attribute does not match ({})",
                if value == "false" { "emptyvaluematching=false" } else { "emptyvaluematching is warned unsupported" },
                sec("8.3.4.5 and PS3.4 C.2.2.2.1")
            ));
        } else if !literal {
            claimed(session, verdict, &line, &EMPTY);
            if results.is_empty() {
                verdict.fail(format!(
                    "{line} returned no Study, though its {keyword} is empty; with Empty Value Matching performed, \"\" matches a zero-length value ({})",
                    sec("8.3.4.5 and PS3.4 C.2.2.2.7")
                ));
            }
        }
    }
}

/// 8.3.4.6 and PS3.4 C.2.2.2.8: with Multiple Value Matching a multi-valued
/// key matches only an entity holding every value. Without it the meaning
/// of a multi-valued key is undefined, so only a performed one is checked.
fn multiple_value(session: &mut Session<'_>, verdict: &mut Verdict, study: &str, held: &str) {
    let first = held.split('\\').next().unwrap_or_default();
    let every = if held.contains('\\') {
        held.to_string()
    } else {
        format!("{held}\\{held}")
    };
    for (key, matches) in [(every, true), (format!("{first}\\ZZ"), false)] {
        let query = format!(
            "StudyInstanceUID={study}&ModalitiesInStudy={}&multiplevaluematching=true",
            encode_query_value(&key)
        );
        let Some((exchange, results)) = search(session, verdict, "/studies", &query) else {
            continue;
        };
        let line = exchange.request_line();
        if warns_unsupported(verdict, &exchange, &MULTIPLE) {
            continue;
        }
        claimed(session, verdict, &line, &MULTIPLE);
        if results.is_empty() == matches {
            verdict.fail(format!(
                "{line} returned {} Studies; the Study's Modalities in Study are {held:?}, and with Multiple Value Matching {key:?} matches only a Study holding every value ({})",
                results.len(),
                sec("8.3.4.6 and PS3.4 C.2.2.2.8")
            ));
        }
    }
}

// --- the Required Matching Attributes (Table 10.6.1-5) ----------------------

/// One IE Level's search and the matching Attributes it supports.
struct Level {
    name: &'static str,
    path: String,
    /// The Unique Key that confines the search to the target.
    unique: (&'static str, String),
    /// The target's search result.
    result: Option<InMemDicomObject>,
    attributes: &'static [(Tag, &'static str, Kind)],
}

/// The value class of a matching Attribute, for a value it cannot match.
#[derive(Clone, Copy)]
enum Kind {
    Date,
    Time,
    Text,
    Name,
    Integer,
    Uid,
}

const STUDY_MATCHING: &[(Tag, &str, Kind)] = &[
    (tags::STUDY_DATE, "StudyDate", Kind::Date),
    (tags::STUDY_TIME, "StudyTime", Kind::Time),
    (tags::ACCESSION_NUMBER, "AccessionNumber", Kind::Text),
    (tags::MODALITIES_IN_STUDY, "ModalitiesInStudy", Kind::Text),
    (
        tags::REFERRING_PHYSICIAN_NAME,
        "ReferringPhysicianName",
        Kind::Name,
    ),
    (tags::PATIENT_NAME, "PatientName", Kind::Name),
    (tags::PATIENT_ID, "PatientID", Kind::Text),
    (tags::STUDY_ID, "StudyID", Kind::Text),
];
const SERIES_MATCHING: &[(Tag, &str, Kind)] = &[
    (tags::MODALITY, "Modality", Kind::Text),
    (tags::SERIES_NUMBER, "SeriesNumber", Kind::Integer),
    (
        tags::PERFORMED_PROCEDURE_STEP_START_DATE,
        "PerformedProcedureStepStartDate",
        Kind::Date,
    ),
    (
        tags::PERFORMED_PROCEDURE_STEP_START_TIME,
        "PerformedProcedureStepStartTime",
        Kind::Time,
    ),
    (
        tags::SCHEDULED_PROCEDURE_STEP_ID,
        "RequestAttributesSequence.ScheduledProcedureStepID",
        Kind::Text,
    ),
    (
        tags::REQUESTED_PROCEDURE_ID,
        "RequestAttributesSequence.RequestedProcedureID",
        Kind::Text,
    ),
];
const INSTANCE_MATCHING: &[(Tag, &str, Kind)] = &[
    (tags::SOP_CLASS_UID, "SOPClassUID", Kind::Uid),
    (tags::INSTANCE_NUMBER, "InstanceNumber", Kind::Integer),
];

/// A value of `kind` that `held` is not.
fn no_match(kind: Kind, held: &str) -> String {
    let pick = |first: &str, second: &str| {
        if held == first {
            second.to_string()
        } else {
            first.to_string()
        }
    };
    match kind {
        Kind::Date => pick("19000102", "19000103"),
        Kind::Time => pick("000001", "000002"),
        Kind::Text => NO_MATCH.to_string(),
        Kind::Name => format!("{NO_MATCH}^{NO_MATCH}"),
        Kind::Integer => pick("999999", "999998"),
        Kind::Uid => ABSENT_UID.to_string(),
    }
}

/// The first Value of `tag` in a top-level Attribute or, for the Request
/// Attributes Sequence keys, in its first Item.
fn held_value(result: &InMemDicomObject, tag: Tag) -> Option<String> {
    let value = match result.get(tag) {
        Some(_) => text(result, tag),
        None => result
            .get(tags::REQUEST_ATTRIBUTES_SEQUENCE)
            .and_then(|element| element.items())
            .and_then(|items| items.first())
            .and_then(|item| text(item, tag)),
    }?;
    let first = value.split('\\').next().unwrap_or_default();
    let first = first.split('=').next().unwrap_or_default().trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// 10.6.1.2.3: "The origin server shall support the matching Attributes
/// specified in Table 10.6.1-5 for each supported IE Level": each, alone
/// with a value the target does not hold, matches nothing; all, with the
/// target's values, match the target, and are returned (10.6.3.3).
fn matching_attributes(session: &mut Session<'_>, target: &Target) {
    let mut verdict = Verdict::default();
    let study = target.study.clone().unwrap_or_default();
    let mut levels = vec![Level {
        name: "Study",
        path: "/studies".into(),
        unique: ("StudyInstanceUID", study.clone()),
        result: target.study_result.clone(),
        attributes: STUDY_MATCHING,
    }];
    if let Some(series) = target.series.clone() {
        levels.push(Level {
            name: "Series",
            path: format!("/studies/{study}/series"),
            unique: ("SeriesInstanceUID", series.clone()),
            result: target.series_result.clone(),
            attributes: SERIES_MATCHING,
        });
        if let Some(instance) = target.instance.clone() {
            levels.push(Level {
                name: "Instance",
                path: format!("/studies/{study}/series/{series}/instances"),
                unique: ("SOPInstanceUID", instance),
                result: target.instance_result.clone(),
                attributes: INSTANCE_MATCHING,
            });
        }
    }
    for level in &levels {
        held_values_match(session, &mut verdict, level);
        for (tag, keyword, kind) in level.attributes {
            let held = level
                .result
                .as_ref()
                .and_then(|result| held_value(result, *tag))
                .unwrap_or_default();
            let query = format!(
                "{}={}&{keyword}={}",
                level.unique.0,
                level.unique.1,
                encode_query_value(&no_match(*kind, &held))
            );
            let Some((exchange, results)) = search(session, &mut verdict, &level.path, &query)
            else {
                continue;
            };
            if !results.is_empty() {
                verdict.fail(format!(
                    "{} returned {} results, though the {} holds {keyword} {held:?}; {keyword} is a Required Matching Attribute, so a value it does not hold matches nothing ({})",
                    exchange.request_line(),
                    results.len(),
                    level.name,
                    sec("10.6.1.2.3 Table 10.6.1-5 and 8.3.4.1.1")
                ));
            }
        }
    }
    session.push(verdict.finish(
        ATTRIBUTES_NAME,
        sec(ATTRIBUTES_SECTIONS),
        "each Required Matching Attribute matches only the values held, and the matched Attributes are returned",
    ));
}

/// The level's search on every matching Attribute the target holds a value
/// of: it returns the target, with each matched Attribute ("All other ...
/// Attributes passed as match ... parameters", 10.6.3.3).
fn held_values_match(session: &mut Session<'_>, verdict: &mut Verdict, level: &Level) {
    let Some(result) = level.result.as_ref() else {
        verdict.note(format!(
            "no {} search result to match by its values",
            level.name
        ));
        return;
    };
    let matched: Vec<(Tag, &str, String)> = level
        .attributes
        .iter()
        .filter(|(tag, _, _)| result.get(*tag).is_some())
        .filter_map(|(tag, keyword, _)| Some((*tag, *keyword, held_value(result, *tag)?)))
        .collect();
    if matched.is_empty() {
        return;
    }
    let mut query = format!("{}={}", level.unique.0, level.unique.1);
    for (_, keyword, value) in &matched {
        query.push_str(&format!("&{keyword}={}", encode_query_value(value)));
    }
    let Some((exchange, results)) = search(session, verdict, &level.path, &query) else {
        return;
    };
    let line = exchange.request_line();
    let [found] = results.as_slice() else {
        verdict.fail(format!(
            "{line} returned {} results; the {}'s own values of {} match it alone ({})",
            results.len(),
            level.name,
            matched
                .iter()
                .map(|(_, keyword, _)| *keyword)
                .collect::<Vec<_>>()
                .join(", "),
            sec("10.6.1.2.3 Table 10.6.1-5")
        ));
        return;
    };
    let missing: Vec<&str> = matched
        .iter()
        .filter(|(tag, _, _)| found.get(*tag).is_none())
        .map(|(_, keyword, _)| *keyword)
        .collect();
    if !missing.is_empty() {
        verdict.fail(format!(
            "{line}: the result lacks {}, passed as match parameters, which the response contains ({})",
            missing.join(", "),
            sec("10.6.3.3.1, 10.6.3.3.2 and 10.6.3.3.3")
        ));
    }
}

// --- the Mandatory Search resources (Tables 10.3-2 and 10.6.1-1) ------------

/// 10.2: the Search Transaction for every Mandatory resource of Table
/// 10.3-2; Table 10.6.1-1 makes All Studies, Study's Series and Study's
/// Series' Instances Mandatory for every origin server, and the other three
/// for a native one. Each matches on the IE Levels of Table 10.6.1-4 and
/// holds the higher levels' Attributes (10.6.3.3.2, 10.6.3.3.3).
fn resources(session: &mut Session<'_>, target: &Target) {
    let mut verdict = Verdict::default();
    for (segment, resource) in [
        ("studies", "All Studies"),
        ("series", "Study's Series"),
        ("instances", "Study's Series' Instances"),
    ] {
        if !session.served.contains(segment) {
            verdict.fail(format!(
                "the {resource} search did not return results; it is Mandatory for every origin server, native or DIMSE proxy ({})",
                sec("10.6.1.1 Table 10.6.1-1 and 10.3 Table 10.3-2")
            ));
        }
    }
    let study = target.study.clone().unwrap_or_default();
    let (Some(series), Some(instance)) = (target.series.clone(), target.instance.clone()) else {
        verdict.not_run("no Series and Instance to search for");
        session.push(verdict.finish(RESOURCES_NAME, sec(RESOURCES_SECTIONS), ""));
        return;
    };
    let series_keys: Vec<(Tag, &str)> = SERIES_RETURN_KEYS.to_vec();
    let study_keys: Vec<(Tag, &str)> = STUDY_RETURN_KEYS.to_vec();
    let instance_keys: Vec<(Tag, &str)> = INSTANCE_RETURN_KEYS.to_vec();
    let probes = [
        (
            "Study's Instances",
            format!("/studies/{study}/instances"),
            format!("SeriesInstanceUID={series}&SOPInstanceUID={instance}"),
            tags::SOP_INSTANCE_UID,
            instance.clone(),
            [series_keys.clone(), instance_keys.clone()].concat(),
            "Tables 10.6.3-4 and 10.6.3-5, as 10.6.3.3.3 requires of Study's Instances",
        ),
        (
            "All Series",
            "/series".to_string(),
            format!("StudyInstanceUID={study}&SeriesInstanceUID={series}"),
            tags::SERIES_INSTANCE_UID,
            series.clone(),
            [study_keys.clone(), series_keys.clone()].concat(),
            "Tables 10.6.3-3 and 10.6.3-4, as 10.6.3.3.2 requires of All Series",
        ),
        (
            "All Instances",
            "/instances".to_string(),
            format!(
                "StudyInstanceUID={study}&SeriesInstanceUID={series}&SOPInstanceUID={instance}"
            ),
            tags::SOP_INSTANCE_UID,
            instance.clone(),
            [study_keys, series_keys, instance_keys].concat(),
            "Tables 10.6.3-3, 10.6.3-4 and 10.6.3-5, as 10.6.3.3.3 requires of All Instances",
        ),
    ];
    for (resource, path, query, key, uid, keys, table) in probes {
        let url = session.url(&path, Some(&query));
        let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) else {
            continue;
        };
        if exchange.status != 200 {
            verdict.fail(format!(
                "{} answered {}; the {resource} resource is Mandatory for the Search Transaction (Table 10.3-2) and for a native origin server, Optional only for a DIMSE proxy ({})",
                exchange.request_line(),
                exchange.status,
                sec("10.2 and 10.6.1.1 Table 10.6.1-1")
            ));
            continue;
        }
        let Some(results) = search_results(&mut verdict, &exchange) else {
            continue;
        };
        if uids(&results, key) != [uid.clone()] {
            verdict.fail(format!(
                "{} returned {:?}; matching on the IE Levels of the resource returns {uid} alone ({})",
                exchange.request_line(),
                uids(&results, key),
                sec("10.6.1.2.3 Table 10.6.1-4")
            ));
            continue;
        }
        return_keys(&mut verdict, 0, &results[0], &keys, table);
    }
    session.push(verdict.finish(
        RESOURCES_NAME,
        sec(RESOURCES_SECTIONS),
        "every Mandatory Search resource matches on its IE Levels and holds the higher levels' Attributes",
    ));
}
