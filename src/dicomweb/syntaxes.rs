//! Transfer Syntax selection (PS3.18 8.7.3 to 8.7.3.4 and 8.7.8.2): the
//! Default Transfer Syntax of a Media Type requested without one, the Media
//! Type selected before the Transfer Syntax, and Implicit VR Little Endian
//! and Explicit VR Big Endian never used. The forbidden ones are gathered
//! over the whole run, so the check is finished last.

use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request};
use super::frames::frame_bytes;
use super::media::{bulk_media_type, media_type_of, MediaType};
use super::multipart::Part;
use super::report::Verdict;
use super::response::{expect_multipart, expect_status, integer, sec};
use super::{DicomWebTransaction, Session, Target};
use crate::lossy::method_for_lossy_transfer_syntax;

const NAME: &str = "dicomweb-transfer-syntaxes";
const SECTIONS: &str = "8.6.2.1, 8.7.3, 8.7.3.1 Table 8.7.3-2, 8.7.3.3, 8.7.3.4 and 8.7.8.2";
const OCTET: &str = "application/octet-stream";
/// An Instance's uncompressed Pixel Data retrieved again at most.
const MAX_REENCODED_BYTES: u64 = 32 * 1024 * 1024;
/// MPEG2 Main Profile / Main Level, which no slide is offered in.
const UNOFFERED_SYNTAX: &str = "1.2.840.10008.1.2.4.100";

/// What the run knows of the target's encoding.
struct Native {
    syntax: Option<String>,
    lossy: bool,
}

pub(super) fn run(session: &mut Session<'_>, target: &Target, object: Option<&InMemDicomObject>) {
    let mut verdict = Verdict::default();
    let syntax = target
        .instance
        .as_ref()
        .and_then(|uid| target.syntaxes.get(uid))
        .cloned();
    let native = Native {
        lossy: syntax
            .as_deref()
            .is_some_and(|syntax| method_for_lossy_transfer_syntax(syntax).is_some()),
        syntax,
    };
    match target.instance_path() {
        None => verdict.not_run("no Instance to target"),
        Some(path) => {
            if session.skipped(DicomWebTransaction::Frames) {
                verdict.note("Frames skipped by request (--skip frames)");
            } else {
                frames(session, &mut verdict, &path, &native);
            }
            if session.skipped(DicomWebTransaction::Instance) {
                verdict.note("Instance retrieval skipped by request (--skip instance)");
            } else {
                instances(session, &mut verdict, &path, object, &native, target.frames);
            }
        }
    }
    session.syntaxes = Some(verdict);
}

/// The forbidden Transfer Syntaxes of the run, and the check.
pub(super) fn finish(session: &mut Session<'_>, forbidden: Vec<String>) {
    let mut verdict = session.syntaxes.take().unwrap_or_default();
    for violation in forbidden {
        verdict.fail(violation);
    }
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "a Media Type without a Transfer Syntax comes in its Default, the Media Type is selected before the Transfer Syntax, and Implicit VR Little Endian and Explicit VR Big Endian are never used",
    ));
}

/// Each part's media type and the Transfer Syntax it names or implies (the
/// first of its Table 8.7.3-4 or 8.7.3-5 row).
fn part_syntaxes(parts: &[Part<'_>]) -> Vec<(String, Option<String>)> {
    parts
        .iter()
        .filter_map(|part| MediaType::parse(part.header("content-type")?).ok())
        .map(|media| {
            let syntax = media
                .parameter("transfer-syntax")
                .map(str::to_string)
                .or_else(|| bulk_media_type(&media.essence).map(|row| row.syntaxes[0].to_string()));
            (media.essence, syntax)
        })
        .collect()
}

fn frames(session: &mut Session<'_>, verdict: &mut Verdict, path: &str, native: &Native) {
    let url = |session: &Session<'_>| session.url(&format!("{path}/frames/1"), None);
    // Table 8.7.3-4: application/octet-stream defaults to Explicit VR Little
    // Endian (8.7.3.3).
    let accept = format!("multipart/related; type=\"{OCTET}\"");
    if let Some(exchange) = session.send(verdict, &Request::get(url(session), &accept)) {
        defaulted(
            session,
            verdict,
            &exchange,
            OCTET,
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
            native,
            "8.7.3.3: without a Transfer Syntax parameter, Explicit VR Little Endian",
        );
    }
    let Some(syntax) = native.syntax.as_deref() else {
        verdict.note("the Transfer Syntax the Frames are held in is not known, so the compressed defaults were not requested");
        return;
    };
    let Some(media) = media_type_of(syntax).filter(|media| *media != OCTET) else {
        return;
    };
    // Table 8.7.3-5: the Media Type of the held Frames without a Transfer
    // Syntax parameter defaults to the first syntax of its row (8.7.3).
    let accept = format!("multipart/related; type=\"{media}\"");
    if let Some(exchange) = session.send(verdict, &Request::get(url(session), &accept)) {
        let default = bulk_media_type(media).map_or(syntax, |row| row.syntaxes[0]);
        if exchange.status == 406 && default != syntax {
            verdict.note(format!(
                "{} answered 406: the Frames are held in {syntax}, and {media}'s Default {default} would need transcoding",
                exchange.request_line()
            ));
        } else {
            defaulted(
                session,
                verdict,
                &exchange,
                media,
                default,
                native,
                "8.7.3 Table 8.7.3-5: without a Transfer Syntax parameter, the Default of the Media Type",
            );
        }
    }
    // 8.7.8.2: "The origin server shall first determine the Selected Media
    // Type ... and then determine the Selected Transfer Syntax", so the
    // preferred uncompressed Media Type wins over the held encoding.
    let accept = format!(
        "multipart/related; type=\"{OCTET}\"; transfer-syntax={}, multipart/related; type=\"{media}\"; transfer-syntax={syntax}; q=0.5",
        uids::EXPLICIT_VR_LITTLE_ENDIAN
    );
    if let Some(exchange) = session.send(verdict, &Request::get(url(session), &accept)) {
        selected_media_type_first(session, verdict, &exchange, media, native);
    }
}

/// The parts are `media` in `default`, unless the origin server holds the
/// Pixel Data lossy compressed (8.7.3.4), when it may keep that syntax or
/// decline with 406.
fn defaulted(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    media: &str,
    default: &str,
    native: &Native,
    reference: &str,
) {
    let line = exchange.request_line();
    if exchange.status == 406 && native.lossy {
        verdict.note(format!(
            "{line} answered 406: the Pixel Data is held lossy compressed (8.7.3.4)"
        ));
        return;
    }
    if !expect_status(verdict, exchange, &[200], reference) {
        return;
    }
    let Some((_, parts)) = expect_multipart(verdict, exchange, None, reference) else {
        return;
    };
    session.audit.parts(exchange, &parts);
    for (index, (essence, syntax)) in part_syntaxes(&parts).into_iter().enumerate() {
        let kept = native.lossy && syntax.as_deref() == native.syntax.as_deref();
        if !kept && (essence != media || syntax.as_deref() != Some(default)) {
            verdict.fail(format!(
                "{line}: part {} is {essence} in {}; {media} requested without a Transfer Syntax parameter is in {default} ({})",
                index + 1,
                syntax.as_deref().unwrap_or("no Transfer Syntax"),
                sec(reference)
            ));
        }
    }
}

fn selected_media_type_first(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    exchange: &Exchange,
    media: &str,
    native: &Native,
) {
    let line = exchange.request_line();
    if !expect_status(verdict, exchange, &[200], "8.7.8.2") {
        return;
    }
    let Some((_, parts)) = expect_multipart(verdict, exchange, None, "8.7.8.2") else {
        return;
    };
    session.audit.parts(exchange, &parts);
    for (index, (essence, syntax)) in part_syntaxes(&parts).into_iter().enumerate() {
        if essence == OCTET && syntax.as_deref() == Some(uids::EXPLICIT_VR_LITTLE_ENDIAN) {
            continue;
        }
        if native.lossy && essence == media {
            verdict.note(format!(
                "{line}: part {} kept the lossy compressed {media} (8.7.3.4)",
                index + 1
            ));
            continue;
        }
        verdict.fail(format!(
            "{line}: part {} is {essence} in {}, though application/octet-stream in Explicit VR Little Endian has qvalue 1 and {media} 0.5; the Selected Media Type is determined first, then its Transfer Syntax ({})",
            index + 1,
            syntax.as_deref().unwrap_or("no Transfer Syntax"),
            sec("8.7.8.2 and 8.7.8.1")
        ));
    }
}

/// The Instance in application/dicom without a Transfer Syntax parameter
/// (8.7.3.4: the Default, Explicit VR Little Endian), and with one the
/// origin server does not offer (8.7.3.1: the Default Media Type), when its
/// uncompressed Pixel Data is small enough to retrieve again.
fn instances(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    path: &str,
    object: Option<&InMemDicomObject>,
    native: &Native,
    frames: Option<u64>,
) {
    let size = object.and_then(frame_bytes).and_then(|bytes| {
        let count = object
            .and_then(|object| integer(object, tags::NUMBER_OF_FRAMES))
            .or(frames)
            .unwrap_or(1);
        bytes.checked_mul(count)
    });
    let limit = MAX_REENCODED_BYTES.min(session.options.http.max_response_bytes / 2);
    match size {
        Some(size) if size <= limit => {}
        Some(size) => {
            verdict.note(format!(
                "the Instance's Pixel Data is {size} bytes uncompressed, more than the {limit} retrieved again, so the Instance defaults were not requested"
            ));
            return;
        }
        None => {
            verdict.note("the Instance's Pixel Data size is not known, so the Instance defaults were not requested");
            return;
        }
    }
    for (accept, reference, tolerates_406) in [
        (
            "multipart/related; type=\"application/dicom\"".to_string(),
            "8.7.3.4: \"If the Transfer Syntax is not specified in a message, then the Default Transfer Syntax shall be used\", Explicit VR Little Endian (Table 8.7.3-2)",
            false,
        ),
        (
            format!("multipart/related; type=\"application/dicom\"; transfer-syntax={UNOFFERED_SYNTAX}"),
            "8.7.3.1: with none of the Acceptable Media Types supported, the Default, application/dicom in Explicit VR Little Endian (or 406, 8.7.8.1)",
            true,
        ),
    ] {
        let url = session.url(path, None);
        let Some(exchange) = session.send(verdict, &Request::get(url, &accept)) else {
            continue;
        };
        if exchange.status == 406 && (tolerates_406 || native.lossy) {
            verdict.note(format!(
                "{} answered 406{}",
                exchange.request_line(),
                if native.lossy { ": the Pixel Data is held lossy compressed (8.7.3.4)" } else { ", as 8.7.8.1 step 5 prescribes" }
            ));
            continue;
        }
        if !expect_status(verdict, &exchange, &[200], reference) {
            continue;
        }
        let Some((_, parts)) = expect_multipart(verdict, &exchange, Some("application/dicom"), reference)
        else {
            continue;
        };
        session.audit.parts(&exchange, &parts);
        for (index, part) in parts.iter().enumerate() {
            let Some(syntax) = file_meta_syntax(part.body) else {
                verdict.fail(format!(
                    "{}: part {} is not a PS3.10 file with File Meta Information ({})",
                    exchange.request_line(),
                    index + 1,
                    sec("8.7.3.1")
                ));
                continue;
            };
            session.audit.forbidden_syntax(
                &format!(
                    "{}: part {}'s File Meta Information",
                    exchange.request_line(),
                    index + 1
                ),
                &syntax,
            );
            let kept = native.lossy && Some(syntax.as_str()) == native.syntax.as_deref();
            if !kept && syntax != uids::EXPLICIT_VR_LITTLE_ENDIAN {
                verdict.fail(format!(
                    "{}: the Instance is in {syntax}; {} ({})",
                    exchange.request_line(),
                    "the Default Transfer Syntax is Explicit VR Little Endian, unless the Pixel Data is held only lossy compressed or is too long for it",
                    sec(reference)
                ));
            }
        }
    }
}

/// The Transfer Syntax UID (0002,0010) of a PS3.10 file.
pub(super) fn file_meta_syntax(bytes: &[u8]) -> Option<String> {
    if bytes.get(128..132) != Some(b"DICM".as_slice()) {
        return None;
    }
    let object = dicom_object::OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .from_reader(bytes)
        .ok()?;
    Some(
        object
            .meta()
            .transfer_syntax
            .trim_end_matches('\0')
            .to_string(),
    )
}
