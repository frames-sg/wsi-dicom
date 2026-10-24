//! The mock origin server: an exported slide served as PS3.18 requires, and
//! broken one way at a time by a `Break`.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use moxcms::ColorProfile;

use super::fixture::{export_wsi, frames, open, to_json};
use super::mock::{MockRequest, MockResponse};
use crate::TransferSyntax;

pub(super) const BASE: &str = "/dicomweb";
pub(super) const J2K_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
const ANNOTATION_CLASS: &str = "1.2.840.10008.5.1.4.1.1.91.1";
const BOUNDARY: &str = "mock-boundary-7f3a";
const INVALID_CREDENTIAL: &str = "Bearer wsi-dicom-invalid-credential";
const STUDY_DESCRIPTION: &str = "Mock Study";
/// The JP2 Signature box (ISO/IEC 15444-1 I.5.1).
const JP2_SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];

/// One way the origin server departs from PS3.18.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Break {
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
    UnknownParameterRejected,
    PagingIgnoresOffset,
    PagingIgnoresLimit,
    EmptyNotArray,
    RenderedUnscaledWrongSize,
    NoContentType,
    MetadataNoContentLocation,
    FramesMislabelled,
    PartNoLength,
    FramesNoType,
    FramesTwoSyntaxes,
    InstanceRejectsWildcard,
    RenderedNoPng,
    RenderedWrongType,
    CapabilitiesNotWadl,
    Capabilities304,
    CapabilitiesTwoApplications,
    CapabilitiesNoBase,
    ThumbnailMultipart,
    StoreNoContentLocation,
    StoreNoModule,
    SearchWrongMediaType,
    RetrieveUrlMissing,
    SeriesMissingKey,
    InstanceMissingKey,
    // Response messages (8.1.2).
    NoResponse,
    StatusInvalid,
    RetrieveStatusMalformed,
    SearchStatusOutOfClass,
    StoreNoResponse,
    // Payload header fields (8.4, 8.6, 8.7.9, 8.8, 10.4.4).
    LengthWithChunked,
    EmptyPayloadUnframed,
    NoContentWithLength,
    NoContentWithType,
    UnquotedDefinedTerm,
    FramesAsDicom,
    // Error responses (8.5).
    UnauthorizedNoChallenge,
    NotAllowedNoAllow,
    // Range (8.6.1.1).
    RangeMalformed,
    // Capabilities (8.8, 8.9.3.2, Annex H).
    CapabilitiesNoCharset,
    CapabilitiesNotApplication,
    CapabilitiesOmitsFrames,
    // Search (8.3.4, 10.6).
    MatchesNotReturned,
    SeriesIncludefieldLeak,
    NonIdempotent,
    PagingWarningNoUrl,
    HierarchyIgnored,
    DateTimeSeparate,
    XmlEmptyNotNative,
    MatchIgnored,
    SearchParameterRejected,
    FuzzyDespiteFalse,
    FuzzyWarningMissing,
    FuzzyWarnedButApplied,
    EmptyMatchedDespiteFalse,
    EmptyNotMatched,
    EmptyUnsupportedButApplied,
    EmptyUndocumented,
    MultipleValueOr,
    MultipleUndocumented,
    MatchingKeyIgnored,
    MatchedStudyAttributeOmitted,
    SeriesSearchMissing,
    StudyInstancesMissing,
    AllInstancesMissing,
    IeLevelIgnored,
    AllSeriesWithoutStudyAttributes,
    AllInstancesWithoutSeriesAttributes,
    // Content negotiation and character sets (8.3.3, 8.7, 8.8).
    UnsupportedAcceptOther,
    WildcardNotDefault,
    InvalidAcceptRejected,
    QValuesIgnored,
    AcceptQueryIgnored,
    MixedQueryAccepted,
    MixedAcceptAccepted,
    NoDefaultNot406,
    UnsupportedCharsetRejected,
    RetrieveAcceptCharsetRejected,
    AcceptCharsetRejected,
    MediaTypeParametersRejected,
    SearchAcceptCharsetRejected,
    RenderedUtf8Rejected,
    // Transfer Syntaxes (8.6.2.1, 8.7.3, 8.7.8.2).
    DefaultSyntaxIgnored,
    OctetStreamDefaultNotExplicit,
    SyntaxBeforeMediaType,
    SyntaxParameterIgnored,
    BigEndianFrames,
    ImplicitVrInstance,
    ExplicitBigEndianInstance,
    InstanceDefaultNotExplicit,
    UnsupportedSyntaxNoDefault,
    // Frames and Instance consistency (10.4.3.3).
    FrameLocationForeign,
    MetadataOmitsAttribute,
    BulkTruncated,
    MetadataBadDate,
    // Rendered resources (8.3.5, 8.6.1.1, 8.7.4).
    RenderedWithSyntax,
    JphWithoutJp2Box,
    RenderedLabelledMultipart,
    ViewportCropsByDefault,
    AnnotationEmptyAccepted,
    AnnotationUnsupportedRejected,
    AnnotationWarningMissing,
    QualityRejected,
    QualityOutOfRangeAccepted,
    WindowRejected,
    WindowIllFormedAccepted,
    CropIgnored,
    ElidedDefaultsWrong,
    ViewportIllFormedAccepted,
    IccNoIgnored,
    IccYesMissing,
    IccYesNotSource,
    IccSrgbWrong,
    IccAdobeWrong,
    IccRommWrong,
    IccP3Wrong,
    // Store (10.5, Annex I).
    StoreRejectsChunked,
    Store202Clean,
    StoreTwoModifiedItems,
    StoreCoercedSilently,
    StoreCoercedNoWarning,
    StoredCorrupted,
    // Recommendations (should), held as advisories.
    RetrieveErrorEmpty,
    ServerErrorEmpty,
    SearchTooBroadEmpty,
    NotAcceptableUnlisted,
    NoAcceptStatusReportJson,
    StoreConflictEmpty,
    Store202Empty,
    TypeQuoted,
    DuplicateContentId,
    IncludefieldIgnored,
    IncludefieldAllIgnored,
    StoreWarningWithoutStatus,
}

/// The slide the mock origin server holds.
pub(super) struct Slide {
    pub(super) study: String,
    pub(super) series: String,
    pub(super) instance: String,
    pub(super) class: String,
    pub(super) file: Vec<u8>,
    /// The Instance re-encoded in Explicit VR Little Endian, for a request
    /// without a Transfer Syntax.
    explicit_file: Vec<u8>,
    pub(super) frames: Vec<Vec<u8>>,
    pub(super) rows: u32,
    pub(super) columns: u32,
    pub(super) icc: Vec<u8>,
    pub(super) metadata: String,
    /// The Metadata of the stored Instance with Patient ID coerced, without
    /// and with the Original Attributes Sequence recording it.
    coerced: String,
    coerced_recorded: String,
    /// Whether an Instance was stored, and the file stored.
    stored: AtomicBool,
    stored_file: Mutex<Option<Vec<u8>>>,
    /// Instance searches answered, for a non-idempotent search.
    searches: AtomicUsize,
}

impl Slide {
    pub(super) fn export(root: &Path) -> Self {
        let path = export_wsi(root, "served", (4, 2, 2), TransferSyntax::Jpeg2000Lossless);
        let mut object = open(&path);
        object.put(DataElement::new(
            tags::STUDY_DESCRIPTION,
            VR::LO,
            PrimitiveValue::from(STUDY_DESCRIPTION),
        ));
        object.write_to_file(&path).unwrap();
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
        let bulk = |tag, top| match tag {
            tags::ICC_PROFILE => Some(format!("instances/{instance}/bulk/icc")),
            tags::PIXEL_DATA if top => Some(format!("instances/{instance}/bulk/pixeldata")),
            _ => None,
        };
        let metadata = to_json(&object, &bulk);
        let mut coerced = (*object).clone();
        let original = coerced.get(tags::PATIENT_ID).cloned();
        coerced.put(DataElement::new(
            tags::PATIENT_ID,
            VR::LO,
            PrimitiveValue::from("COERCED"),
        ));
        let mut recorded = coerced.clone();
        recorded.put(original_attributes(original));
        Self {
            study: text(tags::STUDY_INSTANCE_UID),
            series: text(tags::SERIES_INSTANCE_UID),
            class: text(tags::SOP_CLASS_UID),
            rows: number(tags::ROWS),
            columns: number(tags::COLUMNS),
            frames: frames(&object),
            file: std::fs::read(&path).unwrap(),
            explicit_file: explicit_copy(&object),
            icc,
            metadata,
            coerced: to_json(&coerced, &bulk),
            coerced_recorded: to_json(&recorded, &bulk),
            stored: AtomicBool::new(false),
            stored_file: Mutex::new(None),
            searches: AtomicUsize::new(0),
            instance,
        }
    }
}

impl Slide {
    /// Forgets what earlier runs stored and searched.
    pub(super) fn reset(&self) {
        self.stored.store(false, Ordering::SeqCst);
        *self.stored_file.lock().unwrap() = None;
        self.searches.store(0, Ordering::SeqCst);
    }
}

/// An Original Attributes Sequence (0400,0561) Item recording `original`.
fn original_attributes(
    original: Option<dicom_object::mem::InMemElement>,
) -> dicom_object::mem::InMemElement {
    let mut modified = InMemDicomObject::new_empty();
    if let Some(original) = original {
        modified.put(original);
    }
    let mut item = InMemDicomObject::new_empty();
    item.put(DataElement::new(
        tags::ATTRIBUTE_MODIFICATION_DATE_TIME,
        VR::DT,
        PrimitiveValue::from("20260101120000"),
    ));
    item.put(DataElement::new(
        tags::MODIFYING_SYSTEM,
        VR::LO,
        PrimitiveValue::from("MOCK"),
    ));
    item.put(DataElement::new(
        tags::REASON_FOR_THE_ATTRIBUTE_MODIFICATION,
        VR::CS,
        PrimitiveValue::from("COERCE"),
    ));
    item.put(DataElement::new(
        tags::MODIFIED_ATTRIBUTES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![modified]),
    ));
    DataElement::new(
        tags::ORIGINAL_ATTRIBUTES_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![item]),
    )
}

/// The Instance in Explicit VR Little Endian with native Pixel Data of its
/// size; only its File Meta Information is read by the checks.
fn explicit_copy(object: &dicom_object::DefaultDicomObject) -> Vec<u8> {
    let mut data: InMemDicomObject = (**object).clone();
    let dimension = |tag| {
        object
            .get(tag)
            .and_then(|element| element.to_int::<usize>().ok())
            .unwrap()
    };
    let size = dimension(tags::ROWS) * dimension(tags::COLUMNS) * 3 * frames(object).len();
    data.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(vec![0u8; size + size % 2]),
    ));
    let meta = FileMetaTableBuilder::new()
        .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
        .media_storage_sop_class_uid(object.meta().media_storage_sop_class_uid())
        .media_storage_sop_instance_uid(object.meta().media_storage_sop_instance_uid());
    let file = data.with_meta(meta).unwrap();
    let mut bytes = Vec::new();
    file.write_all(&mut bytes).unwrap();
    bytes
}

/// The first ICC Profile (0028,2000), at the top level or in an Optical Path
/// Sequence Item.
fn icc_profile(object: &InMemDicomObject) -> Option<Vec<u8>> {
    if let Some(element) = object.get(tags::ICC_PROFILE) {
        return element.to_bytes().ok().map(|bytes| bytes.to_vec());
    }
    object
        .get(tags::OPTICAL_PATH_SEQUENCE)?
        .items()?
        .iter()
        .find_map(icc_profile)
}

fn json(body: String) -> MockResponse {
    MockResponse::new(200, "application/dicom+json", body)
}

/// A multipart/related response of `parts` (Content-Type, Content-Location,
/// payload).
pub(super) fn multipart(
    root: &str,
    parts: Vec<(String, Option<String>, Vec<u8>)>,
    truncated: bool,
) -> MockResponse {
    multipart_framed(root, parts, truncated, true)
}

/// `multipart`, each part with a Content-Length if `lengths`.
fn multipart_framed(
    root: &str,
    parts: Vec<(String, Option<String>, Vec<u8>)>,
    truncated: bool,
    lengths: bool,
) -> MockResponse {
    let mut body = Vec::new();
    for (content_type, location, payload) in parts {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Type: {content_type}\r\n").as_bytes(),
        );
        if lengths {
            body.extend_from_slice(format!("Content-Length: {}\r\n", payload.len()).as_bytes());
        }
        if let Some(location) = location {
            body.extend_from_slice(format!("Content-Location: {location}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&payload);
        body.extend_from_slice(b"\r\n");
    }
    if !truncated {
        body.extend_from_slice(format!("--{BOUNDARY}--").as_bytes());
    }
    MockResponse::new(
        200,
        &format!("multipart/related; type=\"{root}\"; boundary={BOUNDARY}"),
        body,
    )
}

/// One media range of an Accept value.
struct Range {
    essence: String,
    kind: Option<String>,
    syntax: Option<String>,
    charset: bool,
    q: f32,
}

/// The media ranges of `accept`, highest qvalue first, invalid ones left
/// out; ties keep their order.
fn ranges(accept: &str) -> Vec<Range> {
    let mut found: Vec<Range> = accept
        .split(',')
        .filter_map(|range| {
            let mut fields = range.split(';').map(str::trim);
            let essence = fields.next()?.to_ascii_lowercase();
            if !essence.contains('/') {
                return None;
            }
            let mut parsed = Range {
                essence,
                kind: None,
                syntax: None,
                charset: false,
                q: 1.0,
            };
            for field in fields {
                let (name, value) = field.split_once('=')?;
                let value = value.trim_matches('"').to_string();
                match name.trim() {
                    "type" => parsed.kind = Some(value),
                    "transfer-syntax" => parsed.syntax = Some(value),
                    "charset" => parsed.charset = true,
                    "q" => parsed.q = value.parse().ok()?,
                    _ => {}
                }
            }
            Some(parsed)
        })
        .collect();
    found.sort_by(|a, b| b.q.total_cmp(&a.q));
    found
}

pub(super) fn respond(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let Some(path) = request.path().strip_prefix(BASE) else {
        return MockResponse::status(404);
    };
    let origin = format!("http://{}", request.header("host").unwrap_or("127.0.0.1"));
    let base = format!("{origin}{BASE}");
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    if request.header("authorization") == Some(INVALID_CREDENTIAL) {
        let response = MockResponse::new(401, "text/plain", "invalid credential");
        return if broken(Break::UnauthorizedNoChallenge) {
            response
        } else {
            response.with("WWW-Authenticate", "Bearer realm=\"mock\"")
        };
    }
    let mut response = match (request.method.as_str(), segments.as_slice()) {
        ("OPTIONS", [""]) | ("OPTIONS", []) => capabilities(&base, breaks),
        ("POST", ["studies"]) => store(slide, &base, breaks, request),
        ("GET", _) => retrieve(slide, &origin, breaks, request, &segments),
        _ if broken(Break::NotAllowedNoAllow) => MockResponse::status(405),
        _ => MockResponse::status(405).with("Allow", "GET, POST"),
    };
    if broken(Break::Unframed) {
        response.unframed = true;
    }
    status_report(&mut response);
    if !broken(Break::TypeQuoted) {
        for (name, value) in &mut response.headers {
            if name == "Content-Type" && value.starts_with("multipart/related") {
                *value = unquoted_type(value);
            }
        }
    }
    response
}

/// An error response's Status Report: what failed, and for 406 the Media
/// Types available (Table 8.5-1).
fn status_report(response: &mut MockResponse) {
    if response.status < 400 || !response.body.is_empty() || response.bare || response.silent {
        return;
    }
    response.body = if response.status == 406 {
        "<html><body>406 Not Acceptable: available application/dicom+json, multipart/related; type=application/dicom+xml</body></html>".into()
    } else {
        format!(
            "<html><body>{} Error: the request failed; it is permanent</body></html>",
            response.status
        )
        .into_bytes()
    };
    if !response
        .headers
        .iter()
        .any(|(name, _)| name == "Content-Type")
    {
        response
            .headers
            .push(("Content-Type".into(), "text/html".into()));
    }
}

/// `multipart/related; type="x"` written `type=x`, as RFC 2387 recommends.
fn unquoted_type(value: &str) -> String {
    match value.split_once("type=\"") {
        Some((head, rest)) => match rest.split_once('"') {
            Some((kind, tail)) => format!("{head}type={kind}{tail}"),
            None => value.to_string(),
        },
        None => value.to_string(),
    }
}

fn capabilities(base: &str, breaks: &BTreeSet<Break>) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    if broken(Break::CapabilitiesMissing) {
        return MockResponse::status(405).with("Allow", "GET, POST");
    }
    if broken(Break::Capabilities304) {
        return MockResponse::status(304);
    }
    if broken(Break::CapabilitiesNotWadl) {
        return MockResponse::new(200, "text/html", "<html><body>DICOMweb</body></html>");
    }
    let base = if broken(Break::CapabilitiesNoBase) {
        String::new()
    } else {
        format!(" base=\"{base}/\"")
    };
    let extra = if broken(Break::CapabilitiesTwoApplications) {
        "<application/>"
    } else {
        ""
    };
    let root = if broken(Break::CapabilitiesNotApplication) {
        "service"
    } else {
        "application"
    };
    let param = |name: &str, omitted: Break| {
        if broken(omitted) {
            String::new()
        } else {
            format!("<param name=\"{name}\" style=\"query\"/>")
        }
    };
    let charset = if broken(Break::CapabilitiesNoCharset) {
        ""
    } else {
        "<param name=\"charset\" style=\"query\"><option value=\"UTF-8\"/></param>"
    };
    let frames = if broken(Break::CapabilitiesOmitsFrames) {
        ""
    } else {
        "<resource path=\"frames\"><resource path=\"{frames}\"><resource path=\"rendered\"/><resource path=\"thumbnail\"/></resource></resource>"
    };
    MockResponse::new(
        200,
        "application/vnd.sun.wadl+xml",
        format!(
            "<?xml version=\"1.0\"?><{root} xmlns=\"http://wadl.dev.java.net/2009/02\"><resources{base}><resource path=\"studies\"><method name=\"GET\"><request>{}{}{charset}</request></method><method name=\"POST\"/><resource path=\"{{study}}\"><resource path=\"thumbnail\"/><resource path=\"series\"><resource path=\"{{series}}\"><resource path=\"metadata\"/><resource path=\"thumbnail\"/><resource path=\"instances\"><resource path=\"{{instance}}\"><resource path=\"metadata\"/><resource path=\"thumbnail\"/>{frames}</resource></resource></resource></resource></resource></resource><resource path=\"series\"/><resource path=\"instances\"/></resources></{root}>{extra}",
            param("emptyvaluematching", Break::EmptyUndocumented),
            param("multiplevaluematching", Break::MultipleUndocumented),
        ),
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
        ["studies"] => studies(slide, base, breaks, request),
        ["studies", study, "series"] if *study == slide.study => {
            series_search(slide, base, breaks, request)
        }
        ["studies", _, "series"] => {
            if broken(Break::HierarchyIgnored) {
                json(format!("[{}]", series_result(slide, base, breaks)))
            } else {
                json("[]".into())
            }
        }
        ["studies", _, "series", _, "instances"] => instances(slide, base, breaks, request),
        ["studies", _, "instances"] => {
            if broken(Break::StudyInstancesMissing) {
                return MockResponse::status(404);
            }
            level_search(slide, base, breaks, request, false, true)
        }
        ["series"] => level_search(slide, base, breaks, request, true, false),
        ["instances"] => {
            if broken(Break::AllInstancesMissing) {
                return MockResponse::status(404);
            }
            if broken(Break::SearchTooBroadEmpty) {
                return MockResponse {
                    bare: true,
                    ..MockResponse::status(413)
                };
            }
            level_search(slide, base, breaks, request, true, true)
        }
        ["studies", _, "series", _, "metadata"] => {
            if broken(Break::MetadataRedirect302) {
                return MockResponse::status(302).with("Location", &format!("{base}/elsewhere"));
            }
            let response = series_metadata(slide, breaks);
            if broken(Break::MetadataNoContentLocation) {
                response
            } else {
                response.with("Content-Location", &format!("{origin}{}", request.target))
            }
        }
        ["studies", _, "series", _, "instances", _, "metadata"] => {
            instance_metadata(slide, breaks, request, &instance_base)
        }
        ["studies", _, "series", _, "instances", _, "frames", list] => {
            frames_response(slide, breaks, accept, list, &instance_base)
        }
        ["studies", _, "series", _, "instances", _, "frames", list, "rendered"] => {
            super::rendering::rendered(slide, breaks, request, list, &instance_base)
        }
        ["studies", _, "series", _, "instances", _, "bulk", "icc"] => {
            let location = if broken(Break::BulkWrongLocation) {
                format!("{instance_base}/bulk/other")
            } else {
                format!("{instance_base}/bulk/icc")
            };
            let icc = if broken(Break::BulkTruncated) {
                slide.icc[..slide.icc.len() - 8].to_vec()
            } else {
                slide.icc.clone()
            };
            multipart(
                "application/octet-stream",
                vec![("application/octet-stream".into(), Some(location), icc)],
                false,
            )
        }
        ["studies", _, "series", _, "instances", _, "thumbnail"] => {
            let mut response = thumbnail(breaks, false);
            if broken(Break::UnquotedDefinedTerm) && response.status == 200 {
                response.headers = vec![(
                    "Content-Type".into(),
                    "image/jpeg; charset=ISO_IR 192".into(),
                )];
            }
            if broken(Break::LengthWithChunked) {
                response.chunked = true;
            }
            response
        }
        ["studies", _, "series", _, "instances", _, "frames", _, "thumbnail"] => {
            if broken(Break::ServerErrorEmpty) {
                return MockResponse {
                    bare: true,
                    ..MockResponse::status(500)
                };
            }
            thumbnail(breaks, false)
        }
        ["studies", _, "thumbnail"] => {
            let mut response = thumbnail(breaks, false);
            if broken(Break::RetrieveStatusMalformed) {
                response.status_line = Some("HTTP/1.1 2000 OK".into());
            }
            response
        }
        ["studies", _, "series", _, "thumbnail"] => {
            let mut response = thumbnail(breaks, true);
            response.silent = broken(Break::NoResponse);
            response
        }
        ["studies", _, "series", _, "instances", _] => {
            instance(slide, breaks, accept, &instance_base)
        }
        _ => MockResponse::status(404),
    }
}

/// A DICOM JSON attribute object.
pub(super) fn attribute(tag: &str, vr: &str, value: Option<String>) -> String {
    match value {
        Some(value) => format!("\"{tag}\":{{\"vr\":\"{vr}\",\"Value\":[{value}]}}"),
        None => format!("\"{tag}\":{{\"vr\":\"{vr}\"}}"),
    }
}

pub(super) fn quoted(text: &str) -> Option<String> {
    Some(format!("\"{text}\""))
}

// --- searches ---------------------------------------------------------------

/// The Study's matching Attributes: keyword and Value.
const STUDY_VALUES: [(&str, &str); 9] = [
    ("StudyDate", "20260101"),
    ("StudyTime", "120000"),
    ("AccessionNumber", ""),
    ("ModalitiesInStudy", "SM"),
    ("ReferringPhysicianName", ""),
    ("PatientName", "Placeholder^Research"),
    ("PatientID", "RESEARCH"),
    ("StudyID", ""),
    ("StudyInstanceUID", ""),
];

/// Whether `held` matches the key `value` by single value, UID list or
/// range matching (PS3.4 C.2.2.2).
fn plain_match(held: &str, value: &str, uid_list: bool) -> bool {
    if uid_list {
        return value.split(',').any(|uid| uid == held);
    }
    match value.split_once('-') {
        Some((low, high)) if !value.contains('^') => {
            (low.is_empty() || held >= low) && (high.is_empty() || held <= high)
        }
        _ => held == value,
    }
}

/// The Study matched against the query, with the 299 Warnings it brings.
fn study_matches(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    warnings: &mut Vec<String>,
) -> bool {
    let broken = |kind| breaks.contains(&kind);
    let flag = |name: &str| request.query(name) == Some("true");
    let fuzzy = flag("fuzzymatching");
    let empty = flag("emptyvaluematching");
    let multiple = flag("multiplevaluematching");
    if fuzzy && !broken(Break::FuzzyWarningMissing) {
        warnings.push("The fuzzymatching parameter is not supported. Only literal matching has been performed.".into());
    }
    if empty && broken(Break::EmptyUnsupportedButApplied) {
        warnings.push("The emptyvaluematching parameter is not supported. Empty Value Matching has not been performed.".into());
    }
    let date = request.decoded("StudyDate");
    let time = request.decoded("StudyTime");
    let combined = matches!((&date, &time), (Some(date), Some(time)) if date.contains('-') && time.contains('-'))
        && !broken(Break::DateTimeSeparate);
    if combined {
        let (date, time) = (
            date.clone().unwrap_or_default(),
            time.clone().unwrap_or_default(),
        );
        let (d1, d2) = date.split_once('-').unwrap_or_default();
        let (t1, t2) = time.split_once('-').unwrap_or_default();
        let held = "20260101120000";
        if held < format!("{d1}{t1}").as_str() || held > format!("{d2}{t2}").as_str() {
            return false;
        }
    }
    for (keyword, held) in STUDY_VALUES {
        let held = if keyword == "StudyInstanceUID" {
            slide.study.as_str()
        } else {
            held
        };
        let Some(value) = request.decoded(keyword) else {
            continue;
        };
        if combined && matches!(keyword, "StudyDate" | "StudyTime") {
            continue;
        }
        if keyword == "PatientID" && broken(Break::MatchingKeyIgnored) {
            continue;
        }
        let matched = if value == "\"\"" {
            let applied = (empty && !broken(Break::EmptyNotMatched))
                || (!empty && broken(Break::EmptyMatchedDespiteFalse));
            applied && held.is_empty()
        } else if keyword == "ModalitiesInStudy" && value.contains('\\') {
            let values = value.split('\\');
            match (multiple, broken(Break::MultipleValueOr)) {
                (true, false) => values.into_iter().all(|value| value == held),
                (true, true) => values.into_iter().any(|value| value == held),
                _ => false,
            }
        } else if keyword.ends_with("Name")
            && !held.is_empty()
            && value.starts_with(held)
            && ((!fuzzy && broken(Break::FuzzyDespiteFalse))
                || (fuzzy && broken(Break::FuzzyWarnedButApplied)))
        {
            true
        } else {
            plain_match(held, &value, keyword == "StudyInstanceUID")
        };
        if !matched {
            return false;
        }
    }
    true
}

fn studies(
    slide: &Slide,
    base: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    if request.header("accept").is_none() && !broken(Break::NoAcceptDefaults) {
        if broken(Break::NotAcceptableUnlisted) {
            return MockResponse::new(406, "text/html", "<html><body>not acceptable</body></html>");
        }
        if broken(Break::NoAcceptStatusReportJson) {
            return MockResponse::new(
                406,
                "application/json",
                "{\"error\":\"application/dicom+json is available\"}",
            );
        }
        return MockResponse::status(406);
    }
    if request.query("limit") == Some("many") && !broken(Break::InvalidLimitAccepted) {
        return MockResponse::new(400, "text/plain", "limit is a uint");
    }
    if request.query("wsidicomprobe").is_some() && broken(Break::UnknownParameterRejected) {
        return MockResponse::new(400, "text/plain", "unknown parameter");
    }
    if request.query("emptyvaluematching").is_some()
        && request.query("fuzzymatching").is_some()
        && broken(Break::SearchParameterRejected)
    {
        return MockResponse::new(400, "text/plain", "unknown parameter");
    }
    if request.header("accept-charset").is_some() && broken(Break::SearchAcceptCharsetRejected) {
        return MockResponse::status(406);
    }
    let mut warnings = Vec::new();
    let matches =
        study_matches(slide, breaks, request, &mut warnings) && !broken(Break::MatchesNotReturned);
    let includes = request
        .decoded("includefield")
        .unwrap_or_default()
        .split(',')
        .any(|field| match field {
            "00081030" | "StudyDescription" => !broken(Break::IncludefieldIgnored),
            "all" => !broken(Break::IncludefieldAllIgnored),
            _ => false,
        });
    let mut result = study_result(slide, base, breaks);
    if includes {
        result = result.replacen(
            ",\"00081190\"",
            &format!(
                ",{},\"00081190\"",
                attribute("00081030", "LO", quoted(STUDY_DESCRIPTION))
            ),
            1,
        );
    }
    if request.query("PatientID").is_some() && broken(Break::MatchedStudyAttributeOmitted) {
        result = result.replace(&attribute("00100020", "LO", quoted("RESEARCH")), "");
        result = result.replace(",,", ",").replace("{,", "{");
    }
    let mut response = json(if matches {
        format!("[{result}]")
    } else {
        "[]".into()
    });
    for warning in warnings {
        response = response.with("Warning", &format!("299 {base}: {warning}"));
    }
    if broken(Break::NoContentType) {
        response.headers.retain(|(name, _)| name != "Content-Type");
    } else if broken(Break::SearchWrongMediaType) {
        response.headers = vec![("Content-Type".into(), "application/json".into())];
    }
    response
}

/// Table 10.6.3-3, in tag order.
fn study_members(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> Vec<String> {
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
    without_retrieve_url(&mut members, breaks);
    members
}

fn study_result(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> String {
    let mut members = study_members(slide, base, breaks);
    if breaks.contains(&Break::IncludefieldLeak) {
        members.insert(12, attribute("00200013", "IS", Some("1".into())));
    }
    format!("{{{}}}", members.join(","))
}

/// The members without Retrieve URL (0008,1190) when it is broken.
fn without_retrieve_url(members: &mut Vec<String>, breaks: &BTreeSet<Break>) {
    if breaks.contains(&Break::RetrieveUrlMissing) {
        members.retain(|member| !member.starts_with("\"00081190\""));
    }
}

/// Table 10.6.3-4.
fn series_members(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> Vec<String> {
    let mut members = vec![
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
    if breaks.contains(&Break::SeriesMissingKey) {
        members.retain(|member| !member.starts_with("\"00200011\""));
    }
    without_retrieve_url(&mut members, breaks);
    members
}

fn series_result(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> String {
    let mut members = series_members(slide, base, breaks);
    if breaks.contains(&Break::SeriesIncludefieldLeak) {
        members.push(attribute("00200013", "IS", Some("1".into())));
    }
    format!("{{{}}}", members.join(","))
}

/// The Series' matching Attributes: keyword and Value.
fn series_matches(slide: &Slide, request: &MockRequest) -> bool {
    [
        ("SeriesInstanceUID", slide.series.as_str(), true),
        ("Modality", "SM", false),
        ("SeriesNumber", "1", false),
        ("PerformedProcedureStepStartDate", "", false),
        ("PerformedProcedureStepStartTime", "", false),
        (
            "RequestAttributesSequence.ScheduledProcedureStepID",
            "",
            false,
        ),
        ("RequestAttributesSequence.RequestedProcedureID", "", false),
    ]
    .into_iter()
    .all(|(keyword, held, uid_list)| {
        request
            .decoded(keyword)
            .is_none_or(|value| plain_match(held, &value, uid_list))
    })
}

fn series_search(
    slide: &Slide,
    base: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    if broken(Break::SeriesSearchMissing) {
        return MockResponse::status(404);
    }
    let accept = request.header("accept").unwrap_or_default();
    if series_matches(slide, request) || broken(Break::MatchIgnored) {
        return json(format!("[{}]", series_result(slide, base, breaks)));
    }
    if accept.contains("application/dicom+xml") {
        let body = if broken(Break::XmlEmptyNotNative) {
            "<?xml version=\"1.0\"?><NativeDicomModel><DicomAttribute tag=\"0020000E\" vr=\"UI\"/></NativeDicomModel>"
        } else {
            "<?xml version=\"1.0\"?><NativeDicomModel/>"
        };
        return multipart(
            "application/dicom+xml",
            vec![(
                "application/dicom+xml".into(),
                None,
                body.as_bytes().to_vec(),
            )],
            false,
        );
    }
    if broken(Break::EmptyAs204) {
        return MockResponse::status(204);
    }
    if broken(Break::NoContentWithLength) {
        return MockResponse {
            length_without_payload: true,
            ..MockResponse::status(204)
        };
    }
    if broken(Break::NoContentWithType) {
        return MockResponse::status(204).with("Content-Type", "application/dicom+json");
    }
    if broken(Break::EmptyNotArray) {
        return json("{}".into());
    }
    let mut response = json("[]".into());
    if broken(Break::SearchStatusOutOfClass) {
        response.status = 600;
    }
    response
}

/// Table 10.6.3-5: the slide and an annotation Instance of the Series.
fn instance_members(slide: &Slide, base: &str, breaks: &BTreeSet<Break>) -> Vec<Vec<String>> {
    let url = |uid: &str| {
        quoted(&format!(
            "{base}/studies/{}/series/{}/instances/{uid}",
            slide.study, slide.series
        ))
    };
    let annotation = format!("{}.9", slide.instance);
    let object = |members: Vec<String>| {
        let mut members = members;
        if breaks.contains(&Break::InstanceMissingKey) {
            members.retain(|member| !member.starts_with("\"00200013\""));
        }
        without_retrieve_url(&mut members, breaks);
        members
    };
    vec![
        object(vec![
            attribute("00080016", "UI", quoted(&slide.class)),
            attribute("00080018", "UI", quoted(&slide.instance)),
            attribute("00081190", "UR", url(&slide.instance)),
            attribute("00083002", "UI", quoted(J2K_LOSSLESS)),
            attribute("00200013", "IS", Some("1".into())),
            attribute("00280008", "IS", Some(slide.frames.len().to_string())),
            attribute("00280010", "US", Some(slide.rows.to_string())),
            attribute("00280011", "US", Some(slide.columns.to_string())),
            attribute("00280100", "US", Some("8".into())),
        ]),
        object(vec![
            attribute("00080016", "UI", quoted(ANNOTATION_CLASS)),
            attribute("00080018", "UI", quoted(&annotation)),
            attribute("00081190", "UR", url(&annotation)),
            attribute("00200013", "IS", Some("2".into())),
        ]),
    ]
}

/// Whether an Instance result (its members) matches the query.
fn instance_matches(members: &[String], request: &MockRequest) -> bool {
    let held = |tag: &str| {
        members
            .iter()
            .find(|member| member.starts_with(&format!("\"{tag}\"")))
            .and_then(|member| member.split("\"Value\":[").nth(1))
            .map(|value| {
                value
                    .trim_end_matches(['}', ']'])
                    .trim_matches('"')
                    .to_string()
            })
            .unwrap_or_default()
    };
    [
        ("SOPInstanceUID", "00080018", true),
        ("SOPClassUID", "00080016", true),
        ("InstanceNumber", "00200013", false),
    ]
    .into_iter()
    .all(|(keyword, tag, uid_list)| {
        request
            .decoded(keyword)
            .is_none_or(|value| plain_match(&held(tag), &value, uid_list))
    })
}

fn instances(
    slide: &Slide,
    base: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let mut results: Vec<String> = instance_members(slide, base, breaks)
        .into_iter()
        .filter(|members| instance_matches(members, request))
        .map(|members| format!("{{{}}}", members.join(",")))
        .collect();
    let call = slide.searches.fetch_add(1, Ordering::SeqCst);
    if broken(Break::NonIdempotent) && call % 2 == 1 {
        results.reverse();
    }
    let offset: usize = request
        .query("offset")
        .and_then(|v| v.parse().ok())
        .filter(|_| !broken(Break::PagingIgnoresOffset))
        .unwrap_or(0);
    let limit: usize = request
        .query("limit")
        .and_then(|v| v.parse().ok())
        .filter(|_| !broken(Break::PagingIgnoresLimit))
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
        let agent = if broken(Break::PagingWarningNoUrl) {
            "-:"
        } else {
            &format!("{base}:")
        };
        response.with(
            "Warning",
            &format!("299 {agent} There are {remaining} additional results that can be requested"),
        )
    } else {
        response
    }
}

/// Study's Instances, All Series and All Instances: the target matched by
/// its UIDs, with the higher levels' Attributes.
fn level_search(
    slide: &Slide,
    base: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    study_level: bool,
    instance_level: bool,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let mut matches = series_matches(slide, request)
        && request
            .decoded("StudyInstanceUID")
            .is_none_or(|uid| uid == slide.study);
    let mut members = Vec::new();
    if study_level && !broken(Break::AllSeriesWithoutStudyAttributes) {
        members.extend(study_members(slide, base, breaks));
    }
    if !(instance_level && broken(Break::AllInstancesWithoutSeriesAttributes)) {
        members.extend(series_members(slide, base, breaks));
    }
    if instance_level {
        let instance = instance_members(slide, base, breaks).remove(0);
        matches &= instance_matches(&instance, request);
        members.extend(instance);
    }
    // The deepest level's Retrieve URL, each Tag once, in Tag order.
    let members: std::collections::BTreeMap<String, String> = members
        .into_iter()
        .map(|member| (member[..10].to_string(), member))
        .collect();
    let members: Vec<String> = members.into_values().collect();
    let mut results = Vec::new();
    if matches {
        results.push(format!("{{{}}}", members.join(",")));
    }
    if !instance_level && broken(Break::IeLevelIgnored) {
        results.push(format!(
            "{{{}}}",
            attribute("0020000E", "UI", quoted("2.25.1234567"))
        ));
    }
    json(format!("[{}]", results.join(",")))
}

// --- metadata ---------------------------------------------------------------

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
    if breaks.contains(&Break::MetadataBadDate) {
        let date = "\"00080020\":{\"vr\":\"DA\",\"Value\":[\"";
        if let Some(at) = metadata.find(date) {
            let start = at + date.len();
            metadata.replace_range(start..start + 8, "2026-01-01");
        }
    }
    json(omitted(metadata, breaks))
}

/// The Metadata without Image Type (0008,0008) when it is broken.
fn omitted(metadata: String, breaks: &BTreeSet<Break>) -> String {
    if !breaks.contains(&Break::MetadataOmitsAttribute) {
        return metadata;
    }
    let start = metadata.find("\"00080008\":").unwrap();
    let end = start + metadata[start..].find('}').unwrap() + 2;
    let mut omitted = metadata;
    omitted.replace_range(start..end, "");
    omitted
}

/// The Instance Metadata, negotiated: the accept Query Parameter first,
/// then the Accept Header Field by qvalue, then the Default, JSON
/// (8.7.8.1); charset values ignored when unsupported (8.8.1); a Range
/// answered 206 (8.6.1.1).
fn instance_metadata(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    instance_base: &str,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let accept = request.header("accept").unwrap_or_default();
    let charset = request.header("accept-charset");
    if charset.is_some() && broken(Break::RetrieveAcceptCharsetRejected) {
        return MockResponse::status(406);
    }
    if charset.is_some_and(|charset| charset.contains("x-wsidicom"))
        && broken(Break::AcceptCharsetRejected)
    {
        return MockResponse::status(406);
    }
    if request.query("charset").is_some() && broken(Break::UnsupportedCharsetRejected) {
        return MockResponse::new(400, "text/plain", "unsupported charset");
    }
    let Some(xml) = metadata_choice(breaks, request, accept) else {
        return MockResponse::new(400, "text/plain", "mixed media types");
    };
    if xml {
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
    if accept.trim() == "application/x-wsidicom-unsupported"
        && broken(Break::UnsupportedAcceptOther)
    {
        return MockResponse::new(200, "application/json", slide.metadata.clone());
    }
    let stored = slide.stored.load(Ordering::SeqCst);
    let body = if stored && broken(Break::StoreCoercedSilently) {
        slide.coerced.clone()
    } else if stored && broken(Break::StoreCoercedNoWarning) {
        slide.coerced_recorded.clone()
    } else {
        omitted(slide.metadata.clone(), breaks)
    };
    if let Some(range) = request.header("range") {
        let last: usize = range
            .strip_prefix("bytes=0-")
            .and_then(|last| last.parse().ok())
            .unwrap_or(0);
        let part = body.as_bytes()[..=last.min(body.len() - 1)].to_vec();
        let response = MockResponse {
            status: 206,
            ..MockResponse::new(206, "application/dicom+json", part)
        };
        return if broken(Break::RangeMalformed) {
            response
        } else {
            response.with(
                "Content-Range",
                &format!("bytes 0-{}/{}", last.min(body.len() - 1), body.len()),
            )
        };
    }
    json(body).with("Content-Location", &format!("{instance_base}/metadata"))
}

/// Whether the Instance Metadata is selected as XML; `None` answers 400.
fn metadata_choice(breaks: &BTreeSet<Break>, request: &MockRequest, accept: &str) -> Option<bool> {
    let broken = |kind| breaks.contains(&kind);
    let is_dicom = |range: &Range| {
        range.essence == "application/dicom+json"
            || range.kind.as_deref() == Some("application/dicom+xml")
    };
    let is_other =
        |range: &Range| range.essence.starts_with("image/") || range.essence.starts_with("text/");
    if let Some(query) = request
        .decoded("accept")
        .filter(|_| !broken(Break::AcceptQueryIgnored))
    {
        let listed = ranges(&query);
        if listed.iter().any(is_dicom)
            && listed.iter().any(is_other)
            && !broken(Break::MixedQueryAccepted)
        {
            return None;
        }
        if let Some(range) = listed.iter().find(|range| is_dicom(range)) {
            return Some(range.essence != "application/dicom+json");
        }
    }
    let mut listed = ranges(accept);
    if broken(Break::InvalidAcceptRejected) && accept.contains("wsidicom-invalid") {
        return None;
    }
    if listed.iter().any(|range| range.charset) && broken(Break::MediaTypeParametersRejected) {
        return None;
    }
    if listed.iter().any(is_dicom)
        && listed.iter().any(is_other)
        && !broken(Break::MixedAcceptAccepted)
    {
        return None;
    }
    if broken(Break::QValuesIgnored) {
        listed.sort_by_key(|range| accept.find(&range.essence));
    }
    for range in &listed {
        if is_dicom(range) {
            return Some(range.essence != "application/dicom+json");
        }
        if range.essence == "*/*" {
            return Some(broken(Break::WildcardNotDefault));
        }
    }
    Some(false)
}

// --- frames and instances ---------------------------------------------------

/// What a Frames request is answered in.
enum FrameEncoding {
    /// Explicit VR Little Endian bytes, labelled with this syntax.
    Uncompressed(&'static str),
    /// The held JPEG 2000 codestreams, labelled with this syntax.
    Native(&'static str),
}

fn frame_encoding(breaks: &BTreeSet<Break>, accept: &str) -> Option<FrameEncoding> {
    let broken = |kind| breaks.contains(&kind);
    let listed = ranges(accept);
    let native = listed.iter().find(|range| {
        range.kind.as_deref() == Some("image/jp2")
            && range
                .syntax
                .as_deref()
                .is_none_or(|syntax| syntax == "*" || syntax == J2K_LOSSLESS)
    });
    if broken(Break::SyntaxBeforeMediaType) && native.is_some() {
        return Some(FrameEncoding::Native(J2K_LOSSLESS));
    }
    for range in &listed {
        match range.kind.as_deref() {
            Some("application/octet-stream") => match range.syntax.as_deref() {
                None if broken(Break::OctetStreamDefaultNotExplicit) => {
                    return Some(FrameEncoding::Uncompressed("1.2.840.10008.1.2.1.98"))
                }
                Some(uids::EXPLICIT_VR_LITTLE_ENDIAN) if broken(Break::SyntaxParameterIgnored) => {
                    return Some(FrameEncoding::Uncompressed("1.2.840.10008.1.2.1.98"))
                }
                _ if broken(Break::BigEndianFrames) => {
                    return Some(FrameEncoding::Uncompressed("1.2.840.10008.1.2.2"))
                }
                None | Some("*") | Some(uids::EXPLICIT_VR_LITTLE_ENDIAN) => {
                    return Some(FrameEncoding::Uncompressed(uids::EXPLICIT_VR_LITTLE_ENDIAN))
                }
                _ => {}
            },
            Some("image/jp2") if native.is_some() => {
                let syntax = if range.syntax.is_none() && broken(Break::DefaultSyntaxIgnored) {
                    "1.2.840.10008.1.2.4.91"
                } else {
                    J2K_LOSSLESS
                };
                return Some(FrameEncoding::Native(syntax));
            }
            _ => {}
        }
    }
    None
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
    let Some(encoding) = frame_encoding(breaks, accept) else {
        return MockResponse::status(406);
    };
    let mut parts = Vec::new();
    for number in &numbers {
        let frame = match slide.frames.get(number.wrapping_sub(1)) {
            Some(frame) => frame.clone(),
            None if broken(Break::MissingFrame200) => slide.frames[0].clone(),
            None if broken(Break::StatusInvalid) => {
                return MockResponse {
                    status_line: Some("HTTP/1.1 099 Mock".into()),
                    ..MockResponse::status(404)
                }
            }
            None if broken(Break::EmptyPayloadUnframed) => {
                return MockResponse {
                    unframed: true,
                    ..MockResponse::status(404)
                }
            }
            None if broken(Break::RetrieveErrorEmpty) => {
                return MockResponse {
                    bare: true,
                    ..MockResponse::status(404)
                }
            }
            None => return MockResponse::status(404),
        };
        let location = if broken(Break::FrameLocationForeign) {
            Some(format!("{instance_base}.7/frames/{number}"))
        } else {
            (!broken(Break::NoContentLocation)).then(|| format!("{instance_base}/frames/{number}"))
        };
        match encoding {
            FrameEncoding::Uncompressed(syntax) => {
                let mut size = (slide.rows * slide.columns * 3) as usize;
                if broken(Break::UncompressedWrongSize) {
                    size -= 1;
                }
                parts.push((
                    format!("application/octet-stream; transfer-syntax={syntax}"),
                    location,
                    vec![0; size],
                ));
            }
            FrameEncoding::Native(syntax) => {
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
                let parameter = if broken(Break::FramesTwoSyntaxes) {
                    format!("transfer-syntax={syntax}; transfer-syntax={syntax}")
                } else {
                    format!("transfer-syntax={syntax}")
                };
                parts.push((format!("{media}; {parameter}"), location, frame));
            }
        }
    }
    let root = parts
        .first()
        .and_then(|(content_type, _, _)| content_type.split(';').next())
        .unwrap_or("image/jp2")
        .to_string();
    let mut response = multipart_framed(
        &root,
        parts,
        broken(Break::FramesTruncated),
        !broken(Break::PartNoLength),
    );
    if broken(Break::FramesMislabelled) {
        response.headers = vec![("Content-Type".into(), "image/jp2".into())];
    } else if broken(Break::FramesNoType) {
        response.headers = vec![(
            "Content-Type".into(),
            format!("multipart/related; boundary={BOUNDARY}"),
        )];
    } else if broken(Break::FramesAsDicom) {
        response.headers = vec![("Content-Type".into(), "application/dicom".into())];
    }
    if broken(Break::DuplicateContentId) {
        response.body = replace_bytes(
            &response.body,
            b"\r\nContent-Type: ",
            b"\r\nContent-ID: <frame@mock>\r\nContent-Type: ",
        );
    }
    response
}

/// `bytes` with each `from` replaced by `to`.
fn replace_bytes(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut replaced = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(from) {
            replaced.extend_from_slice(to);
            index += from.len();
        } else {
            replaced.push(bytes[index]);
            index += 1;
        }
    }
    replaced
}

/// Whether the Instance is answered in the held JPEG 2000: for any Transfer
/// Syntax, or when broken, without one or with one not offered.
fn held_syntax(breaks: &BTreeSet<Break>, accept: &str) -> bool {
    let broken = |kind| breaks.contains(&kind);
    let named = accept.contains("transfer-syntax=");
    accept.contains("transfer-syntax=*")
        || (broken(Break::InstanceDefaultNotExplicit) && !named)
        || (broken(Break::UnsupportedSyntaxNoDefault) && named)
}

fn instance(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    accept: &str,
    instance_base: &str,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let wildcard = accept.contains("transfer-syntax=*");
    if broken(Break::InstanceRejectsWildcard) && wildcard {
        return MockResponse::status(406);
    }
    let stored = slide.stored_file.lock().unwrap().clone();
    let (file, syntax) = if let Some(stored) = stored {
        let kept = if broken(Break::StoredCorrupted) {
            stored.len() - 64
        } else {
            stored.len()
        };
        (stored[..kept].to_vec(), J2K_LOSSLESS)
    } else if broken(Break::InstanceNotPart10) {
        (slide.file[132..].to_vec(), J2K_LOSSLESS)
    } else if held_syntax(breaks, accept) {
        (slide.file.clone(), J2K_LOSSLESS)
    } else {
        (slide.explicit_file.clone(), uids::EXPLICIT_VR_LITTLE_ENDIAN)
    };
    let label = if broken(Break::ImplicitVrInstance) && wildcard {
        "1.2.840.10008.1.2"
    } else if broken(Break::ExplicitBigEndianInstance) && !wildcard {
        "1.2.840.10008.1.2.2"
    } else {
        syntax
    };
    multipart(
        "application/dicom",
        vec![(
            format!("application/dicom; transfer-syntax={label}"),
            Some(instance_base.to_string()),
            file,
        )],
        false,
    )
}

// --- thumbnails and store -----------------------------------------------------

fn thumbnail(breaks: &BTreeSet<Break>, series: bool) -> MockResponse {
    if breaks.contains(&Break::ThumbnailNone)
        || (series && breaks.contains(&Break::ThumbnailPartial))
    {
        return MockResponse::status(404);
    }
    let jpeg = crate::test_support::encode_test_jpeg(16, 16, [10, 20, 30]);
    if breaks.contains(&Break::ThumbnailMultipart) {
        return multipart(
            "image/jpeg",
            vec![("image/jpeg".into(), Some("thumbnail".into()), jpeg)],
            false,
        );
    }
    MockResponse::new(200, "image/jpeg", jpeg).with("Content-Location", "thumbnail")
}

/// A Store Instances Response Module (Annex I) for the slide.
fn store(
    slide: &Slide,
    base: &str,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    if broken(Break::StoreNoResponse) {
        return MockResponse {
            silent: true,
            ..MockResponse::default()
        };
    }
    let chunked = request
        .header("transfer-encoding")
        .is_some_and(|value| value.contains("chunked"));
    if chunked && broken(Break::StoreRejectsChunked) {
        return MockResponse::status(411);
    }
    if broken(Break::StoreConflictEmpty) {
        return MockResponse {
            bare: true,
            ..MockResponse::status(409)
        };
    }
    if broken(Break::Store202Empty) {
        return MockResponse {
            bare: true,
            ..MockResponse::status(202)
        };
    }
    slide.stored.store(true, Ordering::SeqCst);
    *slide.stored_file.lock().unwrap() = stored_part(&request.body);
    let mut item = vec![
        attribute("00081150", "UI", quoted(&slide.class)),
        attribute("00081155", "UI", quoted(&slide.instance)),
        attribute(
            "00081190",
            "UR",
            quoted(&format!(
                "{base}/studies/{}/series/{}/instances/{}",
                slide.study, slide.series, slide.instance
            )),
        ),
    ];
    if broken(Break::StoreCoercedSilently) {
        item.push(attribute("00081196", "US", Some("45056".into())));
    }
    if broken(Break::StoreTwoModifiedItems) {
        let modified = format!("{{{}}}", attribute("00100020", "LO", quoted("RESEARCH")));
        item.push(attribute(
            "04000561",
            "SQ",
            Some(format!(
                "{{{},{},{},{}}}",
                attribute("04000550", "SQ", Some(format!("{modified},{modified}"))),
                attribute("04000562", "DT", quoted("20260101120000")),
                attribute("04000563", "LO", quoted("MOCK")),
                attribute("04000565", "CS", quoted("COERCE"))
            )),
        ));
    }
    let study_url = format!("{base}/studies/{}", slide.study);
    let module = if broken(Break::StoreFailed) {
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
            attribute("00081199", "SQ", Some(format!("{{{}}}", item.join(","))))
        )
    };
    let mut response = json(if broken(Break::StoreNoModule) {
        "[]".into()
    } else {
        format!("[{module}]")
    });
    if broken(Break::Store202Clean) {
        response.status = 202;
    }
    if broken(Break::StoreWarningWithoutStatus) {
        response = response.with("Warning", &format!("299 {base}: stored with changes"));
    }
    if !broken(Break::StoreNoLocation) {
        response = response.with("Location", &study_url);
    }
    if !broken(Break::StoreNoContentLocation) {
        response = response.with("Content-Location", &study_url);
    }
    response
}

/// ICC profiles of the named color spaces, for the rendered images.
pub(super) fn named_profile(space: &str) -> Vec<u8> {
    match space {
        "srgb" => crate::icc::synthetic_srgb_icc_profile().unwrap(),
        "displayp3" => crate::icc::synthetic_display_p3_icc_profile().unwrap(),
        "adobergb" => ColorProfile::new_adobe_rgb().encode().unwrap(),
        "rommrgb" => ColorProfile::new_pro_photo_rgb().encode().unwrap(),
        other => panic!("no profile for {other}"),
    }
}

/// `jpeg` with `profile` in APP2 ICC_PROFILE segments after its SOI.
pub(super) fn with_icc(jpeg: Vec<u8>, profile: &[u8]) -> Vec<u8> {
    let chunks: Vec<&[u8]> = profile.chunks(60_000).collect();
    let mut bytes = jpeg[..2].to_vec();
    for (index, chunk) in chunks.iter().enumerate() {
        let mut payload = b"ICC_PROFILE\0".to_vec();
        payload.push(u8::try_from(index + 1).unwrap());
        payload.push(u8::try_from(chunks.len()).unwrap());
        payload.extend_from_slice(chunk);
        bytes.extend_from_slice(&[0xFF, 0xE2]);
        bytes.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        bytes.extend_from_slice(&payload);
    }
    bytes.extend_from_slice(&jpeg[2..]);
    bytes
}

/// A JP2 file: the Signature box, then the codestream.
pub(super) fn jp2(codestream: &[u8]) -> Vec<u8> {
    let mut bytes = JP2_SIGNATURE.to_vec();
    bytes.extend_from_slice(codestream);
    bytes
}

/// The PS3.10 file of a Store request's one application/dicom part: from
/// its preamble to the delimiter that follows it.
fn stored_part(body: &[u8]) -> Option<Vec<u8>> {
    let magic = body.windows(4).position(|window| window == b"DICM")?;
    let start = magic.checked_sub(128)?;
    let end = body.windows(4).rposition(|window| window == b"\r\n--")?;
    (end > start).then(|| body[start..end].to_vec())
}
