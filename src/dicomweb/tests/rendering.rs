//! The mock origin server's Rendered Frames: the Rendering Query Parameters
//! of PS3.18 8.3.5.1 applied to the slide's Frame size, and broken one way
//! at a time.

use std::collections::BTreeSet;

use super::mock::{MockRequest, MockResponse};
use super::origin::{jp2, named_profile, with_icc, Break, Slide};

/// The rendered Frames of `list`, negotiated and parameterized.
pub(super) fn rendered(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    list: &str,
    instance_base: &str,
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let accept = request.header("accept").unwrap_or_default();
    if accept.contains("x-wsidicom-unsupported")
        && list.contains(',')
        && !broken(Break::NoDefaultNot406)
    {
        return MockResponse::status(406);
    }
    if request.query("charset").is_some() && broken(Break::RenderedUtf8Rejected) {
        return MockResponse::status(406);
    }
    let mut warnings = Vec::new();
    if let Err(reason) = parameters(breaks, request, &mut warnings) {
        return MockResponse::new(400, "text/plain", reason);
    }
    let Some(size) = size(slide, breaks, request) else {
        return MockResponse::new(400, "text/plain", "ill-formed viewport");
    };
    let mut response = if accept.contains("image/png") {
        if broken(Break::RenderedNoPng) {
            return MockResponse::status(406);
        }
        MockResponse::new(200, "image/png", png(size.0, size.1))
    } else if accept.contains("image/jph") {
        let body = if broken(Break::JphWithoutJp2Box) {
            slide.frames[0].clone()
        } else {
            jp2(&slide.frames[0])
        };
        MockResponse::new(200, "image/jph", body)
    } else if broken(Break::RenderedWrongType) {
        MockResponse::new(200, "application/octet-stream", vec![0; 16])
    } else {
        jpeg(slide, breaks, request, size)
    };
    let base = instance_base.split("/studies/").next().unwrap_or_default();
    for warning in warnings {
        response = response.with("Warning", &format!("299 {base}: {warning}"));
    }
    response.with("Content-Location", &format!("{instance_base}/frames/1"))
}

/// The annotation, quality and window parameters (8.3.5.1.1, 8.3.5.1.2,
/// 8.3.5.1.4): the reason a value answers 400, or the Warnings it brings.
fn parameters(
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    warnings: &mut Vec<String>,
) -> Result<(), &'static str> {
    let broken = |kind| breaks.contains(&kind);
    if let Some(annotation) = request.decoded("annotation") {
        if annotation.is_empty() && !broken(Break::AnnotationEmptyAccepted) {
            return Err("annotation has no values");
        }
        let unsupported: Vec<&str> = annotation
            .split(',')
            .filter(|value| !value.is_empty() && !matches!(*value, "patient" | "technique"))
            .collect();
        if !unsupported.is_empty() {
            if broken(Break::AnnotationUnsupportedRejected) {
                return Err("unsupported annotation");
            }
            if !broken(Break::AnnotationWarningMissing) {
                warnings.push(format!(
                    "The following annotation values are not supported: {}",
                    unsupported.join(",")
                ));
            }
        }
    }
    if let Some(quality) = request.decoded("quality") {
        let valid = quality
            .parse::<u32>()
            .is_ok_and(|quality| (1..=100).contains(&quality));
        if valid && broken(Break::QualityRejected) {
            return Err("quality not supported");
        }
        if !valid && !broken(Break::QualityOutOfRangeAccepted) {
            return Err("quality is between 1 and 100");
        }
    }
    if let Some(window) = request.decoded("window") {
        let fields: Vec<&str> = window.split(',').collect();
        let valid = matches!(fields.as_slice(), [center, width, function]
            if center.parse::<f64>().is_ok()
                && width.parse::<f64>().is_ok()
                && matches!(*function, "linear" | "linear-exact" | "sigmoid"));
        if valid && broken(Break::WindowRejected) {
            return Err("window not supported");
        }
        if !valid && !broken(Break::WindowIllFormedAccepted) {
            return Err("window has center, width and function");
        }
    }
    Ok(())
}

/// The rendered size (8.3.5.1.3): the region sx, sy, sw, sh of the Frame,
/// scaled into the viewport; `None` for an ill-formed viewport.
fn size(slide: &Slide, breaks: &BTreeSet<Break>, request: &MockRequest) -> Option<(u32, u32)> {
    let broken = |kind| breaks.contains(&kind);
    let (columns, rows) = (slide.columns, slide.rows);
    let Some(viewport) = request.decoded("viewport") else {
        return Some(if broken(Break::RenderedUnscaledWrongSize) {
            (columns + 7, rows + 7)
        } else {
            (columns, rows)
        });
    };
    let fields: Vec<&str> = viewport.split(',').collect();
    let number = |index: usize| {
        fields
            .get(index)
            .and_then(|field| field.parse::<u32>().ok())
    };
    let (Some(width), Some(height)) = (number(0), number(1)) else {
        return broken(Break::ViewportIllFormedAccepted).then_some((columns, rows));
    };
    if broken(Break::RenderedWrongSize) {
        return Some((columns, rows));
    }
    let given = |index: usize| fields.get(index).filter(|field| !field.is_empty());
    let sx = given(2)
        .and_then(|field| field.parse::<u32>().ok())
        .unwrap_or(0);
    let sy = given(3)
        .and_then(|field| field.parse::<u32>().ok())
        .unwrap_or(0);
    let elided = if broken(Break::ElidedDefaultsWrong) {
        1
    } else {
        columns - sx
    };
    let mut region = (
        given(4)
            .and_then(|field| field.parse().ok())
            .unwrap_or(elided),
        given(5)
            .and_then(|field| field.parse().ok())
            .unwrap_or(rows - sy),
    );
    if broken(Break::CropIgnored) {
        region = (columns, rows);
    }
    if fields.len() == 2 && broken(Break::ViewportCropsByDefault) {
        region = (columns, (rows / 2).max(1));
    }
    Some(fit(region, (width, height)))
}

/// A region scaled into the viewport, keeping its aspect ratio.
fn fit((width, height): (u32, u32), (into_width, into_height): (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(into_width) / f64::from(width.max(1)))
        .min(f64::from(into_height) / f64::from(height.max(1)));
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

/// A baseline JPEG of `size`, with the ICC Profile its iccprofile value
/// names (8.3.5.1.5).
fn jpeg(
    slide: &Slide,
    breaks: &BTreeSet<Break>,
    request: &MockRequest,
    size: (u32, u32),
) -> MockResponse {
    let broken = |kind| breaks.contains(&kind);
    let mut jpeg = crate::test_support::encode_test_jpeg(size.0, size.1, [200, 120, 80]);
    if broken(Break::RenderedProgressive) {
        if let Some(at) = jpeg.windows(2).position(|pair| pair == [0xFF, 0xC0]) {
            jpeg[at + 1] = 0xC2;
        }
    }
    let profile = match request.decoded("iccprofile").as_deref() {
        Some("no") if broken(Break::IccNoIgnored) => Some(slide.icc.clone()),
        Some("yes") if broken(Break::IccYesMissing) => None,
        Some("yes") if broken(Break::IccYesNotSource) => Some(named_profile("displayp3")),
        Some("yes") => Some(slide.icc.clone()),
        Some("srgb") if broken(Break::IccSrgbWrong) => Some(named_profile("adobergb")),
        Some("adobergb") if broken(Break::IccAdobeWrong) => Some(named_profile("srgb")),
        Some("rommrgb") if broken(Break::IccRommWrong) => Some(named_profile("srgb")),
        Some("displayp3") if broken(Break::IccP3Wrong) => Some(named_profile("srgb")),
        Some(space @ ("srgb" | "adobergb" | "rommrgb" | "displayp3")) => Some(named_profile(space)),
        _ => None,
    };
    if let Some(profile) = profile {
        let profile = profile.strip_suffix(&[0]).unwrap_or(&profile).to_vec();
        jpeg = with_icc(jpeg, &profile);
    }
    let content_type = if broken(Break::RenderedLabelledMultipart) {
        "multipart/related; type=\"image/jpeg\"; boundary=mock-boundary-7f3a"
    } else if broken(Break::RenderedWithSyntax) {
        "image/jpeg; transfer-syntax=1.2.840.10008.1.2.4.50"
    } else {
        "image/jpeg"
    };
    MockResponse::new(200, content_type, jpeg)
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
