//! The HTTP transport under the DICOMweb client: ureq with rustls when the
//! `dicomweb` feature is enabled. Without it every request fails with that
//! reason, so a file-validation build carries no network stack.

use super::client::Method;

/// Status, lowercased header fields and body of one response.
pub(super) type RawResponse = (u16, Vec<(String, String)>, Vec<u8>);

/// One request, as the client has prepared it. A build without the
/// transport never sends it.
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(super) struct Outgoing<'r> {
    pub(super) method: Method,
    pub(super) url: &'r str,
    pub(super) accept: Option<&'r str>,
    pub(super) content_type: Option<&'r str>,
    pub(super) authorization: Option<&'r str>,
    pub(super) body: &'r [u8],
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

    use super::{Method, Outgoing, RawResponse};
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
        ) -> Result<RawResponse, String> {
            let failure = |err| transport_error(err, max_response_bytes);
            let response = match outgoing.method {
                Method::Get => headers(self.agent.get(outgoing.url), outgoing).call(),
                Method::Options => headers(self.agent.options(outgoing.url), outgoing).call(),
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
                return Err(too_large(max_response_bytes));
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
        builder
    }

    fn transport_error(err: ureq::Error, max_response_bytes: u64) -> String {
        match err {
            ureq::Error::BodyExceedsLimit(_) => too_large(max_response_bytes),
            ureq::Error::Timeout(which) => format!("timed out ({which:?})"),
            other => other.to_string(),
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

    use super::{Outgoing, RawResponse};
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
        ) -> Result<RawResponse, String> {
            Err(UNAVAILABLE.to_string())
        }
    }
}
