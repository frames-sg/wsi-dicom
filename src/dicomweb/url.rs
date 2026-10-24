//! The absolute http(s) URLs the checker requests, and RFC 3986 5.2
//! resolution of the references servers return (BulkDataURI, Location).

use std::fmt;

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
}

impl Url {
    /// Parses an absolute URL. Credentials in the URL are refused: they would
    /// be logged with every request, so a token is passed by environment
    /// variable instead.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        if text.bytes().any(|byte| byte <= b' ' || byte >= 0x7F) {
            return Err(format!(
                "URL {text:?} contains whitespace, control or non-ASCII characters"
            ));
        }
        let (scheme, rest) = text
            .split_once("://")
            .ok_or_else(|| format!("URL {text:?} is not absolute (scheme://host/path)"))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!("URL {text:?} is not http or https"));
        }
        if rest.contains('#') {
            return Err(format!("URL {text:?} has a fragment"));
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
                "URL {text:?} carries userinfo; pass credentials with --bearer-token-env instead"
            ));
        }
        if authority.is_empty() {
            return Err(format!("URL {text:?} has no host"));
        }
        Ok(Self {
            scheme,
            authority: authority.to_ascii_lowercase(),
            path: path.to_string(),
            query,
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
    pub(crate) fn join(&self, suffix: &str, query: Option<&str>) -> Url {
        Url {
            scheme: self.scheme.clone(),
            authority: self.authority.clone(),
            path: format!("{}{suffix}", self.path.trim_end_matches('/')),
            query: query.map(str::to_string),
        }
    }

    /// The path and query, for reports.
    pub(crate) fn target(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{query}", self.path_or_root()),
            None => self.path_or_root().to_string(),
        }
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
            return Url::parse(reference);
        }
        if reference.starts_with("//") {
            return Url::parse(&format!("{}:{reference}", self.scheme));
        }
        let reference = reference.split('#').next().unwrap_or_default();
        let (path, query) = match reference.split_once('?') {
            Some((path, query)) => (path, Some(query.to_string())),
            None => (reference, None),
        };
        let merged = if path.is_empty() {
            return Ok(Url {
                query: query.or_else(|| self.query.clone()),
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
        })
    }
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
}
