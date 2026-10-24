//! DICOMweb origin server conformance for the transactions a whole-slide
//! viewer depends on (PS3.18 of the pinned edition): Retrieve Capabilities
//! (8.9), Search (10.6), Retrieve of Metadata, Frames, Bulk Data, Rendered
//! and Thumbnail resources and Instances (10.4), and, only when asked, Store
//! (10.5). Every check cites the sections it holds the server to; an
//! optional capability the server lacks is reported as unsupported, not as a
//! failure. What is retrieved is validated as a file would be: the Metadata
//! as DICOM JSON and as Data Sets, the Frames' codestreams, an Instance.

mod audit;
mod capabilities;
mod client;
mod frames;
mod instance;
mod media;
mod multipart;
mod negotiation;
mod probes;
mod queries;
mod rendered;
mod rendered_parameters;
mod report;
mod response;
mod retrieve;
mod search;
mod store;
mod syntaxes;
mod transport;
mod url;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use serde::{Deserialize, Serialize};

use audit::{Audit, Resource};
pub(crate) use client::Client;
pub use client::DicomWebHttpOptions;
use client::{Exchange, Request};
use report::Verdict;
pub use report::{DicomWebCheck, DicomWebExchange, DicomWebReport, DicomWebStatus};
use response::sec;
pub(crate) use url::Url;

use crate::{
    Error, OptionalConformance, ValidationCheck, ValidationOptions, ValidationReport, DICOM_EDITION,
};

/// The DICOM edition every citation names.
pub(crate) const EDITION: &str = DICOM_EDITION;

/// A group of transactions that can be left out of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DicomWebTransaction {
    /// Retrieve Capabilities (OPTIONS on the Base URI).
    Capabilities,
    /// Study, Series and Instance searches.
    Search,
    /// Series and Instance Metadata, JSON and XML.
    Metadata,
    /// Frames, compressed and uncompressed.
    Frames,
    /// Bulk Data named by BulkDataURI.
    Bulkdata,
    /// Rendered Frames.
    Rendered,
    /// Thumbnails.
    Thumbnail,
    /// Retrieve of a whole Instance, which can be large.
    Instance,
}

/// Options for a DICOMweb origin server conformance run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct DicomWebOptions {
    /// The Base URI of the Studies Service, e.g. `https://host/dicomweb`.
    pub base_url: String,
    /// The Study to target; found by searching for a slide (Modalities in
    /// Study SM) when absent.
    pub study: Option<String>,
    /// The Series to target; the first SM Series when absent.
    pub series: Option<String>,
    /// The Instance to target; the VL Whole Slide Microscopy Instance with
    /// the fewest Frames beyond one when absent.
    pub instance: Option<String>,
    /// Frame Numbers to retrieve; Frames 1 and 2 when empty.
    pub frames: Vec<u32>,
    /// Timeouts, limits, credentials and trust anchors.
    pub http: DicomWebHttpOptions,
    /// Requests sent at most; checks that would exceed it are not run. A
    /// full run against a conforming server sends about 100.
    pub max_requests: usize,
    /// Transactions left out.
    pub skip: Vec<DicomWebTransaction>,
    /// A PS3.10 file to store with STOW-RS and read back. Store changes the
    /// server's state, so it runs only when this is set.
    pub store_file: Option<PathBuf>,
    /// Send the two requests that ask for a 401 and a 405 (PS3.18 Table
    /// 8.5-1): one with a bearer token no server issued, and a TRACE. Off by
    /// default: a bad credential can raise alerts or lock an account, and
    /// security scanners flag TRACE.
    pub probe_error_responses: bool,
    /// How the retrieved objects are validated.
    pub validation: ValidationOptions,
}

impl Default for DicomWebOptions {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            study: None,
            series: None,
            instance: None,
            frames: Vec::new(),
            http: DicomWebHttpOptions::default(),
            max_requests: 192,
            skip: Vec::new(),
            store_file: None,
            probe_error_responses: false,
            validation: ValidationOptions::default(),
        }
    }
}

/// Validates a retrieved PS3.10 file as `validate_dicom_path` does.
type FileValidator<'v> = dyn Fn(&Path, &ValidationOptions) -> Result<ValidationReport, Error> + 'v;

/// Check a DICOMweb origin server against PS3.18 for the transactions a
/// whole-slide viewer uses, and validate what it returns.
pub fn check_dicomweb_server(options: &DicomWebOptions) -> Result<DicomWebReport, Error> {
    check_with_validator(options, &|path, validation| {
        crate::validate_dicom_path(path, validation)
    })
}

/// `check_dicomweb_server` with the file validation of a retrieved Instance
/// given, so that tests run it without external validators.
fn check_with_validator(
    options: &DicomWebOptions,
    validate_file: &FileValidator<'_>,
) -> Result<DicomWebReport, Error> {
    let invalid = |reason: String| Error::InvalidOptions { reason };
    let base = Url::parse(options.base_url.trim_end_matches('/')).map_err(invalid)?;
    if base.has_query() {
        return Err(invalid(format!(
            "base URL {} has a query; the Base URI of a service has none (PS3.18 {EDITION} 8.2)",
            crate::validation::disclosure::withheld(&base.to_string())
        )));
    }
    let client = Client::new(&options.http)?
        .for_origin(&base)?
        .with_budget(options.max_requests);
    let mut session = Session {
        client: &client,
        base: base.clone(),
        options,
        checks: Vec::new(),
        audit: Audit::default(),
        objects: Vec::new(),
        optional: Vec::new(),
        conventions: None,
        files: Vec::new(),
        retrieved: false,
        validate_file,
        capabilities: None,
        served: BTreeSet::new(),
        syntaxes: None,
        instance_metadata: None,
    };
    let mut target = Target {
        study: options.study.clone(),
        series: options.series.clone(),
        instance: options.instance.clone(),
        ..Target::default()
    };
    capabilities::run(&mut session);
    search::run(&mut session, &mut target);
    let metadata = retrieve::series_metadata(&mut session, &mut target);
    let held = metadata
        .as_ref()
        .zip(target.instance.as_deref())
        .and_then(|(metadata, uid)| metadata.object(uid));
    queries::run(&mut session, &target, held);
    let fetched = retrieve::bulkdata(&mut session, metadata.as_ref());
    let object = metadata
        .as_ref()
        .zip(target.instance.as_deref())
        .and_then(|(metadata, uid)| metadata.object_with(uid, &fetched));
    let pixel_uri = metadata
        .as_ref()
        .zip(target.instance.as_deref())
        .and_then(|(metadata, uid)| metadata.pixel_data_uri(uid));
    if let Some(syntax) = frames::run(&mut session, &target, object.as_ref(), pixel_uri.as_ref()) {
        if let Some(uid) = &target.instance {
            target.syntaxes.insert(uid.clone(), syntax);
        }
    }
    retrieve::validate_metadata(&mut session, metadata.as_ref(), &fetched, &target);
    retrieve::instance_metadata(&mut session, &target, metadata.as_ref());
    negotiation::run(&mut session, &target);
    syntaxes::run(&mut session, &target, object.as_ref());
    rendered::run(&mut session, &target, object.as_ref());
    rendered_parameters::run(&mut session, &target, object.as_ref());
    rendered::thumbnails(&mut session, &target);
    let held = instance::Held {
        metadata: metadata.as_ref(),
        fetched: &fetched,
    };
    instance::retrieve(&mut session, &target, &held);
    probes::range(&mut session, &target);
    probes::errors(&mut session, &target);
    store::run(&mut session);
    search::retrieve_url_check(&mut session, &target);
    let audit = std::mem::take(&mut session.audit).finish();
    syntaxes::finish(&mut session, audit.forbidden);
    capabilities::finish(&mut session);
    session.checks.extend(audit.checks);
    let Session {
        checks,
        objects,
        optional,
        conventions,
        files,
        ..
    } = session;
    let conventions = conventions.unwrap_or_else(|| {
        crate::validation::conventions_not_evaluated(
            &options.validation,
            &PathBuf::from(base.to_string()),
            "the run retrieved no Series Metadata, so no convention of the profile was evaluated",
        )
    });
    Ok(DicomWebReport {
        base_url: base.to_string(),
        study: target
            .study
            .as_deref()
            .map(|value| crate::validation::disclosure::withheld(value).to_string()),
        series: target
            .series
            .as_deref()
            .map(|value| crate::validation::disclosure::withheld(value).to_string()),
        instance: target
            .instance
            .as_deref()
            .map(|value| crate::validation::disclosure::withheld(value).to_string()),
        checks,
        objects: ValidationReport {
            profile: options.validation.profile,
            input: PathBuf::from(base.to_string()),
            files,
            checks: objects,
            optional,
            conventions,
        },
        exchanges: client.exchanges(),
    })
}

/// The Study, Series and Instance the transactions target, and what the
/// Search said about them.
#[derive(Debug, Clone, Default)]
struct Target {
    study: Option<String>,
    series: Option<String>,
    instance: Option<String>,
    /// Number of Frames of the Instance.
    frames: Option<u64>,
    /// Transfer Syntax UIDs by SOP Instance UID: a single Available Transfer
    /// Syntax UID a Search returned, or the one a Frames response named.
    syntaxes: BTreeMap<String, String>,
    /// Whether any Search result was read.
    searched: bool,
    /// Levels with a Search result that lacks Retrieve URL (0008,1190).
    missing_retrieve_url: Vec<String>,
    /// The Search results of the Study, Series and Instance targeted, for
    /// the matching checks that query by their values.
    study_result: Option<InMemDicomObject>,
    series_result: Option<InMemDicomObject>,
    instance_result: Option<InMemDicomObject>,
}

impl Target {
    fn note_retrieve_url(&mut self, level: &str, result: &InMemDicomObject) {
        self.searched = true;
        let missing = result.get(tags::RETRIEVE_URL).is_none();
        if missing && !self.missing_retrieve_url.iter().any(|known| known == level) {
            self.missing_retrieve_url.push(level.to_string());
        }
    }

    fn series_path(&self) -> Option<String> {
        Some(format!(
            "/studies/{}/series/{}",
            self.study.as_ref()?,
            self.series.as_ref()?
        ))
    }

    fn instance_path(&self) -> Option<String> {
        Some(format!(
            "{}/instances/{}",
            self.series_path()?,
            self.instance.as_ref()?
        ))
    }
}

/// One conformance run in progress.
struct Session<'s> {
    client: &'s Client,
    base: Url,
    options: &'s DicomWebOptions,
    checks: Vec<DicomWebCheck>,
    audit: Audit,
    /// The intrinsic checks of what was retrieved.
    objects: Vec<ValidationCheck>,
    optional: Vec<OptionalConformance>,
    /// The convention profiles' results on the Series Metadata.
    conventions: Option<crate::validation::ConventionReport>,
    /// The URLs whose payloads were validated as objects.
    files: Vec<PathBuf>,
    /// Whether a Retrieve of the Study's resources answered 200.
    retrieved: bool,
    validate_file: &'s FileValidator<'s>,
    /// The Retrieve Capabilities check, finished once the run knows which
    /// resources the origin server serves.
    capabilities: Option<capabilities::Pending>,
    /// The resource path segments (Annex H Table H-1) that answered 200.
    served: BTreeSet<&'static str>,
    /// The Transfer Syntax probes, finished with the run's forbidden uses.
    syntaxes: Option<Verdict>,
    /// The Instance Metadata as first retrieved, for the Range check.
    instance_metadata: Option<Vec<u8>>,
}

impl Session<'_> {
    fn skipped(&self, transaction: DicomWebTransaction) -> bool {
        self.options.skip.contains(&transaction)
    }

    /// The validation of a retrieved or stored file: its DICOM checks only,
    /// since the conventions run on the Series Metadata that lists it.
    fn file_options(&self) -> ValidationOptions {
        let mut options = self.options.validation.clone();
        options.conventions.clear();
        options.fail_on_conventions = false;
        options
    }

    fn url(&self, path: &str, query: Option<&str>) -> Url {
        self.base.join(path, query)
    }

    fn push(&mut self, check: DicomWebCheck) {
        self.checks.push(check);
    }

    /// Records check `name` as not run.
    fn not_run(&mut self, name: &str, sections: &str, reason: &str) {
        let mut verdict = Verdict::default();
        verdict.not_run(reason);
        self.push(verdict.finish(name, sec(sections), ""));
    }

    /// Sends `request` for the check of `verdict`, auditing the payload's
    /// header fields. A request that brings no response leaves the check not
    /// run, with why.
    fn send(&mut self, verdict: &mut Verdict, request: &Request<'_>) -> Option<Exchange> {
        let line = format!("{} {}", request.method.name(), request.url.target());
        verdict.request(line.clone());
        match self.client.send(request) {
            Ok(exchange) => {
                let resource = resource_of(&exchange, &self.base);
                self.audit.response(&exchange, resource);
                if !exchange.redirects.is_empty() {
                    verdict.note(format!(
                        "{} redirected: {}",
                        request.url.target(),
                        exchange.redirects.join(", ")
                    ));
                }
                Some(exchange)
            }
            Err(failure) => {
                self.audit.failure(&line, &failure);
                verdict.not_run(format!("{line}: {failure}"));
                None
            }
        }
    }

    /// Records that the resources of `segment` (Table H-1) answered 200.
    fn served(&mut self, segment: &'static str) {
        self.served.insert(segment);
    }
}

/// The kind of resource a request targeted.
fn resource_of(exchange: &Exchange, base: &Url) -> Resource {
    if exchange.method != client::Method::Get {
        return Resource::Other;
    }
    match exchange.url.segments_below(base).as_deref() {
        Some(
            ["studies"]
            | ["series"]
            | ["instances"]
            | ["studies", _, "series"]
            | ["studies", _, "instances"]
            | ["studies", _, "series", _, "instances"],
        ) => Resource::Search,
        Some(["studies", _, "series", _, "instances", _]) => Resource::Instance,
        _ => Resource::Retrieve,
    }
}
