//! The HTTP transport under the DICOMweb client: ureq with rustls when the
//! `dicomweb` feature is enabled. Without it every request fails with that
//! reason, so a file-validation build carries no network stack.

use std::fmt;

use super::client::Method;

/// Status, lowercased header fields and body of one response.
pub(super) type RawResponse = (u16, Vec<(String, String)>, Vec<u8>);

/// What the origin server did wrong when a request brought no response: the
/// faults of PS3.18 8.1.2, which "shall always return a response message"
/// with "a valid 3-digit status code" (8.1.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) enum Fault {
    /// The response is not an HTTP/1.1 response message, such as a status
    /// line without a valid 3-digit status code.
    Malformed,
    /// The connection closed or was reset before a complete response.
    Closed,
}

/// Why a request brought no usable response.
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    pub(crate) reason: String,
    /// The origin server's fault, if it is one; `None` for a timeout, a
    /// size limit, an unreachable host or a TLS failure.
    pub(crate) fault: Option<Fault>,
}

impl Failure {
    pub(super) fn other(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            fault: None,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

/// One request, as the client has prepared it. A build without the
/// transport never sends it.
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(super) struct Outgoing<'r> {
    pub(super) method: Method,
    pub(super) url: &'r str,
    pub(super) accept: Option<&'r str>,
    pub(super) content_type: Option<&'r str>,
    pub(super) authorization: Option<&'r str>,
    /// Further header fields, such as Accept-Charset or Range.
    pub(super) headers: &'r [(&'r str, &'r str)],
    pub(super) body: &'r [u8],
    /// Send the body with Transfer-Encoding: chunked rather than
    /// Content-Length.
    pub(super) chunked: bool,
}

#[cfg(feature = "dicomweb")]
pub(super) use enabled::Transport;

#[cfg(not(feature = "dicomweb"))]
pub(super) use disabled::Transport;

#[cfg(feature = "dicomweb")]
mod enabled {
    use std::path::Path;
    use std::time::Duration;

    use ureq::tls::{PemItem, RootCerts, TlsConfig, TlsProvider};
    use ureq::{Agent, RequestBuilder};

    use super::{Failure, Fault, Method, Outgoing, RawResponse};
    use crate::Error;

    /// Bytes of a CA bundle read before it is refused.
    const MAX_CA_FILE_BYTES: u64 = 4 * 1024 * 1024;

    /// The message of a response body larger than the policy admits.
    fn too_large(max_response_bytes: u64) -> String {
        format!("the response body exceeds max_response_bytes={max_response_bytes}")
    }

    /// A blocking HTTP/1.1 agent that never follows redirects itself and
    /// reports every status as a response.
    pub(in crate::dicomweb) struct Transport {
        agent: Agent,
    }

    impl Transport {
        pub(in crate::dicomweb) fn new(
            timeout_secs: u64,
            ca_file: Option<&Path>,
        ) -> Result<Self, Error> {
            let roots = match ca_file {
                Some(path) => RootCerts::new_with_certs(
                    &read_ca_file(path).map_err(|reason| Error::InvalidOptions { reason })?,
                ),
                None => RootCerts::WebPki,
            };
            let tls = TlsConfig::builder()
                .provider(TlsProvider::Rustls)
                .root_certs(roots)
                .build();
            let config = Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(Duration::from_secs(timeout_secs)))
                .user_agent(concat!("wsi-dicom/", env!("CARGO_PKG_VERSION")))
                .accept("")
                .tls_config(tls)
                .build();
            Ok(Self {
                agent: Agent::new_with_config(config),
            })
        }

        pub(in crate::dicomweb) fn call(
            &self,
            outgoing: &Outgoing<'_>,
            max_response_bytes: u64,
        ) -> Result<RawResponse, Failure> {
            let failure = |err| transport_error(err, max_response_bytes);
            let response = match outgoing.method {
                Method::Get => headers(self.agent.get(outgoing.url), outgoing).call(),
                Method::Options => headers(self.agent.options(outgoing.url), outgoing).call(),
                Method::Trace => headers(self.agent.trace(outgoing.url), outgoing).call(),
                Method::Post if outgoing.chunked => {
                    let mut body = outgoing.body;
                    headers(self.agent.post(outgoing.url), outgoing)
                        .send(ureq::SendBody::from_reader(&mut body))
                }
                Method::Post => {
                    headers(self.agent.post(outgoing.url), outgoing).send(outgoing.body)
                }
            };
            let mut response = response.map_err(failure)?;
            let status = response.status().as_u16();
            let fields = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_ascii_lowercase(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect();
            // LimitReader fails on the read after its limit, even at the end
            // of the body, so one byte more than the policy is admitted and
            // checked.
            let body = response
                .body_mut()
                .with_config()
                .limit(max_response_bytes.saturating_add(1))
                .read_to_vec()
                .map_err(failure)?;
            if body.len() as u64 > max_response_bytes {
                return Err(Failure::other(too_large(max_response_bytes)));
            }
            Ok((status, fields, body))
        }
    }

    fn headers<B>(mut builder: RequestBuilder<B>, outgoing: &Outgoing<'_>) -> RequestBuilder<B> {
        if let Some(accept) = outgoing.accept {
            builder = builder.header("Accept", accept);
        }
        if let Some(content_type) = outgoing.content_type {
            builder = builder.header("Content-Type", content_type);
        }
        if let Some(authorization) = outgoing.authorization {
            builder = builder.header("Authorization", authorization);
        }
        for (name, value) in outgoing.headers {
            builder = builder.header(*name, *value);
        }
        builder
    }

    fn transport_error(err: ureq::Error, max_response_bytes: u64) -> Failure {
        let fault = |fault, reason: String| Failure {
            reason,
            fault: Some(fault),
        };
        match err {
            ureq::Error::BodyExceedsLimit(_) => Failure::other(too_large(max_response_bytes)),
            ureq::Error::Timeout(which) => Failure::other(format!("timed out ({which:?})")),
            ureq::Error::Protocol(err) => fault(
                Fault::Malformed,
                format!("the response is not an HTTP/1.1 response message: {err}"),
            ),
            ureq::Error::Http(err) => fault(
                Fault::Malformed,
                format!("the response is not an HTTP/1.1 response message: {err}"),
            ),
            ureq::Error::Io(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                fault(
                    Fault::Closed,
                    format!("the connection closed before a complete response: {err}"),
                )
            }
            other => Failure::other(other.to_string()),
        }
    }

    fn read_ca_file(path: &Path) -> Result<Vec<ureq::tls::Certificate<'static>>, String> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(MAX_CA_FILE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|err| format!("cannot read CA file {}: {err}", path.display()))?;
        if bytes.len() as u64 > MAX_CA_FILE_BYTES {
            return Err(format!(
                "CA file {} exceeds {MAX_CA_FILE_BYTES} bytes",
                path.display()
            ));
        }
        let mut certificates = Vec::new();
        for item in ureq::tls::parse_pem(&bytes) {
            match item {
                Ok(PemItem::Certificate(certificate)) => certificates.push(certificate),
                Ok(_) => {}
                Err(err) => return Err(format!("CA file {} is not PEM: {err}", path.display())),
            }
        }
        if certificates.is_empty() {
            return Err(format!("CA file {} holds no certificate", path.display()));
        }
        Ok(certificates)
    }
}

#[cfg(not(feature = "dicomweb"))]
mod disabled {
    use std::path::Path;

    use super::{Failure, Outgoing, RawResponse};
    use crate::Error;

    const UNAVAILABLE: &str =
        "this wsi-dicom build has no DICOMweb transport; build it with `--features dicomweb`";

    /// The transport of a build without the `dicomweb` feature: it cannot
    /// be created, so no request is ever sent.
    pub(in crate::dicomweb) struct Transport {
        _unconstructible: (),
    }

    impl Transport {
        pub(in crate::dicomweb) fn new(
            _timeout_secs: u64,
            _ca_file: Option<&Path>,
        ) -> Result<Self, Error> {
            Err(Error::Unsupported {
                reason: UNAVAILABLE.to_string(),
            })
        }

        pub(in crate::dicomweb) fn call(
            &self,
            _outgoing: &Outgoing<'_>,
            _max_response_bytes: u64,
        ) -> Result<RawResponse, Failure> {
            Err(Failure::other(UNAVAILABLE))
        }
    }
}
