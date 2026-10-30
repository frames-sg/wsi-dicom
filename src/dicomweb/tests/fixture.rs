//! Test fixtures: an exported VL Whole Slide Microscopy Instance, and its
//! DICOM JSON written per PS3.18 Annex F.

use std::path::{Path, PathBuf};

use base64::Engine;
use dicom_core::value::{PrimitiveValue, Value};
use dicom_core::{Tag, VR};
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use crate::TransferSyntax;

/// The first Instance exported from a synthetic RGB source of `width` by
/// `height` pixels in `tile` pixel tiles.
pub(crate) fn export_wsi(
    root: &Path,
    name: &str,
    (width, height, tile): (u32, u32, u32),
    transfer_syntax: TransferSyntax,
) -> PathBuf {
    let source = root.join(format!("{name}-source.dcm"));
    let pixels = crate::synthetic_source::deterministic_rgb_pixels(width, height).unwrap();
    crate::synthetic_source::write_rgb_source_dicom(
        &source,
        "1.2.826.0.1.3680043.10.999.401",
        "1.2.826.0.1.3680043.10.999.402",
        width,
        height,
        pixels,
    )
    .unwrap();
    crate::export_dicom(crate::ExportRequest {
        source_path: source,
        output_dir: root.join(format!("{name}-out")),
        options: crate::ExportOptions {
            tile_size: tile,
            transfer_syntax,
            encode_backend: crate::EncodeBackendPreference::CpuOnly,
            ..crate::ExportOptions::default()
        },
        color_management: crate::ColorManagement::SourceOrSrgb,
        metadata: crate::MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
    .instances
    .remove(0)
    .path
}

/// The exported Instance as read back.
pub(crate) fn open(path: &Path) -> DefaultDicomObject {
    dicom_object::open_file(path).unwrap()
}

/// The encapsulated Frames of an exported Instance, one Fragment each.
#[cfg(feature = "dicomweb")]
pub(crate) fn frames(object: &InMemDicomObject) -> Vec<Vec<u8>> {
    match object
        .get(dicom_dictionary_std::tags::PIXEL_DATA)
        .map(|element| element.value())
    {
        Some(Value::PixelSequence(sequence)) => sequence
            .fragments()
            .iter()
            .map(|fragment| fragment.to_vec())
            .collect(),
        _ => panic!("the exported Instance has encapsulated Pixel Data"),
    }
}

/// DICOM JSON (PS3.18 F.2) of `object` as an Array of one Data Set;
/// `bulk(tag, top_level)` names the BulkDataURI an Attribute is given as.
pub(crate) fn to_json(
    object: &InMemDicomObject,
    bulk: &dyn Fn(Tag, bool) -> Option<String>,
) -> String {
    format!("[{}]", data_set(object, bulk, true))
}

fn data_set(
    object: &InMemDicomObject,
    bulk: &dyn Fn(Tag, bool) -> Option<String>,
    top: bool,
) -> String {
    let members: Vec<String> = object
        .iter()
        .filter(|element| element.header().tag.element() != 0x0000)
        .map(|element| {
            let tag = element.header().tag;
            format!(
                "\"{:04X}{:04X}\":{}",
                tag.group(),
                tag.element(),
                attribute(element, bulk, top)
            )
        })
        .collect();
    format!("{{{}}}", members.join(","))
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

fn attribute(
    element: &InMemElement,
    bulk: &dyn Fn(Tag, bool) -> Option<String>,
    top: bool,
) -> String {
    let tag = element.header().tag;
    let vr = element.vr();
    let head = format!("\"vr\":\"{}\"", vr.to_string());
    if let Some(uri) = bulk(tag, top) {
        return format!("{{{head},\"BulkDataURI\":{}}}", quoted(&uri));
    }
    let payload = match element.value() {
        Value::Sequence(sequence) => {
            let items: Vec<String> = sequence
                .items()
                .iter()
                .map(|item| data_set(item, bulk, false))
                .collect();
            (!items.is_empty()).then(|| format!("\"Value\":[{}]", items.join(",")))
        }
        Value::PixelSequence(_) => Some("\"BulkDataURI\":\"bulk/pixeldata\"".to_string()),
        Value::Primitive(PrimitiveValue::Empty) => None,
        Value::Primitive(value) if is_blank(vr, value) => None,
        Value::Primitive(value) => Some(primitive(vr, value)),
    };
    match payload {
        Some(payload) => format!("{{{head},{payload}}}"),
        None => format!("{{{head}}}"),
    }
}

/// A text Value of only empty strings, which F.2.5 encodes without "Value".
fn is_blank(vr: VR, value: &PrimitiveValue) -> bool {
    let binary_or_numeric = matches!(
        vr,
        VR::OB
            | VR::OD
            | VR::OF
            | VR::OL
            | VR::OV
            | VR::OW
            | VR::UN
            | VR::US
            | VR::SS
            | VR::UL
            | VR::SL
            | VR::SV
            | VR::UV
            | VR::FL
            | VR::FD
            | VR::AT
    );
    !binary_or_numeric
        && value
            .to_multi_str()
            .iter()
            .all(|text| text.trim_end_matches(['\0', ' ']).is_empty())
}

fn primitive(vr: VR, value: &PrimitiveValue) -> String {
    let strings = || -> Vec<String> {
        value
            .to_multi_str()
            .iter()
            .map(|text| text.trim_end_matches(['\0', ' ']).to_string())
            .collect()
    };
    let values: Vec<String> = match vr {
        VR::OB | VR::OD | VR::OF | VR::OL | VR::OV | VR::OW | VR::UN => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(value.to_bytes());
            return format!("\"InlineBinary\":\"{encoded}\"");
        }
        VR::US | VR::SS | VR::UL | VR::SL | VR::SV | VR::UV => value
            .to_multi_int::<i128>()
            .unwrap()
            .iter()
            .map(i128::to_string)
            .collect(),
        VR::FL | VR::FD => value
            .to_multi_float64()
            .unwrap()
            .iter()
            .map(f64::to_string)
            .collect(),
        VR::IS => strings()
            .iter()
            .map(|text| match text.trim().parse::<i64>() {
                Ok(number) => number.to_string(),
                Err(_) => quoted(text),
            })
            .collect(),
        VR::AT => match value {
            PrimitiveValue::Tags(found) => found
                .iter()
                .map(|tag| format!("\"{:04X}{:04X}\"", tag.group(), tag.element()))
                .collect(),
            _ => panic!("AT holds Tags"),
        },
        VR::PN => strings()
            .iter()
            .map(|name| {
                if name.is_empty() {
                    return "null".to_string();
                }
                let groups: Vec<String> = ["Alphabetic", "Ideographic", "Phonetic"]
                    .iter()
                    .zip(name.split('='))
                    .filter(|(_, group)| !group.is_empty())
                    .map(|(key, group)| format!("\"{key}\":{}", quoted(group)))
                    .collect();
                format!("{{{}}}", groups.join(","))
            })
            .collect(),
        _ => strings()
            .iter()
            .map(|text| {
                if text.is_empty() {
                    "null".to_string()
                } else {
                    quoted(text)
                }
            })
            .collect(),
    };
    format!("\"Value\":[{}]", values.join(","))
}
