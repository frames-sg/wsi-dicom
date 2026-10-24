//! Rendered Frames (PS3.18 10.4.1.1.3, Mandatory in Table 10.3-2) and
//! Thumbnails (10.4.1.1.4, Optional), checked from their image headers.

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use super::client::{Exchange, Redirects, Request};
use super::media::MediaType;
use super::multipart;
use super::report::Verdict;
use super::response::{expect_media_type, expect_status, integer, sec};
use super::{DicomWebTransaction, Session, Target};

const RENDERED_NAME: &str = "dicomweb-retrieve-rendered";
const RENDERED_SECTIONS: &str =
    "10.4.1.1.3 Table 10.4.1-3, 10.3 Table 10.3-2, 8.7.4 Table 8.7.4-1, 8.3.5.1.3, 8.6.1.1 and 10.4.3.3.3";
/// The JP2 Signature box (ISO/IEC 15444-1 I.5.1).
const JP2_SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];
const THUMBNAIL_NAME: &str = "dicomweb-retrieve-thumbnail";
const THUMBNAIL_SECTIONS: &str = "10.4.1.1.4 Table 10.4.1-4, 10.4.3.3.4 and 10.4.4 Table 10.4.4-1";
/// The viewport of the scaled rendering.
const VIEWPORT: u32 = 64;

/// Rendered Frame 1: image/jpeg (Default) at its size and in a viewport,
/// and image/png (Required) at its size.
pub(super) fn run(session: &mut Session<'_>, target: &Target, object: Option<&InMemDicomObject>) {
    if let Some(reason) = unavailable(session, target, DicomWebTransaction::Rendered, "rendered") {
        session.not_run(RENDERED_NAME, RENDERED_SECTIONS, &reason);
        return;
    }
    let mut verdict = Verdict::default();
    let path = format!(
        "{}/frames/1/rendered",
        target.instance_path().unwrap_or_default()
    );
    let size = object.and_then(|object| {
        Some((
            u32::try_from(integer(object, tags::COLUMNS)?).ok()?,
            u32::try_from(integer(object, tags::ROWS)?).ok()?,
        ))
    });
    let viewport = format!("viewport={VIEWPORT},{VIEWPORT}");
    for (query, media, expected) in [
        (None, "image/jpeg", size),
        (
            Some(viewport.as_str()),
            "image/jpeg",
            size.map(fit_viewport),
        ),
        (None, "image/png", size),
    ] {
        let url = session.url(&path, query);
        let Some(exchange) = session.send(&mut verdict, &Request::get(url, media)) else {
            continue;
        };
        if !expect_status(
            &mut verdict,
            &exchange,
            &[200],
            "10.4.3.1 and 10.3 Table 10.3-2",
        ) {
            continue;
        }
        session.audit.representation(&exchange);
        session.served("rendered");
        rendered_image(&mut verdict, &exchange, media, expected);
    }
    jph(session, &mut verdict, &path);
    session.push(verdict.finish(
        RENDERED_NAME,
        sec(RENDERED_SECTIONS),
        "Frame 1 renders as baseline JPEG and as PNG at its size, and scales into a viewport",
    ));
}

/// 8.7.4: "When an image/jph Media Type is returned, the image shall be
/// encoded with the JP2 box being present"; image/jph is Optional (Table
/// 8.7.4-1).
fn jph(session: &mut Session<'_>, verdict: &mut Verdict, path: &str) {
    let url = session.url(path, None);
    let Some(exchange) = session.send(verdict, &Request::get(url, "image/jph")) else {
        return;
    };
    let line = exchange.request_line();
    let media = exchange.media_type().and_then(Result::ok);
    if exchange.status != 200
        || media.as_ref().map(|media| media.essence.as_str()) != Some("image/jph")
    {
        let shown = media
            .as_ref()
            .map_or("without Content-Type", MediaType::reported_essence);
        verdict.note(format!(
            "{line} answered {} {}: image/jph is not offered, which is Optional (Table 8.7.4-1)",
            exchange.status, shown
        ));
        return;
    }
    if !exchange.body.starts_with(&JP2_SIGNATURE) {
        verdict.fail(format!(
            "{line}: the image/jph image does not begin with the JP2 Signature box; a rendered image/jph is encoded with the JP2 box present ({})",
            sec("8.7.4")
        ));
    }
}

/// 8.6.1.1: "A message with a single part payload shall have a Content-Type
/// Header Field with a single part media-type". A rendered image is one
/// representation; a multipart Content-Type over a payload that is not
/// multipart breaks that.
fn single_part(verdict: &mut Verdict, exchange: &Exchange) {
    let Some(Ok(media)) = exchange.media_type() else {
        return;
    };
    if !media.essence.starts_with("multipart/") {
        return;
    }
    let framed = media
        .parameter("boundary")
        .is_some_and(|boundary| !multipart::parse(&exchange.body, boundary).parts.is_empty());
    if !framed {
        verdict.fail(format!(
            "{}: the payload is one image, not multipart, yet its Content-Type is {}; a single part payload has a single part media type ({})",
            exchange.request_line(),
            media.reported_essence(),
            sec("8.6.1.1")
        ));
    }
}

fn unavailable(
    session: &Session<'_>,
    target: &Target,
    transaction: DicomWebTransaction,
    flag: &str,
) -> Option<String> {
    if session.skipped(transaction) {
        Some(format!("skipped by request (--skip {flag})"))
    } else if target.instance_path().is_none() {
        Some("no Instance to target".into())
    } else {
        None
    }
}

/// 8.3.5.1.3: scaled "until either the rendered content width or height is
/// the same as the viewport width or height, whichever avoids truncation",
/// keeping the aspect ratio.
fn fit_viewport((width, height): (u32, u32)) -> (u32, u32) {
    let scale = f64::from(VIEWPORT) / f64::from(width.max(height).max(1));
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

/// A rendered image: the media type asked for without a Transfer Syntax
/// parameter (8.7.4), JPEG baseline for image/jpeg, and the expected size.
fn rendered_image(
    verdict: &mut Verdict,
    exchange: &Exchange,
    media: &str,
    expected: Option<(u32, u32)>,
) {
    single_part(verdict, exchange);
    let Some(found) = expect_media_type(verdict, exchange, media, "8.7.4 Table 8.7.4-1") else {
        return;
    };
    let line = exchange.request_line();
    if found.parameter("transfer-syntax").is_some() {
        verdict.fail(format!(
            "{line}: {media} has a transfer-syntax parameter, which a Rendered Media Type shall not have ({})",
            sec("8.7.4")
        ));
    }
    let size = if media == "image/png" {
        png_size(&exchange.body)
    } else {
        jpeg_baseline_size(&exchange.body)
    };
    match (size, expected) {
        (Err(reason), _) => verdict.fail(format!("{line}: {reason} ({})", sec("8.7.4"))),
        (Ok(size), Some(expected)) if !close(size, expected) => verdict.fail(format!(
            "{line}: the image is {}x{}; {}x{} is required ({})",
            size.0,
            size.1,
            expected.0,
            expected.1,
            sec("8.3.5.1.3")
        )),
        _ => {}
    }
}

/// Sizes equal within one pixel of rounding.
fn close(found: (u32, u32), expected: (u32, u32)) -> bool {
    found.0.abs_diff(expected.0) <= 1 && found.1.abs_diff(expected.1) <= 1
}

/// The width and height of a JPEG whose frame is "JPEG baseline lossy 8-bit
/// Huffman encoded non-hierarchical non-sequential" (8.7.4): SOF0 with
/// 8-bit samples.
pub(super) fn jpeg_baseline_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err("the payload is not a JPEG stream: it does not begin with SOI".into());
    }
    let mut position = 2;
    loop {
        let marker_start = bytes[position..]
            .iter()
            .position(|byte| *byte == 0xFF)
            .map(|offset| position + offset)
            .ok_or("the JPEG stream ends before a frame header")?;
        let mut marker_at = marker_start + 1;
        while bytes.get(marker_at) == Some(&0xFF) {
            marker_at += 1;
        }
        let marker = *bytes
            .get(marker_at)
            .ok_or("the JPEG stream ends inside a marker")?;
        let segment = marker_at + 1;
        match marker {
            0xD0..=0xD9 | 0x01 => {
                position = segment;
                continue;
            }
            0xDA => return Err("the JPEG stream reaches SOS before a frame header".into()),
            0xC0 => {
                let header = bytes
                    .get(segment..segment + 7)
                    .ok_or("the SOF0 frame header is truncated")?;
                if header[2] != 8 {
                    return Err(format!(
                        "SOF0 sample precision is {}; baseline is 8-bit",
                        header[2]
                    ));
                }
                let height = u32::from(u16::from_be_bytes([header[3], header[4]]));
                let width = u32::from(u16::from_be_bytes([header[5], header[6]]));
                return Ok((width, height));
            }
            0xC1..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Err(format!(
                    "the frame is SOF{} (FFC{:X}), not the baseline process SOF0",
                    marker - 0xC0,
                    marker - 0xC0
                ));
            }
            _ => {
                let length = bytes
                    .get(segment..segment + 2)
                    .map(|length| usize::from(u16::from_be_bytes([length[0], length[1]])))
                    .ok_or("a JPEG marker segment is truncated")?;
                position = segment + length;
                if position >= bytes.len() {
                    return Err("the JPEG stream ends before a frame header".into());
                }
            }
        }
    }
}

/// The width and height in a PNG IHDR chunk.
fn png_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if !bytes.starts_with(&SIGNATURE) {
        return Err("the payload is not a PNG image: no PNG signature".into());
    }
    let header = bytes
        .get(8..24)
        .filter(|header| &header[4..8] == b"IHDR")
        .ok_or("the PNG image does not begin with an IHDR chunk")?;
    let width = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let height = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    Ok((width, height))
}

/// Thumbnails: Optional, but "If the origin server supports any of the
/// Thumbnail resources, it shall support all of them" (10.4.1.1.4); each is a
/// single image in the Default image/jpeg (Table 10.4.4-1), or a redirect to
/// a rendered resource (10.4.3.3.4).
pub(super) fn thumbnails(session: &mut Session<'_>, target: &Target) {
    if let Some(reason) = unavailable(session, target, DicomWebTransaction::Thumbnail, "thumbnail")
    {
        session.not_run(THUMBNAIL_NAME, THUMBNAIL_SECTIONS, &reason);
        return;
    }
    let instance = target.instance_path().unwrap_or_default();
    let series = target.series_path().unwrap_or_default();
    let study = format!("/studies/{}", target.study.as_deref().unwrap_or_default());
    let resources = [
        format!("{instance}/thumbnail"),
        format!("{instance}/frames/1/thumbnail"),
        format!("{series}/thumbnail"),
        format!("{study}/thumbnail"),
    ];
    let mut verdict = Verdict::default();
    for (index, resource) in resources.iter().enumerate() {
        let request = Request {
            redirects: Redirects::Thumbnail,
            ..Request::get(session.url(resource, None), "image/jpeg")
        };
        let Some(exchange) = session.send(&mut verdict, &request) else {
            break;
        };
        let offered = exchange.status == 200;
        if offered && index == 0 {
            session.served("thumbnail");
        }
        if index == 0 && !offered && matches!(exchange.status, 400 | 404 | 405 | 406 | 501) {
            verdict.unsupported(format!(
                "{} answered {}: the origin server offers no Thumbnail resources, which are optional (Table 10.3-2)",
                exchange.request_line(),
                exchange.status
            ));
            break;
        }
        let reference = if index == 0 {
            "10.4.3.1"
        } else {
            "10.4.1.1.4: an origin server offering one Thumbnail resource offers all"
        };
        if !expect_status(&mut verdict, &exchange, &[200], reference) {
            continue;
        }
        session.audit.representation(&exchange);
        if expect_media_type(
            &mut verdict,
            &exchange,
            "image/jpeg",
            "10.4.4 Table 10.4.4-1",
        )
        .is_some()
        {
            if let Err(reason) = jpeg_baseline_size(&exchange.body) {
                verdict.fail(format!(
                    "{}: {reason} ({})",
                    exchange.request_line(),
                    sec("8.7.4")
                ));
            }
        }
    }
    session.push(verdict.finish(
        THUMBNAIL_NAME,
        sec(THUMBNAIL_SECTIONS),
        "the Instance, Frame, Series and Study Thumbnails are single JPEG images",
    ));
}
