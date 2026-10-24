//! Standard-derived requirement checks for the VL Whole Slide Microscopy IOD.
//!
//! `tables.rs` and `dictionary.rs` are generated from the pinned DICOM DocBook
//! sources by `scripts/generate-iod-tables.py`; `conditions.rs` holds one
//! hand-written predicate per generated condition.

mod codes;
mod conditions;
#[rustfmt::skip]
mod dictionary;
mod dispositions;
mod engine;
mod file_meta;
mod item_character_set;
mod model;
mod modules;
mod report;
#[rustfmt::skip]
mod requirements;
mod scope;
#[rustfmt::skip]
mod tables;
mod templates;
#[rustfmt::skip]
mod value_sets;
mod values;

#[cfg(test)]
mod tests;

use std::path::Path;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::{RequirementDisposition, RequirementEntry, ValidationCheck};
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

/// Runs every IOD requirement family when either the Data Set or File Meta
/// Information declares a VL Whole Slide Microscopy SOP Instance, with the
/// optional notes when `conformance` is `Optional`.
pub(super) fn run_iod_checks(
    path: &Path,
    object: &DefaultDicomObject,
    conformance: crate::ConformanceLevel,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    if !parsed_file_applies(object) {
        return (Vec::new(), None);
    }
    let findings = evaluate(path, object);
    let notes = conformance.is_optional().then(|| OptionalNotes {
        present: findings.optional.describe_present(),
        absent: findings.optional.describe_absent(),
        advisories: findings.advisories.describe(),
    });
    let edition = tables::EDITION;
    let checks = vec![
        findings.modules.check(
            path,
            IOD_MODULES_RULE,
            &format!("DICOM PS3.3 {edition} A.32.8 Tables A.32.8-1 and A.32.8-2"),
        ),
        findings.presence.check(
            path,
            IOD_ATTRIBUTE_PRESENCE_RULE,
            &format!("DICOM PS3.3 {edition} A.32.8 and PS3.5 7.4"),
        ),
        findings.enumerated.check(
            path,
            IOD_ENUMERATED_VALUES_RULE,
            &format!("DICOM PS3.3 {edition} A.32.8 Enumerated Values"),
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
            &format!("DICOM PS3.3 {edition} A.32.8 Sequence Item counts"),
        ),
        findings.value_sets.check(
            path,
            IOD_VALUE_SETS_RULE,
            &format!("DICOM PS3.3 {edition} 8.8 and PS3.16 {edition} Defined Context Groups and Templates"),
        ),
        findings
            .file_meta
            .check(path, FILE_META_RULE, &file_meta_citation()),
    ];
    (checks, notes)
}

fn parsed_file_applies(object: &DefaultDicomObject) -> bool {
    declares_vl_wsi(object) || is_vl_wsi_uid(object.meta().media_storage_sop_class_uid())
}

/// Whether the Data Set's SOP Class UID is VL Whole Slide Microscopy.
pub(super) fn declares_vl_wsi(object: &DefaultDicomObject) -> bool {
    object
        .element(tags::SOP_CLASS_UID)
        .ok()
        .and_then(|element| element.to_str().ok())
        .is_some_and(|uid| is_vl_wsi_uid(&uid))
}

fn is_vl_wsi_uid(uid: &str) -> bool {
    uid.trim_end_matches(['\0', ' ']) == crate::VL_WSI_SOP_CLASS_UID
}

fn file_meta_citation() -> String {
    format!("DICOM PS3.10 {} 7.1", tables::EDITION)
}

/// The File Meta check of a file the DICOM parser could not open. It runs
/// when the encoded group declares a VL Whole Slide Microscopy SOP Instance,
/// or cannot be read well enough to tell; a file declaring another SOP Class
/// is outside this IOD.
pub(super) fn run_unparsed_file_meta_check(path: &Path) -> Option<ValidationCheck> {
    let mut findings = Findings::default();
    let sop_class = file_meta::check_unparsed_file_meta(path, &mut findings);
    if sop_class.as_deref().is_some_and(|uid| !is_vl_wsi_uid(uid)) {
        return None;
    }
    Some(
        findings
            .file_meta
            .check(path, FILE_META_RULE, &file_meta_citation()),
    )
}

fn evaluate(path: &Path, object: &DefaultDicomObject) -> Findings {
    let mut findings = Findings::default();
    file_meta::check_file_meta(path, object, &mut findings);
    let root: &InMemDicomObject = object;
    let transfer_syntax = object.meta().transfer_syntax();
    let scope = Scope::top(root, transfer_syntax);
    modules::check_modules(&mut findings, &scope);
    modules::check_functional_groups(&mut findings, &scope);
    ValueChecks {
        vr_vm: &mut findings.vr_vm,
        format: &mut findings.value_format,
        encoding: Encoding {
            explicit_vr: transfer_syntax != uids::IMPLICIT_VR_LITTLE_ENDIAN,
            extended_characters: extended_character_set(root),
        },
    }
    .data_set(root, "", 0);
    findings
}

/// Every normative statement in the scope of the IOD, with how validation
/// holds an object to it.
pub(in crate::validation) fn catalog() -> Vec<RequirementEntry> {
    let entry = |id, statement: model::Statement, disposition| RequirementEntry {
        id,
        part: statement.part,
        section: statement.section,
        keyword: statement.keyword,
        text: statement.text,
        disposition,
    };
    requirements::TABLE_ENFORCED
        .iter()
        .map(|statement| entry(None, *statement, RequirementDisposition::Tables))
        .chain(requirements::Requirement::ALL.iter().map(|requirement| {
            entry(
                Some(requirement.id()),
                requirement.statement(),
                requirement.disposition(),
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
