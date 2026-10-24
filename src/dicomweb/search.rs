//! The Search Transaction, QIDO-RS (PS3.18 10.6 and 8.3.4).

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request};
use super::report::Verdict;
use super::response::{expect_json, expect_multipart, expect_status, integer, sec, tag_text, text};
use super::url::Url;
use super::{DicomWebTransaction, Session, Target};
use crate::validation::is_image_storage_sop_class;

pub(super) const JSON: &str = "application/dicom+json";
const XML_ACCEPT: &str = "multipart/related; type=\"application/dicom+xml\"";
/// A UID no origin server holds, for a search with no match.
pub(super) const ABSENT_UID: &str = "2.25.299792458299792458299792458299792458";

/// Table 10.6.3-3, Type U and R.
pub(super) const STUDY_RETURN_KEYS: [(Tag, &str); 13] = [
    (tags::STUDY_DATE, "Study Date"),
    (tags::STUDY_TIME, "Study Time"),
    (tags::ACCESSION_NUMBER, "Accession Number"),
    (tags::MODALITIES_IN_STUDY, "Modalities in Study"),
    (tags::REFERRING_PHYSICIAN_NAME, "Referring Physician's Name"),
    (tags::PATIENT_NAME, "Patient's Name"),
    (tags::PATIENT_ID, "Patient ID"),
    (tags::PATIENT_BIRTH_DATE, "Patient's Birth Date"),
    (tags::PATIENT_SEX, "Patient's Sex"),
    (tags::STUDY_INSTANCE_UID, "Study Instance UID"),
    (tags::STUDY_ID, "Study ID"),
    (
        tags::NUMBER_OF_STUDY_RELATED_SERIES,
        "Number of Study Related Series",
    ),
    (
        tags::NUMBER_OF_STUDY_RELATED_INSTANCES,
        "Number of Study Related Instances",
    ),
];
/// Table 10.6.3-4, Type U and R; Retrieve URL is checked apart.
pub(super) const SERIES_RETURN_KEYS: [(Tag, &str); 4] = [
    (tags::MODALITY, "Modality"),
    (tags::SERIES_INSTANCE_UID, "Series Instance UID"),
    (tags::SERIES_NUMBER, "Series Number"),
    (
        tags::NUMBER_OF_SERIES_RELATED_INSTANCES,
        "Number of Series Related Instances",
    ),
];
/// Table 10.6.3-5, Type U and R; Retrieve URL is checked apart.
pub(super) const INSTANCE_RETURN_KEYS: [(Tag, &str); 3] = [
    (tags::SOP_CLASS_UID, "SOP Class UID"),
    (tags::SOP_INSTANCE_UID, "SOP Instance UID"),
    (tags::INSTANCE_NUMBER, "Instance Number"),
];
/// Table 10.6.3-5 "Shall be present if known": an origin server holding an
/// image knows its Rows, Columns and Bits Allocated.
const IMAGE_RETURN_KEYS: [(Tag, &str); 3] = [
    (tags::ROWS, "Rows"),
    (tags::COLUMNS, "Columns"),
    (tags::BITS_ALLOCATED, "Bits Allocated"),
];

/// Runs the Search checks and fills in the Series and Instance to target.
pub(super) fn run(session: &mut Session<'_>, target: &mut Target) {
    if session.skipped(DicomWebTransaction::Search) {
        for name in SEARCH_CHECKS {
            session.not_run(name, "10.6", "skipped by request (--skip search)");
        }
        return;
    }
    studies(session, target);
    let Some(study) = target.study.clone() else {
        for name in &SEARCH_CHECKS[1..] {
            session.not_run(
                name,
                "10.6",
                "no Study to search: none was given and none was found",
            );
        }
        return;
    };
    let all_series = series(session, target, &study);
    let all_instances = match target.series.clone() {
        Some(series) => instances(session, target, &study, &series),
        None => Vec::new(),
    };
    paging(
        session,
        &study,
        target.series.as_deref(),
        &all_series,
        &all_instances,
    );
    matching(session, target, &study, &all_series);
    parameters(session, &study);
    accept_required(session, &study);
}

const SEARCH_CHECKS: [&str; 7] = [
    "dicomweb-search-studies",
    "dicomweb-search-series",
    "dicomweb-search-instances",
    "dicomweb-search-paging",
    "dicomweb-search-matching",
    "dicomweb-search-parameters",
    "dicomweb-accept-required",
];

/// The Study search, which also finds a slide's Study when none is given.
/// Instance Number is asked for as an includefield: an Instance Level
/// Attribute, which a Study search "shall not" return (10.6.3.3.1).
fn studies(session: &mut Session<'_>, target: &mut Target) {
    let section = sec("10.6.1, 10.6.3.3.1 Table 10.6.3-3, 10.6.4 and 8.3.4.3");
    let mut verdict = Verdict::default();
    let query = match &target.study {
        Some(study) => format!("StudyInstanceUID={study}&includefield=00200013"),
        None => "ModalitiesInStudy=SM&limit=1&includefield=00200013".to_string(),
    };
    let url = session.url("/studies", Some(&query));
    let results = session
        .send(&mut verdict, &Request::get(url, JSON))
        .and_then(|exchange| search_results(&mut verdict, &exchange));
    if let Some(results) = results {
        match target.study.clone() {
            Some(study) => {
                let matching = results
                    .iter()
                    .filter(|result| {
                        text(result, tags::STUDY_INSTANCE_UID).as_deref() == Some(study.as_str())
                    })
                    .count();
                if results.len() != 1 || matching != 1 {
                    verdict.fail(format!(
                        "matching Study Instance UID {study} returned {} results, {matching} with that UID; single value matching returns the one Study ({})",
                        results.len(),
                        sec("8.3.4.1.1")
                    ));
                }
            }
            None => {
                if results.len() > 1 {
                    verdict.fail(format!(
                        "limit=1 returned {} results ({})",
                        results.len(),
                        sec("8.3.4.4")
                    ));
                }
                target.study = results
                    .first()
                    .and_then(|result| text(result, tags::STUDY_INSTANCE_UID));
                if target.study.is_none() {
                    verdict.not_run("no Study with Modalities in Study SM was found; give --study");
                }
            }
        }
        session.served("studies");
        target.study_result = target.study.as_deref().and_then(|study| {
            results
                .iter()
                .find(|result| text(result, tags::STUDY_INSTANCE_UID).as_deref() == Some(study))
                .cloned()
        });
        for (index, result) in results.iter().enumerate() {
            return_keys(
                &mut verdict,
                index,
                result,
                &STUDY_RETURN_KEYS,
                "Table 10.6.3-3",
            );
            target.note_retrieve_url("Study", result);
            if result.get(tags::INSTANCE_NUMBER).is_some() {
                verdict.fail(format!(
                    "result {}: Instance Number (0020,0013), an Instance Level includefield, is returned by a Study search, which shall not return it ({})",
                    index + 1,
                    sec("10.6.3.3.1")
                ));
            }
        }
    }
    session.push(verdict.finish(
        "dicomweb-search-studies",
        section,
        "the Study search returns the Study with the Table 10.6.3-3 Attributes and no Instance Level includefield",
    ));
}

/// The Study's Series, hierarchical (Table 10.6.1-1), with Instance Number
/// asked for as an includefield: an Instance Level Attribute, which a Series
/// search "shall not" return (10.6.3.3.2).
fn series(session: &mut Session<'_>, target: &mut Target, study: &str) -> Vec<InMemDicomObject> {
    let section = sec("10.6.1 Table 10.6.1-1, 10.6.3.3.2 Table 10.6.3-4");
    let mut verdict = Verdict::default();
    let url = session.url(
        &format!("/studies/{study}/series"),
        Some("includefield=00200013"),
    );
    let results = session
        .send(&mut verdict, &Request::get(url, JSON))
        .and_then(|exchange| search_results(&mut verdict, &exchange));
    if results.is_some() {
        session.served("series");
    }
    let results = results.unwrap_or_default();
    for (index, result) in results.iter().enumerate() {
        return_keys(
            &mut verdict,
            index,
            result,
            &SERIES_RETURN_KEYS,
            "Table 10.6.3-4",
        );
        target.note_retrieve_url("Series", result);
        if result.get(tags::INSTANCE_NUMBER).is_some() {
            verdict.fail(format!(
                "result {}: Instance Number (0020,0013), an Instance Level includefield, is returned by a Series search, which shall not return it ({})",
                index + 1,
                sec("10.6.3.3.2")
            ));
        }
    }
    let uids: Vec<String> = results
        .iter()
        .filter_map(|result| text(result, tags::SERIES_INSTANCE_UID))
        .collect();
    match target.series.clone() {
        Some(series) if !uids.contains(&series) && !verdict.failed() => verdict.fail(format!(
            "Series {series} is not among the {} Series the search returned ({})",
            uids.len(),
            sec("10.6.1.1")
        )),
        Some(_) => {}
        None => {
            target.series = results
                .iter()
                .find(|result| text(result, tags::MODALITY).as_deref() == Some("SM"))
                .or(results.first())
                .and_then(|result| text(result, tags::SERIES_INSTANCE_UID));
        }
    }
    target.series_result = target.series.as_deref().and_then(|series| {
        results
            .iter()
            .find(|result| text(result, tags::SERIES_INSTANCE_UID).as_deref() == Some(series))
            .cloned()
    });
    session.push(verdict.finish(
        "dicomweb-search-series",
        section,
        "the Study's Series search returns each Series with the Table 10.6.3-4 Attributes",
    ));
    results
}

/// The Series' Instances, hierarchical, with Available Transfer Syntax UID
/// (PS3.4 C.6.1.1.5.2) asked for.
fn instances(
    session: &mut Session<'_>,
    target: &mut Target,
    study: &str,
    series: &str,
) -> Vec<InMemDicomObject> {
    let section = sec("10.6.1 Table 10.6.1-1, 10.6.3.3.3 Table 10.6.3-5");
    let mut verdict = Verdict::default();
    let url = session.url(
        &format!("/studies/{study}/series/{series}/instances"),
        Some("includefield=00083002"),
    );
    let results = session
        .send(&mut verdict, &Request::get(url, JSON))
        .and_then(|exchange| search_results(&mut verdict, &exchange));
    if results.is_some() {
        session.served("instances");
    }
    let results = results.unwrap_or_default();
    for (index, result) in results.iter().enumerate() {
        return_keys(
            &mut verdict,
            index,
            result,
            &INSTANCE_RETURN_KEYS,
            "Table 10.6.3-5",
        );
        target.note_retrieve_url("Instance", result);
        let sop_class = text(result, tags::SOP_CLASS_UID).unwrap_or_default();
        if is_image_storage_sop_class(&sop_class) {
            return_keys(
                &mut verdict,
                index,
                result,
                &IMAGE_RETURN_KEYS,
                "Table 10.6.3-5, \"Shall be present if known\"",
            );
        }
        if let (Some(uid), Some(syntaxes)) = (
            text(result, tags::SOP_INSTANCE_UID),
            result
                .get(tags::AVAILABLE_TRANSFER_SYNTAX_UID)
                .and_then(|element| element.to_multi_str().ok()),
        ) {
            if let [syntax] = syntaxes.as_ref() {
                target
                    .syntaxes
                    .insert(uid, syntax.trim_end_matches('\0').to_string());
            }
        }
    }
    choose_instance(target, &results, &mut verdict);
    target.instance_result = target.instance.as_deref().and_then(|instance| {
        results
            .iter()
            .find(|result| text(result, tags::SOP_INSTANCE_UID).as_deref() == Some(instance))
            .cloned()
    });
    session.push(verdict.finish(
        "dicomweb-search-instances",
        section,
        "the Series' Instances search returns each Instance with the Table 10.6.3-5 Attributes",
    ));
    results
}

/// The Instance to retrieve: the one given, else the VL Whole Slide
/// Microscopy Instance with the fewest Frames beyond one, so that a Frames
/// request names two Frames and stays small.
fn choose_instance(target: &mut Target, results: &[InMemDicomObject], verdict: &mut Verdict) {
    if let Some(instance) = &target.instance {
        let listed = results.iter().any(|result| {
            text(result, tags::SOP_INSTANCE_UID).as_deref() == Some(instance.as_str())
        });
        if !listed && !results.is_empty() {
            verdict.fail(format!(
                "Instance {instance} is not among the {} Instances the search returned ({})",
                results.len(),
                sec("10.6.1.1")
            ));
        }
        if let Some(result) = results.iter().find(|result| {
            text(result, tags::SOP_INSTANCE_UID).as_deref() == Some(instance.as_str())
        }) {
            target.frames = integer(result, tags::NUMBER_OF_FRAMES);
        }
        return;
    }
    let wsi = |result: &&InMemDicomObject| {
        text(result, tags::SOP_CLASS_UID).as_deref() == Some(crate::VL_WSI_SOP_CLASS_UID)
    };
    let frames = |result: &&InMemDicomObject| integer(result, tags::NUMBER_OF_FRAMES).unwrap_or(1);
    let chosen = results
        .iter()
        .filter(wsi)
        .filter(|result| frames(result) > 1)
        .min_by_key(|result| frames(result))
        .or_else(|| results.iter().find(wsi))
        .or(results.first());
    if let Some(result) = chosen {
        target.instance = text(result, tags::SOP_INSTANCE_UID);
        target.frames = integer(result, tags::NUMBER_OF_FRAMES);
    }
}

/// 8.3.4.4: `limit` and `offset` page the same ordered list of matches, and
/// a page that leaves matches unreturned says so in a Warning that names the
/// service (8.3.4.4.1, Table 10.6.3-1); the unpaged search, sent again,
/// returns the same ordered list (8.3.4.4.1: "The search requests shall be
/// idempotent").
fn paging(
    session: &mut Session<'_>,
    study: &str,
    series: Option<&str>,
    all_series: &[InMemDicomObject],
    instances: &[InMemDicomObject],
) {
    let section = sec("8.3.4.4, 8.3.4.4.1 and 10.6.3.1 Table 10.6.3-1");
    let mut verdict = Verdict::default();
    let (path, query, key, listed) = match series {
        Some(series) if instances.len() >= 2 => (
            format!("/studies/{study}/series/{series}/instances"),
            "includefield=00083002",
            tags::SOP_INSTANCE_UID,
            instances,
        ),
        _ if all_series.len() >= 2 => (
            format!("/studies/{study}/series"),
            "includefield=00200013",
            tags::SERIES_INSTANCE_UID,
            all_series,
        ),
        _ => {
            verdict.not_run("paging needs two matches, and the Study's Series and the Series' Instances have fewer");
            session.push(verdict.finish("dicomweb-search-paging", section, ""));
            return;
        }
    };
    let expected: Vec<Option<String>> = listed.iter().map(|found| text(found, key)).collect();
    let again = session.url(&path, Some(query));
    if let Some(exchange) = session.send(&mut verdict, &Request::get(again, JSON)) {
        if let Some(results) = search_results(&mut verdict, &exchange) {
            let returned: Vec<Option<String>> =
                results.iter().map(|result| text(result, key)).collect();
            if returned != expected {
                verdict.fail(format!(
                    "{} sent twice returned {:?} and then {:?}; two identical searches return the same ordered list of matches ({})",
                    exchange.request_line(),
                    expected,
                    returned,
                    sec("8.3.4.4.1")
                ));
            }
        }
    }
    for (offset, page_query) in [(0usize, "limit=1"), (1, "limit=1&offset=1")] {
        let query = format!("{query}&{page_query}");
        let url = session.url(&path, Some(&query));
        let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) else {
            break;
        };
        let Some(results) = search_results(&mut verdict, &exchange) else {
            continue;
        };
        let line = exchange.request_line();
        let returned: Vec<Option<String>> =
            results.iter().map(|result| text(result, key)).collect();
        if returned.len() != 1 || returned[0] != expected[offset] {
            verdict.fail(format!(
                "{line} returned {:?}; the unpaged search lists {:?} at offset {offset} of the same ordered list ({})",
                returned,
                expected[offset],
                sec("8.3.4.4.1")
            ));
        }
        let remaining = expected.len().saturating_sub(offset + 1);
        let warning = exchange
            .headers
            .iter()
            .find(|(name, value)| name == "warning" && value.trim_start().starts_with("299"))
            .map(|(_, value)| value.as_str());
        match warning {
            None if remaining > 0 => verdict.fail(format!(
                "{line} leaves {remaining} matches unreturned but has no \"Warning: 299 <service>: There are {remaining} additional results that can be requested\" ({})",
                sec("8.3.4.4.1")
            )),
            Some(value) => warning_agent(&mut verdict, &line, value),
            None => {}
        }
    }
    session.push(verdict.finish(
        "dicomweb-search-paging",
        section,
        "two identical searches list the same ordered matches, and limit and offset page them, with the 299 Warning naming the service while matches remain",
    ));
}

/// The warn-agent of a Search Warning: "<service> is the base URI for the
/// origin server" (8.3.4.4.1), and "the Warning Header Field shall contain a
/// URL" (Table 10.6.3-1).
pub(super) fn warning_agent(verdict: &mut Verdict, line: &str, value: &str) {
    let agent = value
        .trim_start()
        .strip_prefix("299")
        .map(str::trim_start)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|agent| agent.trim_end_matches(':'))
        .unwrap_or_default();
    if Url::parse(agent).is_err() {
        verdict.fail(format!(
            "{line}: Warning {value:?} names {agent:?} where the URL of the service is required ({})",
            sec("8.3.4.4.1 and 10.6.3.1 Table 10.6.3-1")
        ));
    }
}

/// 8.3.4.1 and Table 10.6.1-5: Modality is a required matching key; a
/// search matching nothing answers 200 with an empty Array, or an empty
/// NativeDicomModel for XML (8.3.4.4.1); a Study that does not exist has no
/// Series (the Hierarchical Search Method, PS3.4 C.4.1.3.1.1); Study Date
/// and Study Time ranges match as one date-time (8.3.4.1.1).
fn matching(
    session: &mut Session<'_>,
    target: &Target,
    study: &str,
    all_series: &[InMemDicomObject],
) {
    let section = sec("8.3.4.1, 8.3.4.1.1, 10.6.1.2.3 Table 10.6.1-5, 10.6.2 and 8.3.4.4.1");
    let mut verdict = Verdict::default();
    let url = session.url(&format!("/studies/{study}/series"), Some("Modality=SM"));
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        if let Some(results) = search_results(&mut verdict, &exchange) {
            modality_matches(&mut verdict, &exchange, &results, all_series);
        }
    }
    let absent = format!("SeriesInstanceUID={ABSENT_UID}");
    let url = session.url(&format!("/studies/{study}/series"), Some(&absent));
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url.clone(), JSON)) {
        empty_result(&mut verdict, &exchange);
    }
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, XML_ACCEPT)) {
        empty_xml_result(&mut verdict, &exchange);
    }
    let url = session.url(&format!("/studies/{ABSENT_UID}/series"), None);
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        empty_result(&mut verdict, &exchange);
    }
    match target.study_result.as_ref().and_then(combined_date_time) {
        Some(query) => {
            let query = format!("StudyInstanceUID={study}&{query}");
            let url = session.url("/studies", Some(&query));
            if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
                if let Some(results) = search_results(&mut verdict, &exchange) {
                    if results.len() != 1 {
                        verdict.fail(format!(
                            "{} returned {} Studies; the Study's date and time lie in the combined date-time range, though its time is outside the time range alone, so combined date-time matching returns it ({})",
                            exchange.request_line(),
                            results.len(),
                            sec("8.3.4.1.1 and PS3.4 C.2.2.2.5")
                        ));
                    }
                }
            }
        }
        None => verdict
            .note("the Study has no Study Date and Study Time to match as a combined date-time"),
    }
    session.push(verdict.finish(
        "dicomweb-search-matching",
        section,
        "Modality and combined date-time matching return exactly the matches, and a search without match answers 200 with [] or an empty NativeDicomModel",
    ));
}

fn modality_matches(
    verdict: &mut Verdict,
    exchange: &Exchange,
    results: &[InMemDicomObject],
    all_series: &[InMemDicomObject],
) {
    let mut returned: Vec<String> = Vec::new();
    for result in results {
        let modality = text(result, tags::MODALITY);
        if modality.as_deref() != Some("SM") {
            verdict.fail(format!(
                "{}: a result has Modality {modality:?}, which does not match SM ({})",
                exchange.request_line(),
                sec("8.3.4.1.1")
            ));
        }
        returned.extend(text(result, tags::SERIES_INSTANCE_UID));
    }
    let mut expected: Vec<String> = all_series
        .iter()
        .filter(|found| text(found, tags::MODALITY).as_deref() == Some("SM"))
        .filter_map(|found| text(found, tags::SERIES_INSTANCE_UID))
        .collect();
    returned.sort();
    expected.sort();
    if !all_series.is_empty() && returned != expected {
        verdict.fail(format!(
            "{}: matched {} Series, but the unconstrained search lists {} with Modality SM ({})",
            exchange.request_line(),
            returned.len(),
            expected.len(),
            sec("8.3.4.1.1")
        ));
    }
}

/// A Study Date range and a Study Time range that hold the Study only when
/// matched as one date-time range (PS3.4 C.2.2.2.5: "the concatenated
/// string values of each Range Matching component matched as if they were a
/// single Attribute of VR DT"): from the Study's date two minutes before its
/// time until a time one minute before it, on a later date; or, near
/// midnight, the mirror image.
fn combined_date_time(study: &InMemDicomObject) -> Option<String> {
    let date = text(study, tags::STUDY_DATE)?;
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let seconds = time_seconds(&text(study, tags::STUDY_TIME)?)?;
    let clock = |seconds: u32| {
        format!(
            "{:02}{:02}{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    };
    if seconds >= 120 {
        Some(format!(
            "StudyDate={date}-29991231&StudyTime={}-{}",
            clock(seconds - 120),
            clock(seconds - 60)
        ))
    } else {
        Some(format!(
            "StudyDate=19000101-{date}&StudyTime={}-{}",
            clock(seconds + 60),
            clock(seconds + 120)
        ))
    }
}

/// Seconds since midnight of a TM Value (HH, HHMM, HHMMSS, HHMMSS.F).
fn time_seconds(value: &str) -> Option<u32> {
    let whole = value.split('.').next()?;
    if whole.len() < 2 || whole.len() > 6 || whole.len() % 2 != 0 {
        return None;
    }
    let field = |at: usize| -> Option<u32> {
        match whole.get(at..at + 2) {
            Some(digits) => digits.parse().ok(),
            None => Some(0),
        }
    };
    let (hours, minutes, seconds) = (field(0)?, field(2)?, field(4)?);
    (hours < 24 && minutes < 60 && seconds < 60).then(|| hours * 3600 + minutes * 60 + seconds)
}

/// 8.3.4.4.1: "When the Acceptable Media Type is application/dicom+xml, the
/// empty payload shall be encoded as an empty NativeDicomModel root element."
fn empty_xml_result(verdict: &mut Verdict, exchange: &Exchange) {
    if !expect_status(
        verdict,
        exchange,
        &[200],
        "8.3.4.4.1 and 10.6.4 Table 10.6.4-1",
    ) {
        return;
    }
    let Some((_, parts)) = expect_multipart(
        verdict,
        exchange,
        Some("application/dicom+xml"),
        "10.6.4 Table 10.6.4-1",
    ) else {
        return;
    };
    let line = exchange.request_line();
    let empty = |body: &[u8]| {
        let text = String::from_utf8_lossy(body);
        text.contains("NativeDicomModel") && !text.contains("DicomAttribute")
    };
    if parts.is_empty() || !parts.iter().all(|part| empty(part.body)) {
        verdict.fail(format!(
            "{line}: no Series matches, and the payload is not an empty NativeDicomModel root element ({})",
            sec("8.3.4.4.1")
        ));
    }
}

/// 8.3.4.4.1: "a 200 (OK) response shall be returned with an empty payload
/// ... encoded as an empty JSON array ([])"; 204 is the retired behavior.
fn empty_result(verdict: &mut Verdict, exchange: &Exchange) {
    let line = exchange.request_line();
    if exchange.status == 204 {
        verdict.fail(format!(
            "{line} answered 204 (No Content), which earlier releases specified; a search without match answers 200 with an empty JSON array ({})",
            sec("8.3.4.4.1")
        ));
        return;
    }
    if !expect_status(verdict, exchange, &[200], "8.3.4.4.1 and 10.6.3.1") {
        return;
    }
    let body = String::from_utf8_lossy(&exchange.body);
    if body.trim() != "[]" {
        let shown: String = body.chars().take(80).collect();
        verdict.fail(format!(
            "{line}: no Series matches, and the payload is {shown:?}; it is an empty JSON array ([]) ({})",
            sec("8.3.4.4.1")
        ));
    }
}

/// 8.3: a supported Query Parameter with an invalid value answers 400, and
/// an unsupported one is ignored.
fn parameters(session: &mut Session<'_>, study: &str) {
    let section = sec("8.3 and 8.3.4.4");
    let mut verdict = Verdict::default();
    let url = session.url(
        "/studies",
        Some(&format!("StudyInstanceUID={study}&limit=many")),
    );
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        if exchange.status != 400 {
            verdict.fail(format!(
                "{} answered {}; \"limit\" is a uint, and a supported Query Parameter with an invalid value answers 400 (Bad Request) ({})",
                exchange.request_line(),
                exchange.status,
                sec("8.3 and 8.3.4.4")
            ));
        }
    }
    let url = session.url(
        "/studies",
        Some(&format!("StudyInstanceUID={study}&wsidicomprobe=1")),
    );
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        if let Some(results) = search_results(&mut verdict, &exchange) {
            if results.len() != 1 {
                verdict.fail(format!(
                    "{} returned {} results; an unsupported Query Parameter is ignored, so the Study is returned as without it ({})",
                    exchange.request_line(),
                    results.len(),
                    sec("8.3")
                ));
            }
        }
    }
    session.push(verdict.finish(
        "dicomweb-search-parameters",
        section,
        "an invalid limit answers 400 and an unsupported Query Parameter is ignored",
    ));
}

/// 8.7.5: "The response to a request without an Accept Header Field shall be
/// 406 (Not Acceptable)."
fn accept_required(session: &mut Session<'_>, study: &str) {
    let section = sec("8.7.5, 8.7.7 and 8.4.1 Table 8.4.1-1");
    let mut verdict = Verdict::default();
    let url = session.url("/studies", Some(&format!("StudyInstanceUID={study}")));
    let request = Request {
        accept: None,
        ..Request::get(url, JSON)
    };
    if let Some(exchange) = session.send(&mut verdict, &request) {
        expect_status(&mut verdict, &exchange, &[406], "8.7.5");
        let essence = exchange
            .media_type()
            .and_then(Result::ok)
            .map(|media| media.essence);
        if !exchange.body.is_empty() && essence.as_deref() != Some("text/html") {
            verdict.advise(format!(
                "{}: the Status Report of a request without an Acceptable Media Type is {}; it should be text/html, the default Media Type of the Text Resource Category ({})",
                exchange.request_line(),
                essence.as_deref().unwrap_or("without Content-Type"),
                sec("8.6.3")
            ));
        }
    }
    session.push(verdict.finish(
        "dicomweb-accept-required",
        section,
        "a request without an Accept header answers 406",
    ));
}

/// The results of a Search response: 200, application/dicom+json (10.6.4)
/// and the DICOM JSON Model.
pub(super) fn search_results(
    verdict: &mut Verdict,
    exchange: &Exchange,
) -> Option<Vec<InMemDicomObject>> {
    if !expect_status(verdict, exchange, &[200], "10.6.3.1") {
        return None;
    }
    expect_json(verdict, exchange, "10.6.4 Table 10.6.4-1")
}

/// The Type U and R Attributes of a result (PS3.4 C.2.2.1: returned, empty
/// if unknown).
pub(super) fn return_keys(
    verdict: &mut Verdict,
    index: usize,
    result: &InMemDicomObject,
    keys: &[(Tag, &str)],
    table: &str,
) {
    let missing: Vec<String> = keys
        .iter()
        .filter(|(tag, _)| result.get(*tag).is_none())
        .map(|(tag, name)| format!("{name} {}", tag_text(*tag)))
        .collect();
    if !missing.is_empty() {
        verdict.fail(format!(
            "result {} lacks {} ({})",
            index + 1,
            missing.join(", "),
            sec(&format!("10.6.3.3 {table}"))
        ));
    }
}

/// Tables 10.6.3-3 to 10.6.3-5: Retrieve URL "Shall be present if the
/// Instance is retrievable by the Retrieve Transaction", judged once a
/// Retrieve has been tried.
pub(super) fn retrieve_url_check(session: &mut Session<'_>, target: &Target) {
    let section = sec("10.6.3.3 Tables 10.6.3-3, 10.6.3-4 and 10.6.3-5, Retrieve URL (0008,1190)");
    let mut verdict = Verdict::default();
    if !target.searched {
        verdict.not_run("no Search results to hold to it");
    } else if !target.missing_retrieve_url.is_empty() {
        let levels = target.missing_retrieve_url.join(", ");
        if session.retrieved {
            verdict.fail(format!(
                "{levels} results lack Retrieve URL (0008,1190), though the origin server answers the Retrieve Transaction for them"
            ));
        } else {
            verdict.not_run(format!(
                "{levels} results lack Retrieve URL (0008,1190), which is required only if the Instances are retrievable, and no Retrieve succeeded"
            ));
        }
    }
    session.push(verdict.finish(
        "dicomweb-search-retrieve-url",
        section,
        "every Study, Series and Instance result has a Retrieve URL",
    ));
}
