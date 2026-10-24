//! Two HTTP behaviors PS3.18 holds every response to: a Range request
//! answered as RFC 7233 specifies, where Range is supported (8.6.1.1), and
//! the 401 and 405 responses with their Header Fields (Table 8.5-1), asked
//! for with an invalid credential and a TRACE request, which change nothing.

use super::client::{Method, Redirects, Request};
use super::report::Verdict;
use super::response::sec;
use super::{DicomWebTransaction, Session, Target};

const RANGE_NAME: &str = "dicomweb-range-requests";
const RANGE_SECTIONS: &str = "8.6.1.1";
/// The bytes asked for: 0 to 15.
const RANGE_LAST: usize = 15;
/// A bearer token no origin server issued.
const INVALID_CREDENTIAL: &str = "Bearer wsi-dicom-invalid-credential";

/// 8.6.1.1: "Single part payloads may be requested with an HTTP Range
/// request. If supported by the server, the response shall be according to
/// the standard HTTP Range response": 206 with the Content-Range of the
/// bytes asked for (RFC 7233 4.1).
pub(super) fn range(session: &mut Session<'_>, target: &Target) {
    let reason = if session.skipped(DicomWebTransaction::Metadata) {
        Some("skipped by request (--skip metadata)")
    } else if target.instance_path().is_none() {
        Some("no Instance to target")
    } else if session.instance_metadata.as_ref().is_none_or(Vec::is_empty) {
        Some("the Instance Metadata was not retrieved whole, so a range of it cannot be compared")
    } else {
        None
    };
    if let Some(reason) = reason {
        session.not_run(RANGE_NAME, RANGE_SECTIONS, reason);
        return;
    }
    let whole = session.instance_metadata.clone().unwrap_or_default();
    let mut verdict = Verdict::default();
    let path = format!("{}/metadata", target.instance_path().unwrap_or_default());
    let range = format!("bytes=0-{RANGE_LAST}");
    let headers = [("Range", range.as_str())];
    let url = session.url(&path, None);
    let request = Request::get(url, "application/dicom+json").with_headers(&headers);
    if let Some(exchange) = session.send(&mut verdict, &request) {
        let line = exchange.request_line();
        match exchange.status {
            200 => verdict.unsupported(format!(
                "{line} with Range: {range} answered 200 with the whole representation: Range, which an origin server may support, is not supported ({})",
                sec(RANGE_SECTIONS)
            )),
            206 => {
                let expected = &whole[..whole.len().min(RANGE_LAST + 1)];
                let content_range = exchange.header("content-range").unwrap_or_default();
                let valid = [
                    format!("bytes 0-{}/{}", expected.len() - 1, whole.len()),
                    format!("bytes 0-{}/*", expected.len() - 1),
                ];
                if !valid.iter().any(|form| form == content_range.trim()) {
                    verdict.fail(format!(
                        "{line}: 206 with Content-Range {content_range:?}; the partial response to Range: {range} has Content-Range {:?} (RFC 7233 4.1, {})",
                        valid[0],
                        sec(RANGE_SECTIONS)
                    ));
                }
                if exchange.body != expected {
                    verdict.fail(format!(
                        "{line}: the 206 payload is {} bytes that are not the first {} bytes of the Instance Metadata (RFC 7233 4.1, {})",
                        exchange.body.len(),
                        expected.len(),
                        sec(RANGE_SECTIONS)
                    ));
                }
            }
            status => verdict.fail(format!(
                "{line} with Range: {range} answered {status}; a supported Range request answers 206, and an origin server without Range support ignores it and answers 200 (RFC 7233 3.1, {})",
                sec(RANGE_SECTIONS)
            )),
        }
    }
    session.push(verdict.finish(
        RANGE_NAME,
        sec(RANGE_SECTIONS),
        "a Range request on the Instance Metadata answers 206 with the bytes and Content-Range asked for",
    ));
}

/// Asks for a 401 with a bearer token no origin server issued, and for a 405
/// with TRACE, which the Studies Service does not define; the responses are
/// held to Table 8.5-1 with every other response of the run. Only with
/// `probe_error_responses`: neither request may surprise the server's owner.
pub(super) fn errors(session: &mut Session<'_>, target: &Target) {
    if !session.options.probe_error_responses {
        return;
    }
    let mut verdict = Verdict::default();
    match target.study.as_deref() {
        Some(study) if !session.skipped(DicomWebTransaction::Search) => {
            let query = format!("StudyInstanceUID={study}");
            let headers = [("Authorization", INVALID_CREDENTIAL)];
            let url = session.url("/studies", Some(&query));
            let request = Request::get(url, "application/dicom+json").with_headers(&headers);
            if let Some(exchange) = session.send(&mut verdict, &request) {
                if exchange.status == 200 {
                    verdict.note(format!(
                        "{} with an invalid bearer token answered 200: the origin server does not authenticate Search requests",
                        exchange.request_line()
                    ));
                }
            }
        }
        _ => verdict.note("no Search to send an invalid credential with"),
    }
    let request = Request {
        method: Method::Trace,
        url: session.url("/studies", None),
        accept: Some("message/http"),
        content_type: None,
        body: None,
        chunked: false,
        headers: &[],
        redirects: Redirects::Never,
    };
    if let Some(exchange) = session.send(&mut verdict, &request) {
        if exchange.status != 405 {
            verdict.note(format!(
                "{} answered {}, not 405",
                exchange.request_line(),
                exchange.status
            ));
        }
    }
    session.audit.error_probes(verdict);
}
