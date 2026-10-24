//! Retrieve of an Instance resource (PS3.18 10.4.1.1.1) validated as a
//! file, and the opt-in Store Transaction (10.5, Annex I) read back.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Method, Redirects, Request};
use super::media::MediaType;
use super::report::Verdict;
use super::response::{expect_json, expect_multipart, expect_status, sec, text};
use super::{DicomWebTransaction, Session, Target};

const RETRIEVE_NAME: &str = "dicomweb-retrieve-instance";
const RETRIEVE_SECTIONS: &str =
    "10.4.1.1.1 Table 10.4.1-1, 10.4.3.3.1, 10.4.4 Table 10.4.4-1, 8.7.3.1 and 8.7.9";
const STORE_NAME: &str = "dicomweb-store";
const STORE_SECTIONS: &str = "10.5, 10.5.3 Tables 10.5.3-1 and 10.5.3-2, and Annex I";
const READBACK_NAME: &str = "dicomweb-store-readback";
const READBACK_SECTIONS: &str = "10.5.2 and 10.4.1.1.2";
const JSON: &str = "application/dicom+json";

/// The Instance in its Default media type, multipart/related;
/// type="application/dicom" (Table 10.4.4-1), any Transfer Syntax; its one
/// part is validated as a PS3.10 file.
pub(super) fn retrieve(session: &mut Session<'_>, target: &Target) {
    if session.skipped(DicomWebTransaction::Instance) {
        session.not_run(
            RETRIEVE_NAME,
            RETRIEVE_SECTIONS,
            "skipped by request (--skip instance)",
        );
        return;
    }
    let Some(path) = target.instance_path() else {
        session.not_run(RETRIEVE_NAME, RETRIEVE_SECTIONS, "no Instance to target");
        return;
    };
    let mut verdict = Verdict::default();
    let url = session.url(&path, None);
    let label = PathBuf::from(url.to_string());
    let accept = "multipart/related; type=\"application/dicom\"; transfer-syntax=*";
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, accept)) {
        if expect_status(&mut verdict, &exchange, &[200], "10.4.3.1") {
            instance_part(session, &mut verdict, &exchange, target, label);
        }
    }
    session.push(verdict.finish(
        RETRIEVE_NAME,
        sec(RETRIEVE_SECTIONS),
        "the Instance comes as one application/dicom part holding a PS3.10 file",
    ));
}

fn instance_part(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    target: &Target,
    label: PathBuf,
) {
    let Some((_, parts)) = expect_multipart(
        verdict,
        exchange,
        Some("application/dicom"),
        "10.4.4 Table 10.4.4-1",
    ) else {
        return;
    };
    session.audit.parts(exchange, &parts);
    let line = exchange.request_line();
    let [part] = parts.as_slice() else {
        verdict.fail(format!(
            "{line}: {} parts; the Instance resource holds one Instance ({})",
            parts.len(),
            sec("10.4.3.3.1")
        ));
        return;
    };
    let media = part
        .header("content-type")
        .and_then(|value| MediaType::parse(value).ok());
    if media.as_ref().map(|media| media.essence.as_str()) != Some("application/dicom") {
        verdict.fail(format!(
            "{line}: the part is not application/dicom ({})",
            sec("8.7.3.1")
        ));
    }
    if part.body.get(128..132) != Some(b"DICM".as_slice()) {
        verdict.fail(format!(
            "{line}: the part is not a PS3.10 file: no \"DICM\" prefix after the 128-byte preamble (PS3.10 {} 7.1, {})",
            super::EDITION,
            sec("8.7.3.1")
        ));
        return;
    }
    let declared = media
        .as_ref()
        .and_then(|media| media.parameter("transfer-syntax"))
        .map(str::to_string);
    validate_file(
        session,
        verdict,
        part.body,
        &label,
        target,
        declared.as_deref(),
    );
}

/// Runs the file validation on the retrieved Instance, the checks labelled
/// with its URL.
fn validate_file(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    bytes: &[u8],
    label: &Path,
    target: &Target,
    declared: Option<&str>,
) {
    let staged = tempfile::NamedTempFile::new().and_then(|mut file| {
        file.write_all(bytes)?;
        file.flush()?;
        Ok(file)
    });
    let file = match staged {
        Ok(file) => file,
        Err(err) => {
            verdict.note(format!(
                "the Instance could not be staged for validation: {err}"
            ));
            return;
        }
    };
    if let Ok(object) = dicom_object::OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .open_file(file.path())
    {
        let meta = object.meta();
        let uid = meta.media_storage_sop_instance_uid.trim_end_matches('\0');
        if target.instance.as_deref() != Some(uid) {
            verdict.fail(format!(
                "the retrieved Instance is {uid:?}, not {} ({})",
                target.instance.as_deref().unwrap_or_default(),
                sec("10.4.3.3.1")
            ));
        }
        let syntax = meta.transfer_syntax.trim_end_matches('\0');
        if let Some(declared) = declared.filter(|declared| *declared != syntax) {
            verdict.note(format!(
                "the part names transfer-syntax={declared} and the File Meta Information {syntax}; the File Meta Information has precedence ({})",
                sec("8.7.9")
            ));
        }
    }
    match (session.validate_file)(file.path(), &session.options.validation) {
        Ok(report) => {
            let staged_path = file.path().to_path_buf();
            session
                .objects
                .extend(report.checks.into_iter().map(|mut check| {
                    if check.path.as_deref() == Some(staged_path.as_path()) {
                        check.path = Some(label.to_path_buf());
                    }
                    check
                }));
            session
                .optional
                .extend(report.optional.into_iter().map(|mut notes| {
                    notes.path = label.to_path_buf();
                    notes
                }));
            session.files.push(label.to_path_buf());
        }
        Err(err) => verdict.note(format!(
            "the retrieved Instance could not be validated: {err}"
        )),
    }
}

/// STOW-RS of the file the user named (10.5): one application/dicom part
/// to the Studies resource, the Store Instances Response Module checked
/// (Annex I), then the Instance's Metadata read back.
pub(super) fn store(session: &mut Session<'_>) {
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
        redirects: Redirects::Never,
    };
    let mut stored = false;
    if let Some(exchange) = session.send(&mut verdict, &request) {
        stored = store_response(&mut verdict, &exchange, &staged);
    }
    session.push(verdict.finish(
        STORE_NAME,
        sec(STORE_SECTIONS),
        "the Instance is stored, with a Store Instances Response Module referencing it",
    ));
    readback(session, &staged, stored);
}

/// The UIDs of the file to store, and its bytes.
struct Staged {
    bytes: Vec<u8>,
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

/// Table 10.5.3-1 status, Table 10.5.3-2 header fields and the Annex I
/// Store Instances Response Module. Returns whether the Instance was stored.
fn store_response(verdict: &mut Verdict, exchange: &Exchange, staged: &Staged) -> bool {
    if !expect_status(verdict, exchange, &[200, 202], "10.5.3.1 Table 10.5.3-1") {
        return false;
    }
    let line = exchange.request_line();
    if exchange.status == 202 {
        verdict.note("202: stored with warnings or failures (10.5.3.1)");
    }
    if exchange.header("location").is_none() {
        verdict.fail(format!(
            "{line}: no Location, which is present when a new resource was created ({})",
            sec("10.5.3.2 Table 10.5.3-2")
        ));
    }
    let Some(objects) = expect_json(verdict, exchange, "10.5.3.3 and Annex I") else {
        return false;
    };
    let Some(module) = objects.first() else {
        verdict.fail(format!(
            "{line}: the payload holds no Store Instances Response Module ({})",
            sec("10.5.3.3")
        ));
        return false;
    };
    if module.get(tags::RETRIEVE_URL).is_none() {
        verdict.fail(format!(
            "{line}: Retrieve URL (0008,1190), Type 2, is absent ({})",
            sec("I.1 Table I.1-1")
        ));
    }
    let failed = module
        .get(tags::FAILED_SOP_SEQUENCE)
        .and_then(|element| element.items())
        .map_or(0, <[InMemDicomObject]>::len);
    if failed > 0 {
        verdict.fail(format!(
            "{line}: Failed SOP Sequence (0008,1198) lists {failed} Instance(s) ({})",
            sec("I.1 Table I.1-1 and I.2.2")
        ));
    }
    let items = module
        .get(tags::REFERENCED_SOP_SEQUENCE)
        .and_then(|element| element.items())
        .unwrap_or_default();
    let referenced = items.iter().find(|item| {
        text(item, tags::REFERENCED_SOP_INSTANCE_UID).as_deref() == Some(staged.instance.as_str())
    });
    match referenced {
        None => {
            verdict.fail(format!(
                "{line}: Referenced SOP Sequence (0008,1199) has no Item for the stored Instance {} ({})",
                staged.instance,
                sec("I.1 Table I.1-1")
            ));
            false
        }
        Some(item) => {
            if text(item, tags::REFERENCED_SOP_CLASS_UID).as_deref() != Some(staged.class.as_str())
            {
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
            true
        }
    }
}

/// The stored Instance reads back: its Metadata names it (10.5.2: "The
/// origin server stores Instances").
fn readback(session: &mut Session<'_>, staged: &Staged, stored: bool) {
    if !stored {
        session.not_run(
            READBACK_NAME,
            READBACK_SECTIONS,
            "the Instance was not stored",
        );
        return;
    }
    let mut verdict = Verdict::default();
    let url = session.url(
        &format!(
            "/studies/{}/series/{}/instances/{}/metadata",
            staged.study, staged.series, staged.instance
        ),
        None,
    );
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, JSON)) {
        if expect_status(&mut verdict, &exchange, &[200], "10.4.3.1") {
            if let Some(objects) = expect_json(&mut verdict, &exchange, "10.4.4 Table 10.4.4-1") {
                let found = objects.iter().any(|object| {
                    text(object, tags::SOP_INSTANCE_UID).as_deref()
                        == Some(staged.instance.as_str())
                });
                if !found {
                    verdict.fail(format!(
                        "{}: the Metadata does not hold the stored Instance {} ({})",
                        exchange.request_line(),
                        staged.instance,
                        sec("10.5.2")
                    ));
                }
            }
        }
    }
    session.push(verdict.finish(
        READBACK_NAME,
        sec(READBACK_SECTIONS),
        "the stored Instance's Metadata is retrieved",
    ));
}
