//! The DICOMweb checks against an in-process origin server that serves an
//! exported slide conformingly, and broken one way at a time: each broken
//! response fails the check that holds the server to the requirement.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dicom_dictionary_std::tags;

use super::fixture::{export_wsi, frames, open, to_json};
use super::mock::{MockRequest, MockResponse, MockServer};
use crate::dicomweb::{
    check_with_validator, DicomWebOptions, DicomWebReport, DicomWebStatus, DicomWebTransaction,
};
use crate::validation::{validate_dicom_path_with_runner, CommandOutcome, ValidationCommandRunner};
use crate::{TransferSyntax, ValidationStatus};

const BASE: &str = "/dicomweb";
const J2K_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
const ANNOTATION_CLASS: &str = "1.2.840.10008.5.1.4.1.1.91.1";

/// One way the origin server departs from PS3.18.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Break {
    CapabilitiesMissing,
    StudyMissingKey,
    IncludefieldLeak,
    NoPagingWarning,
    EmptyAs204,
    InvalidLimitAccepted,
    NoAcceptDefaults,
    MetadataFileMeta,
    MetadataLowercaseTag,
    FramesWrongMediaType,
    FramesTruncated,
    FramesCorrupted,
    NoContentLocation,
    UncompressedWrongSize,
    MissingFrame200,
    BulkWrongLocation,
    RenderedProgressive,
    RenderedWrongSize,
    ThumbnailNone,
    ThumbnailPartial,
    InstanceNotPart10,
    StoreNoLocation,
    StoreFailed,
    Unframed,
    MetadataRedirect302,
}

/// The slide the mock origin server holds.
struct Slide {
    study: String,
    series: String,
    instance: String,
    class: String,
    file: Vec<u8>,
    frames: Vec<Vec<u8>>,
    rows: u32,
    columns: u32,
    icc: Vec<u8>,
    metadata: String,
}

impl Slide {
    fn export(root: &Path) -> Self {
        let path = export_wsi(root, "served", (4, 2, 2), TransferSyntax::Jpeg2000Lossless);
        let object = open(&path);
        let text = |tag| {
            object
                .get(tag)
                .and_then(|element| element.to_str().ok())
                .map(|value| value.trim_end_matches(['\0', ' ']).to_string())
                .unwrap()
        };
        let number = |tag| {
            object
                .get(tag)
                .and_then(|element| element.to_int::<u32>().ok())
                .unwrap()
        };
        let instance = text(tags::SOP_INSTANCE_UID);
        let icc = icc_profile(&object).expect("the export embeds an ICC Profile");
        let metadata = to_json(&object, &|tag, top| match tag {
            tags::ICC_PROFILE => Some(format!("instances/{instance}/bulk/icc")),
            tags::PIXEL_DATA if top => Some(format!("instances/{instance}/bulk/pixeldata")),
            _ => None,
        });
        Self {
            study: text(tags::STUDY_INSTANCE_UID),
            series: text(tags::SERIES_INSTANCE_UID),
            class: text(tags::SOP_CLASS_UID),
            rows: number(tags::ROWS),
            columns: number(tags::COLUMNS),
            frames: frames(&object),
            file: std::fs::read(&path).unwrap(),
            icc,
            metadata,
            instance,
        }
    }
}

/// The first ICC Profile (0028,2000), at the top level or in an Optical Path
/// Sequence Item.
fn icc_profile(object: &dicom_object::InMemDicomObject) -> Option<Vec<u8>> {
    if let Some(element) = object.get(tags::ICC_PROFILE) {
        return element.to_bytes().ok().map(|bytes| bytes.to_vec());
    }
    object
        .get(tags::OPTICAL_PATH_SEQUENCE)?
        .items()?
        .iter()
        .find_map(icc_profile)
}

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

struct Run {
    report: DicomWebReport,
    requests: Vec<MockRequest>,
}

impl Run {
    fn check(&self, name: &str) -> &crate::dicomweb::DicomWebCheck {
        self.report
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no check {name}: {:#?}", self.report.checks))
    }

    fn status(&self, name: &str) -> DicomWebStatus {
        self.check(name).status
    }

    fn object_check(&self, name: &str) -> &crate::ValidationCheck {
        self.report
            .objects
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("no object check {name}"))
    }
}

/// Runs the checks against a mock server holding `slide`, broken by `breaks`.
fn run(slide: Arc<Slide>, breaks: &[Break], configure: impl FnOnce(&mut DicomWebOptions)) -> Run {
    let breaks: Arc<BTreeSet<Break>> = Arc::new(breaks.iter().copied().collect());
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

fn slide() -> (tempfile::TempDir, Arc<Slide>) {
    let root = tempfile::tempdir().unwrap();
    let slide = Arc::new(Slide::export(root.path()));
    (root, slide)
}

// --- the mock origin server ---------------------------------------------

fn json(body: String) -> MockResponse {
    MockResponse::new(200, "application/dicom+json", body)
}

/// A multipart/related response of `parts` (Content-Type, Content-Location,
/// payload).
fn multipart(
    root: &str,
    parts: Vec<(String, Option<String>, Vec<u8>)>,
    truncated: bool,
) -> MockResponse {
    let boundary = "mock-boundary-7f3a";
    let mut body = Vec::new();
    for (content_type, location, payload) in parts {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        if let Some(location) = location {
            body.extend_from_slice(format!("Content-Location: {location}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&payload);
        body.extend_from_slice(b"\r\n");
    }
    if !truncated {
        body.extend_from_slice(format!("--{boundary}--").as_bytes());
    }
    MockResponse::new(
        200,
        &format!("multipart/related; type=\"{root}\"; boundary={boundary}"),
        body,
    )
}

fn respond(slide: &Slide, breaks: &BTreeSet<Break>, request: &MockRequest) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let Some(path) = request.path().strip_prefix(BASE) else {
        return MockResponse::status(404);
    };
    let origin = format!("http://{}", request.header("host").unwrap_or("127.0.0.1"));
    let base = format!("{origin}{BASE}");
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let mut response = match (request.method.as_str(), segments.as_slice()) {
        ("OPTIONS", [""]) | ("OPTIONS", []) => {
            capabilities(&base, broken(Break::CapabilitiesMissing))
        }
        ("POST", ["studies"]) => store(slide, &base, breaks),
        ("GET", _) => retrieve(slide, &origin, breaks, request, &segments),
        _ => MockResponse::status(405).with("Allow", "GET"),
    };
    if broken(Break::Unframed) {
        response.unframed = true;
    }
    response
}

fn capabilities(base: &str, missing: bool) -> MockResponse {
    if missing {
        return MockResponse::status(405).with("Allow", "GET, POST");
    }
    MockResponse::new(
        200,
        "application/vnd.sun.wadl+xml",
        format!("<?xml version=\"1.0\"?><application xmlns=\"http://wadl.dev.java.net/2009/02\"><resources base=\"{base}/\"><resource path=\"studies\"/></resources></application>"),
    )
}

fn retrieve(
    slide: &Slide,
    origin: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    segments: &[&str],
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let base = &format!("{origin}{BASE}");
    let accept = request.header("accept").unwrap_or_default();
    let instance_base = format!(
        "{base}/studies/{}/series/{}/instances/{}",
        slide.study, slide.series, slide.instance
    );
    match segments {
        ["studies"] => {
            if request.header("accept").is_none() && !broken(Break::NoAcceptDefaults) {
                return MockResponse::status(406);
            }
            if request.query("limit") == Some("many") && !broken(Break::InvalidLimitAccepted) {
                return MockResponse::new(400, "text/plain", "limit is a uint");
            }
            let matches = request
                .query("StudyInstanceUID")
                .is_none_or(|uid| uid == slide.study);
            json(if matches {
                format!("[{}]", study_result(slide, base, breaks))
            } else {
                "[]".into()
            })
        }
        ["studies", study, "series"] if *study == slide.study => {
            if request
                .query("SeriesInstanceUID")
                .is_some_and(|uid| uid != slide.series)
            {
                return if broken(Break::EmptyAs204) {
                    MockResponse::status(204)
                } else {
                    json("[]".into())
                };
            }
            json(format!("[{}]", series_result(slide, base)))
        }
        ["studies", _, "series", _, "instances"] => {
            let results = instance_results(slide, base);
            let offset: usize = request
                .query("offset")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let limit: usize = request
                .query("limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(usize::MAX);
            let page: Vec<&String> = results.iter().skip(offset).take(limit).collect();
            let remaining = results.len().saturating_sub(offset + page.len());
            let body = format!(
                "[{}]",
                page.iter()
                    .map(|result| result.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let response = json(body);
            if remaining > 0 && !broken(Break::NoPagingWarning) {
                response.with(
                    "Warning",
                    &format!("299 {base}: There are {remaining} additional results that can be requested"),
                )
            } else {
                response
            }
        }
        ["studies", _, "series", _, "metadata"] => {
            if broken(Break::MetadataRedirect302) {
                return MockResponse::status(302).with("Location", &format!("{base}/elsewhere"));
            }
            series_metadata(slide, breaks)
                .with("Content-Location", &format!("{origin}{}", request.target))
        }
        ["studies", _, "series", _, "instances", _, "metadata"] => {
            if accept.contains("application/dicom+xml") {
                return multipart(
                    "application/dicom+xml",
                    vec![(
                        "application/dicom+xml".into(),
                        Some(format!("{instance_base}/metadata")),
                        b"<?xml version=\"1.0\"?><NativeDicomModel/>".to_vec(),
                    )],
                    false,
                );
            }
            json(slide.metadata.clone())
                .with("Content-Location", &format!("{instance_base}/metadata"))
        }
        ["studies", _, "series", _, "instances", _, "frames", list] => {
            frames_response(slide, breaks, accept, list, &instance_base)
        }
        ["studies", _, "series", _, "instances", _, "frames", _, "rendered"] => rendered(
            slide,
            breaks,
            accept,
            request.query("viewport").is_some(),
            &instance_base,
        ),
        ["studies", _, "series", _, "instances", _, "bulk", "icc"] => {
            let location = if broken(Break::BulkWrongLocation) {
                format!("{instance_base}/bulk/other")
            } else {
                format!("{instance_base}/bulk/icc")
            };
            multipart(
                "application/octet-stream",
                vec![(
                    "application/octet-stream".into(),
                    Some(location),
                    slide.icc.clone(),
                )],
                false,
            )
        }
        ["studies", _, "series", _, "instances", _, "thumbnail"] => thumbnail(breaks, false),
        ["studies", _, "series", _, "instances", _, "frames", _, "thumbnail"]
        | ["studies", _, "thumbnail"] => thumbnail(breaks, false),
        ["studies", _, "series", _, "thumbnail"] => thumbnail(breaks, true),
        ["studies", _, "series", _, "instances", _] => {
            let file = if broken(Break::InstanceNotPart10) {
                slide.file[132..].to_vec()
            } else {
                slide.file.clone()
            };
            multipart(
                "application/dicom",
                vec![(
                    format!("application/dicom; transfer-syntax={J2K_LOSSLESS}"),
                    Some(instance_base.clone()),
                    file,
                )],
                false,
            )
        }
        _ => MockResponse::status(404),
    }
}

/// A DICOM JSON attribute object.
fn attribute(tag: &str, vr: &str, value: Option<String>) -> String {
    match value {
        Some(value) => format!("\"{tag}\":{{\"vr\":\"{vr}\",\"Value\":[{value}]}}"),
        None => format!("\"{tag}\":{{\"vr\":\"{vr}\"}}"),
    }
}

fn quoted(text: &str) -> Option<String> {
    Some(format!("\"{text}\""))
}

/// Table 10.6.3-3, in tag order.
fn study_result(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> String {
    let mut members = vec![
        attribute("00080020", "DA", quoted("20260101")),
        attribute("00080030", "TM", quoted("120000")),
        attribute("00080050", "SH", None),
        attribute("00080061", "CS", quoted("SM")),
        attribute("00080090", "PN", None),
        attribute(
            "00081190",
            "UR",
            quoted(&format!("{base}/studies/{}", slide.study)),
        ),
        attribute(
            "00100010",
            "PN",
            Some("{\"Alphabetic\":\"Placeholder^Research\"}".into()),
        ),
        attribute("00100020", "LO", quoted("RESEARCH")),
        attribute("00100030", "DA", None),
        attribute("00100040", "CS", None),
        attribute("0020000D", "UI", quoted(&slide.study)),
        attribute("00200010", "SH", None),
        attribute("00201206", "IS", Some("1".into())),
        attribute("00201208", "IS", Some("2".into())),
    ];
    if breaks.contains(&Break::StudyMissingKey) {
        members.retain(|member| !member.starts_with("\"00100030\""));
    }
    if breaks.contains(&Break::IncludefieldLeak) {
        members.insert(12, attribute("00200013", "IS", Some("1".into())));
    }
    format!("{{{}}}", members.join(","))
}

/// Table 10.6.3-4.
fn series_result(slide: &Slide, base: &str) -> String {
    let members = [
        attribute("00080060", "CS", quoted("SM")),
        attribute(
            "00081190",
            "UR",
            quoted(&format!(
                "{base}/studies/{}/series/{}",
                slide.study, slide.series
            )),
        ),
        attribute("0020000E", "UI", quoted(&slide.series)),
        attribute("00200011", "IS", Some("1".into())),
        attribute("00201209", "IS", Some("2".into())),
    ];
    format!("{{{}}}", members.join(","))
}

/// Table 10.6.3-5: the slide and an annotation Instance of the Series.
fn instance_results(slide: &Slide, base: &str) -> Vec<String> {
    let url = |uid: &str| {
        quoted(&format!(
            "{base}/studies/{}/series/{}/instances/{uid}",
            slide.study, slide.series
        ))
    };
    let annotation = format!("{}.9", slide.instance);
    vec![
        format!(
            "{{{}}}",
            [
                attribute("00080016", "UI", quoted(&slide.class)),
                attribute("00080018", "UI", quoted(&slide.instance)),
                attribute("00081190", "UR", url(&slide.instance)),
                attribute("00083002", "UI", quoted(J2K_LOSSLESS)),
                attribute("00200013", "IS", Some("1".into())),
                attribute("00280008", "IS", Some(slide.frames.len().to_string())),
                attribute("00280010", "US", Some(slide.rows.to_string())),
                attribute("00280011", "US", Some(slide.columns.to_string())),
                attribute("00280100", "US", Some("8".into())),
            ]
            .join(",")
        ),
        format!(
            "{{{}}}",
            [
                attribute("00080016", "UI", quoted(ANNOTATION_CLASS)),
                attribute("00080018", "UI", quoted(&annotation)),
                attribute("00081190", "UR", url(&annotation)),
                attribute("00200013", "IS", Some("2".into())),
            ]
            .join(",")
        ),
    ]
}

fn series_metadata(slide: &Slide, breaks: &BTreeSet<Break>) -> MockResponse {
    let mut metadata = slide.metadata.clone();
    if breaks.contains(&Break::MetadataFileMeta) {
        metadata = metadata.replacen(
            "[{",
            &format!("[{{\"00020010\":{{\"vr\":\"UI\",\"Value\":[\"{J2K_LOSSLESS}\"]}},"),
            1,
        );
    }
    if breaks.contains(&Break::MetadataLowercaseTag) {
        metadata = metadata.replacen("\"0020000D\"", "\"0020000d\"", 1);
    }
    json(metadata)
}

fn frames_response(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    accept: &str,
    list: &str,
    instance_base: &str,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let numbers: Vec<usize> = list
        .split(',')
        .filter_map(|number| number.parse().ok())
        .collect();
    let mut parts = Vec::new();
    for number in &numbers {
        let frame = match slide.frames.get(number.wrapping_sub(1)) {
            Some(frame) => frame.clone(),
            None if broken(Break::MissingFrame200) => slide.frames[0].clone(),
            None => return MockResponse::status(404),
        };
        let location =
            (!broken(Break::NoContentLocation)).then(|| format!("{instance_base}/frames/{number}"));
        let uncompressed =
            accept.contains("type=\"application/octet-stream\"") && !accept.contains("image/");
        if uncompressed {
            let mut size = (slide.rows * slide.columns * 3) as usize;
            if broken(Break::UncompressedWrongSize) {
                size -= 1;
            }
            parts.push((
                "application/octet-stream; transfer-syntax=1.2.840.10008.1.2.1".to_string(),
                location,
                vec![0; size],
            ));
            continue;
        }
        let mut frame = frame;
        if broken(Break::FramesCorrupted) && *number == 1 {
            frame[2] = 0;
            frame[3] = 0;
        }
        let media = if broken(Break::FramesWrongMediaType) {
            "image/jpeg"
        } else {
            "image/jp2"
        };
        parts.push((
            format!("{media}; transfer-syntax={J2K_LOSSLESS}"),
            location,
            frame,
        ));
    }
    let root = parts
        .first()
        .and_then(|(content_type, _, _)| content_type.split(';').next())
        .unwrap_or("image/jp2")
        .to_string();
    multipart(&root, parts, broken(Break::FramesTruncated))
}

fn rendered(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    accept: &str,
    viewport: bool,
    instance_base: &str,
) -> MockResponse {
    let (width, height) = if viewport && !breaks.contains(&Break::RenderedWrongSize) {
        (64, 64)
    } else {
        (slide.columns, slide.rows)
    };
    let response = if accept.contains("image/png") {
        MockResponse::new(200, "image/png", png(width, height))
    } else {
        let mut jpeg = crate::test_support::encode_test_jpeg(width, height, [200, 120, 80]);
        if breaks.contains(&Break::RenderedProgressive) {
            if let Some(at) = jpeg.windows(2).position(|pair| pair == [0xFF, 0xC0]) {
                jpeg[at + 1] = 0xC2;
            }
        }
        MockResponse::new(200, "image/jpeg", jpeg)
    };
    response.with("Content-Location", &format!("{instance_base}/frames/1"))
}

/// A PNG signature and IHDR chunk: what the rendered check reads.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn thumbnail(breaks: &BTreeSet<Break>, series: bool) -> MockResponse {
    if breaks.contains(&Break::ThumbnailNone)
        || (series && breaks.contains(&Break::ThumbnailPartial))
    {
        return MockResponse::status(404);
    }
    MockResponse::new(
        200,
        "image/jpeg",
        crate::test_support::encode_test_jpeg(16, 16, [10, 20, 30]),
    )
    .with("Content-Location", "thumbnail")
}

/// A Store Instances Response Module (Annex I) for the slide.
fn store(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> MockResponse {
    let item = format!(
        "{{{},{},{}}}",
        attribute("00081150", "UI", quoted(&slide.class)),
        attribute("00081155", "UI", quoted(&slide.instance)),
        attribute(
            "00081190",
            "UR",
            quoted(&format!(
                "{base}/studies/{}/series/{}/instances/{}",
                slide.study, slide.series, slide.instance
            ))
        )
    );
    let study_url = format!("{base}/studies/{}", slide.study);
    let module = if breaks.contains(&Break::StoreFailed) {
        format!(
            "{{{},{}}}",
            attribute("00081190", "UR", quoted(&study_url)),
            attribute(
                "00081198",
                "SQ",
                Some(format!(
                    "{{{},{},{}}}",
                    attribute("00081150", "UI", quoted(&slide.class)),
                    attribute("00081155", "UI", quoted(&slide.instance)),
                    attribute("00081197", "US", Some("272".into()))
                ))
            )
        )
    } else {
        format!(
            "{{{},{}}}",
            attribute("00081190", "UR", quoted(&study_url)),
            attribute("00081199", "SQ", Some(item))
        )
    };
    let response = json(format!("[{module}]"));
    if breaks.contains(&Break::StoreNoLocation) {
        response
    } else {
        response.with("Location", &study_url)
    }
}

// --- the tests ------------------------------------------------------------

#[test]
fn a_conforming_origin_server_passes_every_required_check() {
    let (_root, slide) = slide();
    let run = run(Arc::clone(&slide), &[], |_| {});
    for check in &run.report.checks {
        let expected = match check.name.as_str() {
            "dicomweb-store" | "dicomweb-store-readback" => DicomWebStatus::NotRun,
            _ => DicomWebStatus::Passed,
        };
        assert_eq!(check.status, expected, "{}: {}", check.name, check.message);
    }
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
