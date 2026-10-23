//! Standard-derived requirement checks for the VL Whole Slide Microscopy IOD.
//!
//! `tables.rs` and `dictionary.rs` are generated from the pinned DICOM DocBook
//! sources by `scripts/generate-iod-tables.py`; `conditions.rs` holds one
//! hand-written predicate per generated condition.

mod conditions;
#[rustfmt::skip]
mod dictionary;
mod engine;
mod file_meta;
mod model;
mod modules;
mod report;
mod scope;
#[rustfmt::skip]
mod tables;
mod values;

#[cfg(test)]
mod tests;

use std::path::Path;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, InMemDicomObject};

use super::ValidationCheck;
use report::Findings;
use scope::Scope;
use values::{Encoding, ValueChecks};

const IOD_MODULES_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-modules";
const IOD_ATTRIBUTE_PRESENCE_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-attribute-presence";
const IOD_ENUMERATED_VALUES_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-enumerated-values";
const IOD_VR_VM_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-vr-vm";
const IOD_VALUE_FORMAT_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-value-format";
const IOD_SEQUENCE_ITEMS_RULE: &str = "intrinsic-wsi-dicom-2026c-iod-sequence-items";
const FILE_META_RULE: &str = "intrinsic-wsi-dicom-2026c-file-meta";

/// Deepest Sequence nesting walked; deeper Items are reported, not checked.
const MAX_NESTING: usize = 32;

/// Runs every IOD requirement family when either the Data Set or File Meta
/// Information declares a VL Whole Slide Microscopy SOP Instance, with the
/// optional items the object lacks when `conformance` is `Optional`.
pub(super) fn run_iod_checks(
    path: &Path,
    object: &DefaultDicomObject,
    conformance: crate::ConformanceLevel,
) -> (Vec<ValidationCheck>, Option<Vec<String>>) {
    if !parsed_file_applies(object) {
        return (Vec::new(), None);
    }
    let findings = evaluate(path, object);
    let optional_absent = (conformance == crate::ConformanceLevel::Optional)
        .then(|| findings.optional_absent.describe());
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
        findings
            .file_meta
            .check(path, FILE_META_RULE, &file_meta_citation()),
    ];
    (checks, optional_absent)
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
