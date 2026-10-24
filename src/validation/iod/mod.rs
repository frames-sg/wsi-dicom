//! Standard-derived requirement checks for the pathology-slide IODs: VL Whole
//! Slide Microscopy Image, Microscopy Bulk Simple Annotations, Segmentation,
//! Parametric Map, the Key Object Selection Document and the Basic Text,
//! Enhanced, Comprehensive and Comprehensive 3D SR documents; and for the
//! Basic Directory IOD of a File-set's DICOMDIR.
//!
//! `tables.rs` and `dictionary.rs` are generated from the pinned DICOM DocBook
//! sources by `scripts/generate-iod-tables.py`; `conditions.rs` holds one
//! hand-written predicate per generated condition. An object is held to the
//! IOD its SOP Class UID names in PS3.4 Table B.5-1.

mod codes;
mod conditions;
// PS3.15 Annex E as data; the de-identification rule reads it. Its whole
// surface is generated, whether or not the rule reads each item yet.
#[allow(dead_code)]
#[rustfmt::skip]
mod confidentiality;
mod coverage;
// The full lookup surface of Tables E.1-1 and E.3.4-1 and CID 7050, of which
// the de-identification rule reads part.
#[allow(dead_code, unused_imports)]
pub(in crate::validation) mod deidentification;
#[rustfmt::skip]
mod dictionary;
mod dispositions;
mod engine;
mod file_meta;
#[allow(dead_code, unused_imports)]
pub(in crate::validation) mod file_set;
mod item_character_set;
mod model;
mod modules;
mod recommendations;
mod report;
#[rustfmt::skip]
mod requirements;
mod scope;
#[allow(dead_code, unused_imports)]
pub(in crate::validation) mod structured_report;
#[rustfmt::skip]
mod tables;
mod templates;
#[rustfmt::skip]
mod value_sets;
mod values;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::Path;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::{
    intrinsic_check, RequirementDisposition, RequirementEntry, ValidationCheck, ValidationStatus,
};
use crate::validation::wsi_conformance::rule_path;
use model::Iod;
use report::Findings;
use scope::Scope;
use values::{Encoding, ValueChecks};

// Shared with the encoding check, which re-reads the Data Set from its bytes.
pub(in crate::validation) use file_meta::{data_set_start, long_length};
pub(in crate::validation) use item_character_set::item_admits_specific_character_set;
pub(in crate::validation) use report::{tag_text, Family};
pub(in crate::validation) use values::{dictionary_vrs, element_label};
// Attribute readers and coded entries the WSI rules share.
pub(in crate::validation) use codes::Coded;
pub(in crate::validation) use scope::{
    integer_value, is_private, items, raw_tag, string_value, tag_values,
};

const IOD_MODULES_RULE: &str = "intrinsic-wsi-dicom-iod-modules";
const IOD_ATTRIBUTE_PRESENCE_RULE: &str = "intrinsic-wsi-dicom-iod-attribute-presence";
const IOD_ENUMERATED_VALUES_RULE: &str = "intrinsic-wsi-dicom-iod-enumerated-values";
const IOD_VR_VM_RULE: &str = "intrinsic-wsi-dicom-iod-vr-vm";
const IOD_VALUE_FORMAT_RULE: &str = "intrinsic-wsi-dicom-iod-value-format";
const IOD_SEQUENCE_ITEMS_RULE: &str = "intrinsic-wsi-dicom-iod-sequence-items";
const IOD_VALUE_SETS_RULE: &str = "intrinsic-wsi-dicom-iod-value-sets";
const FILE_META_RULE: &str = "intrinsic-wsi-dicom-file-meta";

/// Deepest Sequence nesting walked; deeper Items are reported, not checked.
pub(in crate::validation) const MAX_NESTING: usize = 32;
/// The DICOM edition the requirement tables are generated from.
pub(in crate::validation) const EDITION: &str = tables::EDITION;
pub(crate) use tables::dicom_edition;

/// One object's optional conformance: the optional items it encodes and those
/// it does not, and its departures from Baseline Context Groups and Templates
/// and undeclared Context Group extensions.
pub(in crate::validation) struct OptionalNotes {
    pub(in crate::validation) present: Vec<String>,
    pub(in crate::validation) absent: Vec<String>,
    pub(in crate::validation) advisories: Vec<String>,
}

/// Inputs that a source other than a PS3.10 file, such as a DICOM JSON
/// document, cannot supply to the IOD checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::validation) struct Unavailable {
    /// Why the encoded File Meta Information is unavailable. The File Meta
    /// check then does not read the source and is reported skipped with
    /// this reason.
    pub(in crate::validation) file_meta: Option<String>,
    /// Whether the top-level Pixel Data, Float Pixel Data and Double Float
    /// Pixel Data Values are stand-ins (e.g. for a BulkDataURI): their
    /// presence counts, but their VR, VM and value format are not checked.
    pub(in crate::validation) pixel_data: bool,
}

/// The top-level Pixel Data Attributes of PS3.3 C.7.6.3, C.7.6.24 and C.7.6.25.
const PIXEL_DATA_TAGS: [dicom_core::Tag; 3] = [
    tags::PIXEL_DATA,
    tags::FLOAT_PIXEL_DATA,
    tags::DOUBLE_FLOAT_PIXEL_DATA,
];

/// PS3.5 8.2: "It is not permitted to have more than one of Pixel Data
/// Provider URL (0028,7FE0), Pixel Data (7FE0,0010), Float Pixel Data
/// (7FE0,0008) or Double Float Pixel Data (7FE0,0009) in the top level Data
/// Set."
fn single_pixel_data(findings: &mut Findings, root: &InMemDicomObject) {
    let present: Vec<dicom_core::Tag> = [tags::PIXEL_DATA_PROVIDER_URL]
        .into_iter()
        .chain(PIXEL_DATA_TAGS)
        .filter(|tag| root.get(*tag).is_some())
        .collect();
    if present.len() > 1 {
        findings.presence.push(|| {
            format!(
                "{} are present; the top level Data Set holds at most one of Pixel Data Provider URL, Pixel Data, Float Pixel Data and Double Float Pixel Data (PS3.5 {} 8.2)",
                present
                    .iter()
                    .map(|tag| report::tag_text(scope::raw_tag(*tag)))
                    .collect::<Vec<_>>()
                    .join(", "),
                tables::EDITION
            )
        });
    }
}

/// Runs every IOD requirement family for the pathology-slide IOD that the
/// Data Set's SOP Class UID names, or else the File Meta Information's, with
/// the optional notes when `conformance` is `Optional`. An object of another
/// SOP Class gets one check saying that no IOD rules apply to it.
pub(super) fn run_iod_checks(
    path: &Path,
    object: &DefaultDicomObject,
    conformance: crate::ConformanceLevel,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    run_iod_checks_on(path, object, &Unavailable::default(), conformance)
}

/// `run_iod_checks` for a parsed object whose results are labelled `label`,
/// a path or a document locator. The File Meta check reads the encoded
/// group 0002 at `label` unless `unavailable.file_meta` says why it cannot;
/// nothing else reads `label`. The object's File Meta table supplies the
/// Transfer Syntax and the fallback SOP Class.
pub(in crate::validation) fn run_iod_checks_on(
    label: &Path,
    object: &DefaultDicomObject,
    unavailable: &Unavailable,
    conformance: crate::ConformanceLevel,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let path = label;
    let iod = match dispatch(object) {
        Ok(iod) => iod,
        Err(no_iod) => return (vec![no_iod.check(path)], None),
    };
    let findings = evaluate(path, object, iod, unavailable);
    let notes = conformance.is_optional().then(|| OptionalNotes {
        present: findings.optional.describe_present(),
        absent: findings.optional.describe_absent(),
        advisories: findings.advisories.describe(),
    });
    let edition = tables::EDITION;
    let section = iod.section;
    let module_tables = match iod.functional_group_table {
        Some(groups) => format!("Tables {} and {groups}", iod.module_table),
        None => format!("Table {}", iod.module_table),
    };
    let checks = vec![
        findings.modules.check(
            path,
            IOD_MODULES_RULE,
            &format!("DICOM PS3.3 {edition} {section} {module_tables}"),
        ),
        findings.presence.check(
            path,
            IOD_ATTRIBUTE_PRESENCE_RULE,
            &format!("DICOM PS3.3 {edition} {section} and PS3.5 7.4"),
        ),
        findings.enumerated.check(
            path,
            IOD_ENUMERATED_VALUES_RULE,
            &format!("DICOM PS3.3 {edition} {section} Enumerated Values"),
        ),
        findings.vr_vm.check(
            path,
            IOD_VR_VM_RULE,
            &format!("DICOM PS3.6 {edition} Tables 6-1 and 7-1"),
        ),
        findings.value_format.check(
            path,
            IOD_VALUE_FORMAT_RULE,
            &format!("DICOM PS3.5 {edition} 6.2 and 7.1"),
        ),
        findings.sequence_items.check(
            path,
            IOD_SEQUENCE_ITEMS_RULE,
            &format!("DICOM PS3.3 {edition} {section} Sequence Item counts"),
        ),
        findings.value_sets.check(
            path,
            IOD_VALUE_SETS_RULE,
            &format!("DICOM PS3.3 {edition} 8.8 and PS3.16 {edition} Defined Context Groups and Templates"),
        ),
        match &unavailable.file_meta {
            None => findings
                .file_meta
                .check(path, FILE_META_RULE, &file_meta_citation()),
            Some(reason) => intrinsic_check(
                FILE_META_RULE,
                rule_path(path),
                ValidationStatus::Skipped,
                format!("{}: {reason}", file_meta_citation()),
            ),
        },
    ];
    (checks, notes)
}

/// Why no IOD's rules apply to an object.
enum NoIod {
    /// A SOP Class of no pathology-slide IOD.
    Unsupported(String),
    /// Neither the Data Set nor the File Meta Information names a SOP Class.
    Undeclared,
}

impl NoIod {
    /// The check result of an object no IOD rules apply to: skipped for
    /// another SOP Class, failed when the object names none.
    fn check(&self, path: &Path) -> ValidationCheck {
        let citation = format!(
            "DICOM PS3.3 {} Annex A and PS3.4 Table B.5-1",
            tables::EDITION
        );
        let (status, reason) = match self {
            Self::Unsupported(uid) => (
                ValidationStatus::Skipped,
                format!(
                    "SOP Class UID \"{uid}\" ({}) is not a SOP Class of the pathology-slide IODs ({}); no IOD rules apply",
                    sop_class_name(uid),
                    iod_list()
                ),
            ),
            Self::Undeclared => (
                ValidationStatus::Failed,
                format!(
                    "neither SOP Class UID (0008,0016) nor Media Storage SOP Class UID (0002,0002) names a SOP Class, so no IOD ({}) can be applied",
                    iod_list()
                ),
            ),
        };
        intrinsic_check(
            IOD_MODULES_RULE,
            rule_path(path),
            status,
            format!("{citation}: {reason}"),
        )
    }
}

/// The Data Set's SOP Class UID selects the IOD; the File Meta Information's
/// Media Storage SOP Class UID does when the Data Set's names none of them.
fn dispatch(object: &DefaultDicomObject) -> Result<&'static Iod, NoIod> {
    let declared = object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .map(|uid| trim_uid(&uid).to_string())
        .filter(|uid| !uid.is_empty());
    let meta = Some(trim_uid(object.meta().media_storage_sop_class_uid()).to_string())
        .filter(|uid| !uid.is_empty());
    if let Some(iod) = [&declared, &meta]
        .into_iter()
        .flatten()
        .find_map(|uid| iod_of(uid.as_str()))
    {
        return Ok(iod);
    }
    Err(declared
        .or(meta)
        .map_or(NoIod::Undeclared, NoIod::Unsupported))
}

fn trim_uid(uid: &str) -> &str {
    uid.trim_end_matches(['\0', ' '])
}

/// The pathology-slide IOD whose SOP Classes include `uid`, or the Basic
/// Directory IOD for the Media Storage Directory Storage SOP Class of a
/// DICOMDIR (PS3.10 8.6).
fn iod_of(uid: &str) -> Option<&'static Iod> {
    tables::IODS
        .iter()
        .chain(std::iter::once(&tables::BASIC_DIRECTORY))
        .find(|iod| iod.sop_classes.iter().any(|class| class.uid == uid))
}

/// "A.32.8 VL Whole Slide Microscopy Image, A.32.2 VL Microscopic Image, ...".
fn iod_list() -> String {
    tables::IODS
        .iter()
        .map(|iod| format!("{} {}", iod.section, iod.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The PS3.4 Table B.5-1 name of a Storage SOP Class.
fn sop_class_name(uid: &str) -> &'static str {
    tables::STORAGE_SOP_CLASSES
        .binary_search_by_key(&uid, |class| class.uid)
        .map_or(
            "not a Standard Storage SOP Class of PS3.4 Table B.5-1",
            |index| tables::STORAGE_SOP_CLASSES[index].name,
        )
}

/// Whether the Data Set's SOP Class UID is VL Whole Slide Microscopy.
pub(super) fn declares_vl_wsi(object: &DefaultDicomObject) -> bool {
    object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .is_some_and(|uid| trim_uid(&uid) == crate::VL_WSI_SOP_CLASS_UID)
}

fn file_meta_citation() -> String {
    format!("DICOM PS3.10 {} 7.1", tables::EDITION)
}

/// The File Meta check of a file the DICOM parser could not open. It runs
/// when the encoded group declares the SOP Class of a pathology-slide IOD, or
/// cannot be read well enough to tell; for another SOP Class no IOD rules
/// apply.
pub(super) fn run_unparsed_file_meta_check(path: &Path) -> ValidationCheck {
    let mut findings = Findings::default();
    let sop_class = file_meta::check_unparsed_file_meta(path, &mut findings);
    match sop_class.as_deref().map(trim_uid) {
        Some(uid) if iod_of(uid).is_none() => NoIod::Unsupported(uid.to_string()).check(path),
        _ => findings
            .file_meta
            .check(path, FILE_META_RULE, &file_meta_citation()),
    }
}

fn evaluate(
    path: &Path,
    object: &DefaultDicomObject,
    iod: &'static Iod,
    unavailable: &Unavailable,
) -> Findings {
    let mut findings = Findings::default();
    if unavailable.file_meta.is_none() {
        let directory = std::ptr::eq(iod, &tables::BASIC_DIRECTORY);
        file_meta::check_file_meta(path, object, directory, &mut findings);
    }
    let root: &InMemDicomObject = object;
    let transfer_syntax = object.meta().transfer_syntax();
    let scope = Scope::top(root, transfer_syntax);
    modules::check_modules(&mut findings, &scope, iod);
    modules::check_functional_groups(&mut findings, &scope, iod);
    let without_pixels;
    let valued = if unavailable.pixel_data {
        let mut copy = root.clone();
        for tag in PIXEL_DATA_TAGS {
            copy.remove_element(tag);
        }
        without_pixels = copy;
        &without_pixels
    } else {
        root
    };
    single_pixel_data(&mut findings, root);
    ValueChecks {
        vr_vm: &mut findings.vr_vm,
        format: &mut findings.value_format,
        advisories: &mut findings.advisories,
        encoding: Encoding {
            explicit_vr: transfer_syntax != uids::IMPLICIT_VR_LITTLE_ENDIAN,
            extended_characters: extended_character_set(root),
        },
    }
    .data_set(valued, "", 0);
    findings
}

/// Every normative statement in the scope of the pathology-slide IODs, with
/// the IODs it applies to and how validation holds their objects to it.
pub(in crate::validation) fn catalog() -> Vec<RequirementEntry> {
    let entry = |id, statement: model::Statement, disposition, iod_dispositions| RequirementEntry {
        id,
        part: statement.part,
        section: statement.section,
        keyword: statement.keyword,
        text: statement.text,
        strength: statement.strength,
        disposition,
        iods: statement.iods.to_vec(),
        iod_dispositions,
    };
    requirements::TABLE_ENFORCED
        .iter()
        .map(|statement| {
            entry(
                None,
                *statement,
                RequirementDisposition::Tables,
                BTreeMap::new(),
            )
        })
        .chain(requirements::Requirement::ALL.iter().map(|requirement| {
            entry(
                Some(requirement.id()),
                requirement.statement(),
                requirement.disposition(),
                requirement.iod_dispositions(),
            )
        }))
        .collect()
}

/// PS3.5 6.2 Value format of one Value as the IOD value-format check applies
/// it, with any repertoire permitted; the writer validates metadata with it.
pub(crate) fn check_value_format(vr: dicom_core::VR, value: &str) -> Result<(), String> {
    values::check_value(vr, value, true)
}

/// Whether `uid` is a PS3.6 Image Storage SOP Class.
pub(crate) fn is_image_storage_sop_class(uid: &str) -> bool {
    dictionary::IMAGE_STORAGE_SOP_CLASSES
        .binary_search(&uid)
        .is_ok()
}

/// The PS3.6 VR alternatives and Value Multiplicity bounds of a registered tag.
pub(crate) fn registered(
    tag: dicom_core::Tag,
) -> Option<(&'static [dicom_core::VR], u32, Option<u32>)> {
    values::entry(tag).map(|entry| (entry.vr, entry.vm.min, entry.vm.max))
}

/// Whether Specific Character Set names a repertoire beyond ISO-IR 6.
fn extended_character_set(root: &InMemDicomObject) -> bool {
    root.get(tags::SPECIFIC_CHARACTER_SET)
        .and_then(|element| element.to_multi_str().ok())
        .is_some_and(|terms| {
            terms
                .iter()
                .map(|term| term.trim())
                .any(|term| !term.is_empty() && term != "ISO_IR 6" && term != "ISO 2022 IR 6")
        })
}
