//! Retrieve of Frames (PS3.18 10.4.1.1.6): the multipart/related framing,
//! the Bulk Data media type of each part's Transfer Syntax (Tables 8.7.3-4
//! and 8.7.3-5), and each Frame's codestream held to the Metadata.

use std::path::PathBuf;

use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request};
use super::media::{any_frame_accept, bulk_media_type, media_type_of, MediaType};
use super::report::Verdict;
use super::response::{expect_multipart, expect_status, integer, sec};
use super::url::Url;
use super::{DicomWebTransaction, Session, Target};
use crate::lossy::method_for_lossy_transfer_syntax;
use crate::validation::check_retrieved_frames;

const NAME: &str = "dicomweb-retrieve-frames";
const SECTIONS: &str =
    "10.4.1.1.6 Table 10.4.1.6-1, 8.6.1.2, 8.7.3.3.2 Table 8.7.3-5, 8.7.3.5.2, 8.7.8.2, 8.7.9 and 10.4.3.3.6";
const UNCOMPRESSED_NAME: &str = "dicomweb-retrieve-frames-uncompressed";
const UNCOMPRESSED_SECTIONS: &str = "10.4.4 Table 10.4.4-1, 8.7.3.3.1 Table 8.7.3-4 and 8.7.3.4";
const MISSING_NAME: &str = "dicomweb-retrieve-frames-missing";
const MISSING_SECTIONS: &str = "10.4.3.1 Table 10.4.3-1 and 8.5";

/// Runs the Frames checks; returns the Transfer Syntax the Frames came in.
pub(super) fn run(
    session: &mut Session<'_>,
    target: &Target,
    object: Option<&InMemDicomObject>,
    pixel_uri: Option<&Url>,
) -> Option<String> {
    let reason = if session.skipped(DicomWebTransaction::Frames) {
        Some("skipped by request (--skip frames)")
    } else if target.instance_path().is_none() {
        Some("no Instance to target")
    } else {
        None
    };
    if let Some(reason) = reason {
        for (name, sections) in [
            (NAME, SECTIONS),
            (UNCOMPRESSED_NAME, UNCOMPRESSED_SECTIONS),
            (MISSING_NAME, MISSING_SECTIONS),
        ] {
            session.not_run(name, sections, reason);
        }
        return None;
    }
    let count = object
        .and_then(|object| integer(object, tags::NUMBER_OF_FRAMES))
        .or(target.frames);
    let numbers = frame_numbers(&session.options.frames, count);
    let syntax = native(session, target, object, &numbers, pixel_uri);
    uncompressed(session, target, object, numbers[0], syntax.as_deref());
    missing(session, target, count);
    syntax
}

/// The Frames to request: those asked for, else Frames 1 and 2 of a
/// multi-frame Instance.
fn frame_numbers(asked: &[u32], count: Option<u64>) -> Vec<usize> {
    if !asked.is_empty() {
        return asked.iter().map(|number| *number as usize).collect();
    }
    if count.unwrap_or(1) >= 2 {
        vec![1, 2]
    } else {
        vec![1]
    }
}

fn list(numbers: &[usize]) -> String {
    numbers
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The Frames in the Transfer Syntax the origin server holds: the one the
/// Search named, else any of Table 8.7.3-5 ("transfer-syntax=*").
/// What a Frames request asked for and the run knows of the Frames.
struct Asked<'a> {
    numbers: &'a [usize],
    known: Option<&'a str>,
    object: Option<&'a InMemDicomObject>,
    /// The Instance's URL and the BulkDataURI of its Pixel Data in the
    /// Metadata, which each part's Content-Location corresponds to.
    instance: Url,
    pixel_uri: Option<&'a Url>,
}

fn native(
    session: &mut Session<'_>,
    target: &Target,
    object: Option<&InMemDicomObject>,
    numbers: &[usize],
    pixel_uri: Option<&Url>,
) -> Option<String> {
    let mut verdict = Verdict::default();
    let path = target.instance_path().unwrap_or_default();
    let known = target
        .instance
        .as_ref()
        .and_then(|uid| target.syntaxes.get(uid))
        .cloned();
    let accept = match &known {
        Some(syntax) => format!(
            "multipart/related; type=\"{}\"; transfer-syntax={syntax}",
            media_type_of(syntax).unwrap_or("application/octet-stream")
        ),
        None => any_frame_accept(),
    };
    let url = session.url(&format!("{path}/frames/{}", list(numbers)), None);
    let label = PathBuf::from(url.to_string());
    let asked = Asked {
        numbers,
        known: known.as_deref(),
        object,
        instance: session.url(&path, None),
        pixel_uri,
    };
    let mut syntax = None;
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, &accept)) {
        if expect_status(&mut verdict, &exchange, &[200], "10.4.3.1") {
            session.retrieved = true;
            session.served("frames");
            syntax = frame_parts(session, &mut verdict, &exchange, &asked, label);
        }
    }
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "the Frames come as one multipart/related part each, in the media type of their Transfer Syntax",
    ));
    syntax
}

/// One part's media type and the Transfer Syntax it names or implies.
struct PartSyntax {
    essence: String,
    syntax: String,
}

fn part_syntax(
    verdict: &mut Verdict,
    line: &str,
    number: usize,
    value: Option<&str>,
) -> Option<PartSyntax> {
    let media = MediaType::parse(value?).ok()?;
    let Some(row) = bulk_media_type(&media.essence) else {
        verdict.fail(format!(
            "{line}: part {number} is {}, which is not a Bulk Data media type of Table 8.7.3-4 or 8.7.3-5 ({})",
            media.essence,
            sec("8.7.3.3")
        ));
        return None;
    };
    if media.count("transfer-syntax") > 1 {
        verdict.fail(format!(
            "{line}: part {number} has {} transfer-syntax parameters; a DICOM media type has at most one ({})",
            media.count("transfer-syntax"),
            sec("8.7.3.5.2 and 8.7.9")
        ));
    }
    let syntax = media
        .parameter("transfer-syntax")
        .unwrap_or(row.syntaxes[0])
        .to_string();
    if !row.syntaxes.contains(&syntax.as_str()) {
        verdict.fail(format!(
            "{line}: part {number} is {} with transfer-syntax={syntax}, which Table 8.7.3-5 assigns {} ({})",
            media.essence,
            media_type_of(&syntax).unwrap_or("no Bulk Data media type"),
            sec("8.7.3.3.2 Table 8.7.3-5")
        ));
    }
    Some(PartSyntax {
        essence: media.essence,
        syntax,
    })
}

fn frame_parts(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    asked: &Asked<'_>,
    label: PathBuf,
) -> Option<String> {
    let (numbers, known, object) = (asked.numbers, asked.known, asked.object);
    let (media, parts) = expect_multipart(verdict, exchange, None, "10.4.4 Table 10.4.4-1")?;
    session.audit.parts(exchange, &parts);
    let line = exchange.request_line();
    if parts.len() != numbers.len() {
        verdict.fail(format!(
            "{line}: {} parts for {} requested Frames; the payload holds the Frames resource's Frames ({})",
            parts.len(),
            numbers.len(),
            sec("10.4.3.3.6")
        ));
    }
    let root = media
        .parameter("type")
        .and_then(|value| MediaType::parse(value).ok())
        .map(|kind| kind.essence);
    let mut frames: Vec<(usize, &[u8])> = Vec::new();
    let mut syntaxes: Vec<String> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let number = numbers.get(index).copied().unwrap_or(index + 1);
        if let Some(location) = part.header("content-location") {
            corresponding_location(verdict, exchange, asked, (index + 1, number), location);
        }
        let Some(found) = part_syntax(verdict, &line, index + 1, part.header("content-type"))
        else {
            continue;
        };
        if index == 0 && root.as_deref().is_some_and(|root| root != found.essence) {
            verdict.fail(format!(
                "{line}: the \"type\" parameter names {}, but the root part is {} ({})",
                root.as_deref().unwrap_or_default(),
                found.essence,
                sec("8.6.1.2")
            ));
        }
        if let Some(known) = known.filter(|known| *known != found.syntax) {
            verdict.fail(format!(
                "{line}: part {} is in Transfer Syntax {}, but the request accepted only {known} ({})",
                index + 1,
                found.syntax,
                sec("8.7.8.2")
            ));
        }
        if found.essence == "application/octet-stream" {
            if let Some(expected) = object.and_then(frame_bytes) {
                if part.body.len() as u64 != expected {
                    verdict.fail(format!(
                        "{line}: Frame {number} is {} bytes of application/octet-stream; Rows, Columns, Samples per Pixel and Bits Allocated give {expected} ({})",
                        part.body.len(),
                        sec("8.7.3.3.1")
                    ));
                }
            }
        } else {
            frames.push((number, part.body));
        }
        if !syntaxes.contains(&found.syntax) {
            syntaxes.push(found.syntax);
        }
    }
    if let [syntax] = syntaxes.as_slice() {
        codestreams(session, verdict, object, syntax, &frames, label);
    } else if syntaxes.len() > 1 {
        verdict.note(format!(
            "the parts came in {} Transfer Syntaxes ({}); their codestreams are not checked",
            syntaxes.len(),
            syntaxes.join(", ")
        ));
    }
    syntaxes.into_iter().next()
}

/// 10.4.3.3.6: "Pixel Data in a multipart response shall have a
/// Content-Location Header Field that corresponds to the URI contained in
/// the corresponding Element in the Metadata": the Pixel Data BulkDataURI,
/// or the Frame of it or of the Instance.
fn corresponding_location(
    verdict: &mut Verdict,
    exchange: &Exchange,
    asked: &Asked<'_>,
    (part, number): (usize, usize),
    location: &str,
) {
    let line = exchange.request_line();
    let resolved = match exchange.url.resolve(location) {
        Ok(resolved) => resolved.to_string(),
        Err(reason) => {
            verdict.fail(format!(
                "{line}: part {part}'s Content-Location {location:?} is not a URI reference: {reason} ({})",
                sec("10.4.3.3.6")
            ));
            return;
        }
    };
    let mut corresponding = vec![format!("{}/frames/{number}", asked.instance)];
    if let Some(pixel) = asked.pixel_uri {
        corresponding.push(pixel.to_string());
        corresponding.push(format!("{pixel}/frames/{number}"));
    }
    if !corresponding.contains(&resolved) {
        verdict.fail(format!(
            "{line}: part {part}'s Content-Location is {resolved}; it corresponds to the Pixel Data of the Instance in the Metadata, such as {} ({})",
            corresponding.join(" or "),
            sec("10.4.3.3.6")
        ));
    }
}

/// The codestream check of the retrieved Frames, as a file's Frames get it.
fn codestreams(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    object: Option<&InMemDicomObject>,
    syntax: &str,
    frames: &[(usize, &[u8])],
    label: PathBuf,
) {
    if frames.is_empty() {
        return;
    }
    let Some(object) = object else {
        verdict.note("no Metadata to hold the Frames' codestreams to");
        return;
    };
    let max_frame_bytes = session.options.validation.max_pixel_frame_bytes;
    if let Some(check) = check_retrieved_frames(&label, object, syntax, frames, max_frame_bytes) {
        session.objects.push(check);
        session.files.push(label);
    }
}

/// The bytes of one native Frame: Rows x Columns x Samples per Pixel x Bits
/// Allocated, packed without padding for Bits Allocated 1 (8.7.3.3.1).
pub(super) fn frame_bytes(object: &InMemDicomObject) -> Option<u64> {
    let bits = integer(object, tags::ROWS)?
        .checked_mul(integer(object, tags::COLUMNS)?)?
        .checked_mul(integer(object, tags::SAMPLES_PER_PIXEL)?)?
        .checked_mul(integer(object, tags::BITS_ALLOCATED)?)?;
    Some(bits.div_ceil(8))
}

/// The Required multipart/related; type="application/octet-stream" in
/// Explicit VR Little Endian (Table 10.4.4-1); 406 is permitted only when
/// the origin server holds lossy compressed Pixel Data (8.7.3.4).
fn uncompressed(
    session: &mut Session<'_>,
    target: &Target,
    object: Option<&InMemDicomObject>,
    number: usize,
    native: Option<&str>,
) {
    let mut verdict = Verdict::default();
    let path = target.instance_path().unwrap_or_default();
    let url = session.url(&format!("{path}/frames/{number}"), None);
    let accept = format!(
        "multipart/related; type=\"application/octet-stream\"; transfer-syntax={}",
        uids::EXPLICIT_VR_LITTLE_ENDIAN
    );
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, &accept)) {
        let lossy = native.is_some_and(|syntax| method_for_lossy_transfer_syntax(syntax).is_some());
        if exchange.status == 406 && lossy {
            verdict.note(format!(
                "406: the origin server holds the Pixel Data lossy compressed ({}), for which it may decline Explicit VR Little Endian ({})",
                native.unwrap_or_default(),
                sec("8.7.3.4")
            ));
        } else if expect_status(&mut verdict, &exchange, &[200], "10.4.4 Table 10.4.4-1") {
            uncompressed_part(session, &mut verdict, &exchange, object, number);
        }
    }
    session.push(verdict.finish(
        UNCOMPRESSED_NAME,
        sec(UNCOMPRESSED_SECTIONS),
        "the Frame comes as application/octet-stream of its native size",
    ));
}

fn uncompressed_part(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    object: Option<&InMemDicomObject>,
    number: usize,
) {
    let Some((_, parts)) = expect_multipart(
        verdict,
        exchange,
        Some("application/octet-stream"),
        "10.4.4 Table 10.4.4-1",
    ) else {
        return;
    };
    session.audit.parts(exchange, &parts);
    let line = exchange.request_line();
    let [part] = parts.as_slice() else {
        verdict.fail(format!(
            "{line}: {} parts for one Frame ({})",
            parts.len(),
            sec("10.4.3.3.6")
        ));
        return;
    };
    let Some(found) = part_syntax(verdict, &line, 1, part.header("content-type")) else {
        return;
    };
    if found.essence != "application/octet-stream"
        || found.syntax != uids::EXPLICIT_VR_LITTLE_ENDIAN
    {
        verdict.fail(format!(
            "{line}: the part is {} in {}; the request accepted application/octet-stream in {} only ({})",
            found.essence,
            found.syntax,
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
            sec("8.7.8.2")
        ));
    }
    if let Some(expected) = object.and_then(frame_bytes) {
        if part.body.len() as u64 != expected {
            verdict.fail(format!(
                "{line}: Frame {number} is {} bytes; uncompressed it is {expected} ({})",
                part.body.len(),
                sec("8.7.3.3.1")
            ));
        }
    }
}

/// A Frame Number beyond Number of Frames is a resource that does not exist:
/// 404 (Table 10.4.3-1), or 400 for an invalid request.
fn missing(session: &mut Session<'_>, target: &Target, count: Option<u64>) {
    let Some(count) = count else {
        session.not_run(
            MISSING_NAME,
            MISSING_SECTIONS,
            "the Number of Frames is not known",
        );
        return;
    };
    let mut verdict = Verdict::default();
    let path = target.instance_path().unwrap_or_default();
    let url = session.url(&format!("{path}/frames/{}", count + 1), None);
    let accept = any_frame_accept();
    if let Some(exchange) = session.send(&mut verdict, &Request::get(url, &accept)) {
        expect_status(
            &mut verdict,
            &exchange,
            &[404, 400],
            "10.4.3.1 Table 10.4.3-1",
        );
    }
    session.push(verdict.finish(
        MISSING_NAME,
        sec(MISSING_SECTIONS),
        "a Frame beyond Number of Frames answers 404",
    ));
}
