//! The Rendering Query Parameters of Rendered Frames (PS3.18 8.3.5.1): the
//! annotation, quality, viewport and window parameters the origin server
//! supports (Table 10.4.1-5), their ill-formed values answered 400, and the
//! optional iccprofile parameter held to the ICC Profiles it embeds.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Request};
use super::rendered::jpeg_baseline_size;
use super::report::Verdict;
use super::response::{expect_media_type, expect_status, integer, sec};
use super::{DicomWebTransaction, Session, Target};
use crate::icc::defines_color_space;

const NAME: &str = "dicomweb-rendered-parameters";
const SECTIONS: &str = "8.3.5, 8.3.5.1.1 to 8.3.5.1.5 and 10.4.1.2 Table 10.4.1-5";
const JPEG: &str = "image/jpeg";
/// The viewport of the cropped renderings.
const VIEWPORT: u32 = 64;

pub(super) fn run(session: &mut Session<'_>, target: &Target, object: Option<&InMemDicomObject>) {
    let reason = if session.skipped(DicomWebTransaction::Rendered) {
        Some("skipped by request (--skip rendered)")
    } else if target.instance_path().is_none() {
        Some("no Instance to target")
    } else {
        None
    };
    if let Some(reason) = reason {
        session.not_run(NAME, SECTIONS, reason);
        return;
    }
    let mut verdict = Verdict::default();
    let path = format!(
        "{}/frames/1/rendered",
        target.instance_path().unwrap_or_default()
    );
    supported(session, &mut verdict, &path);
    annotations(session, &mut verdict, &path);
    match object.and_then(frame_size) {
        Some(size) => viewports(session, &mut verdict, &path, size),
        None => verdict.note("the Frame's Rows and Columns are not known, so the cropping viewports were not requested"),
    }
    icc_profiles(session, &mut verdict, &path, object.and_then(source_icc));
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "annotation, quality, viewport and window are supported, their ill-formed values answer 400, and iccprofile embeds the profiles it names",
    ));
}

/// Columns and Rows of a Frame.
fn frame_size(object: &InMemDicomObject) -> Option<(u32, u32)> {
    Some((
        u32::try_from(integer(object, tags::COLUMNS)?).ok()?,
        u32::try_from(integer(object, tags::ROWS)?).ok()?,
    ))
}

/// One rendered JPEG request with `query`.
fn rendered(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    path: &str,
    query: &str,
) -> Option<Exchange> {
    let url = session.url(path, Some(query));
    session.send(verdict, &Request::get(url, JPEG))
}

/// 200 with an image/jpeg payload.
fn rendered_ok(verdict: &mut Verdict, exchange: &Exchange, reference: &str) -> bool {
    expect_status(verdict, exchange, &[200], reference)
        && expect_media_type(verdict, exchange, JPEG, reference).is_some()
}

/// Table 10.4.1-5: annotation, quality, viewport and window are Mandatory
/// for Rendered Resources, so a valid value renders; 8.3.5.1.2 and
/// 8.3.5.1.4: a quality outside 1 to 100 or a window without its three
/// values answers 400.
fn supported(session: &mut Session<'_>, verdict: &mut Verdict, path: &str) {
    for (query, status, reference) in [
        ("annotation=patient", 200, "10.4.1.2 Table 10.4.1-5 and 8.3.5: annotation is supported"),
        ("quality=90", 200, "10.4.1.2 Table 10.4.1-5 and 8.3.5: quality is supported"),
        ("window=128,256,linear", 200, "10.4.1.2 Table 10.4.1-5 and 8.3.5: window is supported"),
        ("quality=101", 400, "8.3.5.1.2: a quality that is not an integer between 1 and 100 answers 400 (Bad Request)"),
        ("window=128", 400, "8.3.5.1.4: a window without its three values answers 400 (Bad Request)"),
        ("annotation=", 400, "8.3.5.1.1: an annotation without values answers 400 (Bad Request)"),
    ] {
        let Some(exchange) = rendered(session, verdict, path, query) else {
            continue;
        };
        if status == 200 {
            rendered_ok(verdict, &exchange, reference);
        } else {
            expect_status(verdict, &exchange, &[status], reference);
        }
    }
}

/// 8.3.5.1.1: "The origin server shall ignore any unsupported parameter
/// values. If unsupported values are present, the origin server shall
/// include the following Header Field: Warning 299 <service>: The following
/// annotation values are not supported: <values>".
fn annotations(session: &mut Session<'_>, verdict: &mut Verdict, path: &str) {
    let Some(exchange) = rendered(session, verdict, path, "annotation=patient,wsidicomprobe")
    else {
        return;
    };
    if !rendered_ok(
        verdict,
        &exchange,
        "8.3.5.1.1: unsupported annotation values are ignored",
    ) {
        return;
    }
    let warned = exchange.headers.iter().any(|(name, value)| {
        name == "warning"
            && value.trim_start().starts_with("299")
            && value.contains("annotation values are not supported")
            && value.contains("wsidicomprobe")
    });
    if !warned {
        verdict.fail(format!(
            "{}: the unsupported annotation value wsidicomprobe brought no \"Warning: 299 <service>: The following annotation values are not supported: wsidicomprobe\" ({})",
            exchange.request_line(),
            sec("8.3.5.1.1")
        ));
    }
}

/// 8.3.5.1.3: the source region sx, sy, sw, sh is cropped, and then scaled
/// into the viewport keeping its aspect ratio; an elided value takes its
/// default; ill-formed dimensions answer 400.
fn viewports(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    path: &str,
    (columns, rows): (u32, u32),
) {
    let half = (rows / 2).max(1);
    let cropped = fit((columns, half));
    for (query, reference) in [
        (
            format!("viewport={VIEWPORT},{VIEWPORT},0,0,{columns},{half}"),
            "8.3.5.1.3: the source is cropped to sx, sy, sw, sh and scaled into the viewport",
        ),
        (
            format!("viewport={VIEWPORT},{VIEWPORT},0,0,,{half}"),
            "8.3.5.1.3: an elided sw defaults to the right edge of the source",
        ),
    ] {
        let Some(exchange) = rendered(session, verdict, path, &query) else {
            continue;
        };
        if !rendered_ok(verdict, &exchange, reference) {
            continue;
        }
        match jpeg_baseline_size(&exchange.body) {
            Err(reason) => verdict.fail(format!(
                "{}: {reason} ({})",
                exchange.request_line(),
                sec("8.7.4")
            )),
            Ok(size) if size.0.abs_diff(cropped.0) > 1 || size.1.abs_diff(cropped.1) > 1 => {
                verdict.fail(format!(
                    "{}: the image is {}x{}; the {columns}x{half} region scaled into {VIEWPORT}x{VIEWPORT} is {}x{} ({})",
                    exchange.request_line(),
                    size.0,
                    size.1,
                    cropped.0,
                    cropped.1,
                    sec(reference)
                ))
            }
            Ok(_) => {}
        }
    }
    if let Some(exchange) = rendered(session, verdict, path, &format!("viewport={VIEWPORT}")) {
        expect_status(
            verdict,
            &exchange,
            &[400],
            "8.3.5.1.3: a viewport without vh is ill-formed and answers 400 (Bad Request)",
        );
    }
}

/// A region scaled into the viewport, "whichever avoids truncation".
fn fit((width, height): (u32, u32)) -> (u32, u32) {
    let scale = f64::from(VIEWPORT) / f64::from(width.max(height).max(1));
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

/// The ICC Profile (0028,2000) of Frame 1: at the top level or in the first
/// Optical Path Sequence Item.
fn source_icc(object: &InMemDicomObject) -> Option<Vec<u8>> {
    let element = object.get(tags::ICC_PROFILE).or_else(|| {
        object
            .get(tags::OPTICAL_PATH_SEQUENCE)?
            .items()?
            .first()?
            .get(tags::ICC_PROFILE)
    })?;
    element.to_bytes().ok().map(|bytes| bytes.to_vec())
}

/// The iccprofile values and the color space each names (8.3.5.1.5), by
/// the Color Space (0028,2002) Defined Term of the same primaries.
const SPACES: [(&str, &str); 4] = [
    ("srgb", "SRGB"),
    ("adobergb", "ADOBERGB"),
    ("rommrgb", "ROMMRGB"),
    ("displayp3", "DISPLAYP3"),
];

/// 8.3.5.1.5, an Optional parameter (Table 10.4.1-5): image/jpeg embeds
/// ICC Profiles in APP2 segments, so "no" embeds none, "yes" the source's
/// ICC Profile (0028,2000), and each named space a profile of that space.
/// An origin server that does not support the parameter ignores it (8.3),
/// which shows as the same embedding for every value.
fn icc_profiles(
    session: &mut Session<'_>,
    verdict: &mut Verdict,
    path: &str,
    source: Option<Vec<u8>>,
) {
    let mut embedded: Vec<(&str, Exchange, Option<Vec<u8>>)> = Vec::new();
    for value in ["no", "yes"]
        .into_iter()
        .chain(SPACES.map(|(value, _)| value))
    {
        let Some(exchange) = rendered(session, verdict, path, &format!("iccprofile={value}"))
        else {
            continue;
        };
        if !rendered_ok(
            verdict,
            &exchange,
            "8.3.5.1.5: image/jpeg embeds ICC Profiles, so an iccprofile value renders, or is ignored when unsupported (8.3)",
        ) {
            continue;
        }
        match embedded_icc(&exchange.body) {
            Ok(profile) => embedded.push((value, exchange, profile)),
            Err(reason) => verdict.fail(format!(
                "{}: {reason} (ISO 15076-1 Annex B, {})",
                exchange.request_line(),
                sec("8.3.5.1.5")
            )),
        }
    }
    let differ = embedded.windows(2).any(|pair| pair[0].2 != pair[1].2);
    if !differ {
        verdict.note("every iccprofile value brought the same embedding, so the Optional iccprofile parameter is not supported (Table 10.4.1-5) and was ignored");
        return;
    }
    for (value, exchange, profile) in &embedded {
        let line = exchange.request_line();
        match (*value, profile) {
            ("no", Some(_)) => verdict.fail(format!(
                "{line}: the image embeds an ICC Profile; \"no\" indicates that no ICC profile shall be present ({})",
                sec("8.3.5.1.5")
            )),
            ("no", None) => {}
            (_, None) => verdict.fail(format!(
                "{line}: the image embeds no ICC Profile, though image/jpeg embeds them and the parameter is supported ({})",
                sec("8.3.5.1.5")
            )),
            ("yes", Some(profile)) => {
                if let Some(source) = source.as_deref() {
                    let unpadded = source.strip_suffix(&[0]).unwrap_or(source);
                    if profile.as_slice() != source && profile.as_slice() != unpadded {
                        verdict.fail(format!(
                            "{line}: the embedded {}-byte ICC Profile is not the {}-byte ICC Profile (0028,2000) of the Frame, which \"yes\" embeds when no color space is named ({})",
                            profile.len(),
                            unpadded.len(),
                            sec("8.3.5.1.5")
                        ));
                    }
                }
            }
            (value, Some(profile)) => named_space(verdict, &line, value, profile),
        }
    }
    verdict.note("whether the pixels were transformed into a named color space is a judgment of the pixels and is not checked");
}

fn named_space(verdict: &mut Verdict, line: &str, value: &str, profile: &[u8]) {
    let Some((_, term)) = SPACES.iter().find(|(name, _)| *name == value) else {
        return;
    };
    match defines_color_space(profile, term) {
        Some(true) => {}
        Some(false) => verdict.fail(format!(
            "{line}: the embedded ICC Profile does not have the primaries of {term}, the color space \"{value}\" names ({})",
            sec("8.3.5.1.5")
        )),
        None => verdict.note(format!(
            "{line}: the embedded ICC Profile is table based or unreadable, so its color space is not decided"
        )),
    }
}

/// The ICC Profile embedded in a JPEG stream's APP2 "ICC_PROFILE" segments,
/// before the first SOS (ISO 15076-1 Annex B).
fn embedded_icc(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut chunks: Vec<(u8, u8, &[u8])> = Vec::new();
    for (marker, payload) in jpeg_segments(bytes)? {
        if marker != 0xE2 {
            continue;
        }
        let Some(rest) = payload.strip_prefix(b"ICC_PROFILE\0") else {
            continue;
        };
        let [sequence, count, data @ ..] = rest else {
            return Err("an APP2 ICC_PROFILE segment is truncated".into());
        };
        chunks.push((*sequence, *count, data));
    }
    if chunks.is_empty() {
        return Ok(None);
    }
    chunks.sort_by_key(|chunk| chunk.0);
    let count = chunks[0].1;
    let complete = usize::from(count) == chunks.len()
        && chunks
            .iter()
            .zip(1..=count)
            .all(|(chunk, sequence)| chunk.0 == sequence && chunk.1 == count);
    if !complete {
        return Err(format!(
            "its {} APP2 ICC_PROFILE segments are not one complete sequence of {count}",
            chunks.len()
        ));
    }
    Ok(Some(
        chunks
            .iter()
            .flat_map(|chunk| chunk.2.iter().copied())
            .collect(),
    ))
}

/// The marker segments of a JPEG stream before its first SOS: each marker
/// and the payload after its length field.
fn jpeg_segments(bytes: &[u8]) -> Result<Vec<(u8, &[u8])>, String> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err("the payload is not a JPEG stream: it does not begin with SOI".into());
    }
    let mut segments = Vec::new();
    let mut position = 2;
    loop {
        let Some(&[0xFF, marker]) = bytes.get(position..position + 2) else {
            return Err("the JPEG stream ends or holds no marker before SOS".into());
        };
        position += 2;
        match marker {
            0xFF => position -= 1,
            0xD0..=0xD7 | 0x01 => {}
            0xDA | 0xD9 => return Ok(segments),
            _ => {
                let length = bytes
                    .get(position..position + 2)
                    .map(|length| usize::from(u16::from_be_bytes([length[0], length[1]])))
                    .filter(|length| *length >= 2)
                    .ok_or("a JPEG marker segment is truncated")?;
                let payload = bytes
                    .get(position + 2..position + length)
                    .ok_or("a JPEG marker segment is truncated")?;
                segments.push((marker, payload));
                position += length;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{embedded_icc, fit};

    #[test]
    fn icc_chunks_concatenate_in_sequence_order() {
        let segment = |sequence: u8, data: &[u8]| {
            let mut payload = b"ICC_PROFILE\0".to_vec();
            payload.extend_from_slice(&[sequence, 2]);
            payload.extend_from_slice(data);
            let mut bytes = vec![0xFF, 0xE2];
            bytes.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
            bytes.extend_from_slice(&payload);
            bytes
        };
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(2, b"cd"));
        jpeg.extend(segment(1, b"ab"));
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
        assert_eq!(embedded_icc(&jpeg).unwrap(), Some(b"abcd".to_vec()));
        assert_eq!(embedded_icc(&[0xFF, 0xD8, 0xFF, 0xDA]).unwrap(), None);
        assert!(embedded_icc(&jpeg[..jpeg.len() - 8]).is_err());
    }

    #[test]
    fn regions_scale_into_the_viewport_without_truncation() {
        assert_eq!(fit((256, 128)), (64, 32));
        assert_eq!(fit((2, 1)), (64, 32));
        assert_eq!(fit((100, 400)), (16, 64));
    }
}
