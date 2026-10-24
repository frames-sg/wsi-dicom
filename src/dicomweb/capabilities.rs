//! The Retrieve Capabilities Transaction (PS3.18 8.9 and Annex H), which
//! "All Representational State Transfer (REST) Web Services defined by this
//! Part of the Standard shall implement" and each Studies Service
//! transaction's conformance requires (10.2).

use super::client::{Method, Redirects, Request};
use super::report::Verdict;
use super::response::{expect_status, sec};
use super::{DicomWebTransaction, Session};
use crate::validation::json_text_error;

const NAME: &str = "dicomweb-capabilities";
const SECTIONS: &str = "8.9, 8.9.4, 10.2 and Annex H";
const WADL: &str = "application/vnd.sun.wadl+xml";
const JSON: &str = "application/json";

pub(super) fn run(session: &mut Session<'_>) {
    if session.skipped(DicomWebTransaction::Capabilities) {
        session.not_run(NAME, SECTIONS, "skipped by request (--skip capabilities)");
        return;
    }
    let mut verdict = Verdict::default();
    let accept = format!("{WADL}, {JSON}");
    let request = Request {
        method: Method::Options,
        url: session.url("/", None),
        accept: Some(&accept),
        content_type: None,
        body: None,
        redirects: Redirects::Listed,
    };
    if let Some(exchange) = session.send(&mut verdict, &request) {
        if expect_status(&mut verdict, &exchange, &[200], "8.9.3.1 and 10.2") {
            description(
                &mut verdict,
                &exchange.body,
                exchange.header("content-type"),
            );
        }
    }
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "OPTIONS on the Base URI returns a Capabilities Description",
    ));
}

/// 8.9.4: WADL or its JSON representation (Annex G); Annex H: one
/// "application" element holding a "resources" element with a "base".
fn description(verdict: &mut Verdict, body: &[u8], content_type: Option<&str>) {
    let essence = content_type
        .and_then(|value| super::media::MediaType::parse(value).ok())
        .map(|media| media.essence);
    match essence.as_deref() {
        Some(WADL) => {
            let text = String::from_utf8_lossy(body);
            let applications = start_tags(&text, "application").len();
            if applications != 1 {
                verdict.fail(format!(
                    "the WADL document holds {applications} \"application\" elements; it holds one ({})",
                    sec("Annex H")
                ));
            }
            let has_base = start_tags(&text, "resources")
                .iter()
                .any(|tag| tag.contains("base="));
            if !has_base {
                verdict.fail(format!(
                    "the WADL document has no \"resources\" element with a \"base\" attribute ({})",
                    sec("Annex H")
                ));
            }
        }
        Some(JSON) => {
            if let Some(reason) = json_text_error(body) {
                verdict.fail(format!(
                    "the application/json Capabilities Description is not JSON: {reason} ({})",
                    sec("8.9.4 and Annex G")
                ));
            }
        }
        other => verdict.fail(format!(
            "the Capabilities Description is {}; it is application/vnd.sun.wadl+xml or application/json ({})",
            other.unwrap_or("without Content-Type"),
            sec("8.9.4")
        )),
    }
}

/// The start tags (between `<` and `>`) whose local name is `local`, with
/// any namespace prefix.
fn start_tags<'t>(text: &'t str, local: &str) -> Vec<&'t str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        let name = tag
            .split(|character: char| character.is_whitespace() || character == '/')
            .next()
            .unwrap_or_default();
        if name.rsplit(':').next() == Some(local) {
            found.push(tag);
        }
        rest = &after[end..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::start_tags;

    #[test]
    fn start_tags_match_local_names_with_or_without_prefix() {
        let wadl = "<?xml version=\"1.0\"?><wadl:application xmlns:wadl=\"x\"><resources base=\"https://h/\"/></wadl:application>";
        assert_eq!(start_tags(wadl, "application").len(), 1);
        assert!(start_tags(wadl, "resources")[0].contains("base="));
        assert!(start_tags("<applications/>", "application").is_empty());
    }
}
