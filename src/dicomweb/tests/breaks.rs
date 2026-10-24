//! Each PS3.18 statement the DICOMweb checks enforce, broken by the mock
//! origin server one at a time: the check that holds the server to it fails
//! with a message that names what was found.

use std::sync::Arc;

use super::origin::Break;
use super::server::{run, slide};
use crate::dicomweb::DicomWebStatus;
use crate::ValidationStatus;

/// The Breaks of a run without Store, the check each fails, and a part of
/// its message.
const BREAKS: &[(Break, &str, &str)] = &[
    // 8.1.2 and 8.1.2.1; 10.4.3, 10.6.3.
    (Break::NoResponse, "dicomweb-response-messages", "/thumbnail: the connection closed before a complete response"),
    (Break::StatusInvalid, "dicomweb-response-messages", "invalid status code: 099"),
    (Break::RetrieveStatusMalformed, "dicomweb-response-messages", "/thumbnail: the response is not an HTTP/1.1 response message"),
    (Break::SearchStatusOutOfClass, "dicomweb-response-messages", "answered 600, which is not a valid 3-digit status code"),
    // 8.4.3, 8.6, 8.7.9, 8.8, 10.4.4.
    (Break::LengthWithChunked, "dicomweb-payload-headers", "Content-Length is present with Transfer-Encoding"),
    (Break::EmptyPayloadUnframed, "dicomweb-payload-headers", "Content-Length is present even if the payload is empty"),
    (Break::NoContentWithLength, "dicomweb-payload-headers", "a 204 response has no payload, yet it has Content-Length"),
    (Break::NoContentWithType, "dicomweb-payload-headers", "yet it has a Content-Type, which is only present when a payload is"),
    (Break::UnquotedDefinedTerm, "dicomweb-payload-headers", "charset=ISO_IR 192, a Defined Term with a space"),
    (Break::FramesAsDicom, "dicomweb-payload-headers", "only used for an individual Instance resource"),
    // 8.5.
    (Break::UnauthorizedNoChallenge, "dicomweb-error-responses", "answered 401 without a WWW-Authenticate"),
    (Break::NotAllowedNoAllow, "dicomweb-error-responses", "answered 405 without an Allow"),
    // 8.6.1.1.
    (Break::RangeMalformed, "dicomweb-range-requests", "206 with Content-Range \"\""),
    // 8.8, 8.9.3.2 and Annex H.
    (Break::CapabilitiesNoCharset, "dicomweb-capabilities", "documents no character set"),
    (Break::CapabilitiesNotApplication, "dicomweb-capabilities", "holds 0 \"application\" elements"),
    (Break::CapabilitiesOmitsFrames, "dicomweb-capabilities", "no \"resource\" element for frames"),
    // 8.3.4.4.1, 10.6.3.1, 10.6.3.3.
    (Break::MatchesNotReturned, "dicomweb-search-studies", "returned 0 results"),
    (Break::SeriesIncludefieldLeak, "dicomweb-search-series", "is returned by a Series search"),
    (Break::NonIdempotent, "dicomweb-search-paging", "sent twice returned"),
    (Break::PagingWarningNoUrl, "dicomweb-search-paging", "where the URL of the service is required"),
    // 8.3.4.1.1, 8.3.4.4.1, 10.6.2.
    (Break::HierarchyIgnored, "dicomweb-search-matching", "2.25.299792458299792458299792458299792458/series: no Series matches"),
    (Break::DateTimeSeparate, "dicomweb-search-matching", "combined date-time matching returns it"),
    (Break::XmlEmptyNotNative, "dicomweb-search-matching", "not an empty NativeDicomModel root element"),
    (Break::MatchIgnored, "dicomweb-search-matching", "SeriesInstanceUID=2.25.299792458299792458299792458299792458: no Series matches"),
    // 8.3.4, 8.3.4.2, 8.3.4.5, 8.3.4.6, 10.6.1.2.
    (Break::SearchParameterRejected, "dicomweb-search-matching-parameters", "multiplevaluematching=false answered 400"),
    (Break::FuzzyDespiteFalse, "dicomweb-search-matching-parameters", "fuzzymatching=false, so the search is performed without Fuzzy Matching"),
    (Break::FuzzyWarningMissing, "dicomweb-search-matching-parameters", "no Warning says fuzzymatching is not supported"),
    (Break::FuzzyWarnedButApplied, "dicomweb-search-matching-parameters", "fuzzymatching is warned unsupported, so the search is performed without Fuzzy Matching"),
    (Break::EmptyMatchedDespiteFalse, "dicomweb-search-matching-parameters", "emptyvaluematching=false, so \"\" is matched literally"),
    (Break::EmptyNotMatched, "dicomweb-search-matching-parameters", "with Empty Value Matching performed, \"\" matches a zero-length value"),
    (Break::EmptyUnsupportedButApplied, "dicomweb-search-matching-parameters", "emptyvaluematching is warned unsupported"),
    (Break::EmptyUndocumented, "dicomweb-search-matching-parameters", "no Warning says emptyvaluematching is not supported"),
    (Break::MultipleValueOr, "dicomweb-search-matching-parameters", "matches only a Study holding every value"),
    (Break::MultipleUndocumented, "dicomweb-search-matching-parameters", "no Warning says multiplevaluematching is not supported"),
    // 10.6.1.2.3, 10.6.3.3.1.
    (Break::MatchingKeyIgnored, "dicomweb-search-matching-attributes", "PatientID is a Required Matching Attribute"),
    (Break::MatchedStudyAttributeOmitted, "dicomweb-search-matching-attributes", "the result lacks PatientID, passed as match parameters"),
    // 10.2, 10.6.1.1, 10.6.1.2.3, 10.6.3.3.2, 10.6.3.3.3.
    (Break::SeriesSearchMissing, "dicomweb-search-resources", "the Study's Series search did not return results"),
    (Break::StudyInstancesMissing, "dicomweb-search-resources", "the Study's Instances resource is Mandatory"),
    (Break::AllInstancesMissing, "dicomweb-search-resources", "the All Instances resource is Mandatory"),
    (Break::IeLevelIgnored, "dicomweb-search-resources", "matching on the IE Levels of the resource returns"),
    (Break::AllSeriesWithoutStudyAttributes, "dicomweb-search-resources", "as 10.6.3.3.2 requires of All Series"),
    (Break::AllInstancesWithoutSeriesAttributes, "dicomweb-search-resources", "as 10.6.3.3.3 requires of Study's Instances"),
    // 8.3.3.1, 8.7.3 Table 8.7.3-1, 8.7.5, 8.7.7, 8.7.8.1, 10.1.2.
    (Break::UnsupportedAcceptOther, "dicomweb-accept-negotiation", "Content-Type is application/json; application/dicom+json is required"),
    (Break::WildcardNotDefault, "dicomweb-accept-negotiation", "\"*/*\" matches the Default Media Type"),
    (Break::InvalidAcceptRejected, "dicomweb-accept-negotiation", "Accept values that are not valid or not supported are ignored"),
    (Break::QValuesIgnored, "dicomweb-accept-negotiation", "prioritized by qvalue"),
    (Break::AcceptQueryIgnored, "dicomweb-accept-negotiation", "the supported Media Type of the Accept Query Parameter is selected"),
    (Break::MixedQueryAccepted, "dicomweb-accept-negotiation", "an Accept Query Parameter with DICOM and non-DICOM Media Types answers 400"),
    (Break::MixedAcceptAccepted, "dicomweb-accept-negotiation", "Acceptable Media Types with DICOM and Rendered Media Types answer 400"),
    (Break::NoDefaultNot406, "dicomweb-accept-negotiation", "a Multi-frame Image has no Default Rendered Media Type"),
    // 8.3.3.2, 8.8, 8.8.1, 10.1.3, 10.4.1.3, 10.4.4, 10.6.1.3.
    (Break::UnsupportedCharsetRejected, "dicomweb-character-sets", "a charset Query Parameter value that is not supported is ignored"),
    (Break::RetrieveAcceptCharsetRejected, "dicomweb-character-sets", "Table 10.4.1-6: the origin server supports Accept-Charset"),
    (Break::AcceptCharsetRejected, "dicomweb-character-sets", "charset values that are not valid or not supported are ignored"),
    (Break::MediaTypeParametersRejected, "dicomweb-character-sets", "the Transfer Syntax and Character Set Media Type parameters are supported"),
    (Break::SearchAcceptCharsetRejected, "dicomweb-character-sets", "supports Accept-Charset in a Search"),
    (Break::RenderedUtf8Rejected, "dicomweb-character-sets", "supports the \"UTF-8\" charset name for Retrieve Rendered"),
    // 8.6.2.1, 8.7.3, 8.7.3.1, 8.7.3.3, 8.7.3.4, 8.7.3.5.2, 8.7.8.2.
    (Break::DefaultSyntaxIgnored, "dicomweb-transfer-syntaxes", "image/jp2 requested without a Transfer Syntax parameter is in 1.2.840.10008.1.2.4.90"),
    (Break::OctetStreamDefaultNotExplicit, "dicomweb-transfer-syntaxes", "application/octet-stream requested without a Transfer Syntax parameter is in 1.2.840.10008.1.2.1 "),
    (Break::SyntaxBeforeMediaType, "dicomweb-transfer-syntaxes", "the Selected Media Type is determined first"),
    (Break::SyntaxParameterIgnored, "dicomweb-retrieve-frames-uncompressed", "the request accepted application/octet-stream in 1.2.840.10008.1.2.1 only"),
    (Break::BigEndianFrames, "dicomweb-transfer-syntaxes", "frames/1: part 1 is in Explicit VR Big Endian (1.2.840.10008.1.2.2), which shall not be used"),
    (Break::ImplicitVrInstance, "dicomweb-transfer-syntaxes", "Implicit VR Little Endian (1.2.840.10008.1.2), which shall not be used"),
    (Break::ExplicitBigEndianInstance, "dicomweb-transfer-syntaxes", "part 1 is in Explicit VR Big Endian (1.2.840.10008.1.2.2), which shall not be used"),
    (Break::InstanceDefaultNotExplicit, "dicomweb-transfer-syntaxes", "the Default Transfer Syntax shall be used"),
    (Break::UnsupportedSyntaxNoDefault, "dicomweb-transfer-syntaxes", "8.7.3.1: with none of the Acceptable Media Types supported"),
    // 10.4.3.3.2, 10.4.3.3.5, 10.4.3.3.6.
    (Break::FrameLocationForeign, "dicomweb-retrieve-frames", "it corresponds to the Pixel Data of the Instance in the Metadata"),
    (Break::MetadataOmitsAttribute, "dicomweb-retrieve-instance", "(0008,0008) that its Metadata lacks"),
    (Break::BulkTruncated, "dicomweb-retrieve-instance", "the payload of a Bulk Data resource holds all its Bulk Data"),
    // 8.3.5.1.3, 8.6.1.1, 8.7.4.
    (Break::JphWithoutJp2Box, "dicomweb-retrieve-rendered", "does not begin with the JP2 Signature box"),
    (Break::RenderedLabelledMultipart, "dicomweb-retrieve-rendered", "a single part payload has a single part media type"),
    (Break::ViewportCropsByDefault, "dicomweb-retrieve-rendered", "is 64x32; 64x64 is required"),
    // 8.3.5, 8.3.5.1.1 to 8.3.5.1.5, 10.4.1.2.
    (Break::AnnotationEmptyAccepted, "dicomweb-rendered-parameters", "an annotation without values answers 400"),
    (Break::AnnotationUnsupportedRejected, "dicomweb-rendered-parameters", "unsupported annotation values are ignored"),
    (Break::AnnotationWarningMissing, "dicomweb-rendered-parameters", "The following annotation values are not supported: wsidicomprobe\" ("),
    (Break::QualityRejected, "dicomweb-rendered-parameters", "quality is supported"),
    (Break::QualityOutOfRangeAccepted, "dicomweb-rendered-parameters", "a quality that is not an integer between 1 and 100 answers 400"),
    (Break::WindowRejected, "dicomweb-rendered-parameters", "window is supported"),
    (Break::WindowIllFormedAccepted, "dicomweb-rendered-parameters", "a window without its three values answers 400"),
    (Break::CropIgnored, "dicomweb-rendered-parameters", "the source is cropped to sx, sy, sw, sh"),
    (Break::ElidedDefaultsWrong, "dicomweb-rendered-parameters", "an elided sw defaults to the right edge"),
    (Break::ViewportIllFormedAccepted, "dicomweb-rendered-parameters", "a viewport without vh is ill-formed"),
    (Break::IccNoIgnored, "dicomweb-rendered-parameters", "\"no\" indicates that no ICC profile shall be present"),
    (Break::IccYesMissing, "dicomweb-rendered-parameters", "the image embeds no ICC Profile"),
    (Break::IccYesNotSource, "dicomweb-rendered-parameters", "ICC Profile (0028,2000) of the Frame"),
    (Break::IccSrgbWrong, "dicomweb-rendered-parameters", "the primaries of SRGB"),
    (Break::IccAdobeWrong, "dicomweb-rendered-parameters", "the primaries of ADOBERGB"),
    (Break::IccRommWrong, "dicomweb-rendered-parameters", "the primaries of ROMMRGB"),
    (Break::IccP3Wrong, "dicomweb-rendered-parameters", "the primaries of DISPLAYP3"),
];

/// The Breaks of a run with Store.
const STORE_BREAKS: &[(Break, &str, &str)] = &[
    (
        Break::StoreNoResponse,
        "dicomweb-response-messages",
        "POST /dicomweb/studies: the connection closed before a complete response",
    ),
    (
        Break::StoreRejectsChunked,
        "dicomweb-store",
        "411 (Length Required) to a chunked request",
    ),
    (
        Break::Store202Clean,
        "dicomweb-store",
        "answered 202, which means warnings or failures exist",
    ),
    (
        Break::StoreTwoModifiedItems,
        "dicomweb-store",
        "has 2 Modified Attributes Sequence (0400,0550) Items",
    ),
    (
        Break::StoreCoercedSilently,
        "dicomweb-store-readback",
        "has no Original Attributes Sequence (0400,0561)",
    ),
    (
        Break::StoreCoercedNoWarning,
        "dicomweb-store-readback",
        "Warning Reason (0008,1196) is absent",
    ),
    (
        Break::StoredCorrupted,
        "dicomweb-store-readback",
        "the stored Instance fails",
    ),
];

#[test]
fn each_broken_requirement_fails_its_check() {
    let (root, slide) = slide();
    let file = root.path().join("store.dcm");
    std::fs::write(&file, &slide.file).unwrap();
    let cases = BREAKS
        .iter()
        .map(|case| (case, false))
        .chain(STORE_BREAKS.iter().map(|case| (case, true)));
    let mut unbroken = Vec::new();
    for (&(broken, name, reason), stores) in cases {
        let run = run(Arc::clone(&slide), &[broken], |options| {
            options.probe_error_responses = true;
            if stores {
                options.store_file = Some(file.clone());
            }
        });
        let check = run.check(name);
        if check.status != DicomWebStatus::Failed || !check.message.contains(reason) {
            unbroken.push(format!(
                "{broken:?} {name} {:?}: {}",
                check.status, check.message
            ));
        }
    }
    assert!(unbroken.is_empty(), "{unbroken:#?}");
}

/// The recommendations ("should") a Break departs from, the check that
/// advises of it, a part of the advisory, and whether the run stores.
const ADVISORIES: &[(Break, &str, &str, bool)] = &[
    (Break::RetrieveErrorEmpty, "dicomweb-error-responses", "answered 404 with no payload; a failure response payload should contain a Status Report explaining the error and whether it is temporary or permanent (PS3.18 2026d 10.4.3.3", false),
    (Break::ServerErrorEmpty, "dicomweb-error-responses", "/thumbnail answered 500 with no payload", false),
    (Break::SearchTooBroadEmpty, "dicomweb-error-responses", "answered 413 with no payload; a failure response payload should contain a Status Report explaining the error and whether it is temporary or permanent (PS3.18 2026d 10.6.3.1 Table 10.6.3-1", false),
    (Break::NotAcceptableUnlisted, "dicomweb-error-responses", "answered 406 with a payload that lists no Media Type", false),
    (Break::NoAcceptStatusReportJson, "dicomweb-accept-required", "it should be text/html", false),
    (Break::StoreConflictEmpty, "dicomweb-error-responses", "POST /dicomweb/studies answered 409 with no payload", true),
    (Break::Store202Empty, "dicomweb-error-responses", "answered 202 with no payload; the payload of a 202 should contain a Status Report", true),
    (Break::TypeQuoted, "dicomweb-payload-headers", "quote the \"type\" parameter, which should not contain quote marks", false),
    (Break::DuplicateContentId, "dicomweb-payload-headers", "Content-ID <frame@mock> names more than one part", false),
    (Break::IncludefieldIgnored, "dicomweb-search-matching-parameters", "includefield=00081030: the Study result lacks StudyDescription (0008,1030)", false),
    (Break::IncludefieldAllIgnored, "dicomweb-search-matching-parameters", "includefield=all: the Study result lacks StudyDescription (0008,1030)", false),
    (Break::StoreWarningWithoutStatus, "dicomweb-store", "holds no DICOM Status Code", true),
];

#[test]
fn each_unfollowed_recommendation_is_an_advisory_and_a_conforming_server_has_none() {
    let (root, slide) = slide();
    let file = root.path().join("store.dcm");
    std::fs::write(&file, &slide.file).unwrap();
    let conforming = run(Arc::clone(&slide), &[], |options| {
        options.store_file = Some(file.clone());
    });
    let advised: Vec<&String> = conforming
        .report
        .checks
        .iter()
        .flat_map(|check| &check.advisories)
        .collect();
    assert!(advised.is_empty(), "{advised:#?}");
    let mut missing = Vec::new();
    for &(broken, name, advisory, stores) in ADVISORIES {
        let run = run(Arc::clone(&slide), &[broken], |options| {
            if stores {
                options.store_file = Some(file.clone());
            }
        });
        let check = run.check(name);
        if !check
            .advisories
            .iter()
            .any(|found| found.contains(advisory))
        {
            missing.push(format!("{broken:?} {name}: {:?}", check.advisories));
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// The requests that ask for a 401 and a 405 are sent only when asked for:
/// a bad credential can raise alerts or lock an account, and security
/// scanners flag TRACE.
#[test]
fn the_error_response_probes_run_only_when_asked_for() {
    let (_root, slide) = slide();
    let probes = |requests: &[super::mock::MockRequest]| {
        requests
            .iter()
            .filter(|request| {
                request.method == "TRACE"
                    || request.header("authorization")
                        == Some("Bearer wsi-dicom-invalid-credential")
            })
            .count()
    };
    let default = run(Arc::clone(&slide), &[], |_| {});
    assert_eq!(probes(&default.requests), 0);
    let check = default.check("dicomweb-error-responses");
    assert_eq!(check.status, DicomWebStatus::NotRun, "{}", check.message);
    assert!(
        check.message.contains("requires --probe-error-responses"),
        "{}",
        check.message
    );
    let asked = run(Arc::clone(&slide), &[], |options| {
        options.probe_error_responses = true;
    });
    assert_eq!(probes(&asked.requests), 2);
    let check = asked.check("dicomweb-error-responses");
    assert_eq!(check.status, DicomWebStatus::Passed, "{}", check.message);
}

/// The stored copy is held to the file stored: the file's own failures are
/// noted, not laid on the origin server, and only a failure the file does
/// not have fails the readback.
#[test]
fn a_non_conformant_stored_file_is_noted_not_failed() {
    use dicom_dictionary_std::tags;
    let (root, slide) = slide();
    let path = root.path().join("non-conformant.dcm");
    std::fs::write(&path, &slide.file).unwrap();
    let mut object = dicom_object::open_file(&path).unwrap();
    object.remove_element(tags::IMAGE_TYPE);
    object.write_to_file(&path).unwrap();
    for (broken, status, reason) in [
        (
            None,
            DicomWebStatus::Passed,
            "the stored file was already non-conformant",
        ),
        (
            Some(Break::StoredCorrupted),
            DicomWebStatus::Failed,
            "which the file stored passes",
        ),
    ] {
        let breaks: Vec<Break> = broken.into_iter().collect();
        let run = run(Arc::clone(&slide), &breaks, |options| {
            options.store_file = Some(path.clone());
        });
        let check = run.check("dicomweb-store-readback");
        assert_eq!(check.status, status, "{broken:?}: {}", check.message);
        assert!(
            check.message.contains(reason),
            "{broken:?}: {}",
            check.message
        );
    }
}

#[test]
fn a_metadata_value_outside_its_vr_fails_the_json_model_check() {
    let (_root, slide) = slide();
    let run = run(Arc::clone(&slide), &[Break::MetadataBadDate], |_| {});
    let check = run.object_check("intrinsic-wsi-dicom-json-model");
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains("DA Value \"2026-01-01\"")
            && check.message.contains("F.1 and F.2.3"),
        "{}",
        check.message
    );
}

/// A status line without a valid 3-digit status code is reported, not
/// panicked on: ureq-proto 0.5, before 0.6.4, unwrapped a status below 100,
/// which a release build (panic = "abort") would have aborted on.
#[test]
fn malformed_status_lines_fail_the_response_check_without_panicking() {
    use super::mock::{MockResponse, MockServer};
    use crate::dicomweb::{check_dicomweb_server, DicomWebOptions, DicomWebTransaction};
    let many_headers = format!("HTTP/1.1 200 OK{}", "\r\nX-Header: 1".repeat(200));
    for status_line in [
        "HTTP/1.1 099 X",
        "HTTP/1.1 000 X",
        "HTTP/1.1 20 OK",
        "HTTP/1.1 2000 OK",
        "HTTP/1.1 abc X",
        "HTTP/1.1",
        "ICY 200 OK",
        // A header name with a control character, and more header fields
        // than the parser holds.
        "HTTP/1.1 200 OK\r\nX-Bad\u{1}: v",
        many_headers.as_str(),
    ] {
        let line = status_line.to_string();
        let server = MockServer::start(move |_| MockResponse {
            status_line: Some(line.clone()),
            ..MockResponse::status(200)
        });
        let options = DicomWebOptions {
            base_url: server.url("/dicomweb"),
            study: Some("1.2.3".into()),
            max_requests: 2,
            skip: vec![DicomWebTransaction::Search],
            ..DicomWebOptions::default()
        };
        let report = check_dicomweb_server(&options).unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.name == "dicomweb-response-messages")
            .unwrap();
        let shown: String = status_line.chars().take(40).collect();
        assert_eq!(
            check.status,
            DicomWebStatus::Failed,
            "{shown}: {}",
            check.message
        );
        assert!(
            check.message.contains("OPTIONS /dicomweb/")
                && check.message.contains("not an HTTP/1.1 response message"),
            "{shown}: {}",
            check.message
        );
    }
}

/// A header value of obs-text (UTF-8 bytes above 0x7F) is a valid field
/// value and reaches the checks without a panic.
#[test]
fn a_header_value_of_obs_text_is_read() {
    use super::mock::{MockResponse, MockServer};
    use crate::dicomweb::{check_dicomweb_server, DicomWebOptions, DicomWebTransaction};
    let server = MockServer::start(|_| MockResponse {
        status_line: Some("HTTP/1.1 405 Mock\r\nX-Text: caf\u{e9}\r\nAllow: GET".into()),
        ..MockResponse::status(405)
    });
    let options = DicomWebOptions {
        base_url: server.url("/dicomweb"),
        study: Some("1.2.3".into()),
        max_requests: 1,
        skip: vec![DicomWebTransaction::Search],
        ..DicomWebOptions::default()
    };
    let report = check_dicomweb_server(&options).unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.name == "dicomweb-response-messages")
        .unwrap();
    assert_eq!(check.status, DicomWebStatus::Passed, "{}", check.message);
}
