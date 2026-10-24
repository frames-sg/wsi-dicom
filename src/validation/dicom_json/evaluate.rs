//! The intrinsic checks of one Data Set read from a DICOM JSON document.

use std::collections::BTreeMap;
use std::path::Path;

use dicom_dictionary_std::{tags, uids};
use dicom_object::{DefaultDicomObject, FileMetaTableBuilder, InMemDicomObject};

use super::json::Json;
use super::model::{BulkReference, Stand};
use super::{cite, reader, JsonRun, JSON_MODEL_RULE};
use crate::validation::digital_signatures::run_digital_signatures_check;
use crate::validation::iod::{
    is_image_storage_sop_class, Family, OptionalNotes, Unavailable, EDITION, MAX_NESTING,
};
use crate::validation::pixel_structure::run_intrinsic_pixel_structure_check;
use crate::validation::transfer_syntaxes::{pixel_encoding, PixelEncoding};
use crate::validation::wsi_conformance::run_intrinsic_wsi_conformance_checks_on;
use crate::validation::{
    intrinsic_check, OptionalConformance, ValidationCheck, ValidationOptions, ValidationStatus,
};

const PIXEL_STRUCTURE_RULE: &str = "intrinsic-pixel-structure";
const ENCODING_RULE: &str = "intrinsic-wsi-dicom-encoding";
const DIGITAL_SIGNATURES_RULE: &str = "intrinsic-wsi-dicom-digital-signatures";
const LOSSY_HISTORY_RULE: &str = "intrinsic-wsi-dicom-lossy-history";

/// Transfer Syntaxes that admit lossless and lossy coding alike, where the
/// codestream shows whether the Frames were lossy compressed (PS3.3
/// C.7.6.1.1.5): the lossy history check reads them in a file.
const CODESTREAM_DECIDES_LOSS: [&str; 4] = [
    uids::JPEG2000,
    uids::JPEG2000MC,
    uids::HTJ2K,
    uids::JPEGLS_NEAR_LOSSLESS,
];

/// Table 8.7.3-2: the Transfer Syntaxes of a Single Frame or Multi-frame
/// Image, in which an origin server may hold the Pixel Data.
const IMAGE_TRANSFER_SYNTAXES: [&str; 19] = [
    uids::EXPLICIT_VR_LITTLE_ENDIAN,
    uids::DEFLATED_IMAGE_FRAME_COMPRESSION,
    uids::JPEG_LOSSLESS_SV1,
    uids::JPEG_BASELINE8_BIT,
    uids::JPEG_EXTENDED12_BIT,
    uids::JPEG_LOSSLESS,
    uids::RLE_LOSSLESS,
    uids::JPEGLS_LOSSLESS,
    uids::JPEGLS_NEAR_LOSSLESS,
    uids::JPEG2000_LOSSLESS,
    uids::JPEG2000,
    uids::JPEG2000MC_LOSSLESS,
    uids::JPEG2000MC,
    uids::HTJ2K_LOSSLESS,
    uids::HTJ2K_LOSSLESS_RPCL,
    uids::HTJ2K,
    uids::JPEGXL_LOSSLESS,
    uids::JPEGXLJPEG_RECOMPRESSION,
    uids::JPEGXL,
];

/// The checks of one Data Set.
pub(super) struct Evaluated {
    pub(super) checks: Vec<ValidationCheck>,
    pub(super) notes: Option<OptionalConformance>,
    /// The object with its first candidate Transfer Syntax, for the set
    /// checks.
    pub(super) reference: Option<DefaultDicomObject>,
    pub(super) bulk: Vec<BulkReference>,
    /// Whether the Transfer Syntax is known, from the options, the run, the
    /// document or a single Available Transfer Syntax UID.
    pub(super) transfer_syntax_known: bool,
}

/// Checks the Data Set `members` at JSON Pointer `pointer`: the JSON Model,
/// then the intrinsic checks with each candidate Transfer Syntax and, while
/// BulkDataURI Values are missing, with both stand-ins.
pub(super) fn instance(
    path: &Path,
    pointer: &str,
    members: &[(String, Json<'_>)],
    run: &JsonRun<'_>,
) -> Evaluated {
    let mut model = Family::default();
    let mut plausible = reader(run.source, Stand::Plausible, run.fetched, Some(&mut model));
    let object = plausible.data_set(members, pointer, 0);
    let bulk = std::mem::take(&mut plausible.bulk);
    let file_meta = std::mem::take(&mut plausible.file_meta);
    drop(plausible);
    let mut checks = vec![model.check(
        path,
        JSON_MODEL_RULE,
        &format!("DICOM PS3.18 {EDITION} Annex F"),
    )];
    let unresolved: Vec<&BulkReference> = bulk
        .iter()
        .filter(|reference| !reference.pixel_data && !run.fetched.contains_key(&reference.uri))
        .collect();
    let known = run
        .options
        .transfer_syntax
        .clone()
        .or_else(|| {
            sop_instance_uid(&object).and_then(|uid| run.transfer_syntaxes.get(&uid).cloned())
        })
        .or_else(|| {
            file_meta
                .iter()
                .find(|(tag, _)| *tag == tags::TRANSFER_SYNTAX_UID)
                .map(|(_, uid)| uid.clone())
        })
        .or_else(|| available_transfer_syntax(&object));
    let candidates: Vec<String> = match &known {
        Some(uid) => vec![uid.clone()],
        None => candidate_syntaxes(&object),
    };
    let options = &run.options.validation;
    // Metadata includes no File Meta Information, and a document holds no
    // encoded Frames: Pixel Data is a BulkDataURI, if anything.
    let unavailable = Unavailable {
        file_meta: Some(format!(
            "needs the PS3.10 File Meta Information, which Metadata does not include ({})",
            cite("10.4.1.1.2")
        )),
        pixel_data: true,
    };
    let mut runs = Vec::with_capacity(candidates.len());
    let mut notes = None;
    let mut reference = None;
    for syntax in &candidates {
        let file = with_meta(&object, syntax);
        let (found, optional) = object_checks(path, &file, &unavailable, options);
        if reference.is_none() {
            notes = optional;
            reference = Some(file);
        }
        runs.push((syntax.clone(), found));
    }
    let mut combined = combine(runs, known.is_none());
    if !unresolved.is_empty() {
        let mut sparse = reader(run.source, Stand::Sparse, run.fetched, None);
        let sparse_object = sparse.data_set(members, pointer, 0);
        let (sparse_checks, _) = object_checks(
            path,
            &with_meta(&sparse_object, &candidates[0]),
            &unavailable,
            options,
        );
        mark_bulk_dependence(&mut combined, &sparse_checks, &unresolved);
    }
    let referenced = candidates
        .iter()
        .all(|syntax| matches!(pixel_encoding(syntax), PixelEncoding::Referenced));
    override_unencoded(&mut combined, path, &object, &bulk, run.fetched, referenced);
    if candidates
        .iter()
        .any(|syntax| CODESTREAM_DECIDES_LOSS.contains(&syntax.as_str()))
    {
        mark_codestream_dependence(&mut combined);
    }
    checks.extend(combined);
    Evaluated {
        checks,
        notes: notes.map(|notes| OptionalConformance {
            path: path.to_path_buf(),
            present: notes.present,
            absent: notes.absent,
            advisories: notes.advisories,
        }),
        reference,
        bulk,
        transfer_syntax_known: known.is_some(),
    }
}

fn trimmed(object: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    object
        .get(tag)
        .and_then(|element| element.to_str().ok())
        .map(|value| value.trim_end_matches(['\0', ' ']).to_string())
}

fn sop_instance_uid(object: &InMemDicomObject) -> Option<String> {
    trimmed(object, tags::SOP_INSTANCE_UID)
}

/// A single Available Transfer Syntax UID (0008,3002) names the one Transfer
/// Syntax the origin server assures (PS3.4 C.6.1.1.5.2).
fn available_transfer_syntax(object: &InMemDicomObject) -> Option<String> {
    let values = object
        .get(tags::AVAILABLE_TRANSFER_SYNTAX_UID)?
        .to_multi_str()
        .ok()?;
    match values.as_ref() {
        [uid] => Some(uid.trim_end_matches(['\0', ' ']).to_string()),
        _ => None,
    }
}

/// Table 8.7.3-2: an image's Transfer Syntaxes; Explicit VR Little Endian,
/// the only one for Text and Other, for any other SOP Class.
fn candidate_syntaxes(object: &InMemDicomObject) -> Vec<String> {
    let sop_class = trimmed(object, tags::SOP_CLASS_UID).unwrap_or_default();
    if is_image_storage_sop_class(&sop_class) {
        IMAGE_TRANSFER_SYNTAXES
            .iter()
            .map(|uid| uid.to_string())
            .collect()
    } else {
        vec![uids::EXPLICIT_VR_LITTLE_ENDIAN.to_string()]
    }
}

/// `object` with File Meta Information naming `syntax` and the Data Set's
/// SOP Class and Instance, which the File Meta checks are not shown.
fn with_meta(object: &InMemDicomObject, syntax: &str) -> DefaultDicomObject {
    let meta = FileMetaTableBuilder::new()
        .media_storage_sop_class_uid(trimmed(object, tags::SOP_CLASS_UID).unwrap_or_default())
        .media_storage_sop_instance_uid(sop_instance_uid(object).unwrap_or_default())
        .transfer_syntax(syntax)
        .build()
        // The builder fails only without a Transfer Syntax, which is given.
        .unwrap_or_else(|err| {
            unreachable!("a File Meta table with a Transfer Syntax builds: {err}")
        });
    object.clone().with_exact_meta(meta)
}

/// The checks of one object that do not read its encoded bytes, with its
/// optional conformance notes; those that need what `unavailable` names are
/// skipped, and none reads `path`, the document's label.
fn object_checks(
    path: &Path,
    object: &DefaultDicomObject,
    unavailable: &Unavailable,
    options: &ValidationOptions,
) -> (Vec<ValidationCheck>, Option<OptionalNotes>) {
    let (mut checks, notes) = run_intrinsic_wsi_conformance_checks_on(
        path,
        object,
        unavailable,
        options.profile,
        options.conformance,
        options.max_pixel_frame_bytes,
    );
    checks.push(run_digital_signatures_check(path, object));
    // JPIP has no inline Pixel Data by definition. Its URL and mutually
    // exclusive pixel elements can be checked from the Metadata alone.
    if matches!(
        pixel_encoding(object.meta().transfer_syntax()),
        PixelEncoding::Referenced
    ) {
        checks.push(run_intrinsic_pixel_structure_check(
            &path.to_path_buf(),
            object,
            options.max_pixel_frame_bytes,
        ));
    }
    (checks, notes)
}

/// Merges the runs of one object over its candidate Transfer Syntaxes,
/// matching each check by name and occurrence.
fn combine(runs: Vec<(String, Vec<ValidationCheck>)>, unknown: bool) -> Vec<ValidationCheck> {
    let mut order: Vec<(String, usize)> = Vec::new();
    let mut by_key: BTreeMap<(String, usize), Vec<(String, ValidationCheck)>> = BTreeMap::new();
    for (syntax, checks) in runs {
        let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
        for check in checks {
            let occurrence = occurrences.entry(check.name.clone()).or_insert(0);
            let key = (check.name.clone(), *occurrence);
            *occurrence += 1;
            let results = by_key.entry(key.clone()).or_default();
            if results.is_empty() {
                order.push(key);
            }
            results.push((syntax.clone(), check));
        }
    }
    order
        .into_iter()
        .filter_map(|key| by_key.remove(&key))
        .map(|results| merge_syntaxes(results, unknown))
        .collect()
}

fn merge_syntaxes(mut results: Vec<(String, ValidationCheck)>, unknown: bool) -> ValidationCheck {
    if !unknown || results.len() == 1 {
        return results.remove(0).1;
    }
    let total = results.len();
    let passing: Vec<&str> = results
        .iter()
        .filter(|(_, check)| check.status == ValidationStatus::Passed)
        .map(|(syntax, _)| syntax.as_str())
        .collect();
    let failing: Vec<&(String, ValidationCheck)> = results
        .iter()
        .filter(|(_, check)| check.status == ValidationStatus::Failed)
        .collect();
    let none_named = format!(
        "the document names no Transfer Syntax ({}), so the check ran with the {total} of Table 8.7.3-2",
        cite("10.4.1.1.2")
    );
    if failing.is_empty() {
        let mut merged = results.remove(0).1;
        merged.message = format!("{}; {none_named}, and fails with none", merged.message);
        return merged;
    }
    if passing.is_empty() {
        let mut merged = failing[0].1.clone();
        merged.message = format!("{}; {none_named}, and passes with none", merged.message);
        return merged;
    }
    let failed_with: Vec<&str> = failing.iter().map(|(syntax, _)| syntax.as_str()).collect();
    let mut merged = failing[0].1.clone();
    merged.status = ValidationStatus::Skipped;
    merged.message = format!(
        "undecidable from the document: {none_named}; it passes only if the Pixel Data is encoded in {} and fails with {}: {}",
        passing.join(", "),
        failed_with.join(", "),
        failing[0].1.message
    );
    merged
}

/// Skips each check whose verdict differs between the two BulkDataURI
/// stand-ins: it reads a Value the document does not hold.
fn mark_bulk_dependence(
    checks: &mut [ValidationCheck],
    sparse: &[ValidationCheck],
    unresolved: &[&BulkReference],
) {
    let named: Vec<String> = unresolved
        .iter()
        .take(4)
        .map(|reference| {
            format!(
                "({:04X},{:04X}) {} at {}",
                reference.tag.group(),
                reference.tag.element(),
                reference.vr,
                reference.pointer
            )
        })
        .collect();
    let more = if unresolved.len() > named.len() {
        ", ..."
    } else {
        ""
    };
    for check in checks.iter_mut() {
        let differs = sparse
            .iter()
            .find(|candidate| candidate.name == check.name)
            .is_some_and(|other| other.status != check.status || other.message != check.message);
        if !differs {
            continue;
        }
        check.status = ValidationStatus::Skipped;
        check.message = format!(
            "depends on Values the document gives only as BulkDataURI ({}{more}), which were not fetched ({}); with stand-ins for them the check reads: {}",
            named.join(", "),
            cite("F.2.6"),
            check.message
        );
    }
}

/// A lossy history that passes without the Frames: in these Transfer
/// Syntaxes a file's codestreams can still show lossy coding the history
/// must record, so the pass is not a verdict.
fn mark_codestream_dependence(checks: &mut [ValidationCheck]) {
    for check in checks.iter_mut().filter(|check| {
        check.name == LOSSY_HISTORY_RULE && check.status == ValidationStatus::Passed
    }) {
        check.status = ValidationStatus::Skipped;
        check.message = format!(
            "with Transfer Syntax {}, the Frames' codestreams show whether they were lossy compressed (PS3.3 {EDITION} C.7.6.1.1.5), and the document holds none; without them the check reads: {}",
            CODESTREAM_DECIDES_LOSS.join(", "),
            check.message
        );
    }
}

/// Adds the checks of encoded bytes a document does not hold, as skipped.
fn override_unencoded(
    checks: &mut Vec<ValidationCheck>,
    path: &Path,
    object: &InMemDicomObject,
    bulk: &[BulkReference],
    fetched: &BTreeMap<String, Vec<u8>>,
    referenced: bool,
) {
    let skipped = |name: &str, message: String| {
        intrinsic_check(
            name,
            Some(path.to_path_buf()),
            ValidationStatus::Skipped,
            message,
        )
    };
    let pixel = bulk
        .iter()
        .find(|reference| reference.pixel_data)
        .map(|reference| {
            format!(
                "gives Pixel Data only as BulkDataURI {}",
                crate::validation::disclosure::withheld(&reference.uri)
            )
        })
        .unwrap_or_else(|| "holds no Pixel Data Value".to_string());
    let frames = cite("10.4.1.1.6");
    let replacements = [
        skipped(
            PIXEL_STRUCTURE_RULE,
            format!("needs the Pixel Data Value, and the document {pixel}; `wsi-dicom dicomweb` checks the Frames an origin server returns ({frames})"),
        ),
        skipped(
            ENCODING_RULE,
            format!("needs the PS3.5 encoded Data Set; a DICOM JSON document holds Values, not their encoding ({})", cite("F.1")),
        ),
    ];
    for replacement in replacements {
        if referenced && replacement.name == PIXEL_STRUCTURE_RULE {
            continue;
        }
        match checks
            .iter_mut()
            .find(|check| check.name == replacement.name)
        {
            Some(check) => *check = replacement,
            None => checks.push(replacement),
        }
    }
    let unread = bulk
        .iter()
        .any(|reference| reference.pixel_data || !fetched.contains_key(&reference.uri));
    if !unread || !holds_signatures(object, 0) {
        return;
    }
    if let Some(check) = checks
        .iter_mut()
        .find(|check| check.name == DIGITAL_SIGNATURES_RULE)
    {
        check.status = ValidationStatus::Skipped;
        check.message = format!(
            "verifying a Digital Signature needs the Value of every signed Data Element, and the document gives some only as BulkDataURI ({}); with stand-ins the check reads: {}",
            cite("F.2.6"),
            check.message
        );
    }
}

/// Whether a Digital Signatures Sequence (FFFA,FFFA) is in `object` or any
/// of its Items.
fn holds_signatures(object: &InMemDicomObject, depth: usize) -> bool {
    object.iter().any(|element| {
        element.header().tag == tags::DIGITAL_SIGNATURES_SEQUENCE
            || (depth < MAX_NESTING
                && element.items().is_some_and(|items| {
                    items.iter().any(|item| holds_signatures(item, depth + 1))
                }))
    })
}
