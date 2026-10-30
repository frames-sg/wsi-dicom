//! What every response of a run is held to, gathered across transactions:
//! a response message with a valid status code (PS3.18 8.1.2), its payload
//! header fields (8.4, 8.6), the Content-Location of each representation,
//! the challenge and Allow fields of 401 and 405 responses (8.5), and the
//! Transfer Syntaxes Web Services never use (8.7.3, 8.6.2.1).

use super::client::Exchange;
use super::media::MediaType;
use super::multipart::Part;
use super::report::{DicomWebCheck, Verdict};
use super::response::sec;
use super::transport::{Failure, Fault};

/// Implicit VR Little Endian and Explicit VR Big Endian, which "shall not be
/// used with Web Services" (8.7.3, 8.7.3.4, 8.6.2.1).
const FORBIDDEN_SYNTAXES: [(&str, &str); 2] = [
    ("1.2.840.10008.1.2", "Implicit VR Little Endian"),
    ("1.2.840.10008.1.2.2", "Explicit VR Big Endian"),
];

/// What a request targeted, for the sections its failure payload cites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resource {
    /// A Search Transaction resource (Table 10.6.1-1).
    Search,
    /// An individual Instance resource, for which single part
    /// application/dicom is permitted (10.4.4).
    Instance,
    /// Any other Retrieve resource.
    Retrieve,
    /// Retrieve Capabilities, Store or a probe.
    Other,
}

/// The checks gathered over every response of a run.
#[derive(Debug, Default)]
pub(crate) struct Audit {
    payload: Verdict,
    payloads: usize,
    location: Verdict,
    representations: usize,
    messages: Verdict,
    responses: usize,
    timeouts: usize,
    errors: Verdict,
    unauthorized: usize,
    not_allowed: usize,
    /// Uses of a forbidden Transfer Syntax, for dicomweb-transfer-syntaxes.
    forbidden: Vec<String>,
    /// Multipart responses whose "type" parameter is quoted, and the first.
    quoted_types: usize,
    first_quoted: Option<String>,
    /// Whether the 401 and 405 probes ran.
    probed: bool,
}

/// The audit's checks, and the forbidden Transfer Syntax uses it saw.
pub(crate) struct Audited {
    pub(crate) checks: [DicomWebCheck; 4],
    pub(crate) forbidden: Vec<String>,
}

impl Audit {
    /// Every response: its status code (8.1.2.1), payload header fields
    /// (8.4.2, 8.4.3, 8.6, 8.7.9), charset quoting (8.8), the 401 and 405
    /// fields (8.5), and single part application/dicom only for an Instance
    /// resource (10.4.4).
    pub(crate) fn response(&mut self, exchange: &Exchange, resource: Resource) {
        self.responses += 1;
        let line = exchange.request_line();
        self.status_report(exchange, &line, resource);
        if !(100..600).contains(&exchange.status) {
            self.messages.fail(format!(
                "{line} answered {}, which is not a valid 3-digit status code: its first digit gives the class, 1 to 5 ({})",
                exchange.status,
                sec("8.1.2.1 and 8.5")
            ));
        }
        self.payload_fields(exchange, &line);
        self.challenge(exchange, &line);
        if let Some(raw) = exchange.header("content-type") {
            self.charset_quoting(&line, "the payload", raw);
            if let Ok(media) = MediaType::parse(raw) {
                self.syntax_parameter(&line, "the payload", &media);
                if media.essence == "multipart/related" && raw.contains("type=\"") {
                    self.quoted_types += 1;
                    self.first_quoted.get_or_insert_with(|| line.clone());
                }
                if media.essence == "application/dicom" && resource != Resource::Instance {
                    self.payload.fail(format!(
                        "{line}: the payload is single part application/dicom, which is only used for an individual Instance resource ({})",
                        sec("10.4.4")
                    ));
                }
            }
        }
    }

    /// The recommendations of a failure payload: "A failure response
    /// payload should contain a Status Report" (8.1.2.3, 10.4.3.3, Table
    /// 10.6.3-1 for 413); Table 8.5-1: a 4xx or 5xx "should return a payload
    /// containing an explanation of the error", a 406 one "that lists the
    /// available Media Types", a 202 "should contain a Status Report".
    fn status_report(&mut self, exchange: &Exchange, line: &str, resource: Resource) {
        let status = exchange.status;
        let empty = exchange.body.is_empty();
        if (400..600).contains(&status) && empty {
            let sections = match (status, resource) {
                (413, Resource::Search) => "10.6.3.1 Table 10.6.3-1 and 8.1.2.3",
                (_, Resource::Retrieve | Resource::Instance) => {
                    "10.4.3.3, 8.1.2.3 and 8.5 Table 8.5-1"
                }
                _ => "8.1.2.3 and 8.5 Table 8.5-1",
            };
            self.errors.advise(format!(
                "{line} answered {status} with no payload; a failure response payload should contain a Status Report explaining the error and whether it is temporary or permanent ({})",
                sec(sections)
            ));
        }
        if status == 406 && !lists_media_types(&exchange.body) {
            self.errors.advise(format!(
                "{line} answered 406 with a payload that lists no Media Type; it should list the available Media Types and their resource identifiers ({})",
                sec("8.5 Table 8.5-1")
            ));
        }
        if status == 202 && empty {
            self.errors.advise(format!(
                "{line} answered 202 with no payload; the payload of a 202 should contain a Status Report ({})",
                sec("8.5 Table 8.5-1")
            ));
        }
    }

    /// A request that brought no response: the origin server's fault when
    /// the response was malformed or the connection closed first.
    pub(crate) fn failure(&mut self, line: &str, failure: &Failure) {
        match failure.fault {
            Some(Fault::Malformed) => self.messages.fail(format!(
                "{line}: {failure}; the response message includes a valid 3-digit status code ({})",
                sec("8.1.2 and 8.1.2.1")
            )),
            Some(Fault::Closed) => self.messages.fail(format!(
                "{line}: {failure}; the origin server shall always return a response message ({})",
                sec("8.1.2")
            )),
            None if failure.reason.starts_with("timed out") => self.timeouts += 1,
            None => {}
        }
    }

    /// 8.6: a 204 or 304 response has no payload (RFC 7230 3.3.3), so
    /// neither Content-Length nor Transfer-Encoding (8.6) nor Content-Type
    /// (8.7.9); any other response has a payload, even an empty one, with
    /// exactly one of them (Table 8.4.3-1), and a Content-Type when it is
    /// not empty (Table 8.4.2-1).
    fn payload_fields(&mut self, exchange: &Exchange, line: &str) {
        let length = exchange.header("content-length");
        let coding = exchange.header("transfer-encoding");
        if matches!(exchange.status, 100..=199 | 204 | 304) {
            if length.is_some() || coding.is_some() {
                self.payload.fail(format!(
                    "{line}: a {} response has no payload, yet it has {}; a message without a payload has neither Transfer-Encoding nor Content-Length ({})",
                    exchange.status,
                    if length.is_some() { "Content-Length" } else { "Transfer-Encoding" },
                    sec("8.6")
                ));
            }
            if exchange.header("content-type").is_some() {
                self.payload.fail(format!(
                    "{line}: a {} response has no payload, yet it has a Content-Type, which is only present when a payload is ({})",
                    exchange.status,
                    sec("8.7.9")
                ));
            }
            return;
        }
        self.payloads += 1;
        if !exchange.body.is_empty() && exchange.header("content-type").is_none() {
            self.payload.fail(format!(
                "{line}: the payload has no Content-Type ({})",
                sec("8.4.2 Table 8.4.2-1")
            ));
        }
        match (length, coding) {
            (None, None) => self.payload.fail(format!(
                "{line}: the payload has neither Content-Length nor Transfer-Encoding; Content-Length is present even if the payload is empty ({})",
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
                    "{line}: the {}-character Content-Length does not describe the payload's {} bytes ({})",
                    length.chars().count(),
                    exchange.body.len(),
                    sec("8.4.3 Table 8.4.3-1")
                ))
            }
            _ => {}
        }
    }

    /// Table 8.5-1: 401 with a WWW-Authenticate challenge, 405 with Allow.
    fn challenge(&mut self, exchange: &Exchange, line: &str) {
        match exchange.status {
            401 => {
                self.unauthorized += 1;
                if exchange.header("www-authenticate").is_none() {
                    self.errors.fail(format!(
                        "{line} answered 401 without a WWW-Authenticate Header Field, which holds at least one challenge (RFC 7235 4.1, {})",
                        sec("8.5 Table 8.5-1")
                    ));
                }
            }
            405 => {
                self.not_allowed += 1;
                if exchange.header("allow").is_none() {
                    self.errors.fail(format!(
                        "{line} answered 405 without an Allow Header Field listing the supported methods ({})",
                        sec("8.5 Table 8.5-1")
                    ));
                }
            }
            _ => {}
        }
    }

    /// 8.8: "DICOM Defined Terms for character sets contain space characters,
    /// and shall be enclosed in double quotes in HTTP Header Fields". Read
    /// from the raw field, since such a value is not a media type parameter.
    fn charset_quoting(&mut self, line: &str, what: &str, raw: &str) {
        let unquoted = raw.split(';').skip(1).find_map(|parameter| {
            let (name, value) = parameter.split_once('=')?;
            let value = value.trim();
            (name.trim().eq_ignore_ascii_case("charset")
                && !value.starts_with('"')
                && value.contains(' '))
            .then_some(value)
        });
        if let Some(value) = unquoted {
            self.payload.fail(format!(
                "{line}: {what} has an unquoted {}-character charset value with a space; a Defined Term with a space is enclosed in double quotes ({})",
                value.chars().count(),
                sec("8.8")
            ));
        }
    }

    fn syntax_parameter(&mut self, line: &str, what: &str, media: &MediaType) {
        if let Some(syntax) = media.parameter("transfer-syntax") {
            self.forbidden_syntax(&format!("{line}: {what}"), syntax);
        }
    }

    /// A retrieved file's File Meta Information Transfer Syntax UID, or a
    /// transfer-syntax parameter.
    pub(crate) fn forbidden_syntax(&mut self, context: &str, syntax: &str) {
        if let Some((_, name)) = FORBIDDEN_SYNTAXES
            .iter()
            .find(|(uid, _)| *uid == syntax.trim_end_matches('\0'))
        {
            self.forbidden.push(format!(
                "{context} is in {name} ({syntax}), which shall not be used with Web Services ({})",
                sec("8.7.3, 8.7.3.4 and 8.6.2.1")
            ));
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
    /// Content-Location; and each part's own header fields.
    pub(crate) fn parts(&mut self, exchange: &Exchange, parts: &[Part<'_>]) {
        self.representations += 1;
        let line = exchange.request_line();
        let missing: Vec<String> = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.header("content-location").is_none())
            .map(|(index, _)| (index + 1).to_string())
            .collect();
        if !missing.is_empty() {
            self.location.fail(format!(
                "{line}: part {} {} no Content-Location, which a part representing a resource has ({})",
                missing.join(", "),
                if missing.len() == 1 { "has" } else { "have" },
                sec("8.6.1.2 Table 8.6.1-1")
            ));
        }
        let mut identifiers: Vec<&str> = Vec::new();
        for part in parts {
            let Some(identifier) = part.header("content-id") else {
                continue;
            };
            if identifiers.contains(&identifier) {
                self.payload.advise(format!(
                    "{line}: the same {}-character Content-ID names more than one part; a content identifier should be unique for each part ({})",
                    identifier.chars().count(),
                    sec("8.7.1")
                ));
                break;
            }
            identifiers.push(identifier);
        }
        for (index, part) in parts.iter().enumerate() {
            let Some(raw) = part.header("content-type") else {
                continue;
            };
            let what = format!("part {}", index + 1);
            self.charset_quoting(&line, &what, raw);
            if let Ok(media) = MediaType::parse(raw) {
                self.syntax_parameter(&line, &what, &media);
            }
        }
    }

    /// Records the requests of the probes that ask for 401 and 405.
    pub(crate) fn error_probes(&mut self, probes: Verdict) {
        self.probed = true;
        self.errors.absorb(probes);
    }

    /// The gathered checks.
    pub(crate) fn finish(self) -> Audited {
        let mut payload = self.payload;
        if let Some(first) = &self.first_quoted {
            payload.advise(format!(
                "{} multipart/related responses, such as {first}, quote the \"type\" parameter, which should not contain quote marks per RFC 2387 ({})",
                self.quoted_types,
                sec("8.7.1")
            ));
        }
        if self.payloads == 0 {
            payload.not_run("no response carried a payload");
        }
        let mut location = self.location;
        if self.representations == 0 {
            location.not_run("no Retrieve response carried a representation");
        }
        let mut messages = self.messages;
        if self.responses == 0 && !messages.failed() {
            messages.not_run("no request brought a response");
        }
        if self.timeouts > 0 {
            messages.note(format!(
                "{} requests timed out, so whether a response would have come is not known",
                self.timeouts
            ));
        }
        let mut errors = self.errors;
        let probes = if self.probed {
            ""
        } else {
            "; asking for them requires --probe-error-responses"
        };
        match (self.unauthorized, self.not_allowed) {
            (0, 0) => errors.not_run(format!(
                "no response was 401 or 405, so WWW-Authenticate and Allow were not exercised{probes}"
            )),
            (0, _) => errors.note(format!(
                "no response was 401, so WWW-Authenticate was not exercised{probes}"
            )),
            (_, 0) => errors.note(format!(
                "no response was 405, so Allow was not exercised{probes}"
            )),
            _ => {}
        }
        Audited {
            checks: [
                payload.finish(
                    "dicomweb-payload-headers",
                    sec("8.4.2 Table 8.4.2-1, 8.4.3 Table 8.4.3-1, 8.6, 8.7.9, 8.8 and 10.4.4"),
                    &format!(
                        "each of {} responses with a payload has one of Content-Length and Transfer-Encoding and a Content-Type, and responses without one have neither",
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
                messages.finish(
                    "dicomweb-response-messages",
                    sec("8.1.2, 8.1.2.1, 10.4.3, 10.5.3 and 10.6.3"),
                    &format!(
                        "each of {} requests brought a response message with a valid 3-digit status code",
                        self.responses
                    ),
                ),
                errors.finish(
                    "dicomweb-error-responses",
                    sec("8.5 Table 8.5-1"),
                    &format!(
                        "{} 401 responses carry WWW-Authenticate and {} 405 responses carry Allow",
                        self.unauthorized, self.not_allowed
                    ),
                ),
            ],
            forbidden: self.forbidden,
        }
    }
}

/// Whether a payload names a media type, as a list of the available ones
/// does.
fn lists_media_types(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&body[..body.len().min(64 * 1024)]);
    ["application/", "image/", "multipart/", "text/", "video/"]
        .iter()
        .any(|kind| text.contains(kind))
}
