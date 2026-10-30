//! Recommendations ("should" statements) the IOD value walk holds. A
//! departure is an advisory in the optional conformance notes, never a
//! failure.

use std::fmt::Display;
use std::ops::Range;

use super::report::Advisories;

/// ICC.1 7.2.15 Rendering Intent, header bytes 64 to 67.
const RENDERING_INTENT: Range<usize> = 64..68;
/// The ICC header, followed by the tag count and table (ICC.1 7.3).
const HEADER_BYTES: usize = 128;
const TAG_ENTRY_BYTES: usize = 12;
/// Tag table entries read at most, against a hostile tag count.
const MAX_TAGS: usize = 1024;
/// The tags whose data is a LUT (ICC.1 9.2): AToB0 to 2, BToA0 to 2, gamut
/// and preview0 to 2.
const LUT_TAGS: [&[u8; 4]; 10] = [
    b"A2B0", b"A2B1", b"A2B2", b"B2A0", b"B2A1", b"B2A2", b"gamt", b"pre0", b"pre1", b"pre2",
];
/// ICC.1 10.10 lut16Type.
const LUT16_TYPE: &[u8; 4] = b"mft2";

/// PS3.3 C.11.15.1.1: "The Rendering Intent should be Perceptual" and "All
/// LUTs should be represented as 16 bit values, using tag type lut16Type".
/// A profile too short for its header or tag table is advised nothing here:
/// the ICC Profile check fails it.
pub(super) fn icc_profile(bytes: &[u8], label: &dyn Display, advisories: &mut Advisories) {
    let edition = super::tables::EDITION;
    let Some(intent) = big_endian(bytes, RENDERING_INTENT.start) else {
        return;
    };
    if intent != 0 {
        advisories.add(format!(
            "{label}: the ICC Profile's Rendering Intent is {} ({intent}); PS3.3 {edition} C.11.15.1.1 recommends Perceptual (0)",
            intent_name(intent)
        ));
    }
    for (signature, kind) in lut_tags(bytes) {
        if &kind != LUT16_TYPE {
            advisories.add(format!(
                "{label}: the ICC Profile's {} tag is of type {:?}; PS3.3 {edition} C.11.15.1.1 recommends lut16Type (\"mft2\") for 16 bit LUT values",
                String::from_utf8_lossy(&signature),
                String::from_utf8_lossy(&kind)
            ));
        }
    }
}

fn intent_name(intent: u32) -> &'static str {
    match intent {
        1 => "Media-Relative Colorimetric",
        2 => "Saturation",
        3 => "ICC-Absolute Colorimetric",
        _ => "not an ICC Rendering Intent",
    }
}

/// The signature and data type of each LUT tag the tag table lists within
/// the profile.
fn lut_tags(bytes: &[u8]) -> Vec<([u8; 4], [u8; 4])> {
    let Some(count) = big_endian(bytes, HEADER_BYTES) else {
        return Vec::new();
    };
    let count = usize::try_from(count).unwrap_or(usize::MAX).min(MAX_TAGS);
    (0..count)
        .filter_map(|index| {
            let entry = HEADER_BYTES + 4 + TAG_ENTRY_BYTES * index;
            let signature = four(bytes, entry)?;
            if !LUT_TAGS.contains(&&signature) {
                return None;
            }
            let offset = usize::try_from(big_endian(bytes, entry + 4)?).ok()?;
            Some((signature, four(bytes, offset)?))
        })
        .collect()
}

fn four(bytes: &[u8], at: usize) -> Option<[u8; 4]> {
    bytes.get(at..at.checked_add(4)?)?.try_into().ok()
}

fn big_endian(bytes: &[u8], at: usize) -> Option<u32> {
    four(bytes, at).map(u32::from_be_bytes)
}

#[cfg(test)]
mod tests {
    use super::{icc_profile, Advisories};

    /// A header and a tag table of `tags` (signature, type), each tag's data
    /// its type signature alone.
    fn profile(intent: u32, tags: &[(&[u8; 4], &[u8; 4])]) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[64..68].copy_from_slice(&intent.to_be_bytes());
        bytes.extend_from_slice(&(tags.len() as u32).to_be_bytes());
        let data = 128 + 4 + 12 * tags.len();
        for (index, (signature, _)) in tags.iter().enumerate() {
            bytes.extend_from_slice(*signature);
            bytes.extend_from_slice(&((data + 4 * index) as u32).to_be_bytes());
            bytes.extend_from_slice(&4u32.to_be_bytes());
        }
        for (_, kind) in tags {
            bytes.extend_from_slice(*kind);
        }
        bytes
    }

    fn advised(bytes: &[u8]) -> Vec<String> {
        let mut advisories = Advisories::default();
        icc_profile(bytes, &"ICC Profile (0028,2000)", &mut advisories);
        advisories.describe()
    }

    #[test]
    fn a_perceptual_lut16_profile_is_advised_nothing() {
        assert!(advised(&profile(0, &[(b"A2B0", b"mft2"), (b"rTRC", b"curv")])).is_empty());
        // A matrix/TRC profile has no LUT tag.
        assert!(advised(&profile(0, &[(b"rXYZ", b"XYZ ")])).is_empty());
    }

    #[test]
    fn another_rendering_intent_and_lut_types_are_advised() {
        let advisories = advised(&profile(
            1,
            &[(b"A2B0", b"mft1"), (b"B2A0", b"mBA "), (b"A2B1", b"mft2")],
        ));
        assert_eq!(advisories.len(), 3, "{advisories:?}");
        let intent = format!(
            "ICC Profile (0028,2000): the ICC Profile's Rendering Intent is Media-Relative Colorimetric (1); PS3.3 {} C.11.15.1.1 recommends Perceptual (0)",
            super::super::tables::EDITION
        );
        assert!(advisories.contains(&intent), "{advisories:?}");
        assert!(advisories
            .iter()
            .any(|advisory| advisory.contains("A2B0 tag is of type \"mft1\"")));
        assert!(advisories
            .iter()
            .any(|advisory| advisory.contains("B2A0 tag is of type \"mBA \"")));
        assert!(!advisories.iter().any(|advisory| advisory.contains("A2B1")));
    }

    #[test]
    fn truncated_profiles_and_hostile_tables_are_not_read_beyond_their_bytes() {
        assert!(advised(&[0; 40]).is_empty());
        let mut bytes = profile(0, &[(b"A2B0", b"mft1")]);
        // A tag count far beyond the table, and an offset beyond the profile.
        bytes[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
        bytes[136..140].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(advised(&bytes).is_empty());
    }
}
