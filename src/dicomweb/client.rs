//! The blocking HTTP client of the DICOMweb checks: per-request timeouts,
//! bounded responses, a request budget, same-origin redirects only where
//! PS3.18 names them, and a bearer token that is read from the environment,
//! sent only to the origin it was given for and never recorded.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::media::MediaType;
use super::multipart;
use super::report::DicomWebExchange;
use super::transport::{Failure, Outgoing, RawResponse, Transport};
use super::url::{shown_reference, Url};
use super::EDITION;
use crate::Error;

/// Redirect hops followed for one request.
const MAX_REDIRECTS: usize = 3;

/// HTTP settings of DICOMweb requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct DicomWebHttpOptions {
    /// Name of the environment variable holding a bearer token. The token
    /// is sent only to the origin of the base URL, never over plain HTTP to
    /// another host, and never written to a report or log.
    pub bearer_token_env: Option<String>,
    /// Timeout of each request, from connecting to the last body byte.
    pub timeout_secs: u64,
    /// Largest response body read; a larger one fails its check.
    pub max_response_bytes: u64,
    /// PEM file of the root certificates to trust instead of the Mozilla
    /// roots, such as an institution's private CA.
    pub ca_file: Option<PathBuf>,
}

impl Default for DicomWebHttpOptions {
    fn default() -> Self {
        Self {
            bearer_token_env: None,
            timeout_secs: 30,
            max_response_bytes: 64 * 1024 * 1024,
            ca_file: None,
        }
    }
}

/// A bearer token, kept out of every `Debug` output.
struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Options,
    Post,
    /// Only to ask for a 405 (Method Not Allowed); it changes nothing.
    Trace,
}

impl Method {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Options => "OPTIONS",
            Self::Post => "POST",
            Self::Trace => "TRACE",
        }
    }
}

/// Which redirects a request follows, to the same origin only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Redirects {
    /// None, as for a Store request, whose payload a redirect would drop.
    Never,
    /// 301 (Moved Permanently) and 303 (See Other), the redirections of
    /// PS3.18 Table 8.5-1.
    Listed,
    /// Also 302, with which "the origin server may return a redirection
    /// response ... to a rendered resource instead of returning a rendered
    /// image" (PS3.18 10.4.3.3.4).
    Thumbnail,
}

/// One request to send.
#[derive(Debug, Clone)]
pub(crate) struct Request<'r> {
    pub(crate) method: Method,
    pub(crate) url: Url,
    /// `None` sends no Accept header at all.
    pub(crate) accept: Option<&'r str>,
    pub(crate) content_type: Option<&'r str>,
    pub(crate) body: Option<&'r [u8]>,
    /// Send the body with Transfer-Encoding: chunked.
    pub(crate) chunked: bool,
    /// Further header fields. An Authorization field here replaces the
    /// bearer token.
    pub(crate) headers: &'r [(&'r str, &'r str)],
    pub(crate) redirects: Redirects,
}

impl<'r> Request<'r> {
    pub(crate) fn get(url: Url, accept: &'r str) -> Self {
        Self {
            method: Method::Get,
            url,
            accept: Some(accept),
            content_type: None,
            body: None,
            chunked: false,
            headers: &[],
            redirects: Redirects::Listed,
        }
    }

    /// `get` with further header fields.
    pub(crate) fn with_headers(mut self, headers: &'r [(&'r str, &'r str)]) -> Self {
        self.headers = headers;
        self
    }
}

/// A response as received.
#[derive(Debug, Clone)]
pub(crate) struct Exchange {
    pub(crate) method: Method,
    /// The URL that answered, after any redirects followed.
    pub(crate) url: Url,
    pub(crate) status: u16,
    /// Header fields in order, names lowercase.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// Redirects followed, as "status location".
    pub(crate) redirects: Vec<String>,
    /// A redirect PS3.18 does not allow here, or to another origin, which
    /// was not followed.
    pub(crate) unfollowed: Option<String>,
}

impl Exchange {
    /// The first value of header field `name` (lowercase).
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    /// "METHOD target", for messages.
    pub(crate) fn request_line(&self) -> String {
        format!("{} {}", self.method.name(), self.url.target())
    }

    /// The parsed Content-Type, if the response has one.
    pub(crate) fn media_type(&self) -> Option<Result<MediaType, String>> {
        self.header("content-type").map(MediaType::parse)
    }
}

/// Sends DICOMweb requests within a budget and keeps a record of each.
pub(crate) struct Client {
    transport: Transport,
    token: Option<Token>,
    /// The origin a token is sent to.
    credential_origin: Option<Url>,
    max_response_bytes: u64,
    max_requests: usize,
    sent: Cell<usize>,
    log: RefCell<Vec<DicomWebExchange>>,
}

impl Client {
    /// A client with `options`; a token, if named, is read now.
    pub(crate) fn new(options: &DicomWebHttpOptions) -> Result<Self, Error> {
        let invalid = |reason: String| Error::InvalidOptions { reason };
        if options.timeout_secs == 0 {
            return Err(invalid("timeout_secs must be at least 1".into()));
        }
        let transport = Transport::new(options.timeout_secs, options.ca_file.as_deref())?;
        let token = match &options.bearer_token_env {
            Some(name) => Some(Token(read_token(name).map_err(invalid)?)),
            None => None,
        };
        Ok(Self {
            transport,
            token,
            credential_origin: None,
            max_response_bytes: options.max_response_bytes,
            max_requests: usize::MAX,
            sent: Cell::new(0),
            log: RefCell::new(Vec::new()),
        })
    }

    /// Sends the token to `origin` only. A token over plain HTTP is refused
    /// unless the origin is the loopback interface.
    pub(crate) fn for_origin(mut self, origin: &Url) -> Result<Self, Error> {
        if self.token.is_some() && !origin.is_https() && !origin.is_loopback() {
            return Err(Error::InvalidOptions {
                reason: format!(
                    "refusing to send a bearer token over plain HTTP to {}; use https",
                    origin.host()
                ),
            });
        }
        self.credential_origin = Some(origin.clone());
        Ok(self)
    }

    /// Stops sending after `max_requests` requests.
    pub(crate) fn with_budget(mut self, max_requests: usize) -> Self {
        self.max_requests = max_requests;
        self
    }

    /// The record of every request sent so far.
    pub(crate) fn exchanges(&self) -> Vec<DicomWebExchange> {
        self.log.borrow().clone()
    }

    /// Sends `request`, following the redirects it allows to its own origin.
    pub(crate) fn send(&self, request: &Request<'_>) -> Result<Exchange, Failure> {
        let mut url = request.url.clone();
        let mut method = request.method;
        let mut followed = Vec::new();
        loop {
            let mut exchange = self.send_once(request, method, &url)?;
            exchange.redirects = followed.clone();
            let allowed = match request.redirects {
                Redirects::Never => false,
                Redirects::Listed => matches!(exchange.status, 301 | 303),
                Redirects::Thumbnail => matches!(exchange.status, 301..=303),
            };
            let is_redirect = (300..400).contains(&exchange.status) && exchange.status != 304;
            if !is_redirect {
                return Ok(exchange);
            }
            let Some(location) = exchange.header("location").map(str::to_string) else {
                return Ok(exchange);
            };
            let next = url.resolve(&location);
            let shown = shown_reference(&location);
            let reason = match &next {
                _ if !allowed => Some(format!(
                    "{} to {shown} is not followed: PS3.18 {EDITION} Table 8.5-1 names 301 and 303{}",
                    exchange.status,
                    if request.redirects == Redirects::Thumbnail { ", and 10.4.3.3.4 302 for a Thumbnail" } else { "" }
                )),
                Err(err) => Some(format!("{} Location {shown:?} is not a URL: {err}", exchange.status)),
                Ok(next) if !next.same_origin(&request.url) => Some(format!(
                    "{} to {} is not followed: it leaves the origin of the request",
                    exchange.status,
                    next.reported()
                )),
                Ok(_) if followed.len() == MAX_REDIRECTS => Some(format!(
                    "more than {MAX_REDIRECTS} redirects; {} to {shown} is not followed",
                    exchange.status
                )),
                Ok(_) => None,
            };
            if let Some(reason) = reason {
                exchange.unfollowed = Some(reason);
                return Ok(exchange);
            }
            let next = next.map_err(Failure::other)?;
            followed.push(format!("{} {}", exchange.status, next.reported()));
            if exchange.status == 303 {
                method = Method::Get;
            }
            url = next;
        }
    }

    fn send_once(
        &self,
        request: &Request<'_>,
        method: Method,
        url: &Url,
    ) -> Result<Exchange, Failure> {
        let sent = self.sent.get();
        if sent >= self.max_requests {
            return Err(Failure::other(format!(
                "the budget of {} requests is spent",
                self.max_requests
            )));
        }
        self.sent.set(sent + 1);
        let started = Instant::now();
        let result = self.call(request, method, url);
        let elapsed_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let record = match &result {
            Ok((status, headers, body)) => DicomWebExchange {
                method: method.name().to_string(),
                url: url.reported(),
                status: Some(*status),
                content_type: headers.iter().find(|(name, _)| name == "content-type").map(
                    |(_, value)| {
                        MediaType::parse(value).map_or_else(
                            |_| {
                                format!(
                                    "invalid Content-Type ({} characters)",
                                    value.chars().count()
                                )
                            },
                            |media| media.reported(),
                        )
                    },
                ),
                body_bytes: body.len() as u64,
                elapsed_millis,
                error: None,
            },
            Err(failure) => DicomWebExchange {
                method: method.name().to_string(),
                url: url.reported(),
                status: None,
                content_type: None,
                body_bytes: 0,
                elapsed_millis,
                // `Failure` maps transport errors to stable categories and URL
                // parsing errors already withhold server-provided references.
                error: Some(failure.reason.clone()),
            },
        };
        self.log.borrow_mut().push(record);
        let (status, headers, body) = result?;
        Ok(Exchange {
            method,
            url: url.clone(),
            status,
            headers,
            body,
            redirects: Vec::new(),
            unfollowed: None,
        })
    }

    /// The Authorization value for `url`: the token, for its origin only.
    fn authorization(&self, url: &Url) -> Option<String> {
        let token = self.token.as_ref()?;
        let origin = self.credential_origin.as_ref()?;
        origin
            .same_origin(url)
            .then(|| format!("Bearer {}", token.0))
    }

    fn call(
        &self,
        request: &Request<'_>,
        method: Method,
        url: &Url,
    ) -> Result<RawResponse, Failure> {
        let target = url.to_string();
        let replaced = request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        let authorization = self.authorization(url).filter(|_| !replaced);
        let outgoing = Outgoing {
            method,
            url: &target,
            accept: request.accept,
            content_type: request.content_type,
            authorization: authorization.as_deref(),
            headers: request.headers,
            body: request.body.unwrap_or_default(),
            chunked: request.chunked,
        };
        self.transport.call(&outgoing, self.max_response_bytes)
    }

    /// Fetches one Bulk Data Value: `uri` resolved against `base` (PS3.18
    /// 10.4.1.1.5.1) and retrieved as the Required multipart/related;
    /// type="application/octet-stream" (Table 10.4.4-1).
    pub(crate) fn fetch_bulkdata(&self, base: Option<&str>, uri: &str) -> Result<Vec<u8>, String> {
        let url = if uri.contains("://") {
            Url::parse(uri)?.server_reference()
        } else {
            let base = base.ok_or_else(|| {
                "a relative BulkDataURI needs the URL of the Metadata request (--bulkdata-base-url)"
                    .to_string()
            })?;
            Url::parse(base)?.resolve(uri)?
        };
        let exchange = self
            .send(&Request::get(url, BULKDATA_ACCEPT))
            .map_err(|failure| failure.reason)?;
        if exchange.status != 200 {
            return Err(format!(
                "{} answered {}",
                exchange.request_line(),
                exchange.status
            ));
        }
        single_bulk_value(&exchange)
    }
}

/// The Required media type of a Bulk Data resource (PS3.18 Table 10.4.4-1).
pub(crate) const BULKDATA_ACCEPT: &str = "multipart/related; type=\"application/octet-stream\"";

/// The one Value a Bulk Data response holds, single part or in one part.
pub(crate) fn single_bulk_value(exchange: &Exchange) -> Result<Vec<u8>, String> {
    let media_type = exchange
        .media_type()
        .ok_or("the response has no Content-Type")??;
    if media_type.essence == "application/octet-stream" {
        return Ok(exchange.body.clone());
    }
    if media_type.essence != "multipart/related" {
        return Err(format!(
            "the response is {}, not multipart/related",
            media_type.reported_essence()
        ));
    }
    let boundary = media_type
        .parameter("boundary")
        .ok_or("the multipart/related Content-Type has no boundary")?;
    let read = multipart::parse(&exchange.body, boundary);
    match read.parts.as_slice() {
        [part] => Ok(part.body.to_vec()),
        parts => Err(format!(
            "the response holds {} parts, not the one Bulk Data Value",
            parts.len()
        )),
    }
}

fn read_token(name: &str) -> Result<String, String> {
    match std::env::var(name) {
        Ok(token) if !token.trim().is_empty() => Ok(token.trim().to_string()),
        Ok(_) => Err(format!("environment variable {name} is empty")),
        Err(_) => Err(format!("environment variable {name} is not set")),
    }
}

#[cfg(all(test, feature = "dicomweb"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{Client, DicomWebHttpOptions, Request};
    use crate::dicomweb::report::Verdict;
    use crate::dicomweb::response::expect_status;
    use crate::dicomweb::tests::mock::{MockResponse, MockServer};
    use crate::dicomweb::url::Url;

    #[test]
    fn report_does_not_repeat_untrusted_response_or_transport_text() {
        const SENTINEL: &str = "PatientSentinel";
        let sent = Arc::new(AtomicUsize::new(0));
        let server = MockServer::start(move |_| {
            if sent.fetch_add(1, Ordering::SeqCst) == 0 {
                MockResponse::new(
                    500,
                    "application/PatientSentinel; boundary=PatientSentinel",
                    SENTINEL,
                )
            } else {
                MockResponse {
                    status: 200,
                    status_line: Some(format!("HTTP/1.1 {SENTINEL}")),
                    ..MockResponse::default()
                }
            }
        });
        let client = Client::new(&DicomWebHttpOptions::default()).unwrap();
        let url = Url::parse(&server.url("/dicomweb")).unwrap();
        let exchange = client
            .send(&Request::get(url.clone(), "application/dicom+json"))
            .unwrap();
        assert_eq!(exchange.body, SENTINEL.as_bytes());
        let mut verdict = Verdict::default();
        assert!(!expect_status(&mut verdict, &exchange, &[200], "8.5"));
        let check = verdict.finish("dicomweb-test", "PS3.18".into(), "passed");

        let failure = client
            .send(&Request::get(url, "application/dicom+json"))
            .unwrap_err();

        let report = serde_json::to_string(&(check, client.exchanges(), failure.reason)).unwrap();
        assert!(!report.contains(SENTINEL), "{report}");
        assert!(report.contains("an unrecognized media type"), "{report}");
        assert!(report.contains("Status Report payload"), "{report}");
        assert!(report.contains("not an HTTP/1.1 response"), "{report}");
    }
}
