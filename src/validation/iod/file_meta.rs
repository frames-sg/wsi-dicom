//! PS3.10 7.1 File Meta Information, parsed from the encoded bytes.
//!
//! dicom-object substitutes defaults for missing File Meta elements and
//! recomputes the group length, so the group is re-read here as encoded.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::engine::{by_tag, flatten, Presence, Walk};
use super::report::{tag_text, Family, Findings};
use super::scope::{raw_tag, string_value, Scope};
use super::tables::FILE_META;
use super::values::{Encoding, ValueChecks};
use crate::validation::transfer_syntaxes::is_jpip_referenced;

const PREAMBLE: usize = 128;
const PREFIX: &[u8; 4] = b"DICM";
/// Largest File Meta Information group accepted (PS3.10 sets no bound).
const MAX_GROUP_LENGTH: u32 = 1024 * 1024;
const CITATION: &str = "PS3.10 7.1 Table 7.1-1";

struct RawElement {
    tag: Tag,
    vr: VR,
    value: Vec<u8>,
}

struct RawGroup {
    elements: Vec<RawElement>,
    length: u32,
    /// Little Endian group number of the first tag after the group, if any.
    next_group: Option<u16>,
}

/// Checks the File Meta Information of `path` against Table 7.1-1.
pub(super) fn check_file_meta(path: &Path, object: &DefaultDicomObject, findings: &mut Findings) {
    let family = &mut findings.file_meta;
    let Some(meta) = check_encoded_group(path, family) else {
        return;
    };
    for (meta_tag, meta_name, data_set_tag, name) in [
        (
            tags::MEDIA_STORAGE_SOP_CLASS_UID,
            "Media Storage SOP Class UID (0002,0002)",
            tags::SOP_CLASS_UID,
            "SOP Class UID (0008,0016)",
        ),
        (
            tags::MEDIA_STORAGE_SOP_INSTANCE_UID,
            "Media Storage SOP Instance UID (0002,0003)",
            tags::SOP_INSTANCE_UID,
            "SOP Instance UID (0008,0018)",
        ),
    ] {
        let declared = string_value(&meta, meta_tag, 1);
        let actual = string_value(object, data_set_tag, 1);
        if declared.is_some() && declared != actual {
            family.push(|| {
                format!(
                    "{meta_name} {:?} does not match {name} {:?} of the Data Set ({CITATION})",
                    declared.unwrap_or_default(),
                    actual.unwrap_or_default()
                )
            });
        }
    }
    let root: &InMemDicomObject = object;
    if let Some(element) = root
        .iter()
        .find(|element| element.header().tag.group() == 0x0002)
    {
        let tag = element.header().tag;
        family.push(|| format!(
            "{} is encoded in the Data Set; group 0002 belongs only to the File Meta Information (PS3.10 7.1)",
            tag_text(raw_tag(tag))
        ));
    }
}

/// Checks the File Meta Information of a file whose Data Set cannot be
/// parsed, such as one without a Transfer Syntax UID. Returns the declared
/// Media Storage SOP Class UID, or `None` when the group cannot be read or
/// does not declare one.
pub(super) fn check_unparsed_file_meta(path: &Path, findings: &mut Findings) -> Option<String> {
    check_encoded_group(path, &mut findings.file_meta)
        .and_then(|meta| string_value(&meta, tags::MEDIA_STORAGE_SOP_CLASS_UID, 1))
}

/// Where the Data Set of `path` starts, after the preamble, prefix and group
/// 0002 as its Group Length frames them, and the declared Transfer Syntax UID
/// (without padding). Framing defects of the group itself are the File Meta
/// check's findings, not repeated here.
pub(in crate::validation) fn data_set_start(path: &Path) -> Result<(u64, String), String> {
    let group = read_group(path, &mut Family::default())?;
    let transfer_syntax = group
        .elements
        .iter()
        .find(|element| element.tag == tags::TRANSFER_SYNTAX_UID)
        .map(|element| {
            String::from_utf8_lossy(&element.value)
                .trim_end_matches(['\0', ' '])
                .to_string()
        })
        .ok_or("the File Meta Information has no Transfer Syntax UID (0002,0010)")?;
    let offset = (PREAMBLE + 4 + 12) as u64 + u64::from(group.length);
    Ok((offset, transfer_syntax))
}

/// Checks the encoded group 0002 of `path` on its own: framing, Table 7.1-1
/// presence, VR, VM and value format, and the version. Returns the group as
/// a data set, or `None` (after recording why) when it cannot be read.
fn check_encoded_group(path: &Path, family: &mut Family) -> Option<InMemDicomObject> {
    let group = match read_group(path, family) {
        Ok(group) => group,
        Err(reason) => {
            family.push(|| format!("{reason} ({CITATION})"));
            return None;
        }
    };
    let elements = &group.elements;
    let meta = meta_object(elements);
    let transfer_syntax = string_value(&meta, tags::TRANSFER_SYNTAX_UID, 1).unwrap_or_default();
    // A Deflated Data Set is compressed, so the bytes after group 0002 are not a tag.
    if group.next_group == Some(0x0002)
        && transfer_syntax != uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN
    {
        family.push(|| format!(
            "File Meta Information Group Length {} ends before the last group 0002 element ({CITATION})",
            group.length
        ));
    }
    if is_jpip_referenced(&transfer_syntax) {
        family.push(|| format!(
            "Transfer Syntax UID (0002,0010) {transfer_syntax:?} is a JPIP Referenced Transfer Syntax, which shall not be used for PS3.10 media storage (PS3.5 {} 10.8, {CITATION})",
            super::tables::EDITION
        ));
    }
    let scope = Scope::top(&meta, &transfer_syntax);
    let mut table_findings = Findings::default();
    let mut definitions = Vec::new();
    flatten(FILE_META.nodes, FILE_META, &mut definitions);
    Walk {
        findings: &mut table_findings,
    }
    .data_set(&scope, "", &by_tag(&definitions), Presence::Evaluate, 0);
    ValueChecks {
        vr_vm: &mut table_findings.vr_vm,
        format: &mut table_findings.value_format,
        encoding: Encoding {
            explicit_vr: true,
            extended_characters: false,
        },
    }
    .data_set(&meta, "", 0);
    family.absorb(table_findings.presence);
    family.absorb(table_findings.vr_vm);
    family.absorb(table_findings.value_format);
    if let Some(version) = elements
        .iter()
        .find(|element| element.tag == tags::FILE_META_INFORMATION_VERSION)
    {
        if version.value.len() != 2 {
            family.push(|| format!(
                "File Meta Information Version (0002,0001) is {:02X?}; the version bit field shall contain two bytes ({CITATION})",
                version.value
            ));
        } else if version.value[1] & 0x01 == 0 {
            family.push(|| format!(
                "File Meta Information Version (0002,0001) is {:02X?}; bit 0 of the second byte is not set for version 1 compatibility ({CITATION})",
                version.value
            ));
        }
    }
    for (tag, name) in [
        (
            tags::SOURCE_PRESENTATION_ADDRESS,
            "Source Presentation Address (0002,0026)",
        ),
        (
            tags::SENDING_PRESENTATION_ADDRESS,
            "Sending Presentation Address (0002,0027)",
        ),
        (
            tags::RECEIVING_PRESENTATION_ADDRESS,
            "Receiving Presentation Address (0002,0028)",
        ),
    ] {
        let Some(address) = string_value(&meta, tag, 1) else {
            continue;
        };
        if let Err(reason) = presentation_address(&address) {
            family.push(|| format!("{name} \"{address}\" {reason} (PS3.10 7.1.1.1)"));
        }
    }
    Some(meta)
}

/// PS3.10 7.1.1.1: `dicom:<host>:<port>` for the DICOM Upper Layer Protocol,
/// or the absolute `http` URL of a Web Services endpoint whatever transport
/// security was used.
fn presentation_address(address: &str) -> Result<(), &'static str> {
    if let Some(rest) = address.strip_prefix("dicom:") {
        let (host, port) = rest.rsplit_once(':').unwrap_or((rest, ""));
        let valid_port = !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok();
        return if !host.is_empty() && !host.contains('/') && valid_port {
            Ok(())
        } else {
            Err("is not \"dicom:\" followed by a host name or IP address, a colon and a port number")
        };
    }
    if let Some(rest) = address.strip_prefix("http://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        return if authority.is_empty() {
            Err("is not an absolute URL; it does not name the host of the endpoint")
        } else {
            Ok(())
        };
    }
    if address.starts_with("https:") {
        return Err("uses the scheme \"https\"; a Web Services presentation address uses \"http\" whether or not secure transport was used");
    }
    Err("is neither a DICOM Upper Layer address \"dicom:<host>:<port>\" nor an \"http\" Web Services URL")
}

/// Reads the preamble, prefix and group 0002 as Explicit VR Little Endian.
fn read_group(path: &Path, family: &mut Family) -> Result<RawGroup, String> {
    let file = File::open(path).map_err(|err| format!("cannot open file: {err}"))?;
    let mut reader = file.take(u64::from(MAX_GROUP_LENGTH) + 256);
    let mut head = [0u8; PREAMBLE + 4 + 12];
    reader
        .read_exact(&mut head)
        .map_err(|err| format!("cannot read the File Preamble and DICOM Prefix: {err}"))?;
    if &head[PREAMBLE..PREAMBLE + 4] != PREFIX {
        return Err("128-byte File Preamble followed by the \"DICM\" Prefix is missing".into());
    }
    let first = &head[PREAMBLE + 4..];
    let group_length_tag = Tag(u16_at(first, 0), u16_at(first, 2));
    if group_length_tag != tags::FILE_META_INFORMATION_GROUP_LENGTH
        || &first[4..6] != b"UL"
        || u16_at(first, 6) != 4
    {
        return Err(
            "the first File Meta element is not File Meta Information Group Length (0002,0000) UL with Value Length 4"
                .into(),
        );
    }
    let group_length = u32::from_le_bytes([first[8], first[9], first[10], first[11]]);
    if group_length > MAX_GROUP_LENGTH {
        return Err(format!(
            "File Meta Information Group Length {group_length} exceeds the {MAX_GROUP_LENGTH}-byte bound"
        ));
    }
    let mut group = vec![0u8; group_length as usize];
    reader.read_exact(&mut group).map_err(|err| {
        format!("File Meta Information is shorter than its Group Length {group_length}: {err}")
    })?;
    let mut elements = vec![RawElement {
        tag: group_length_tag,
        vr: VR::UL,
        value: first[8..12].to_vec(),
    }];
    let mut offset = 0usize;
    while offset < group.len() {
        let element = parse_element(&group, &mut offset, family)?;
        if let Some(previous) = elements.last() {
            if element.tag <= previous.tag {
                family.push(|| {
                    format!(
                    "{} follows {}; File Meta elements shall be in ascending tag order (PS3.5 7.1)",
                    tag_text(raw_tag(element.tag)),
                    tag_text(raw_tag(previous.tag))
                )
                });
            }
        }
        elements.push(element);
    }
    let mut next = [0u8; 2];
    let next_group = reader
        .read_exact(&mut next)
        .is_ok()
        .then(|| u16::from_le_bytes(next));
    Ok(RawGroup {
        elements,
        length: group_length,
        next_group,
    })
}

fn parse_element(
    group: &[u8],
    offset: &mut usize,
    family: &mut Family,
) -> Result<RawElement, String> {
    let start = *offset;
    let header = group
        .get(start..start + 8)
        .ok_or("File Meta Information Group Length cuts an element header")?;
    let tag = Tag(u16_at(header, 0), u16_at(header, 2));
    let vr = VR::from_binary([header[4], header[5]])
        .ok_or_else(|| format!("{} has an invalid VR", tag_text(raw_tag(tag))))?;
    let (length, value_start) = if long_length(vr) {
        let extended = group
            .get(start + 8..start + 12)
            .ok_or("File Meta Information Group Length cuts an element header")?;
        if u16_at(header, 6) != 0 {
            family.push(|| {
                format!(
                    "{}: reserved bytes after the VR are not 0000H (PS3.5 7.1.2)",
                    tag_text(raw_tag(tag))
                )
            });
        }
        (
            u32::from_le_bytes([extended[0], extended[1], extended[2], extended[3]]) as usize,
            start + 12,
        )
    } else {
        (usize::from(u16_at(header, 6)), start + 8)
    };
    if tag.group() != 0x0002 {
        return Err(format!(
            "{} lies inside the File Meta Information Group Length",
            tag_text(raw_tag(tag))
        ));
    }
    let value = group
        .get(value_start..value_start.saturating_add(length))
        .ok_or_else(|| {
            format!(
                "{} extends beyond the File Meta Information Group Length",
                tag_text(raw_tag(tag))
            )
        })?
        .to_vec();
    *offset = value_start + length;
    Ok(RawElement { tag, vr, value })
}

/// PS3.5 Table 7.1-1: VRs with a 4-byte Value Length in Explicit VR.
pub(in crate::validation) fn long_length(vr: VR) -> bool {
    matches!(
        vr,
        VR::OB
            | VR::OD
            | VR::OF
            | VR::OL
            | VR::OV
            | VR::OW
            | VR::SQ
            | VR::SV
            | VR::UC
            | VR::UN
            | VR::UR
            | VR::UT
            | VR::UV
    )
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// The group as an in-memory Data Set, so the generic checks apply to it.
fn meta_object(elements: &[RawElement]) -> InMemDicomObject {
    let mut object = InMemDicomObject::new_empty();
    for element in elements {
        let value = match element.vr {
            VR::UL => PrimitiveValue::U32(
                element
                    .value
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| u32::from_le_bytes(*chunk))
                    .collect(),
            ),
            VR::AE | VR::CS | VR::SH | VR::LO | VR::UI => {
                let text: String = element.value.iter().map(|byte| char::from(*byte)).collect();
                PrimitiveValue::Strs(text.split('\\').map(str::to_string).collect())
            }
            VR::UR | VR::UT | VR::ST | VR::LT => {
                PrimitiveValue::Str(element.value.iter().map(|byte| char::from(*byte)).collect())
            }
            _ => PrimitiveValue::U8(element.value.clone().into()),
        };
        let length = u32::try_from(element.value.len()).unwrap_or(u32::MAX);
        object.put(DataElement::new_with_len(
            element.tag,
            element.vr,
            dicom_core::Length(length),
            value,
        ));
    }
    object
}
