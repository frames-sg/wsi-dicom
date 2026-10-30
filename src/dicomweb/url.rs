//! The absolute http(s) URLs the checker requests, and RFC 3986 5.2
//! resolution of the references servers return (BulkDataURI, Location).

use std::fmt;

use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::Tag;
use dicom_dictionary_std::StandardDataDictionary;

use crate::validation::disclosure::{shown, withheld, Nesting};

/// An absolute `http` or `https` URL without userinfo or fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Url {
    /// Lowercase `http` or `https`.
    scheme: String,
    /// Lowercase host, with a port when one is written.
    authority: String,
    /// The path, starting with `/` (or empty).
    path: String,
    query: Option<String>,
    /// A server-provided reference can contain identifiers in any component.
    server_reference: bool,
}

/// `value` percent-encoded for a query component: unreserved characters as
/// they are, every other byte of its UTF-8 encoding as %XX (RFC 3986 2.1).
pub(crate) fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

impl Url {
    /// Parses an absolute URL. Credentials in the URL are refused: they would
    /// be logged with every request, so a token is passed by environment
    /// variable instead.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let shown = || shown_reference(text);
        if text.bytes().any(|byte| byte <= b' ' || byte >= 0x7F) {
            return Err(format!(
                "URL {:?} contains whitespace, control or non-ASCII characters",
                shown()
            ));
        }
        let (scheme, rest) = text
            .split_once("://")
            .ok_or_else(|| format!("URL {:?} is not absolute (scheme://host/path)", shown()))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!("URL {:?} is not http or https", shown()));
        }
        if rest.contains('#') {
            return Err(format!("URL {:?} has a fragment", shown()));
        }
        let (before_query, query) = match rest.split_once('?') {
            Some((before, query)) => (before, Some(query.to_string())),
            None => (rest, None),
        };
        let (authority, path) = match before_query.find('/') {
            Some(slash) => (&before_query[..slash], &before_query[slash..]),
            None => (before_query, ""),
        };
        if authority.contains('@') {
            return Err(format!(
                "URL {:?} carries userinfo; pass credentials with --bearer-token-env instead",
                shown()
            ));
        }
        if authority.is_empty() {
            return Err(format!("URL {:?} has no host", shown()));
        }
        Ok(Self {
            scheme,
            authority: authority.to_ascii_lowercase(),
            path: path.to_string(),
            query,
            server_reference: false,
        })
    }

    /// Whether the URL has a query component.
    pub(crate) fn has_query(&self) -> bool {
        self.query.is_some()
    }

    /// Whether the URL uses TLS.
    pub(crate) fn is_https(&self) -> bool {
        self.scheme == "https"
    }

    /// The host, without port or IPv6 brackets.
    pub(crate) fn host(&self) -> &str {
        let authority = self.authority.as_str();
        if let Some(rest) = authority.strip_prefix('[') {
            return rest.split(']').next().unwrap_or(rest);
        }
        authority.split(':').next().unwrap_or(authority)
    }

    fn port(&self) -> u16 {
        let explicit = match self.authority.rfind(':') {
            Some(colon) if !self.authority[colon..].contains(']') => {
                self.authority[colon + 1..].parse().ok()
            }
            _ => None,
        };
        explicit.unwrap_or(if self.is_https() { 443 } else { 80 })
    }

    /// RFC 6454 origin: scheme, host and port.
    pub(crate) fn same_origin(&self, other: &Url) -> bool {
        self.scheme == other.scheme && self.host() == other.host() && self.port() == other.port()
    }

    /// Whether the host is the loopback interface, where a plain HTTP request
    /// does not leave the machine.
    pub(crate) fn is_loopback(&self) -> bool {
        matches!(self.host(), "localhost" | "127.0.0.1" | "::1")
    }

    /// This URL with `suffix` (starting with `/`) appended to its path and
    /// the query replaced by `query`.
    /// The path segments of this URL below `base`, if it is under `base`.
    pub(crate) fn segments_below<'u>(&'u self, base: &Url) -> Option<Vec<&'u str>> {
        if !self.same_origin(base) {
            return None;
        }
        let rest = self.path.strip_prefix(base.path.trim_end_matches('/'))?;
        if !(rest.is_empty() || rest.starts_with('/')) {
            return None;
        }
        Some(
            rest.split('/')
                .filter(|segment| !segment.is_empty())
                .collect(),
        )
    }

    pub(crate) fn join(&self, suffix: &str, query: Option<&str>) -> Url {
        Url {
            scheme: self.scheme.clone(),
            authority: self.authority.clone(),
            path: format!("{}{suffix}", self.path.trim_end_matches('/')),
            query: query.map(str::to_string),
            server_reference: self.server_reference,
        }
    }

    /// The URL for reports: `target`'s path and query after the origin.
    pub(crate) fn reported(&self) -> String {
        if self.server_reference {
            return withheld(&self.to_string()).to_string();
        }
        format!("{}://{}{}", self.scheme, self.authority, self.target())
    }

    /// The path and query, for reports. The value of a query key naming an
    /// Attribute that messages withhold (`disclosure`), such as
    /// PatientID=... or 00100010=..., is shown as messages show that
    /// Attribute's Values: by its length.
    pub(crate) fn target(&self) -> String {
        if self.server_reference {
            return withheld(&self.to_string()).to_string();
        }
        let path = self
            .path_or_root()
            .split('/')
            .scan(false, |identifier, segment| {
                let shown = if *identifier && !segment.is_empty() {
                    withheld(segment).to_string()
                } else {
                    segment.to_string()
                };
                *identifier = matches!(segment, "studies" | "series" | "instances");
                Some(shown)
            })
            .collect::<Vec<_>>()
            .join("/");
        match &self.query {
            Some(query) => format!("{path}?{}", shown_query(query)),
            None => path,
        }
    }

    pub(crate) fn server_reference(mut self) -> Self {
        self.server_reference = true;
        self
    }

    fn path_or_root(&self) -> &str {
        if self.path.is_empty() {
            "/"
        } else {
            &self.path
        }
    }

    /// Resolves an RFC 3986 URI-reference against this URL (RFC 3986 5.2.2).
    pub(crate) fn resolve(&self, reference: &str) -> Result<Url, String> {
        if reference.contains("://") {
            return Url::parse(reference).map(Url::server_reference);
        }
        if reference.starts_with("//") {
            return Url::parse(&format!("{}:{reference}", self.scheme)).map(Url::server_reference);
        }
        let reference = reference.split('#').next().unwrap_or_default();
        let (path, query) = match reference.split_once('?') {
            Some((path, query)) => (path, Some(query.to_string())),
            None => (reference, None),
        };
        let merged = if path.is_empty() {
            return Ok(Url {
                query: query.or_else(|| self.query.clone()),
                server_reference: true,
                ..self.clone()
            });
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            let base = self.path_or_root();
            let directory = &base[..base.rfind('/').map_or(0, |slash| slash + 1)];
            format!("{directory}{path}")
        };
        Ok(Url {
            scheme: self.scheme.clone(),
            authority: self.authority.clone(),
            path: remove_dot_segments(&merged),
            query,
            server_reference: true,
        })
    }
}

/// Untrusted URI references can carry identifiers or credentials in any component.
pub(crate) fn shown_reference(text: &str) -> String {
    withheld(text).to_string()
}

/// `query` with the values of its parameters that name withheld Attributes
/// withheld.
fn shown_query(query: &str) -> String {
    query
        .split('&')
        .map(|parameter| match parameter.split_once('=') {
            Some((key, value)) if !value.is_empty() => match withheld_key(key) {
                Some(tag) => format!("{key}={}", shown(tag, &percent_decoded(value))),
                None => parameter.to_string(),
            },
            _ => parameter.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// The Attribute a query key names (PS3.18 8.3.4.1: a keyword or tag, or a
/// dotted path of them into Sequences), when messages withhold its Values.
fn withheld_key(key: &str) -> Option<Tag> {
    let mut tags = key.split('.').map(|part| {
        let hex = part.len() == 8 && part.bytes().all(|byte| byte.is_ascii_hexdigit());
        if hex {
            let value = u32::from_str_radix(part, 16).ok()?;
            Some(Tag((value >> 16) as u16, value as u16))
        } else {
            StandardDataDictionary
                .by_name(part)
                .map(|entry| entry.tag())
        }
    });
    let mut nesting = Nesting::default();
    let mut last = tags.next()??;
    for tag in tags {
        nesting = nesting.below(last);
        last = tag?;
    }
    nesting.withholds(last).then_some(last)
}

/// `value` with its %XX escapes decoded (RFC 3986 2.1), as text.
fn percent_decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| value.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}{}", self.scheme, self.authority, self.path)?;
        if let Some(query) = &self.query {
            write!(f, "?{query}")?;
        }
        Ok(())
    }
}

/// RFC 3986 5.2.4.
fn remove_dot_segments(path: &str) -> String {
    let mut output: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            "." => {
                if index == last {
                    output.push("");
                }
            }
            ".." => {
                if output.len() > 1 {
                    output.pop();
                }
                if index == last {
                    output.push("");
                }
            }
            other => output.push(other),
        }
    }
    let joined = output.join("/");
    if path.starts_with('/') && !joined.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::Url;

    #[test]
    fn credentials_fragments_and_other_schemes_are_refused() {
        for text in [
            "https://user:secret@example.org/dicomweb",
            "https://example.org/dicomweb#x",
            "ftp://example.org/",
            "example.org/dicomweb",
            "https://exa mple.org/",
        ] {
            assert!(Url::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn reports_withhold_instance_uids_and_server_supplied_references() {
        let base = Url::parse("https://example.org/dicomweb").unwrap();
        let url = base.join(
            "/studies/1.2.987/series/1.2.988/instances/1.2.989/frames/1",
            None,
        );
        assert!(!url.reported().contains("1.2.98"));
        assert!(url.target().ends_with("/frames/1"));
        assert!(url.to_string().contains("1.2.989"));
        for uri in [
            "/PatientSentinel/bulk",
            "https://example.org/PatientSentinel?token=SecretSentinel",
        ] {
            let resolved = base.resolve(uri).unwrap();
            assert!(!resolved.reported().contains("Sentinel"));
            assert!(resolved.to_string().contains("PatientSentinel"));
        }
        let error = Url::parse("https://example.org/PatientSentinel bad").unwrap_err();
        assert!(!error.contains("PatientSentinel"));
    }

    #[test]
    fn references_resolve_per_rfc_3986() {
        let base = Url::parse("https://Example.org:443/dicomweb/studies/1.2/metadata").unwrap();
        for (reference, expected) in [
            (
                "./bulkdata/00282000",
                "https://example.org:443/dicomweb/studies/1.2/bulkdata/00282000",
            ),
            ("/other/x?y=1", "https://example.org:443/other/x?y=1"),
            ("../../a", "https://example.org:443/dicomweb/a"),
            ("http://elsewhere/x", "http://elsewhere/x"),
            ("//cdn.example.org/x", "https://cdn.example.org/x"),
        ] {
            assert_eq!(base.resolve(reference).unwrap().to_string(), expected);
        }
    }

    #[test]
    fn origins_compare_scheme_host_and_default_port() {
        let base = Url::parse("https://example.org/dicomweb").unwrap();
        assert!(base.same_origin(&Url::parse("https://EXAMPLE.org:443/x").unwrap()));
        assert!(!base.same_origin(&Url::parse("http://example.org/x").unwrap()));
        assert!(!base.same_origin(&Url::parse("https://example.org:8443/x").unwrap()));
        assert!(!base.same_origin(&Url::parse("https://evil.example/x").unwrap()));
        let local = Url::parse("http://127.0.0.1:8080/").unwrap();
        assert!(local.is_loopback() && !base.is_loopback());
        assert_eq!(
            base.join("/studies", Some("limit=1")).target(),
            "/dicomweb/studies?limit=1"
        );
    }

    /// A reported request line withholds the query values that match on
    /// identifying Attributes, by keyword, by tag or through a Sequence,
    /// while the request sent keeps them.
    #[test]
    fn reported_targets_withhold_identifying_query_values() {
        let base = Url::parse("https://example.org/dicomweb").unwrap();
        let url = base.join(
            "/studies",
            Some("StudyInstanceUID=1.2.3&PatientName=DOE%5EJANE&00100020=MRN12345&RequestAttributesSequence.AccessionNumber=A9&StudyDate=20260102-&includefield=00100010&limit=1&PatientID="),
        );
        let withheld = |count: usize| format!("(value withheld: identifying; {count} characters)");
        assert_eq!(
            url.target(),
            format!(
                "/dicomweb/studies?StudyInstanceUID={}&PatientName={}&00100020={}&RequestAttributesSequence.AccessionNumber={}&StudyDate={}&includefield=00100010&limit=1&PatientID=",
                withheld(5),
                withheld(8),
                withheld(8),
                withheld(2),
                withheld(9)
            )
        );
        assert!(url
            .to_string()
            .contains("PatientName=DOE%5EJANE&00100020=MRN12345"));
    }

    #[test]
    fn query_values_are_percent_encoded_except_unreserved_characters() {
        assert_eq!(
            super::encode_query_value("Doe^Jane \"\"\\é"),
            "Doe%5EJane%20%22%22%5C%C3%A9"
        );
        assert_eq!(super::encode_query_value("1.2-a_b~"), "1.2-a_b~");
    }

    #[test]
    fn segments_below_the_base_exclude_other_origins_and_paths() {
        let base = Url::parse("https://example.org/dicomweb").unwrap();
        let url = Url::parse("https://example.org/dicomweb/studies/1.2/series?x=1").unwrap();
        assert_eq!(
            url.segments_below(&base),
            Some(vec!["studies", "1.2", "series"])
        );
        let sibling = Url::parse("https://example.org/dicomwebx/studies").unwrap();
        assert_eq!(sibling.segments_below(&base), None);
        let other = Url::parse("https://other.example/dicomweb/studies").unwrap();
        assert_eq!(other.segments_below(&base), None);
    }
}
