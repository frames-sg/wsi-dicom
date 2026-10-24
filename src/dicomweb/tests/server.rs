//! The DICOMweb checks against an in-process origin server that serves an
//! exported slide conformingly, and broken one way at a time: each broken
//! response fails the check that holds the server to the requirement.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::mock::{MockRequest, MockResponse, MockServer};
use super::origin::{respond, Break, Slide, BASE};
use crate::dicomweb::{
    check_with_validator, DicomWebOptions, DicomWebReport, DicomWebStatus, DicomWebTransaction,
};
use crate::validation::{validate_dicom_path_with_runner, CommandOutcome, ValidationCommandRunner};
use crate::ValidationStatus;

/// A runner that finds no external validator, so a retrieved Instance gets
/// the intrinsic checks only.
struct NoTools;

impl ValidationCommandRunner for NoTools {
    fn find_command(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    fn run(
        &self,
        _program: &Path,
        _args: &[OsString],
        _timeout: Duration,
        _max_output_bytes: usize,
    ) -> Result<CommandOutcome, io::Error> {
        Err(io::Error::other("no external validator in these tests"))
    }
}

pub(super) struct Run {
    pub(super) report: DicomWebReport,
    pub(super) requests: Vec<MockRequest>,
}

impl Run {
    pub(super) fn check(&self, name: &str) -> &crate::dicomweb::DicomWebCheck {
        self.report
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no check {name}: {:#?}", self.report.checks))
    }

    pub(super) fn status(&self, name: &str) -> DicomWebStatus {
        self.check(name).status
    }

    pub(super) fn object_check(&self, name: &str) -> &crate::ValidationCheck {
        self.report
            .objects
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no object check {name}"))
    }
}

/// Runs the checks against a mock server holding `slide`, broken by `breaks`.
pub(super) fn run(
    slide: Arc<Slide>,
    breaks: &[Break],
    configure: impl FnOnce(&mut DicomWebOptions),
) -> Run {
    let breaks: Arc<BTreeSet<Break>> = Arc::new(breaks.iter().copied().collect());
    slide.reset();
    let server = {
        let slide = Arc::clone(&slide);
        let breaks = Arc::clone(&breaks);
        MockServer::start(move |request| respond(&slide, &breaks, request))
    };
    let mut options = DicomWebOptions {
        base_url: server.url(BASE),
        study: Some(slide.study.clone()),
        ..DicomWebOptions::default()
    };
    configure(&mut options);
    let report = check_with_validator(&options, &|path, validation| {
        validate_dicom_path_with_runner(path, validation, &NoTools)
    })
    .unwrap();
    Run {
        report,
        requests: server.requests(),
    }
}

pub(super) fn slide() -> (tempfile::TempDir, Arc<Slide>) {
    let root = tempfile::tempdir().unwrap();
    let slide = Arc::new(Slide::export(root.path()));
    (root, slide)
}

// --- the tests ------------------------------------------------------------

#[test]
fn a_conforming_origin_server_passes_every_required_check() {
    let (_root, slide) = slide();
    let run = run(Arc::clone(&slide), &[], |options| {
        options.probe_error_responses = true;
    });
    let unexpected: Vec<String> = run
        .report
        .checks
        .iter()
        .filter(|check| {
            let expected = match check.name.as_str() {
                "dicomweb-store" | "dicomweb-store-readback" => DicomWebStatus::NotRun,
                _ => DicomWebStatus::Passed,
            };
            check.status != expected
        })
        .map(|check| format!("{} {:?}: {}", check.name, check.status, check.message))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:#?}");
    // The default request budget leaves room for a server holding more
    // Bulk Data and a larger Series than the slide.
    let budget = DicomWebOptions::default().max_requests;
    assert!(
        run.requests.len() + 64 <= budget,
        "{} requests against a budget of {budget}",
        run.requests.len()
    );
    let failed: Vec<String> = run
        .report
        .objects
        .checks
        .iter()
        .filter(|check| check.status == ValidationStatus::Failed)
        .map(|check| format!("{}: {}", check.name, check.message))
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
    for name in [
        "intrinsic-wsi-dicom-json-model",
        "intrinsic-wsi-dicom-json-bulkdata",
        "intrinsic-wsi-dicom-iod-attribute-presence",
        "intrinsic-wsi-dicom-codestream",
        "intrinsic-wsi-dicom-icc-profile",
    ] {
        assert!(
            run.report
                .objects
                .checks
                .iter()
                .any(|check| check.name == name && check.status == ValidationStatus::Passed),
            "no passing {name}"
        );
    }
    assert_eq!(
        run.report.instance.as_deref(),
        Some(slide.instance.as_str())
    );
    assert!(!run.report.has_failures());
    // Each frames request names the Frames' own media type and syntax.
    assert!(run.requests.iter().any(|request| {
        request.path().ends_with("/frames/1,2")
            && request.header("accept").is_some_and(|accept| {
                accept.contains("type=\"image/jp2\"; transfer-syntax=1.2.840.10008.1.2.4.90")
            })
    }));
}

#[test]
fn each_broken_response_fails_the_check_that_requires_it() {
    let (_root, slide) = slide();
    for (broken, check, reason) in [
        (Break::CapabilitiesMissing, "dicomweb-capabilities", "405"),
        (
            Break::StudyMissingKey,
            "dicomweb-search-studies",
            "Patient's Birth Date",
        ),
        (
            Break::IncludefieldLeak,
            "dicomweb-search-studies",
            "shall not return it",
        ),
        (
            Break::NoPagingWarning,
            "dicomweb-search-paging",
            "Warning: 299",
        ),
        (Break::EmptyAs204, "dicomweb-search-matching", "204"),
        (
            Break::InvalidLimitAccepted,
            "dicomweb-search-parameters",
            "400 (Bad Request)",
        ),
        (
            Break::NoAcceptDefaults,
            "dicomweb-accept-required",
            "406 is required",
        ),
        (
            Break::FramesWrongMediaType,
            "dicomweb-retrieve-frames",
            "Table 8.7.3-5",
        ),
        (
            Break::FramesTruncated,
            "dicomweb-retrieve-frames",
            "truncated",
        ),
        (
            Break::NoContentLocation,
            "dicomweb-content-location",
            "no Content-Location",
        ),
        (
            Break::UncompressedWrongSize,
            "dicomweb-retrieve-frames-uncompressed",
            "uncompressed it is",
        ),
        (
            Break::MissingFrame200,
            "dicomweb-retrieve-frames-missing",
            "404 or 400 is required",
        ),
        (
            Break::BulkWrongLocation,
            "dicomweb-retrieve-bulkdata",
            "Content-Location",
        ),
        (
            Break::RenderedProgressive,
            "dicomweb-retrieve-rendered",
            "SOF2",
        ),
        (
            Break::RenderedWrongSize,
            "dicomweb-retrieve-rendered",
            "64x64 is required",
        ),
        (
            Break::ThumbnailPartial,
            "dicomweb-retrieve-thumbnail",
            "offers all",
        ),
        (
            Break::InstanceNotPart10,
            "dicomweb-retrieve-instance",
            "DICM",
        ),
        (
            Break::Unframed,
            "dicomweb-payload-headers",
            "neither Content-Length nor Transfer-Encoding",
        ),
        (
            Break::MetadataRedirect302,
            "dicomweb-retrieve-metadata",
            "is not followed",
        ),
        (
            Break::UnknownParameterRejected,
            "dicomweb-search-parameters",
            "wsidicomprobe=1 answered 400",
        ),
        (
            Break::PagingIgnoresOffset,
            "dicomweb-search-paging",
            "at offset 1 of the same ordered list",
        ),
        (
            Break::PagingIgnoresLimit,
            "dicomweb-search-paging",
            "at offset 0 of the same ordered list",
        ),
        (
            Break::EmptyNotArray,
            "dicomweb-search-matching",
            "it is an empty JSON array ([])",
        ),
        (
            Break::RenderedUnscaledWrongSize,
            "dicomweb-retrieve-rendered",
            "8.3.5.1.3",
        ),
        (
            Break::NoContentType,
            "dicomweb-payload-headers",
            "the payload has no Content-Type",
        ),
        (
            Break::MetadataNoContentLocation,
            "dicomweb-content-location",
            "though the payload is a representation of the Target Resource",
        ),
        (
            Break::FramesMislabelled,
            "dicomweb-retrieve-frames",
            "Content-Type is image/jp2; multipart/related is required",
        ),
        (
            Break::PartNoLength,
            "dicomweb-retrieve-frames",
            "has no Content-Length",
        ),
        (
            Break::FramesNoType,
            "dicomweb-retrieve-frames",
            "has no \"type\" parameter",
        ),
        (
            Break::FramesTwoSyntaxes,
            "dicomweb-retrieve-frames",
            "2 transfer-syntax parameters",
        ),
        (
            Break::InstanceRejectsWildcard,
            "dicomweb-retrieve-instance",
            "406",
        ),
        (Break::RenderedNoPng, "dicomweb-retrieve-rendered", "406"),
        (
            Break::RenderedWrongType,
            "dicomweb-retrieve-rendered",
            "Content-Type is application/octet-stream; image/jpeg is required",
        ),
        (
            Break::CapabilitiesNotWadl,
            "dicomweb-capabilities",
            "text/html",
        ),
        (Break::Capabilities304, "dicomweb-capabilities", "304"),
        (
            Break::CapabilitiesTwoApplications,
            "dicomweb-capabilities",
            "2 \"application\" elements",
        ),
        (
            Break::CapabilitiesNoBase,
            "dicomweb-capabilities",
            "\"base\" attribute",
        ),
        (
            Break::ThumbnailMultipart,
            "dicomweb-retrieve-thumbnail",
            "Content-Type is multipart/related; image/jpeg is required",
        ),
        (
            Break::SearchWrongMediaType,
            "dicomweb-search-studies",
            "application/dicom+json is required",
        ),
        (
            Break::RetrieveUrlMissing,
            "dicomweb-search-retrieve-url",
            "Study, Series, Instance results lack Retrieve URL",
        ),
        (
            Break::SeriesMissingKey,
            "dicomweb-search-series",
            "Series Number",
        ),
        (
            Break::InstanceMissingKey,
            "dicomweb-search-instances",
            "Instance Number",
        ),
        (
            Break::RenderedWithSyntax,
            "dicomweb-retrieve-rendered",
            "which a Rendered Media Type shall not have",
        ),
    ] {
        let run = run(Arc::clone(&slide), &[broken], |_| {});
        let found = run.check(check);
        assert_eq!(
            found.status,
            DicomWebStatus::Failed,
            "{broken:?}: {}",
            found.message
        );
        assert!(
            found.message.contains(reason),
            "{broken:?}: {}",
            found.message
        );
    }
}

#[test]
fn broken_metadata_and_frames_fail_the_object_checks() {
    let (_root, slide) = slide();
    for (broken, name, reason) in [
        (
            Break::MetadataFileMeta,
            "intrinsic-wsi-dicom-json-model",
            "10.4.1.1.2",
        ),
        (
            Break::MetadataLowercaseTag,
            "intrinsic-wsi-dicom-json-model",
            "lowercase",
        ),
        (
            Break::FramesCorrupted,
            "intrinsic-wsi-dicom-codestream",
            "Frame 1",
        ),
    ] {
        let run = run(Arc::clone(&slide), &[broken], |_| {});
        let check = run.object_check(name);
        assert_eq!(
            check.status,
            ValidationStatus::Failed,
            "{broken:?}: {}",
            check.message
        );
        assert!(
            check.message.contains(reason),
            "{broken:?}: {}",
            check.message
        );
        assert!(run.report.has_failures());
    }
}

#[test]
fn missing_optional_thumbnails_are_unsupported_not_failed() {
    let (_root, slide) = slide();
    let run = run(Arc::clone(&slide), &[Break::ThumbnailNone], |_| {});
    assert_eq!(
        run.status("dicomweb-retrieve-thumbnail"),
        DicomWebStatus::Unsupported
    );
    assert!(!run.report.has_failures(), "{:#?}", run.report.checks);
}

#[test]
fn store_runs_only_when_allowed_and_reads_the_instance_back() {
    let (root, slide) = slide();
    let file = root.path().join("store.dcm");
    std::fs::write(&file, &slide.file).unwrap();
    let skipped = run(Arc::clone(&slide), &[], |_| {});
    assert!(skipped
        .requests
        .iter()
        .all(|request| request.method != "POST"));
    assert_eq!(skipped.status("dicomweb-store"), DicomWebStatus::NotRun);

    let stored = run(Arc::clone(&slide), &[], |options| {
        options.store_file = Some(file.clone())
    });
    assert_eq!(
        stored.status("dicomweb-store"),
        DicomWebStatus::Passed,
        "{}",
        stored.check("dicomweb-store").message
    );
    assert_eq!(
        stored.status("dicomweb-store-readback"),
        DicomWebStatus::Passed
    );
    let post = stored
        .requests
        .iter()
        .find(|request| request.method == "POST")
        .unwrap();
    assert!(post
        .header("content-type")
        .is_some_and(|value| value.starts_with("multipart/related; type=\"application/dicom\"")));
    assert!(post
        .body
        .windows(slide.file.len())
        .any(|window| window == slide.file.as_slice()));

    for (broken, reason) in [
        (Break::StoreNoLocation, "no Location"),
        (Break::StoreNoContentLocation, "no Content-Location"),
        (Break::StoreNoModule, "no Store Instances Response Module"),
        (Break::StoreFailed, "Failed SOP Sequence"),
    ] {
        let run = run(Arc::clone(&slide), &[broken], |options| {
            options.store_file = Some(file.clone())
        });
        let check = run.check("dicomweb-store");
        assert_eq!(
            check.status,
            DicomWebStatus::Failed,
            "{broken:?}: {}",
            check.message
        );
        assert!(
            check.message.contains(reason),
            "{broken:?}: {}",
            check.message
        );
    }
}

/// A run that stores emits every check the requirement catalog credits in
/// the dicomweb scope, and no DICOMweb check the catalog does not know.
#[test]
fn a_run_emits_every_dicomweb_check_the_catalog_names() {
    let (root, slide) = slide();
    let file = root.path().join("store.dcm");
    std::fs::write(&file, &slide.file).unwrap();
    let run = run(Arc::clone(&slide), &[], |options| {
        options.store_file = Some(file.clone())
    });
    let emitted: BTreeSet<&str> = run
        .report
        .checks
        .iter()
        .map(|check| check.name.as_str())
        .chain(
            run.report
                .objects
                .checks
                .iter()
                .map(|check| check.name.as_str()),
        )
        .collect();
    for rule in crate::validation::DICOMWEB_RULES {
        assert!(emitted.contains(rule), "{rule} is not emitted");
    }
    for check in &run.report.checks {
        assert!(
            crate::validation::DICOMWEB_RULES.contains(&check.name.as_str()),
            "{} is not among the catalog's DICOMweb checks",
            check.name
        );
    }
}

#[test]
fn the_bearer_token_goes_to_the_base_origin_only_and_is_never_reported() {
    let (_root, slide) = slide();
    // PATH is set wherever tests run, so it stands in for a token without
    // mutating the environment of concurrent tests.
    let token = std::env::var("PATH").unwrap();
    let run = run(Arc::clone(&slide), &[], |options| {
        options.http.bearer_token_env = Some("PATH".into());
    });
    let expected = format!("Bearer {}", token.trim());
    assert!(run
        .requests
        .iter()
        .all(|request| request.header("authorization") == Some(expected.as_str())));
    let report = serde_json::to_string(&run.report).unwrap();
    assert!(!report.contains(token.trim()));
}

#[test]
fn a_token_is_refused_over_plain_http_to_another_host() {
    let mut options = DicomWebOptions {
        base_url: "http://dicomweb.example.org/dicomweb".into(),
        ..DicomWebOptions::default()
    };
    options.http.bearer_token_env = Some("PATH".into());
    let error = crate::dicomweb::check_dicomweb_server(&options).unwrap_err();
    assert!(error.to_string().contains("plain HTTP"));
}

#[test]
fn the_request_budget_and_response_limit_stop_checks_without_failing_them() {
    let (_root, slide) = slide();
    let limited = run(Arc::clone(&slide), &[], |options| options.max_requests = 2);
    assert!(limited.requests.len() <= 2);
    assert!(limited.report.checks.iter().any(|check| {
        check.status == DicomWebStatus::NotRun && check.message.contains("budget")
    }));
    let small = run(Arc::clone(&slide), &[], |options| {
        options.series = Some(slide.series.clone());
        options.instance = Some(slide.instance.clone());
        options.http.max_response_bytes = 64;
    });
    let metadata = small.check("dicomweb-retrieve-metadata");
    assert_eq!(
        metadata.status,
        DicomWebStatus::NotRun,
        "{}",
        metadata.message
    );
    assert!(metadata.message.contains("max_response_bytes=64"));
}

#[test]
fn a_request_that_outlives_its_timeout_is_not_run() {
    let server = MockServer::start(|_| MockResponse {
        delay: Some(Duration::from_secs(3)),
        ..MockResponse::status(200)
    });
    let mut options = DicomWebOptions {
        base_url: server.url(BASE),
        study: Some("1.2.3".into()),
        max_requests: 1,
        skip: vec![DicomWebTransaction::Search],
        ..DicomWebOptions::default()
    };
    options.http.timeout_secs = 1;
    let report = crate::dicomweb::check_dicomweb_server(&options).unwrap();
    let capabilities = report
        .checks
        .iter()
        .find(|check| check.name == "dicomweb-capabilities")
        .unwrap();
    assert_eq!(capabilities.status, DicomWebStatus::NotRun);
    assert!(
        capabilities.message.contains("timed out"),
        "{}",
        capabilities.message
    );
}

#[test]
fn skipped_transactions_send_no_request() {
    let (_root, slide) = slide();
    let run = run(Arc::clone(&slide), &[], |options| {
        options.skip = vec![
            DicomWebTransaction::Capabilities,
            DicomWebTransaction::Instance,
            DicomWebTransaction::Thumbnail,
        ];
    });
    assert!(run
        .requests
        .iter()
        .all(|request| request.method != "OPTIONS"
            && !request.path().ends_with("/thumbnail")
            && !request
                .path()
                .ends_with(&format!("/instances/{}", slide.instance))));
    assert_eq!(
        run.status("dicomweb-retrieve-instance"),
        DicomWebStatus::NotRun
    );
}
