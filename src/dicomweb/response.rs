//! What every response is held to: its status, its Content-Type, its payload
//! header fields (PS3.18 8.4) and, for a representation of a resource, its
//! Content-Location; and the reading of DICOM JSON and multipart payloads.

use std::collections::BTreeMap;

use dicom_core::Tag;
use dicom_object::InMemDicomObject;

use super::client::Exchange;
use super::media::MediaType;
use super::multipart::{self, Part};
use super::report::{DicomWebCheck, Verdict};
use super::EDITION;
use crate::validation::read_json_data_sets;

/// "PS3.18 <edition> <sections>".
pub(crate) fn sec(sections: &str) -> String {
    format!("PS3.18 {EDITION} {sections}")
}

/// The start of a payload as text, for the Status Report of an error.
fn status_report(exchange: &Exchange) -> String {
    if exchange.body.is_empty() {
        return String::new();
    }
    let text: String = String::from_utf8_lossy(&exchange.body[..exchange.body.len().min(160)])
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    format!(" ({:?})", text.trim())
}

/// Whether the status is one of `statuses`; records a violation otherwise.
pub(crate) fn expect_status(
    verdict: &mut Verdict,
    exchange: &Exchange,
    statuses: &[u16],
    reference: &str,
) -> bool {
    if statuses.contains(&exchange.status) {
        return true;
    }
    let expected: Vec<String> = statuses.iter().map(u16::to_string).collect();
    let unfollowed = exchange
        .unfollowed
        .as_deref()
        .map(|reason| format!("; {reason}"))
        .unwrap_or_default();
    verdict.fail(format!(
        "{} answered {}{}{unfollowed}; {} is required ({})",
        exchange.request_line(),
        exchange.status,
        status_report(exchange),
        expected.join(" or "),
        sec(reference)
    ));
    false
}

/// The Content-Type, required to be `essence`.
pub(crate) fn expect_media_type(
    verdict: &mut Verdict,
    exchange: &Exchange,
    essence: &str,
    reference: &str,
) -> Option<MediaType> {
    let line = exchange.request_line();
    match exchange.media_type() {
        None => {
            verdict.fail(format!(
                "{line}: the payload has no Content-Type; {essence} is required ({})",
                sec(&format!("{reference}, 8.4.2 Table 8.4.2-1"))
            ));
            None
        }
        Some(Err(reason)) => {
            verdict.fail(format!("{line}: Content-Type {reason} ({})", sec("8.7")));
            None
        }
        Some(Ok(media)) if media.essence != essence => {
            verdict.fail(format!(
                "{line}: Content-Type is {}; {essence} is required ({})",
                media.essence,
                sec(reference)
            ));
            None
        }
        Some(Ok(media)) => Some(media),
    }
}

/// The Data Sets of an application/dicom+json response, its JSON Model
/// violations recorded.
pub(crate) fn expect_json(
    verdict: &mut Verdict,
    exchange: &Exchange,
    reference: &str,
) -> Option<Vec<InMemDicomObject>> {
    expect_media_type(verdict, exchange, "application/dicom+json", reference)?;
    match read_json_data_sets(&exchange.body, &BTreeMap::new()) {
        Ok((objects, violations)) => {
            if let Some(violations) = violations {
                verdict.fail(format!("{}: {violations}", exchange.request_line()));
            }
            Some(objects)
        }
        Err(reason) => {
            verdict.fail(format!("{}: {reason}", exchange.request_line()));
            None
        }
    }
}

/// The parts of a multipart/related response (PS3.18 8.6.1.2, 8.7.1): the
/// required "type" and "boundary" parameters, `root` the media type the
/// "type" parameter must name if given, the framing, and each part's
/// Content-Type and Content-Length.
pub(crate) fn expect_multipart<'e>(
    verdict: &mut Verdict,
    exchange: &'e Exchange,
    root: Option<&str>,
    reference: &str,
) -> Option<(MediaType, Vec<Part<'e>>)> {
    let media = expect_media_type(verdict, exchange, "multipart/related", reference)?;
    let line = exchange.request_line();
    match media.parameter("type").map(MediaType::parse) {
        None => verdict.fail(format!(
            "{line}: multipart/related has no \"type\" parameter, which is required ({})",
            sec("8.7.1 and 8.7.3.5.1")
        )),
        Some(Err(reason)) => verdict.fail(format!(
            "{line}: the \"type\" parameter {reason} ({})",
            sec("8.7.1")
        )),
        Some(Ok(kind)) => {
            if let Some(root) = root.filter(|root| *root != kind.essence) {
                verdict.fail(format!(
                    "{line}: the \"type\" parameter is {}; {root} is required ({})",
                    kind.essence,
                    sec(reference)
                ));
            }
        }
    }
    let Some(boundary) = media.parameter("boundary") else {
        verdict.fail(format!(
            "{line}: multipart/related has no \"boundary\" parameter ({})",
            sec("8.7.3.5.1")
        ));
        return None;
    };
    let read = multipart::parse(&exchange.body, boundary);
    for violation in &read.violations {
        verdict.fail(format!("{line}: {violation}"));
    }
    let chunked = exchange.header("transfer-encoding").is_some();
    for (index, part) in read.parts.iter().enumerate() {
        let number = index + 1;
        match part.header("content-type").map(MediaType::parse) {
            None => verdict.fail(format!(
                "{line}: part {number} has no Content-Type ({})",
                sec("8.6.1.2 Table 8.6.1-1")
            )),
            Some(Err(reason)) => verdict.fail(format!(
                "{line}: part {number} Content-Type {reason} ({})",
                sec("8.6.1.2")
            )),
            Some(Ok(_)) => {}
        }
        if !chunked && part.header("content-length").is_none() {
            verdict.fail(format!(
                "{line}: part {number} has no Content-Length, which a part has unless the response payload has a transfer encoding ({})",
                sec("8.6.1.2 Table 8.6.1-1")
            ));
        }
    }
    Some((media, read.parts))
}

/// The trimmed text of a Data Set's Attribute.
pub(crate) fn text(object: &InMemDicomObject, tag: Tag) -> Option<String> {
    object
        .get(tag)
        .and_then(|element| element.to_str().ok())
        .map(|value| value.trim_end_matches(['\0', ' ']).trim().to_string())
}

/// An Attribute's first Value as an unsigned integer.
pub(crate) fn integer(object: &InMemDicomObject, tag: Tag) -> Option<u64> {
    object
        .get(tag)
        .and_then(|element| element.to_int::<u64>().ok())
}

/// "(gggg,eeee)".
pub(crate) fn tag_text(tag: Tag) -> String {
    format!("({:04X},{:04X})", tag.group(), tag.element())
}

/// The payload header fields of every response and the Content-Location of
/// each representation, gathered across transactions.
#[derive(Debug, Default)]
pub(crate) struct Audit {
    payload: Verdict,
    payloads: usize,
    location: Verdict,
    representations: usize,
}

impl Audit {
    /// PS3.18 8.4.2, 8.4.3 and 8.6: a payload has a Content-Type and exactly
    /// one of Content-Length and Transfer-Encoding.
    pub(crate) fn payload(&mut self, exchange: &Exchange) {
        if exchange.body.is_empty() {
            return;
        }
        self.payloads += 1;
        let line = exchange.request_line();
        if exchange.header("content-type").is_none() {
            self.payload.fail(format!(
                "{line}: the payload has no Content-Type ({})",
                sec("8.4.2 Table 8.4.2-1")
            ));
        }
        match (exchange.header("content-length"), exchange.header("transfer-encoding")) {
            (None, None) => self.payload.fail(format!(
                "{line}: the payload has neither Content-Length nor Transfer-Encoding ({})",
                sec("8.4.3 Table 8.4.3-1 and 8.6")
            )),
            (Some(_), Some(_)) => self.payload.fail(format!(
                "{line}: Content-Length is present with Transfer-Encoding, which it shall not be ({})",
                sec("8.4.3 Table 8.4.3-1")
            )),
            (Some(length), None)
                if length.trim().parse::<usize>().ok() != Some(exchange.body.len()) =>
            {
                self.payload.fail(format!(
                    "{line}: Content-Length {length} but the payload holds {} bytes ({})",
                    exchange.body.len(),
                    sec("8.4.3 Table 8.4.3-1")
                ))
            }
            _ => {}
        }
    }

    /// Table 8.4.2-1: a single part payload that represents the Target
    /// Resource has a Content-Location.
    pub(crate) fn representation(&mut self, exchange: &Exchange) {
        self.representations += 1;
        if exchange.header("content-location").is_none() {
            self.location.fail(format!(
                "{}: no Content-Location, though the payload is a representation of the Target Resource ({})",
                exchange.request_line(),
                sec("8.4.2 Table 8.4.2-1")
            ));
        }
    }

    /// Table 8.6.1-1: each part that represents a resource has a
    /// Content-Location.
    pub(crate) fn parts(&mut self, exchange: &Exchange, parts: &[Part<'_>]) {
        self.representations += 1;
        let missing: Vec<String> = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.header("content-location").is_none())
            .map(|(index, _)| (index + 1).to_string())
            .collect();
        if !missing.is_empty() {
            self.location.fail(format!(
                "{}: part {} {} no Content-Location, which a part representing a resource has ({})",
                exchange.request_line(),
                missing.join(", "),
                if missing.len() == 1 { "has" } else { "have" },
                sec("8.6.1.2 Table 8.6.1-1")
            ));
        }
    }

    /// The two gathered checks.
    pub(crate) fn checks(self) -> [DicomWebCheck; 2] {
        let mut payload = self.payload;
        if self.payloads == 0 {
            payload.not_run("no response carried a payload");
        }
        let mut location = self.location;
        if self.representations == 0 {
            location.not_run("no Retrieve response carried a representation");
        }
        [
            payload.finish(
                "dicomweb-payload-headers",
                sec("8.4.2 Table 8.4.2-1, 8.4.3 Table 8.4.3-1 and 8.6"),
                &format!(
                    "each of {} payloads has a Content-Type and one of Content-Length and Transfer-Encoding",
                    self.payloads
                ),
            ),
            location.finish(
                "dicomweb-content-location",
                sec("8.4.2 Table 8.4.2-1 and 8.6.1.2 Table 8.6.1-1"),
                &format!(
                    "each of {} Retrieve representations has a Content-Location",
                    self.representations
                ),
            ),
        ]
    }
}
