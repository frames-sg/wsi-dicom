//! Retrieve of an Instance resource (PS3.18 10.4.1.1.1) validated as a
//! file, and the Metadata and Bulk Data the run retrieved held to it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request};
use super::media::MediaType;
use super::report::Verdict;
use super::response::{expect_multipart, expect_status, sec, tag_text, text};
use super::retrieve::Metadata;
use super::{DicomWebTransaction, Session, Target};
use crate::validation::{bulk_references, JsonSource};

const RETRIEVE_NAME: &str = "dicomweb-retrieve-instance";
const RETRIEVE_SECTIONS: &str =
    "10.4.1.1.1 Table 10.4.1-1, 10.4.3.3.1, 10.4.3.3.2, 10.4.3.3.5, 10.4.4 Table 10.4.4-1, 8.7.3.1 and 8.7.9";

/// What the run retrieved of the Instance before it: its Series Metadata
/// and the Bulk Data Values fetched by BulkDataURI.
pub(super) struct Held<'h> {
    pub(super) metadata: Option<&'h Metadata>,
    pub(super) fetched: &'h BTreeMap<String, Vec<u8>>,
}

/// The Instance in its Default media type, multipart/related;
/// type="application/dicom" (Table 10.4.4-1), any Transfer Syntax; its one
/// part is validated as a PS3.10 file, and the Metadata and Bulk Data
/// retrieved are held to it.
pub(super) fn retrieve(session: &mut Session<'_>, target: &Target, held: &Held<'_>) {
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
            instance_part(session, &mut verdict, &exchange, target, held, label);
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
    held: &Held<'_>,
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
    if let Ok(object) = dicom_object::OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .from_reader(part.body)
    {
        if let Some(uid) = target.instance.as_deref() {
            held_to_instance(verdict, &line, object.into_inner(), uid, held);
        }
    }
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
    let file = match stage_file(bytes) {
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
        session.audit.forbidden_syntax(
            &format!("{}: the Instance's File Meta Information", label.display()),
            syntax,
        );
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

/// `bytes` in a temporary file, for the validators that read files.
pub(super) fn stage_file(bytes: &[u8]) -> std::io::Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(file)
}

/// 10.4.3.3.2: "The payload for a Metadata Resource shall ... contain all
/// Attributes in the resource", so every Attribute of the Instance before
/// its Pixel Data, File Meta Information aside, is in its Metadata; and
/// 10.4.3.3.5: "The payload for a Bulk Data Resource shall contain all the
/// Bulk Data for the resource", so each Value fetched is the Instance's.
fn held_to_instance(
    verdict: &mut Verdict,
    line: &str,
    instance: InMemDicomObject,
    uid: &str,
    held: &Held<'_>,
) {
    let Some(metadata) = held.metadata else {
        return;
    };
    let Some(index) = metadata
        .objects
        .iter()
        .position(|object| text(object, tags::SOP_INSTANCE_UID).as_deref() == Some(uid))
    else {
        return;
    };
    let described: BTreeSet<Tag> = metadata.objects[index]
        .iter()
        .map(|element| element.header().tag)
        .collect();
    let missing: Vec<String> = instance
        .iter()
        .map(|element| element.header().tag)
        .filter(|tag| tag.group() != 0x0002 && tag.element() != 0x0000)
        .filter(|tag| !described.contains(tag))
        .map(tag_text)
        .collect();
    if !missing.is_empty() {
        verdict.fail(format!(
            "{line}: the Instance holds {} that its Metadata lacks{}; the Metadata contains all Attributes in the resource ({})",
            missing.iter().take(8).cloned().collect::<Vec<_>>().join(", "),
            if missing.len() > 8 { format!(" and {} more", missing.len() - 8) } else { String::new() },
            sec("10.4.3.3.2")
        ));
    }
    let prefix = format!("/{index}/");
    for reference in bulk_references(&metadata.bytes, JsonSource::Metadata)
        .unwrap_or_default()
        .into_iter()
        .filter(|reference| !reference.pixel_data && reference.pointer.starts_with(&prefix))
    {
        let Some(value) = held.fetched.get(&reference.uri) else {
            continue;
        };
        let segments: Vec<&str> = reference.pointer[prefix.len()..].split('/').collect();
        let Some(bytes) =
            element_at(&instance, &segments).and_then(|element| element.to_bytes().ok())
        else {
            continue;
        };
        let unpadded = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
        if value.as_slice() != &*bytes && value.as_slice() != unpadded {
            verdict.fail(format!(
                "{line}: the Bulk Data of {} is {} bytes that are not the {}-byte {} of the Instance; the payload of a Bulk Data resource holds all its Bulk Data ({})",
                reference.uri,
                value.len(),
                bytes.len(),
                tag_text(reference.tag),
                sec("10.4.3.3.5")
            ));
        }
    }
}

/// The element a JSON Pointer's segments below a Data Set name: tags, and
/// "Value" with an Item index for each Sequence on the way.
fn element_at<'o>(object: &'o InMemDicomObject, segments: &[&str]) -> Option<&'o InMemElement> {
    let (first, rest) = segments.split_first()?;
    let raw = u32::from_str_radix(first, 16).ok()?;
    let element = object.get(Tag((raw >> 16) as u16, raw as u16))?;
    match rest {
        [] => Some(element),
        ["Value", index, rest @ ..] => {
            let item = element.items()?.get(index.parse::<usize>().ok()?)?;
            element_at(item, rest)
        }
        _ => None,
    }
}
