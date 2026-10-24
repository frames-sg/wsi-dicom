//! The Retrieve Capabilities Transaction (PS3.18 8.9 and Annex H), which
//! "All Representational State Transfer (REST) Web Services defined by this
//! Part of the Standard shall implement" and each Studies Service
//! transaction's conformance requires (10.2). The description is held to
//! what the rest of the run finds the origin server to serve, so the check
//! is finished last.

use super::client::{Method, Redirects, Request};
use super::report::Verdict;
use super::response::{expect_status, sec};
use super::{DicomWebTransaction, Session};
use crate::validation::json_text_error;

const NAME: &str = "dicomweb-capabilities";
const SECTIONS: &str = "8.9, 8.9.3.2, 8.9.4, 8.8, 10.2 and Annex H";
const WADL: &str = "application/vnd.sun.wadl+xml";
const JSON: &str = "application/json";
/// Bytes of a Capabilities Description searched for documented names.
const MAX_DESCRIPTION_BYTES: usize = 4 * 1024 * 1024;

/// The Capabilities check until the run is over.
pub(super) struct Pending {
    verdict: Verdict,
    /// The description, if a 200 response carried one.
    text: Option<String>,
    /// Whether the description is a WADL document.
    wadl: bool,
}

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
        chunked: false,
        headers: &[],
        redirects: Redirects::Listed,
    };
    let mut text = None;
    let mut wadl = false;
    if let Some(exchange) = session.send(&mut verdict, &request) {
        if expect_status(&mut verdict, &exchange, &[200], "8.9.3.1 and 10.2") {
            wadl = description(
                &mut verdict,
                &exchange.body,
                exchange.header("content-type"),
            );
            let shown = &exchange.body[..exchange.body.len().min(MAX_DESCRIPTION_BYTES)];
            text = Some(String::from_utf8_lossy(shown).into_owned());
        }
    }
    session.capabilities = Some(Pending {
        verdict,
        text,
        wadl,
    });
}

/// Whether the Capabilities Description names `name`, such as a Query
/// Parameter it documents; `None` when there is no description to read.
pub(super) fn documents(session: &Session<'_>, name: &str) -> Option<bool> {
    let text = session.capabilities.as_ref()?.text.as_deref()?;
    Some(
        text.to_ascii_lowercase()
            .contains(&name.to_ascii_lowercase()),
    )
}

/// Holds the description to the character sets (8.8) and the resources
/// (Annex H) the run found, and pushes the check.
pub(super) fn finish(session: &mut Session<'_>) {
    let Some(Pending {
        mut verdict,
        text,
        wadl,
    }) = session.capabilities.take()
    else {
        return;
    };
    if let Some(text) = text.as_deref() {
        if !text.to_ascii_lowercase().contains("utf-8") {
            verdict.fail(format!(
                "the Capabilities Description documents no character set, not even UTF-8, the Default Character Set of every DICOM Media Type (8.7.3.5.3) that the origin server supports; the Retrieve Capabilities response \"shall also document all supported character sets\" ({})",
                sec("8.8")
            ));
        }
        if wadl {
            resources(&mut verdict, text, &session.served);
        }
    }
    session.push(verdict.finish(
        NAME,
        sec(SECTIONS),
        "OPTIONS on the Base URI returns a Capabilities Description that documents the served resources and character sets",
    ));
}

/// Annex H: "the WADL content shall include a "resource" element for the
/// Target Resource ... describing all methods and child resources", so each
/// resource the origin server answered for has a resource element.
fn resources(verdict: &mut Verdict, text: &str, served: &std::collections::BTreeSet<&str>) {
    let described: Vec<&str> = start_tags(text, "resource")
        .into_iter()
        .filter_map(|tag| attribute(tag, "path"))
        .flat_map(|path| path.split('/'))
        .map(|segment| segment.trim_matches(['{', '}']))
        .collect();
    let missing: Vec<&str> = served
        .iter()
        .copied()
        .filter(|segment| !described.contains(segment))
        .collect();
    if !missing.is_empty() {
        verdict.fail(format!(
            "the WADL document has no \"resource\" element for {}, which the origin server serves; it describes all child resources of the Target Resource ({})",
            missing.join(", "),
            sec("Annex H Table H-1")
        ));
    }
}

/// 8.9.4: WADL or its JSON representation; Annex H: one "application"
/// element holding a "resources" element with a "base". Returns whether the
/// description is WADL.
fn description(verdict: &mut Verdict, body: &[u8], content_type: Option<&str>) -> bool {
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
                    sec("Annex H and 8.9.3.2")
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
            true
        }
        Some(JSON) => {
            if let Some(reason) = json_text_error(body) {
                verdict.fail(format!(
                    "the application/json Capabilities Description is not JSON: {reason} ({})",
                    sec("8.9.4 and Annex G")
                ));
            }
            false
        }
        other => {
            verdict.fail(format!(
                "the Capabilities Description is {}; it is application/vnd.sun.wadl+xml or application/json ({})",
                other.unwrap_or("without Content-Type"),
                sec("8.9.4")
            ));
            false
        }
    }
}

/// The value of attribute `name` in a start tag.
fn attribute<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &rest[1..];
    Some(&value[..value.find(quote)?])
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
    use super::{attribute, start_tags};

    #[test]
    fn start_tags_match_local_names_with_or_without_prefix() {
        let wadl = "<?xml version=\"1.0\"?><wadl:application xmlns:wadl=\"x\"><resources base=\"https://h/\"/></wadl:application>";
        assert_eq!(start_tags(wadl, "application").len(), 1);
        assert!(start_tags(wadl, "resources")[0].contains("base="));
        assert!(start_tags("<applications/>", "application").is_empty());
    }

    #[test]
    fn attributes_read_single_or_double_quoted_values() {
        assert_eq!(
            attribute("resource path=\"{study}/series\"", "path"),
            Some("{study}/series")
        );
        assert_eq!(attribute("resource path='frames'", "path"), Some("frames"));
        assert_eq!(attribute("resource id=\"x\"", "path"), None);
    }
}
