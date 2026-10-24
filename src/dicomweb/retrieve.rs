//! Retrieve of Metadata and Bulk Data resources (PS3.18 10.4.1.1.2 and
//! 10.4.1.1.5), and the validation of the Metadata as DICOM JSON and as
//! Data Sets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request, BULKDATA_ACCEPT};
use super::media::MediaType;
use super::report::Verdict;
use super::response::{expect_multipart, expect_status, integer, sec, text};
use super::url::Url;
use super::{DicomWebTransaction, Session, Target};
use crate::validation::{
    bulk_references, read_json_data_sets, validate_json_bytes, DicomJsonOptions, JsonRun,
    JsonSource,
};

const JSON: &str = "application/dicom+json";
const XML_ACCEPT: &str = "multipart/related; type=\"application/dicom+xml\"";
/// Distinct BulkDataURIs fetched per run.
const MAX_BULK_FETCHES: usize = 8;

/// A Series Metadata response.
pub(super) struct Metadata {
    pub(super) url: Url,
    pub(super) bytes: Vec<u8>,
    pub(super) objects: Vec<InMemDicomObject>,
}

impl Metadata {
    /// The Data Set of SOP Instance `uid`.
    pub(super) fn object(&self, uid: &str) -> Option<&InMemDicomObject> {
        self.objects
            .iter()
            .find(|object| text(object, tags::SOP_INSTANCE_UID).as_deref() == Some(uid))
    }

    /// The Data Set of SOP Instance `uid` read again with the `fetched`
    /// BulkDataURI Values, such as the ICC Profile a Frame is compared with.
    pub(super) fn object_with(
        &self,
        uid: &str,
        fetched: &BTreeMap<String, Vec<u8>>,
    ) -> Option<InMemDicomObject> {
        read_json_data_sets(&self.bytes, fetched)
            .ok()?
            .0
            .into_iter()
            .find(|object| text(object, tags::SOP_INSTANCE_UID).as_deref() == Some(uid))
    }
}

/// Series Metadata (Table 10.4.1-2) in its Default media type,
/// application/dicom+json (Table 10.4.4-1). Picks the Instance to target
/// when the Search did not.
pub(super) fn series_metadata(session: &mut Session<'_>, target: &mut Target) -> Option<Metadata> {
    const NAME: &str = "dicomweb-retrieve-metadata";
    const SECTIONS: &str = "10.4.1.1.2 Table 10.4.1-2, 10.4.3.3.2, 10.4.4 Table 10.4.4-1 and F.2.1";
    if session.skipped(DicomWebTransaction::Metadata) {
        session.not_run(NAME, SECTIONS, "skipped by request (--skip metadata)");
        return None;
    }
    let Some(path) = target.series_path() else {
        session.not_run(NAME, SECTIONS, "no Series to target");
        return None;
    };
    let mut verdict = Verdict::default();
    let url = session.url(&format!("{path}/metadata"), None);
    let metadata = session
        .send(&mut verdict, &Request::get(url, JSON))
        .and_then(|exchange| read_metadata(session, &mut verdict, exchange, target));
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "the Series Metadata is an application/dicom+json Array of the Series' Instances",
    ));
    metadata
}

fn read_metadata(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: Exchange,
    target: &mut Target,
) -> Option<Metadata> {
    if !expect_status(verdict, &exchange, &[200], "10.4.3.1") {
        return None;
    }
    session.retrieved = true;
    session.audit.representation(&exchange);
    super::response::expect_media_type(verdict, &exchange, JSON, "10.4.4 Table 10.4.4-1")?;
    if !exchange.body.trim_ascii_start().starts_with(b"[") {
        verdict.fail(format!(
            "{}: the Metadata is not a top-level Array; JSON Metadata of one or more Instances is a single Array ({})",
            exchange.request_line(),
            sec("F.2.1 and 10.4.4")
        ));
    }
    let objects = match read_json_data_sets(&exchange.body, &BTreeMap::new()) {
        Ok((objects, _)) => objects,
        Err(reason) => {
            verdict.fail(format!("{}: {reason}", exchange.request_line()));
            return None;
        }
    };
    let line = exchange.request_line();
    let series = target.series.clone().unwrap_or_default();
    for (index, object) in objects.iter().enumerate() {
        let found = text(object, tags::SERIES_INSTANCE_UID).unwrap_or_default();
        if found != series {
            verdict.fail(format!(
                "{line}: Instance {} has Series Instance UID {found:?}; Series Metadata holds the Instances of Series {series} ({})",
                index + 1,
                sec("10.4.1.1.2")
            ));
        }
    }
    match target.instance.clone() {
        Some(uid) => {
            let present = objects.iter().any(|object| {
                text(object, tags::SOP_INSTANCE_UID).as_deref() == Some(uid.as_str())
            });
            if !present {
                verdict.fail(format!(
                    "{line}: the Series Metadata holds no Instance {uid} ({})",
                    sec("10.4.1.1.2")
                ));
            }
        }
        None => target.instance = smallest_slide(&objects),
    }
    if let Some(object) = target.instance.as_deref().and_then(|uid| {
        objects
            .iter()
            .find(|object| text(object, tags::SOP_INSTANCE_UID).as_deref() == Some(uid))
    }) {
        target.frames = target
            .frames
            .or_else(|| integer(object, tags::NUMBER_OF_FRAMES));
    }
    Some(Metadata {
        url: exchange.url.clone(),
        bytes: exchange.body,
        objects,
    })
}

/// The VL Whole Slide Microscopy Instance with the fewest Frames beyond one.
fn smallest_slide(objects: &[InMemDicomObject]) -> Option<String> {
    let frames = |object: &&InMemDicomObject| integer(object, tags::NUMBER_OF_FRAMES).unwrap_or(1);
    objects
        .iter()
        .filter(|object| {
            text(object, tags::SOP_CLASS_UID).as_deref() == Some(crate::VL_WSI_SOP_CLASS_UID)
        })
        .filter(|object| frames(object) > 1)
        .min_by_key(|object| frames(object))
        .or(objects.first())
        .and_then(|object| text(object, tags::SOP_INSTANCE_UID))
}

/// Bulk Data resources (10.4.1.1.5): the non-Pixel Data BulkDataURIs of the
/// Metadata, retrieved in the Required multipart/related;
/// type="application/octet-stream" (Table 10.4.4-1), each part carrying the
/// BulkDataURI as Content-Location (10.4.3.3.5).
pub(super) fn bulkdata(
    session: &mut Session<'_>,
    metadata: Option<&Metadata>,
) -> BTreeMap<String, Vec<u8>> {
    const NAME: &str = "dicomweb-retrieve-bulkdata";
    const SECTIONS: &str =
        "10.4.1.1.5, 10.4.1.1.5.1, 10.4.3.3.5, 10.4.4 Table 10.4.4-1 and 8.7.3.3.1";
    let mut fetched = BTreeMap::new();
    if session.skipped(DicomWebTransaction::Bulkdata) {
        session.not_run(NAME, SECTIONS, "skipped by request (--skip bulkdata)");
        return fetched;
    }
    let Some(metadata) = metadata else {
        session.not_run(NAME, SECTIONS, "no Metadata to read BulkDataURIs from");
        return fetched;
    };
    let mut uris: Vec<String> = Vec::new();
    for reference in bulk_references(&metadata.bytes, JsonSource::Metadata).unwrap_or_default() {
        if !reference.pixel_data && !uris.contains(&reference.uri) {
            uris.push(reference.uri);
        }
    }
    let mut verdict = Verdict::default();
    if uris.is_empty() {
        verdict.not_run("the Metadata gives no Value other than Pixel Data as BulkDataURI, and Pixel Data is retrieved as Frames");
    }
    if uris.len() > MAX_BULK_FETCHES {
        verdict.note(format!(
            "{} of {} distinct BulkDataURIs retrieved",
            MAX_BULK_FETCHES,
            uris.len()
        ));
    }
    for uri in uris.iter().take(MAX_BULK_FETCHES) {
        let url = match metadata.url.resolve(uri) {
            Ok(url) => url,
            Err(reason) => {
                verdict.fail(format!(
                    "BulkDataURI {uri:?} does not resolve against {}: {reason} ({})",
                    metadata.url,
                    sec("10.4.1.1.5.1")
                ));
                continue;
            }
        };
        let Some(exchange) =
            session.send(&mut verdict, &Request::get(url.clone(), BULKDATA_ACCEPT))
        else {
            continue;
        };
        if let Some(value) = bulk_value(session, &mut verdict, &exchange, &url) {
            fetched.insert(uri.clone(), value);
        }
    }
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "each BulkDataURI answers one application/octet-stream part whose Content-Location is the URI",
    ));
    fetched
}

fn bulk_value(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    requested: &Url,
) -> Option<Vec<u8>> {
    if !expect_status(verdict, exchange, &[200], "10.4.3.1") {
        return None;
    }
    let (_, parts) = expect_multipart(
        verdict,
        exchange,
        Some("application/octet-stream"),
        "10.4.4 Table 10.4.4-1",
    )?;
    session.audit.parts(exchange, &parts);
    let line = exchange.request_line();
    let [part] = parts.as_slice() else {
        verdict.fail(format!(
            "{line}: {} parts; a single Bulk Data URI's payload holds its one element ({})",
            parts.len(),
            sec("10.4.3.3.5")
        ));
        return None;
    };
    let essence = part
        .header("content-type")
        .and_then(|value| MediaType::parse(value).ok())
        .map(|media| media.essence);
    if essence.as_deref() != Some("application/octet-stream") {
        verdict.fail(format!(
            "{line}: the part is {}; uncompressed Bulk Data is application/octet-stream ({})",
            essence.unwrap_or_else(|| "without Content-Type".into()),
            sec("8.7.3.3.1")
        ));
    }
    match part.header("content-location").map(|location| exchange.url.resolve(location)) {
        Some(Ok(location)) if location.to_string() != requested.to_string() => verdict.fail(format!(
            "{line}: the part's Content-Location is {location}; it is the BulkDataURI {requested} of the Metadata ({})",
            sec("10.4.3.3.5")
        )),
        Some(Err(reason)) => verdict.fail(format!(
            "{line}: the part's Content-Location is not a URI reference: {reason} ({})",
            sec("10.4.3.3.5")
        )),
        _ => {}
    }
    Some(part.body.to_vec())
}

/// The Metadata held to Annex F and to the intrinsic checks of a file, with
/// the Bulk Data and Transfer Syntaxes the run learned.
pub(super) fn validate_metadata(
    session: &mut Session<'_>,
    metadata: Option<&Metadata>,
    fetched: &BTreeMap<String, Vec<u8>>,
    target: &Target,
) {
    let Some(metadata) = metadata else {
        return;
    };
    let options = DicomJsonOptions {
        validation: session.options.validation.clone(),
        transfer_syntax: None,
        fetch_bulkdata: None,
        bulkdata_base_url: Some(metadata.url.to_string()),
    };
    let label = PathBuf::from(metadata.url.to_string());
    let run = JsonRun {
        label: label.clone(),
        source: JsonSource::Metadata,
        options: &options,
        fetched,
        transfer_syntaxes: &target.syntaxes,
        fetch_requested: true,
        fetch_notes: &[],
    };
    let outcome = validate_json_bytes(&metadata.bytes, &run);
    session.objects.extend(outcome.checks);
    session.optional.extend(outcome.optional);
    session.files.push(label);
}

/// Instance Metadata (Table 10.4.1-2) as JSON, holding what the Series
/// Metadata holds for the Instance, and in the Required
/// multipart/related; type="application/dicom+xml" (Table 10.4.4-1).
pub(super) fn instance_metadata(
    session: &mut Session<'_>,
    target: &Target,
    series: Option<&Metadata>,
) {
    const JSON_NAME: &str = "dicomweb-retrieve-instance-metadata";
    const JSON_SECTIONS: &str = "10.4.1.1.2 Table 10.4.1-2, 10.4.3.3.2 and 10.4.4 Table 10.4.4-1";
    const XML_NAME: &str = "dicomweb-retrieve-metadata-xml";
    const XML_SECTIONS: &str = "10.4.4 Table 10.4.4-1, 8.7.3.2 Table 8.7.3-3 and 8.6.1.2";
    let path = target.instance_path();
    let reason = if session.skipped(DicomWebTransaction::Metadata) {
        Some("skipped by request (--skip metadata)")
    } else if path.is_none() {
        Some("no Instance to target")
    } else {
        None
    };
    if let Some(reason) = reason {
        session.not_run(JSON_NAME, JSON_SECTIONS, reason);
        session.not_run(XML_NAME, XML_SECTIONS, reason);
        return;
    }
    let path = path.unwrap_or_default();
    let uid = target.instance.clone().unwrap_or_default();
    let url = session.url(&format!("{path}/metadata"), None);
    let mut verdict = Verdict::default();
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url.clone(), JSON)) {
        let expected = series.and_then(|metadata| metadata.object(&uid));
        instance_json(session, &mut verdict, &exchange, &uid, expected);
    }
    session.push(verdict.finish(
        JSON_NAME,
        sec(JSON_SECTIONS),
        "the Instance Metadata is one Data Set, with the Attributes the Series Metadata holds for it",
    ));
    let mut verdict = Verdict::default();
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, XML_ACCEPT)) {
        instance_xml(session, &mut verdict, &exchange);
    }
    session.push(verdict.finish(
        XML_NAME,
        sec(XML_SECTIONS),
        "the Instance Metadata is returned as multipart/related application/dicom+xml",
    ));
}

fn instance_json(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    uid: &str,
    expected: Option<&InMemDicomObject>,
) {
    if !expect_status(verdict, exchange, &[200], "10.4.3.1") {
        return;
    }
    session.audit.representation(exchange);
    if super::response::expect_media_type(verdict, exchange, JSON, "10.4.4 Table 10.4.4-1")
        .is_none()
    {
        return;
    }
    let line = exchange.request_line();
    if !exchange.body.trim_ascii_start().starts_with(b"[") {
        verdict.fail(format!(
            "{line}: the Metadata is not a top-level Array ({})",
            sec("F.2.1 and 10.4.4")
        ));
    }
    let objects = match read_json_data_sets(&exchange.body, &BTreeMap::new()) {
        Ok((objects, violations)) => {
            if let Some(violations) = violations {
                verdict.fail(format!("{line}: {violations}"));
            }
            objects
        }
        Err(reason) => {
            verdict.fail(format!("{line}: {reason}"));
            return;
        }
    };
    let [object] = objects.as_slice() else {
        verdict.fail(format!(
            "{line}: {} Data Sets; the Metadata of one Instance is one ({})",
            objects.len(),
            sec("10.4.1.1.2")
        ));
        return;
    };
    if text(object, tags::SOP_INSTANCE_UID).as_deref() != Some(uid) {
        verdict.fail(format!(
            "{line}: SOP Instance UID is {:?}, not {uid} ({})",
            text(object, tags::SOP_INSTANCE_UID),
            sec("10.4.1.1.2")
        ));
    }
    if let Some(expected) = expected {
        let held: BTreeSet<Tag> = object.iter().map(|element| element.header().tag).collect();
        let listed: BTreeSet<Tag> = expected
            .iter()
            .map(|element| element.header().tag)
            .collect();
        let differ: Vec<String> = held
            .symmetric_difference(&listed)
            .take(8)
            .map(|tag| super::response::tag_text(*tag))
            .collect();
        if !differ.is_empty() {
            verdict.fail(format!(
                "{line}: the Instance Metadata and the Series Metadata of the Instance differ in {}; each holds all Attributes of the resource ({})",
                differ.join(", "),
                sec("10.4.3.3.2")
            ));
        }
    }
}

fn instance_xml(session: &mut Session<'_>, verdict: &mut Verdict, exchange: &Exchange) {
    if !expect_status(
        verdict,
        exchange,
        &[200],
        "10.4.3.1 and 10.4.4 Table 10.4.4-1",
    ) {
        return;
    }
    let Some((_, parts)) = expect_multipart(
        verdict,
        exchange,
        Some("application/dicom+xml"),
        "10.4.4 Table 10.4.4-1",
    ) else {
        return;
    };
    session.audit.parts(exchange, &parts);
    let line = exchange.request_line();
    if parts.is_empty() {
        return;
    }
    for (index, part) in parts.iter().enumerate() {
        let essence = part
            .header("content-type")
            .and_then(|value| MediaType::parse(value).ok())
            .map(|media| media.essence);
        if essence.as_deref() != Some("application/dicom+xml") {
            verdict.fail(format!(
                "{line}: part {} is {}, not application/dicom+xml ({})",
                index + 1,
                essence.unwrap_or_else(|| "without Content-Type".into()),
                sec("8.7.3.2 Table 8.7.3-3")
            ));
        }
        if !String::from_utf8_lossy(part.body).contains("NativeDicomModel") {
            verdict.fail(format!(
                "{line}: part {} holds no NativeDicomModel element (PS3.19 Native DICOM Model, {})",
                index + 1,
                sec("8.7.3.2 Table 8.7.3-3")
            ));
        }
    }
}
