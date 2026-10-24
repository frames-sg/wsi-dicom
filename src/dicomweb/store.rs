//! The opt-in Store Transaction (PS3.18 10.5, Annex I): the file the user
//! named stored with a chunked request, the Store Instances Response Module
//! checked, and the stored Instance read back and held to the file stored.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Method, Redirects, Request};
use super::instance::stage_file;
use super::report::Verdict;
use super::response::{expect_json, expect_multipart, expect_status, sec, tag_text, text};
use super::Session;
use crate::ValidationStatus;

const STORE_NAME: &str = "dicomweb-store";
const STORE_SECTIONS: &str =
    "10.5, 10.5.1.3 Table 10.5.1-2, 10.5.3 Tables 10.5.3-1 and 10.5.3-2, and Annex I";
const READBACK_NAME: &str = "dicomweb-store-readback";
const READBACK_SECTIONS: &str = "10.5.2, 10.5.3, 10.4.1.1.2 and I.2.1";
const JSON: &str = "application/dicom+json";
/// Warning Reasons (0008,1196) of Table I.2-1 for modified and for
/// discarded Data Elements.
const COERCION: u16 = 0xB000;
const DISCARDED: u16 = 0xB006;
/// The VRs whose Values the Metadata holds as the Strings the file holds,
/// compared to find coerced Attributes.
const COMPARED_VRS: [VR; 15] = [
    VR::AE,
    VR::AS,
    VR::CS,
    VR::DA,
    VR::DT,
    VR::LO,
    VR::LT,
    VR::PN,
    VR::SH,
    VR::ST,
    VR::TM,
    VR::UC,
    VR::UI,
    VR::UR,
    VR::UT,
];

/// STOW-RS of the file the user named (10.5): one application/dicom part
/// to the Studies resource, sent chunked, since the origin server supports
/// Transfer-Encoding as well as Content-Length (Table 10.5.1-2).
pub(super) fn run(session: &mut Session<'_>) {
    let Some(file) = session.options.store_file.clone() else {
        let reason =
            "Store changes the origin server's state, so it runs only with --allow-store <FILE>";
        session.not_run(STORE_NAME, STORE_SECTIONS, reason);
        session.not_run(READBACK_NAME, READBACK_SECTIONS, reason);
        return;
    };
    let staged = match stage(&file, session.options.validation.max_input_bytes) {
        Ok(staged) => staged,
        Err(reason) => {
            session.not_run(STORE_NAME, STORE_SECTIONS, &reason);
            session.not_run(READBACK_NAME, READBACK_SECTIONS, &reason);
            return;
        }
    };
    let mut verdict = Verdict::default();
    let boundary = boundary_for(&staged.bytes);
    let content_type =
        format!("multipart/related; type=\"application/dicom\"; boundary={boundary}");
    let mut body = format!(
        "--{boundary}\r\nContent-Type: application/dicom\r\nContent-Length: {}\r\n\r\n",
        staged.bytes.len()
    )
    .into_bytes();
    body.extend_from_slice(&staged.bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
    let request = Request {
        method: Method::Post,
        url: session.url("/studies", None),
        accept: Some(JSON),
        content_type: Some(&content_type),
        body: Some(&body),
        chunked: true,
        headers: &[],
        redirects: Redirects::Never,
    };
    let mut outcome = None;
    if let Some(exchange) = session.send(&mut verdict, &request) {
        if exchange.status == 411 {
            verdict.fail(format!(
                "{} answered 411 (Length Required) to a chunked request; the origin server supports Transfer-Encoding (Table 10.5.1-2, {})",
                exchange.request_line(),
                sec("10.5.1.3")
            ));
        } else {
            outcome = store_response(&mut verdict, &exchange, &staged);
        }
    }
    session.push(verdict.finish(
        STORE_NAME,
        sec(STORE_SECTIONS),
        "the Instance is stored from a chunked request, with a Store Instances Response Module referencing it",
    ));
    readback(session, &staged, outcome);
}

/// The file to store: its bytes, its Data Set before Pixel Data, its UIDs.
struct Staged {
    bytes: Vec<u8>,
    object: InMemDicomObject,
    study: String,
    series: String,
    instance: String,
    class: String,
}

fn stage(path: &Path, max_bytes: u64) -> Result<Staged, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(max_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
        })
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "{} exceeds max_input_bytes={max_bytes}",
            path.display()
        ));
    }
    let object = dicom_object::OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .from_reader(bytes.as_slice())
        .map_err(|err| format!("{} is not a PS3.10 file: {err}", path.display()))?;
    let uid = |tag, name: &str| {
        text(&object, tag).ok_or_else(|| format!("{} has no {name}", path.display()))
    };
    Ok(Staged {
        study: uid(tags::STUDY_INSTANCE_UID, "Study Instance UID")?,
        series: uid(tags::SERIES_INSTANCE_UID, "Series Instance UID")?,
        instance: uid(tags::SOP_INSTANCE_UID, "SOP Instance UID")?,
        class: uid(tags::SOP_CLASS_UID, "SOP Class UID")?,
        object: object.into_inner(),
        bytes,
    })
}

/// A boundary that does not occur in the payload.
fn boundary_for(payload: &[u8]) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let mut boundary = format!("wsi-dicom-{nanos:x}");
    while payload
        .windows(boundary.len())
        .any(|window| window == boundary.as_bytes())
    {
        boundary.push('x');
    }
    boundary
}

/// What the response says of the stored Instance: its Warning Reason, if
/// any.
struct Stored {
    warning: Option<u16>,
}

/// The Items of Sequence `tag`, or none.
fn items(object: &InMemDicomObject, tag: Tag) -> &[InMemDicomObject] {
    object
        .get(tag)
        .and_then(|element| element.items())
        .unwrap_or_default()
}

/// Table 10.5.3-1 status, Table 10.5.3-2 header fields and the Annex I
/// Store Instances Response Module. Returns the stored Instance's outcome.
fn store_response(verdict: &mut Verdict, exchange: &Exchange, staged: &Staged) -> Option<Stored> {
    if !expect_status(verdict, exchange, &[200, 202], "10.5.3.1 Table 10.5.3-1") {
        return None;
    }
    let line = exchange.request_line();
    if exchange.status == 202 {
        verdict.note("202: stored with warnings or failures (10.5.3.1)");
    }
    for (_, warning) in exchange
        .headers
        .iter()
        .filter(|(name, _)| name == "warning")
    {
        let coded = warning
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word.len() == 4 && word.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if !coded {
            verdict.advise(format!(
                "{line}: Warning {warning:?} holds no DICOM Status Code; the text returned in the Warning Header Field should contain one and a descriptive reason ({})",
                sec("10.5.3.2")
            ));
        }
    }
    for header in ["Location", "Content-Location"] {
        if exchange.header(&header.to_ascii_lowercase()).is_none() {
            verdict.fail(format!(
                "{line}: no {header}, which is present when a new resource was created ({})",
                sec("10.5.3.2 Table 10.5.3-2")
            ));
        }
    }
    let objects = expect_json(verdict, exchange, "10.5.3.3 and Annex I")?;
    let Some(module) = objects.first() else {
        verdict.fail(format!(
            "{line}: the payload holds no Store Instances Response Module ({})",
            sec("10.5.3.3")
        ));
        return None;
    };
    if module.get(tags::RETRIEVE_URL).is_none() {
        verdict.fail(format!(
            "{line}: Retrieve URL (0008,1190), Type 2, is absent ({})",
            sec("I.1 Table I.1-1")
        ));
    }
    let failed = items(module, tags::FAILED_SOP_SEQUENCE);
    if !failed.is_empty() {
        verdict.fail(format!(
            "{line}: Failed SOP Sequence (0008,1198) lists {} Instance(s) ({})",
            failed.len(),
            sec("I.1 Table I.1-1 and I.2.2")
        ));
    }
    for (sequence, name) in [
        (tags::FAILED_SOP_SEQUENCE, "Failed SOP Sequence (0008,1198)"),
        (
            tags::OTHER_FAILURES_SEQUENCE,
            "Other Failures Sequence (0008,119A)",
        ),
    ] {
        if items(module, sequence)
            .iter()
            .any(|item| item.get(tags::FAILURE_REASON).is_none())
        {
            verdict.fail(format!(
                "{line}: an Item of {name} has no Failure Reason (0008,1197), Type 1 ({})",
                sec("I.1 Table I.1-1")
            ));
        }
    }
    let referenced = items(module, tags::REFERENCED_SOP_SEQUENCE);
    let warned = referenced
        .iter()
        .any(|item| item.get(tags::WARNING_REASON).is_some());
    appropriate_status(verdict, exchange, failed.len(), warned, module);
    let Some(item) = referenced.iter().find(|item| {
        text(item, tags::REFERENCED_SOP_INSTANCE_UID).as_deref() == Some(staged.instance.as_str())
    }) else {
        verdict.fail(format!(
            "{line}: Referenced SOP Sequence (0008,1199) has no Item for the stored Instance {} ({})",
            staged.instance,
            sec("I.1 Table I.1-1")
        ));
        return None;
    };
    referenced_item(verdict, &line, item, staged);
    Some(Stored {
        warning: item
            .get(tags::WARNING_REASON)
            .and_then(|element| element.to_int::<u16>().ok()),
    })
}

/// 10.5.3: "The response shall contain an appropriate status code": 200
/// when every Instance was stored, 202 when some were with warnings or
/// failures (Table 10.5.3-1).
fn appropriate_status(
    verdict: &mut Verdict,
    exchange: &Exchange,
    failed: usize,
    warned: bool,
    module: &InMemDicomObject,
) {
    let line = exchange.request_line();
    let other_failures = !items(module, tags::OTHER_FAILURES_SEQUENCE).is_empty();
    if exchange.status == 200 && (failed > 0 || other_failures) {
        verdict.fail(format!(
            "{line} answered 200, which means every Instance was stored, yet the module lists failures; 202 (some stored) or 409 (none) is appropriate ({})",
            sec("10.5.3 and 10.5.3.1 Table 10.5.3-1")
        ));
    }
    if exchange.status == 202 && failed == 0 && !other_failures && !warned {
        verdict.fail(format!(
            "{line} answered 202, which means warnings or failures exist, yet the module lists none; 200 is appropriate ({})",
            sec("10.5.3 and 10.5.3.1 Table 10.5.3-1")
        ));
    }
}

/// The stored Instance's Item: its SOP Class, Retrieve URL, and each
/// Original Attributes Sequence Item's one Modified Attributes Sequence
/// Item (I.1: "Only a single Item shall be included in this sequence").
fn referenced_item(verdict: &mut Verdict, line: &str, item: &InMemDicomObject, staged: &Staged) {
    if text(item, tags::REFERENCED_SOP_CLASS_UID).as_deref() != Some(staged.class.as_str()) {
        verdict.fail(format!(
            "{line}: the Referenced SOP Sequence Item's Referenced SOP Class UID is not {} ({})",
            staged.class,
            sec("I.1 Table I.1-1")
        ));
    }
    if item.get(tags::RETRIEVE_URL).is_none() {
        verdict.fail(format!(
            "{line}: the Referenced SOP Sequence Item has no Retrieve URL (0008,1190), Type 2 ({})",
            sec("I.1 Table I.1-1")
        ));
    }
    for (index, original) in items(item, tags::ORIGINAL_ATTRIBUTES_SEQUENCE)
        .iter()
        .enumerate()
    {
        let modified = items(original, tags::MODIFIED_ATTRIBUTES_SEQUENCE).len();
        if modified != 1 {
            verdict.fail(format!(
                "{line}: Original Attributes Sequence (0400,0561) Item {} has {modified} Modified Attributes Sequence (0400,0550) Items; only a single Item is included ({})",
                index + 1,
                sec("I.1 Table I.1-1")
            ));
        }
    }
}

/// The stored Instance reads back: its Metadata names it, holds the file's
/// values or records the coercion (10.5.2) with its Warning Reason (I.2.1),
/// and the Instance conforms as a file (10.5.2).
fn readback(session: &mut Session<'_>, staged: &Staged, outcome: Option<Stored>) {
    let Some(stored) = outcome else {
        session.not_run(
            READBACK_NAME,
            READBACK_SECTIONS,
            "the Instance was not stored",
        );
        return;
    };
    let mut verdict = Verdict::default();
    let path = format!(
        "/studies/{}/series/{}/instances/{}",
        staged.study, staged.series, staged.instance
    );
    let url = session.url(&format!("{path}/metadata"), None);
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        if expect_status(&mut verdict, &exchange, &[200], "10.4.3.1") {
            if let Some(objects) = expect_json(&mut verdict, &exchange, "10.4.4 Table 10.4.4-1") {
                match objects.iter().find(|object| {
                    text(object, tags::SOP_INSTANCE_UID).as_deref()
                        == Some(staged.instance.as_str())
                }) {
                    Some(object) => coercion(&mut verdict, &exchange, staged, object, &stored),
                    None => verdict.fail(format!(
                        "{}: the Metadata does not hold the stored Instance {} ({})",
                        exchange.request_line(),
                        staged.instance,
                        sec("10.5.2")
                    )),
                }
            }
        }
    }
    conforms(session, &mut verdict, &path);
    session.push(verdict.finish(
        READBACK_NAME,
        sec(READBACK_SECTIONS),
        "the stored Instance reads back with the values stored, or its coercions recorded, and conforms as a file",
    ));
}

/// 10.5.2 and 10.5.3: "If any Attribute is coerced or corrected, the
/// Original Attribute Sequence (0400,0561) shall be included in the DICOM
/// Object that is stored"; I.2.1: such an Instance's Warning Reason is
/// Coercion of Data Elements (B000) or Elements Discarded (B006).
fn coercion(
    verdict: &mut Verdict,
    exchange: &Exchange,
    staged: &Staged,
    stored: &InMemDicomObject,
    outcome: &Stored,
) {
    let line = exchange.request_line();
    let changed: Vec<String> = staged
        .object
        .iter()
        .filter(|element| {
            element.header().tag.group() != 0x0002 && COMPARED_VRS.contains(&element.vr())
        })
        .filter(|element| {
            stored.get(element.header().tag).is_none_or(|found| {
                normalized(found.to_str().ok()) != normalized(element.to_str().ok())
            })
        })
        .map(|element| tag_text(element.header().tag))
        .collect();
    if changed.is_empty() {
        return;
    }
    let shown = changed
        .iter()
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items(stored, tags::ORIGINAL_ATTRIBUTES_SEQUENCE).is_empty() {
        verdict.fail(format!(
            "{line}: the stored Instance does not hold the stored file's {shown}, so they were coerced or corrected, yet it has no Original Attributes Sequence (0400,0561) ({})",
            sec("10.5.2 and 10.5.3")
        ));
    }
    if !matches!(outcome.warning, Some(COERCION | DISCARDED)) {
        verdict.fail(format!(
            "{line}: {shown} were coerced or removed in storage, yet the Store response's Warning Reason (0008,1196) is {}; it is B000 (Coercion of Data Elements) or B006 (Elements Discarded) ({})",
            outcome
                .warning
                .map_or("absent".to_string(), |warning| format!("{warning:04X}")),
            sec("I.2.1 Table I.2-1")
        ));
    }
}

fn normalized(value: Option<std::borrow::Cow<'_, str>>) -> String {
    value
        .map(|value| value.trim_end_matches(['\0', ' ']).trim().to_string())
        .unwrap_or_default()
}

/// 10.5.2: "The stored Instance(s) shall fully conform to the IOD and
/// encoding requirements of PS3.3 and PS3.5": the Instance retrieved and
/// validated as a file, against the file stored validated the same way. The
/// origin server fails only checks the stored file passes; the file's own
/// failures are the user's, and noted.
fn conforms(session: &mut Session<'_>, verdict: &mut Verdict, path: &str) {
    let url = session.url(path, None);
    let accept = "multipart/related; type=\"application/dicom\"; transfer-syntax=*";
    let Some(exchange) = session.send(verdict, &Request::get(url, accept)) else {
        return;
    };
    if !expect_status(verdict, &exchange, &[200], "10.4.3.1") {
        return;
    }
    let Some((_, parts)) = expect_multipart(
        verdict,
        &exchange,
        Some("application/dicom"),
        "10.4.4 Table 10.4.4-1",
    ) else {
        return;
    };
    let line = exchange.request_line();
    let [part] = parts.as_slice() else {
        verdict.fail(format!(
            "{line}: {} parts; the Instance resource holds one Instance ({})",
            parts.len(),
            sec("10.4.3.3.1")
        ));
        return;
    };
    let file = match stage_file(part.body) {
        Ok(file) => file,
        Err(err) => {
            verdict.note(format!(
                "the stored Instance could not be staged for validation: {err}"
            ));
            return;
        }
    };
    let original = match session.options.store_file.as_deref() {
        Some(stored) => failed_checks(session, stored),
        None => return,
    };
    let original = match original {
        Ok(failed) => failed,
        Err(err) => {
            verdict.note(format!(
                "the file stored could not be validated, so the stored Instance is not compared with it: {err}"
            ));
            return;
        }
    };
    if !original.is_empty() {
        verdict.note(format!(
            "the stored file was already non-conformant: {} checks failed before storing ({})",
            original.len(),
            original.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    match failed_checks(session, file.path()) {
        Ok(copy) => {
            let introduced: Vec<&str> = copy
                .iter()
                .filter(|name| !original.contains(*name))
                .map(String::as_str)
                .collect();
            if !introduced.is_empty() {
                verdict.fail(format!(
                    "{line}: the stored Instance fails {}, which the file stored passes; stored Instances fully conform to the IOD and encoding requirements of PS3.3 and PS3.5 ({})",
                    introduced.join(", "),
                    sec("10.5.2")
                ));
            }
        }
        Err(err) => verdict.fail(format!(
            "{line}: the stored Instance is not a readable PS3.10 file, though the file stored is: {err} ({})",
            sec("10.5.2")
        )),
    }
}

/// The names of the checks `path` fails as a file.
fn failed_checks(session: &Session<'_>, path: &Path) -> Result<BTreeSet<String>, crate::Error> {
    let report = (session.validate_file)(path, &session.options.validation)?;
    Ok(report
        .checks
        .into_iter()
        .filter(|check| check.status == ValidationStatus::Failed)
        .map(|check| check.name)
        .collect())
}
