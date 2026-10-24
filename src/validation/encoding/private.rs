//! Private Data Element blocks of one Data Set or Item (PS3.5 7.8.1).

use std::collections::BTreeMap;

use dicom_core::Tag;

use crate::validation::iod::{element_label, Family};

/// The Private Creators and used blocks of one Data Set; an Item keeps its
/// own, since Items do not inherit reservations.
#[derive(Debug, Default)]
pub(super) struct PrivateBlocks {
    /// Identification codes, without padding, by (group, block).
    creators: BTreeMap<(u16, u8), Vec<u8>>,
    /// The first element of each block in use.
    used: BTreeMap<(u16, u8), Tag>,
}

/// Whether `tag` is a Private Creator Data Element (gggg,0010-00FF).
pub(super) fn is_creator(tag: Tag) -> bool {
    !tag.group().is_multiple_of(2) && (0x0010..=0x00FF).contains(&tag.element())
}

impl PrivateBlocks {
    /// Records a Private Data Element (odd group) of the Data Set.
    pub(super) fn element(&mut self, tag: Tag, path: &str, findings: &mut Family) {
        match tag.element() {
            // Retired Group Length (PS3.5 7.2) and the Private Creators.
            0x0000 | 0x0010..=0x00FF => {}
            0x0001..=0x000F | 0x0100..=0x0FFF => findings.push(|| {
                format!(
                    "{}: Elements (gggg,0001-000F) and (gggg,0100-0FFF) of an odd group shall not be used (PS3.5 7.8.1)",
                    element_label(path, tag)
                )
            }),
            element => {
                self.used
                    .entry((tag.group(), (element >> 8) as u8))
                    .or_insert(tag);
            }
        }
    }

    /// Checks a Private Creator Value as encoded. In Implicit VR the
    /// value-format check reads it as UN, so its VM is checked here.
    pub(super) fn creator(
        &mut self,
        tag: Tag,
        value: &[u8],
        implicit_vr: bool,
        path: &str,
        findings: &mut Family,
    ) {
        let label = || element_label(path, tag);
        // NULL padding is the padding rule's finding, not part of the code.
        let mut trimmed = value.trim_ascii();
        while let [rest @ .., 0] = trimmed {
            trimmed = rest.trim_ascii_end();
        }
        let identifier = trimmed.to_vec();
        if identifier.is_empty() {
            findings.push(|| format!(
                "{}: Private Creator without an identification code; one shall reserve the block (PS3.5 7.8.1)",
                label()
            ));
        }
        if let Some(byte) = value
            .iter()
            .find(|byte| !(0x20..=0x7E).contains(*byte) && **byte != 0)
        {
            findings.push(|| format!(
                "{}: Private Creator byte {byte:02X}H is outside the Default Character Repertoire, which Private Creators shall use whatever Specific Character Set (PS3.5 7.8.1)",
                label()
            ));
        }
        if implicit_vr && identifier.contains(&b'\\') {
            findings.push(|| {
                format!(
                    "{}: Private Creator holds several Values; its VM shall be 1 (PS3.5 7.8.1)",
                    label()
                )
            });
        }
        let group = tag.group();
        if !identifier.is_empty() {
            if let Some(((_, block), _)) = self
                .creators
                .iter()
                .find(|((other, _), code)| *other == group && **code == identifier)
            {
                let block = *block;
                findings.push(|| format!(
                    "{}: identification code \"{}\" already reserves block ({group:04X},00{block:02X}); a Private Creator identifier may be used only once within a Group (PS3.5 7.8.1)",
                    label(),
                    String::from_utf8_lossy(&identifier)
                ));
            }
        }
        self.creators
            .insert((group, tag.element() as u8), identifier);
    }

    /// Reports the blocks used without a Private Creator in this Data Set.
    pub(super) fn finish(&self, path: &str, item: bool, findings: &mut Family) {
        for (&(group, block), &tag) in &self.used {
            if self.creators.contains_key(&(group, block)) {
                continue;
            }
            findings.push(|| {
                format!(
                    "{}: no Private Creator ({group:04X},00{block:02X}) reserves its block in this {} (PS3.5 7.8.1)",
                    element_label(path, tag),
                    if item {
                        "Item; Items do not inherit the reservations of the Data Set that contains them"
                    } else {
                        "Data Set"
                    }
                )
            });
        }
    }
}
