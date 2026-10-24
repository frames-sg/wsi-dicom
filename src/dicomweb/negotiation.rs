//! Content negotiation (PS3.18 8.3.3, 8.7.5 to 8.7.8) and character sets
//! (8.8), exercised on the Instance Metadata, whose Default Media Type is
//! application/dicom+json and Required one multipart/related;
//! type="application/dicom+xml" (Table 10.4.4-1), and on the Rendered
//! Frames.

use super::client::{Exchange, Request};
use super::report::Verdict;
use super::response::{expect_media_type, expect_multipart, expect_status, sec};
use super::{DicomWebTransaction, Session, Target};

const ACCEPT_NAME: &str = "dicomweb-accept-negotiation";
const ACCEPT_SECTIONS: &str =
    "8.3.3.1, 8.7.3 Table 8.7.3-1, 8.7.5, 8.7.7, 8.7.8.1, 10.1.2 Table 10.1.2-1 and 10.4.1.2 Table 10.4.1-5";
const CHARSET_NAME: &str = "dicomweb-character-sets";
const CHARSET_SECTIONS: &str =
    "8.3.3.2, 8.8, 8.8.1, 10.1.2 Table 10.1.2-1, 10.1.3, 10.4.1.3 Table 10.4.1-6, 10.4.4 and 10.6.1.3 Table 10.6.1-6";
const JSON: &str = "application/dicom+json";
const XML: &str = "application/dicom+xml";
const XML_ACCEPT: &str = "multipart/related; type=\"application/dicom+xml\"";
const UNSUPPORTED_TYPE: &str = "application/x-wsidicom-unsupported";
const UNSUPPORTED_CHARSET: &str = "x-wsidicom-unsupported";

pub(super) fn run(session: &mut Session<'_>, target: &Target) {
    let reason = if session.skipped(DicomWebTransaction::Metadata) {
        Some("skipped by request (--skip metadata)")
    } else if target.instance_path().is_none() {
        Some("no Instance to target")
    } else {
        None
    };
    if let Some(reason) = reason {
        session.not_run(ACCEPT_NAME, ACCEPT_SECTIONS, reason);
        session.not_run(CHARSET_NAME, CHARSET_SECTIONS, reason);
        return;
    }
    accept(session, target);
    character_sets(session, target);
}

/// What one request must bring.
enum Expect {
    /// 200 in this single part media type.
    Single(&'static str),
    /// 200 in multipart/related of this media type.
    Multipart(&'static str),
    Status(u16),
}

/// One negotiation request and what it must bring, and why.
struct Probe<'p> {
    path: String,
    query: Option<&'p str>,
    accept: &'p str,
    headers: &'p [(&'p str, &'p str)],
    expect: Expect,
    reference: &'p str,
}

fn probe(session: &mut Session<'_>, verdict: &mut Verdict, probe: &Probe<'_>) {
    let url = session.url(&probe.path, probe.query);
    let request = Request::get(url, probe.accept).with_headers(probe.headers);
    let Some(exchange) = session.send(verdict, &request) else {
        return;
    };
    held_to(verdict, &exchange, &probe.expect, probe.reference);
}

fn held_to(verdict: &mut Verdict, exchange: &Exchange, expect: &Expect, reference: &str) {
    match expect {
        Expect::Status(status) => {
            expect_status(verdict, exchange, &[*status], reference);
        }
        Expect::Single(essence) => {
            if expect_status(verdict, exchange, &[200], reference) {
                expect_media_type(verdict, exchange, essence, reference);
            }
        }
        Expect::Multipart(root) => {
            if expect_status(verdict, exchange, &[200], reference) {
                expect_multipart(verdict, exchange, Some(root), reference);
            }
        }
    }
}

/// 8.7.8.1: the Selected Media Type comes from the Accept Query Parameter
/// first, then the Accept Header Field, each by qvalue, then the Default
/// Media Type for a wildcard, else 406; values that are invalid or not
/// supported are ignored (8.7.7); DICOM and non-DICOM Media Types together
/// answer 400 (8.3.3.1, 8.7.5).
fn accept(session: &mut Session<'_>, target: &Target) {
    let mut verdict = Verdict::default();
    let metadata = format!("{}/metadata", target.instance_path().unwrap_or_default());
    unsupported_only(session, &mut verdict, &metadata);
    let both = format!("{XML_ACCEPT}, {JSON}; q=0.1");
    let ignored = format!("wsidicom-invalid, {UNSUPPORTED_TYPE}; q=0.9, {JSON}; q=0.5");
    let weighted = format!("{JSON}; q=0.5, {XML_ACCEPT}");
    for probe_case in [
        Probe {
            path: metadata.clone(),
            query: None,
            accept: "*/*",
            headers: &[],
            expect: Expect::Single(JSON),
            reference: "8.7.7: \"*/*\" matches the Default Media Type, application/dicom+json (Table 10.4.4-1)",
        },
        Probe {
            path: metadata.clone(),
            query: None,
            accept: &ignored,
            headers: &[],
            expect: Expect::Single(JSON),
            reference: "8.7.7: Accept values that are not valid or not supported are ignored",
        },
        Probe {
            path: metadata.clone(),
            query: None,
            accept: &weighted,
            headers: &[],
            expect: Expect::Multipart(XML),
            reference: "8.7.8.1: media ranges are prioritized by qvalue (RFC 7231 5.3.1), and the XML one has 1",
        },
        Probe {
            path: metadata.clone(),
            query: Some("accept=application/dicom%2Bjson"),
            accept: &both,
            headers: &[],
            expect: Expect::Single(JSON),
            reference: "8.7.8.1 and Table 10.4.1-5: the supported Media Type of the Accept Query Parameter is selected before the Accept Header Field's",
        },
        Probe {
            path: metadata.clone(),
            query: Some("accept=application/dicom%2Bjson,text/html"),
            accept: "*/*",
            headers: &[],
            expect: Expect::Status(400),
            reference: "8.3.3.1: an Accept Query Parameter with DICOM and non-DICOM Media Types answers 400 (Bad Request)",
        },
        Probe {
            path: metadata.clone(),
            query: None,
            accept: "application/dicom+json, image/png",
            headers: &[],
            expect: Expect::Status(400),
            reference: "8.7.5: Acceptable Media Types with DICOM and Rendered Media Types answer 400 (Bad Request)",
        },
    ] {
        probe(session, &mut verdict, &probe_case);
    }
    no_default(session, &mut verdict, target);
    session.push(verdict.finish(
        ACCEPT_NAME,
        sec(ACCEPT_SECTIONS),
        "the Selected Media Type follows the Accept Query Parameter, then the Accept Header Field by qvalue, then the Default Media Type, and mixed DICOM and non-DICOM types answer 400",
    ));
}

/// 8.3.3.1 and Table 8.7.3-1: when none of the Acceptable Media Types is
/// supported the response is in the Default Media Type. 8.7.8.1 and Table
/// 10.4.3-1 answer the same request 406 when no wildcard matches the
/// Default, so either is accepted.
fn unsupported_only(session: &mut Session<'_>, verdict: &mut Verdict, metadata: &str) {
    let url = session.url(metadata, None);
    let Some(exchange) = session.send(verdict, &Request::get(url, UNSUPPORTED_TYPE)) else {
        return;
    };
    if exchange.status == 406 {
        verdict.note(format!(
            "{} answered 406, as 8.7.8.1 step 5 and Table 10.4.3-1 prescribe; 8.3.3.1 and Table 8.7.3-1 would return the Default Media Type",
            exchange.request_line()
        ));
        return;
    }
    held_to(
        verdict,
        &exchange,
        &Expect::Single(JSON),
        "8.3.3.1 and 8.7.3 Table 8.7.3-1: with none of the Acceptable Media Types supported, the Default Media Type (or 406, 8.7.8.1)",
    );
}

/// 8.3.3.1: "If there is no default Media Type defined for the Target
/// Resource, the origin server response shall be 406". Rendered Frames of
/// a Multi-frame Image have none (Table 8.7.4-1).
fn no_default(session: &mut Session<'_>, verdict: &mut Verdict, target: &Target) {
    if session.skipped(DicomWebTransaction::Rendered) {
        verdict.note("Rendered resources skipped by request (--skip rendered)");
        return;
    }
    if target.frames.unwrap_or(1) < 2 {
        verdict.note("the Instance has one Frame, so it has no Multi-frame Image rendering without a Default Media Type");
        return;
    }
    probe(
        session,
        verdict,
        &Probe {
            path: format!(
                "{}/frames/1,2/rendered",
                target.instance_path().unwrap_or_default()
            ),
            query: None,
            accept: "image/x-wsidicom-unsupported",
            headers: &[],
            expect: Expect::Status(406),
            reference: "8.3.3.1 and 8.7.8.1: a Multi-frame Image has no Default Rendered Media Type (Table 8.7.4-1), so an unsupported one answers 406",
        },
    );
}

/// 8.8: charset values are ignored when unsupported (8.3.3.2, 8.8.1), the
/// Accept-Charset Header Field (Tables 10.4.1-6, 10.6.1-6), the charset and
/// Transfer Syntax Media Type parameters (10.1.3, 10.4.4) and the charset
/// Query Parameter (Table 10.1.2-1) are supported, and Retrieve Rendered
/// supports "UTF-8" (8.8).
fn character_sets(session: &mut Session<'_>, target: &Target) {
    let mut verdict = Verdict::default();
    let instance = target.instance_path().unwrap_or_default();
    let metadata = format!("{instance}/metadata");
    let unsupported_query = format!("charset={UNSUPPORTED_CHARSET}");
    let parameters = format!("{JSON}; transfer-syntax=1.2.840.10008.1.2.1; charset=utf-8");
    let mut probes = vec![
        Probe {
            path: metadata.clone(),
            query: Some(unsupported_query.as_str()),
            accept: JSON,
            headers: &[],
            expect: Expect::Single(JSON),
            reference: "8.3.3.2: a charset Query Parameter value that is not supported is ignored",
        },
        Probe {
            path: metadata.clone(),
            query: None,
            accept: JSON,
            headers: &[("Accept-Charset", "utf-8")],
            expect: Expect::Single(JSON),
            reference: "10.4.1.3 Table 10.4.1-6: the origin server supports Accept-Charset",
        },
        Probe {
            path: metadata.clone(),
            query: None,
            accept: JSON,
            headers: &[("Accept-Charset", UNSUPPORTED_CHARSET)],
            expect: Expect::Single(JSON),
            reference: "8.8.1: charset values that are not valid or not supported are ignored",
        },
        Probe {
            path: metadata,
            query: None,
            accept: &parameters,
            headers: &[],
            expect: Expect::Single(JSON),
            reference: "10.1.3 and 10.4.4: the Transfer Syntax and Character Set Media Type parameters are supported",
        },
    ];
    let search = target
        .study
        .as_ref()
        .map(|study| format!("StudyInstanceUID={study}"));
    match &search {
        Some(query) if !session.skipped(DicomWebTransaction::Search) => probes.push(Probe {
            path: "/studies".into(),
            query: Some(query.as_str()),
            accept: JSON,
            headers: &[("Accept-Charset", "utf-8")],
            expect: Expect::Single(JSON),
            reference:
                "10.6.1.3 Table 10.6.1-6: the origin server supports Accept-Charset in a Search",
        }),
        _ => verdict.note("Search skipped, so Accept-Charset was not sent to it"),
    }
    if session.skipped(DicomWebTransaction::Rendered) {
        verdict.note("Rendered resources skipped by request (--skip rendered)");
    } else {
        probes.push(Probe {
            path: format!("{instance}/frames/1/rendered"),
            query: Some("charset=UTF-8"),
            accept: "image/jpeg",
            headers: &[],
            expect: Expect::Single("image/jpeg"),
            reference:
                "8.8: the origin server supports the \"UTF-8\" charset name for Retrieve Rendered",
        });
    }
    for probe_case in &probes {
        probe(session, &mut verdict, probe_case);
    }
    session.push(verdict.finish(
        CHARSET_NAME,
        sec(CHARSET_SECTIONS),
        "charset values are accepted or ignored as unsupported, and UTF-8 is supported",
    ));
}
