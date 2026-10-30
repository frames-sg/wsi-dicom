//! External validators use several diagnostic formats. Withhold a diagnostic
//! that names an identifying Attribute, including its continuation lines.

use std::sync::OnceLock;

use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::Tag;
use dicom_dictionary_std::{tags, StandardDataDictionary};

use super::{withholds, WITHHELD};
use crate::validation::iod::deidentification::{confidentiality_attributes, TagPattern};

/// Filter captured output after its original text has determined the verdict.
pub(in crate::validation) fn diagnostics(output: &str) -> String {
    let mut result = String::with_capacity(output.len());
    let mut continuation = false;
    for line in output.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let severity = ["Error", "Warning", "Info", "E:", "W:", "I:"]
            .into_iter()
            .find(|prefix| trimmed.starts_with(prefix));
        if severity.is_some() || trimmed.is_empty() {
            continuation = false;
        }
        if let Some(attribute) = identifying_attribute(line) {
            result.push_str(severity.unwrap_or("Diagnostic"));
            result.push_str(" - ");
            result.push_str(&attribute);
            result.push_str(": ");
            result.push_str(WITHHELD);
            continuation = true;
        } else if continuation {
            result.push_str("(diagnostic continuation withheld: identifying)");
        } else {
            result.push_str(line);
            continue;
        }
        if line.ends_with('\n') {
            result.push('\n');
        }
    }
    result
}

fn identifying_attribute(line: &str) -> Option<String> {
    // dciodvfy paths, DCMTK and validate_iods all use parenthesized tags.
    // Checking every tag also recognizes identifying Sequence ancestors.
    for tail in line.split('(').skip(1) {
        let Some((body, _)) = tail.split_once(')') else {
            continue;
        };
        if body.len() > 24 {
            continue;
        }
        let Some((group, element)) = body.split_once(',') else {
            continue;
        };
        let hex = |value: &str| {
            let value = value.trim().strip_prefix("0x").unwrap_or(value.trim());
            (value.len() == 4)
                .then(|| u16::from_str_radix(value, 16).ok())
                .flatten()
        };
        if let Some(tag) = hex(group).zip(hex(element)).map(|(g, e)| Tag(g, e)) {
            if withholds(tag) {
                return Some(tag.to_string());
            }
        }
    }
    let mut normalized = format!(" {} ", words(line));
    // Longer names win: "SOP Class UID" must not match the identifying
    // Content Item Attribute named simply "UID".
    for (needle, label, identifying) in labels() {
        if normalized.contains(needle) {
            if *identifying {
                return Some(label.clone());
            }
            normalized = normalized.replace(needle, " ");
        }
    }
    None
}

fn words(value: &str) -> String {
    value
        .to_lowercase()
        .replace("'s", "")
        .replace("’s", "")
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn labels() -> &'static [(String, String, bool)] {
    static LABELS: OnceLock<Vec<(String, String, bool)>> = OnceLock::new();
    LABELS.get_or_init(|| {
        let mut labels = Vec::new();
        let mut add = |name: &str, tag: Tag| {
            let label = format!("{name} {tag}");
            labels.push((format!(" {} ", words(name)), label.clone(), withholds(tag)));
            if let Some(entry) = StandardDataDictionary.by_tag(tag) {
                labels.push((format!(" {} ", words(entry.alias())), label, withholds(tag)));
            }
        };
        for row in confidentiality_attributes() {
            if let TagPattern::Tag(raw) = row.tag {
                add(row.name, Tag((raw >> 16) as u16, raw as u16));
            }
        }
        for (name, tag) in [
            ("Text Value", tags::TEXT_VALUE),
            ("SOP Class UID", tags::SOP_CLASS_UID),
            ("Referenced SOP Class UID", tags::REFERENCED_SOP_CLASS_UID),
            (
                "Media Storage SOP Class UID",
                tags::MEDIA_STORAGE_SOP_CLASS_UID,
            ),
            ("Transfer Syntax UID", tags::TRANSFER_SYNTAX_UID),
            ("Coding Scheme UID", tags::CODING_SCHEME_UID),
        ] {
            add(name, tag);
        }
        labels.sort_by_key(|(needle, _, _)| std::cmp::Reverse(needle.len()));
        labels.dedup();
        labels
    })
}
