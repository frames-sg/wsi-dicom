//! DICOM JSON documents (PS3.18 Annex F), such as a Retrieve Metadata
//! response or a saved `.json` file, held to the requirements of a file.
//!
//! Each Data Set is checked against the DICOM JSON Model, read into an
//! in-memory object and run through the intrinsic checks a file gets. What
//! only encoded bytes show is reported as such: Pixel Data, which a document
//! gives as a BulkDataURI if at all, the PS3.5 encoding, and the File Meta
//! Information, which Metadata does not include (PS3.18 10.4.1.1.2).
//!
//! Metadata describes the Resource, not a Representation, so it names no
//! Transfer Syntax. Unless one is known, the checks run once per Transfer
//! Syntax an origin server may hold the Pixel Data in (PS3.18 Table 8.7.3-2);
//! a check that passes with some and fails with others is undecidable from
//! the document and reported as skipped with both lists. Likewise, a check
//! whose verdict changes with the stand-in for a Value given only as a
//! BulkDataURI depends on that Value and is skipped until it is fetched.

mod evaluate;
mod json;
mod model;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dicom_object::InMemDicomObject;
use serde::{Deserialize, Serialize};

use super::iod::{Family, EDITION, MAX_NESTING};
use super::wsi_conformance::Origin;
use super::{
    add_set_advisories, intrinsic_check, set_checks, OptionalConformance, SetCorpus,
    ValidationCheck, ValidationOptions, ValidationReport, ValidationStatus,
};
use crate::dicomweb::{Client, Url};
use crate::Error;
use json::Json;
pub(crate) use model::{BulkReference, Source};
use model::{Reader, Stand};

const JSON_MODEL_RULE: &str = "intrinsic-wsi-dicom-json-model";
const JSON_BULKDATA_RULE: &str = "intrinsic-wsi-dicom-json-bulkdata";
/// Distinct BulkDataURIs one document may have fetched.
const MAX_BULK_FETCHES: usize = 64;

/// Options for validating a DICOM JSON document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct DicomJsonOptions {
    /// Rule set, conformance level and limits; `max_input_bytes` bounds the
    /// document.
    pub validation: ValidationOptions,
    /// The Transfer Syntax UID the Pixel Data is encoded in, when known. A
    /// document names none (PS3.18 10.4.1.1.2) unless it holds a Transfer
    /// Syntax UID (0002,0010) or a single Available Transfer Syntax UID
    /// (0008,3002).
    pub transfer_syntax: Option<String>,
    /// Fetch the Values given as BulkDataURI, other than Pixel Data, over
    /// DICOMweb. Off by default: nothing leaves the machine unless asked.
    pub fetch_bulkdata: Option<crate::DicomWebHttpOptions>,
    /// The URL relative BulkDataURIs resolve against (PS3.18 10.4.1.1.5.1):
    /// the Metadata request URL the document was retrieved from. A bearer
    /// token is sent to its origin only.
    pub bulkdata_base_url: Option<String>,
}

impl Default for DicomJsonOptions {
    fn default() -> Self {
        Self {
            validation: ValidationOptions {
                max_input_bytes: 256 * 1024 * 1024,
                ..ValidationOptions::default()
            },
            transfer_syntax: None,
            fetch_bulkdata: None,
            bulkdata_base_url: None,
        }
    }
}

/// Validate a DICOM JSON document (PS3.18 Annex F), such as a saved
/// Retrieve Metadata response: its JSON Model, then each Data Set as the
/// intrinsic checks hold a file.
pub fn validate_dicom_json_path(
    path: impl AsRef<Path>,
    options: &DicomJsonOptions,
) -> Result<ValidationReport, Error> {
    let path = path.as_ref();
    let bytes = read_bounded(path, options.validation.max_input_bytes)?;
    let (fetched, fetch_notes) = match &options.fetch_bulkdata {
        Some(http) => fetch_bulk_values(&bytes, http, options.bulkdata_base_url.as_deref())?,
        None => (BTreeMap::new(), Vec::new()),
    };
    let run = JsonRun {
        label: path.to_path_buf(),
        source: Source::File,
        options,
        fetched: &fetched,
        transfer_syntaxes: &BTreeMap::new(),
        fetch_requested: options.fetch_bulkdata.is_some(),
        fetch_notes: &fetch_notes,
    };
    let outcome = validate_json_bytes(&bytes, &run);
    Ok(ValidationReport {
        profile: options.validation.profile,
        input: path.to_path_buf(),
        files: vec![path.to_path_buf()],
        checks: outcome.checks,
        optional: outcome.optional,
        conventions: super::ConventionReport::default(),
    })
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|err| Error::Validation {
        reason: format!("cannot open {}: {err}", path.display()),
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| Error::Validation {
            reason: format!("cannot read {}: {err}", path.display()),
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::Validation {
            reason: format!("{} exceeds max_input_bytes={max_bytes}", path.display()),
        });
    }
    Ok(bytes)
}

/// The fetched Values by BulkDataURI, and the URIs that could not be fetched.
type FetchedBulkData = (BTreeMap<String, Vec<u8>>, Vec<String>);

/// Fetches the non-Pixel Data BulkDataURI Values of a saved document. A
/// bearer token needs the base URL, the one origin it is sent to: a hostile
/// document could otherwise name any host.
fn fetch_bulk_values(
    bytes: &[u8],
    http: &crate::DicomWebHttpOptions,
    base: Option<&str>,
) -> Result<FetchedBulkData, Error> {
    let invalid = |reason: String| Error::InvalidOptions { reason };
    let client = Client::new(http)?;
    let client =
        match (base, http.bearer_token_env.is_some()) {
            (Some(base), _) => client.for_origin(&Url::parse(base).map_err(invalid)?)?,
            (None, true) => return Err(invalid(
                "a bearer token for BulkDataURIs needs --bulkdata-base-url, the origin it is for"
                    .into(),
            )),
            (None, false) => client,
        };
    let mut fetched = BTreeMap::new();
    let mut notes = Vec::new();
    let references = bulk_references(bytes, Source::File).unwrap_or_default();
    for reference in references.iter().filter(|reference| !reference.pixel_data) {
        if fetched.contains_key(&reference.uri) {
            continue;
        }
        if fetched.len() + notes.len() == MAX_BULK_FETCHES {
            notes.push(format!(
                "{}: more than {MAX_BULK_FETCHES} distinct BulkDataURIs; the rest are not fetched",
                reference.uri
            ));
            break;
        }
        match client.fetch_bulkdata(base, &reference.uri) {
            Ok(value) => {
                fetched.insert(reference.uri.clone(), value);
            }
            Err(reason) => notes.push(format!("{}: {reason}", reference.uri)),
        }
    }
    Ok((fetched, notes))
}

/// One validation of a document's bytes.
pub(crate) struct JsonRun<'r> {
    /// The document's name in check paths; each Data Set is `label#/index`.
    pub(crate) label: PathBuf,
    pub(crate) source: Source,
    pub(crate) options: &'r DicomJsonOptions,
    /// Bulk Data Values fetched by URI.
    pub(crate) fetched: &'r BTreeMap<String, Vec<u8>>,
    /// Transfer Syntax UIDs known by SOP Instance UID, such as those a Frames
    /// response or a Search named.
    pub(crate) transfer_syntaxes: &'r BTreeMap<String, String>,
    pub(crate) fetch_requested: bool,
    /// BulkDataURIs that could not be fetched, with why.
    pub(crate) fetch_notes: &'r [String],
}

/// The checks of a document, and each Data Set as read.
pub(crate) struct JsonOutcome {
    pub(crate) checks: Vec<ValidationCheck>,
    pub(crate) optional: Vec<OptionalConformance>,
}

/// Every BulkDataURI of a DICOM JSON document, in document order; `None`
/// when the document is not JSON.
pub(crate) fn bulk_references(bytes: &[u8], source: Source) -> Option<Vec<BulkReference>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let parsed = json::parse(text, json_depth()).ok()?;
    let empty = BTreeMap::new();
    let mut references = Vec::new();
    for (pointer, members) in data_sets(&parsed) {
        let mut model = reader(source, Stand::Sparse, &empty, None);
        model.data_set(members, &pointer, 0);
        references.extend(model.bulk);
    }
    Some(references)
}

/// The Data Sets of a DICOM JSON response, such as Search results or a Store
/// Instances Response Module, with the `fetched` BulkDataURI Values, and the
/// summary of its JSON Model violations. An error when the payload is not
/// JSON or not an Array or object.
pub(crate) fn read_json_data_sets(
    bytes: &[u8],
    fetched: &BTreeMap<String, Vec<u8>>,
) -> Result<(Vec<InMemDicomObject>, Option<String>), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| format!("the payload is not UTF-8 ({err}) ({})", cite("F.2")))?;
    let parsed = json::parse(text, json_depth())
        .map_err(|err| format!("the payload is not JSON text (RFC 8259): {err}"))?;
    if !matches!(parsed, Json::Array(_) | Json::Object(_)) {
        return Err(format!(
            "the payload is {}; results are an Array of DICOM JSON Model objects ({})",
            parsed.kind(),
            cite("F.2.1")
        ));
    }
    let mut violations = Family::default();
    if let Json::Array(elements) = &parsed {
        for (index, element) in elements.iter().enumerate() {
            if !matches!(element, Json::Object(_)) {
                violations.push(|| {
                    format!(
                        "/{index}: a result is {}, not a DICOM JSON Model object ({})",
                        element.kind(),
                        cite("F.2.1")
                    )
                });
            }
        }
    }
    let mut objects = Vec::new();
    for (pointer, members) in data_sets(&parsed) {
        let mut model = reader(Source::File, Stand::Sparse, fetched, Some(&mut violations));
        objects.push(model.data_set(members, &pointer, 0));
    }
    let summary = (!violations.is_empty()).then(|| {
        violations
            .check(
                Path::new(""),
                JSON_MODEL_RULE,
                &format!("DICOM {}", cite("Annex F")),
            )
            .message
    });
    Ok((objects, summary))
}

/// Whether `bytes` are RFC 8259 JSON text.
pub(crate) fn json_text_error(bytes: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => return Some(format!("not UTF-8: {err}")),
    };
    json::parse(text, json_depth())
        .err()
        .map(|err| err.to_string())
}

/// Arrays and objects a Data Set at MAX_NESTING Sequence levels needs: an
/// Attribute object, its Value Array and an Item per level, and the top.
fn json_depth() -> usize {
    MAX_NESTING * 3 + 4
}

fn reader<'r>(
    source: Source,
    stand: Stand,
    fetched: &'r BTreeMap<String, Vec<u8>>,
    violations: Option<&'r mut Family>,
) -> Reader<'r> {
    Reader {
        source,
        stand,
        fetched,
        violations,
        bulk: Vec::new(),
        file_meta: Vec::new(),
    }
}

/// The DICOM JSON Model objects of a document, each with its JSON Pointer:
/// the objects of the top-level Array (F.2.1), or a lone top-level object.
fn data_sets<'j, 'a>(parsed: &'j Json<'a>) -> Vec<(String, &'j [(String, Json<'a>)])> {
    match parsed {
        Json::Array(elements) => elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| match element {
                Json::Object(members) => Some((format!("/{index}"), members.as_slice())),
                _ => None,
            })
            .collect(),
        Json::Object(members) => vec![(String::new(), members.as_slice())],
        _ => Vec::new(),
    }
}

/// The value of the one member named `key`, when exactly one is.
fn sole<'j, 'a>(members: &'j [(String, Json<'a>)], key: &str) -> Option<&'j Json<'a>> {
    let mut found = members.iter().filter(|(name, _)| name == key);
    match (found.next(), found.next()) {
        (Some((_, value)), None) => Some(value),
        _ => None,
    }
}

/// The SOP Instance UID (0008,0018) the members of a Data Set give, when
/// they plainly give one.
fn instance_uid<'j>(members: &'j [(String, Json<'_>)]) -> Option<&'j str> {
    let Json::Object(attribute) = sole(members, "00080018")? else {
        return None;
    };
    let Json::Array(values) = sole(attribute, "Value")? else {
        return None;
    };
    match values.as_slice() {
        [Json::String(uid)] => Some(uid.trim_end_matches(['\0', ' '])),
        _ => None,
    }
}

/// The SOP Instance UIDs that more than one Data Set of the document gives.
fn repeated_instance_uids<'j>(
    data_sets: &[(String, &'j [(String, Json<'_>)])],
) -> BTreeSet<&'j str> {
    let mut seen = BTreeSet::new();
    let mut repeated = BTreeSet::new();
    for uid in data_sets
        .iter()
        .filter_map(|(_, members)| instance_uid(members))
    {
        if !seen.insert(uid) {
            repeated.insert(uid);
        }
    }
    repeated
}

/// Only Data Sets that share a SOP Instance UID are compared as copies, so
/// only they, and any whose UID is not plainly given, are digested.
fn copy_origin(members: &[(String, Json<'_>)], repeated: &BTreeSet<&str>) -> Origin {
    match instance_uid(members) {
        Some(uid) if !repeated.contains(uid) => Origin::UniqueLabel,
        _ => Origin::Label,
    }
}

fn cite(section: &str) -> String {
    format!("PS3.18 {EDITION} {section}")
}

/// Validates the bytes of one DICOM JSON document.
pub(crate) fn validate_json_bytes(bytes: &[u8], run: &JsonRun<'_>) -> JsonOutcome {
    let label = run.label.display().to_string();
    let failed = |message: String| JsonOutcome {
        checks: vec![intrinsic_check(
            JSON_MODEL_RULE,
            Some(run.label.clone()),
            ValidationStatus::Failed,
            message,
        )],
        optional: Vec::new(),
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            return failed(format!(
                "DICOM {}: the document is not UTF-8 ({err}); the DICOM JSON Model's character repertoire is UTF-8",
                cite("F.2")
            ))
        }
    };
    let parsed = match json::parse(text, json_depth()) {
        Ok(parsed) => parsed,
        Err(err) => {
            return failed(format!(
                "DICOM {}: the document is not JSON text (RFC 8259): {err}",
                cite("F.1")
            ))
        }
    };
    let mut checks = Vec::new();
    if let Some(check) = document_check(&parsed, run) {
        checks.push(check);
    }
    let mut optional = Vec::new();
    let mut corpus = SetCorpus::default();
    let mut bulk_seen = Vec::new();
    let data_sets = data_sets(&parsed);
    let repeated = repeated_instance_uids(&data_sets);
    for (pointer, members) in data_sets {
        let path = PathBuf::from(format!("{label}#{pointer}"));
        let evaluated = evaluate::instance(&path, &pointer, members, run);
        checks.extend(evaluated.checks);
        optional.extend(evaluated.notes);
        if let Some(object) = &evaluated.reference {
            corpus.observe(&path, object, copy_origin(members, &repeated));
        }
        bulk_seen.extend(evaluated.bulk);
    }
    let set = set_checks(&corpus, run.options.validation.profile);
    checks.extend(set.checks);
    add_set_advisories(&mut optional, set.advisories);
    checks.extend(bulk_check(&run.label, &bulk_seen, run));
    JsonOutcome { checks, optional }
}

/// F.2.1: the document is an Array of DICOM JSON Model objects; a saved
/// document may hold one object alone.
fn document_check(parsed: &Json<'_>, run: &JsonRun<'_>) -> Option<ValidationCheck> {
    let mut document = Family::default();
    match parsed {
        Json::Array(elements) => {
            for (index, element) in elements.iter().enumerate() {
                if !matches!(element, Json::Object(_)) {
                    document.push(|| format!(
                        "/{index}: a result is {}; multiple results are an Array of DICOM JSON Model objects ({})",
                        element.kind(),
                        cite("F.2.1")
                    ));
                }
            }
        }
        Json::Object(_) if run.source == Source::File => {}
        other => document.push(|| format!(
            "the document is {}; the Metadata of one or more Instances is a single top-level Array of DICOM JSON Model objects ({}, {})",
            other.kind(),
            cite("F.2.1"),
            cite("10.4.4")
        )),
    }
    (!document.is_empty()).then(|| {
        document.check(
            &run.label,
            JSON_MODEL_RULE,
            &format!("DICOM {}", cite("Annex F")),
        )
    })
}

/// Which BulkDataURI Values the checks saw, and which only as stand-ins.
fn bulk_check(label: &Path, bulk: &[BulkReference], run: &JsonRun<'_>) -> Option<ValidationCheck> {
    if bulk.iter().all(|reference| reference.pixel_data) {
        return None;
    }
    let missing: Vec<&BulkReference> = bulk
        .iter()
        .filter(|reference| !reference.pixel_data && !run.fetched.contains_key(&reference.uri))
        .collect();
    if missing.is_empty() {
        return Some(intrinsic_check(
            JSON_BULKDATA_RULE,
            Some(label.to_path_buf()),
            ValidationStatus::Passed,
            format!(
                "DICOM {}: every BulkDataURI Value other than Pixel Data was fetched and checked",
                cite("F.2.6")
            ),
        ));
    }
    let listed: Vec<String> = missing
        .iter()
        .take(8)
        .map(|reference| format!("{} {}", reference.pointer, reference.uri))
        .collect();
    let more = missing.len() - listed.len();
    let why = if run.fetch_requested {
        format!("could not be fetched ({})", run.fetch_notes.join("; "))
    } else {
        "were not fetched; rerun with --fetch-bulkdata (and --bulkdata-base-url for relative URIs) to check them".to_string()
    };
    Some(intrinsic_check(
        JSON_BULKDATA_RULE,
        Some(label.to_path_buf()),
        ValidationStatus::Skipped,
        format!(
            "DICOM {}: {} BulkDataURI Value(s) {why}; the checks whose verdict depends on them are skipped: {}{}",
            cite("F.2.6"),
            missing.len(),
            listed.join(", "),
            if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            }
        ),
    ))
}
