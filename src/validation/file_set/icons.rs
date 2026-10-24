//! PS3.3 F.7: the Icon Image Key (0088,0200) of a Directory Record is
//! restricted beyond the Icon Image Sequence Macro. Its presence and the
//! Types of its Image Pixel attributes are the IOD checks'; where an
//! attribute is absent there is nothing here to compare.

use std::collections::BTreeMap;

use dicom_core::Tag;

use super::records::{Record, Value, ICON_IMAGE_SEQUENCE};
use crate::validation::disclosure::shown;
use crate::validation::iod::Family;

const SAMPLES_PER_PIXEL: Tag = Tag(0x0028, 0x0002);
const PHOTOMETRIC_INTERPRETATION: Tag = Tag(0x0028, 0x0004);
const PLANAR_CONFIGURATION: Tag = Tag(0x0028, 0x0006);
const PIXEL_ASPECT_RATIO: Tag = Tag(0x0028, 0x0034);
const BITS_ALLOCATED: Tag = Tag(0x0028, 0x0100);
const BITS_STORED: Tag = Tag(0x0028, 0x0101);
const HIGH_BIT: Tag = Tag(0x0028, 0x0102);
const PIXEL_REPRESENTATION: Tag = Tag(0x0028, 0x0103);

/// Checks every Item of every record's Icon Image Sequence.
pub(super) fn check_icons(records: &[Record], family: &mut Family) {
    for record in records {
        let Some(sequence) = record.get(ICON_IMAGE_SEQUENCE) else {
            continue;
        };
        for icon in &sequence.item_sets {
            let place = format!(
                "the Icon Image of the {} record at byte {}",
                record.record_type(),
                record.offset
            );
            check_icon(icon, &place, family);
        }
    }
}

fn check_icon(icon: &BTreeMap<Tag, Value>, place: &str, family: &mut Family) {
    let unsigned = |tag: Tag| icon.get(&tag).and_then(Value::unsigned);
    if let Some(samples) = unsigned(SAMPLES_PER_PIXEL).filter(|samples| *samples != 1) {
        family.push(|| {
            format!(
                "{place} has Samples per Pixel (0028,0002) {samples}, but shall have 1 (PS3.3 F.7)"
            )
        });
    }
    let photometric = icon.get(&PHOTOMETRIC_INTERPRETATION).map(Value::text);
    if let Some(photometric) = photometric
        .as_deref()
        .filter(|value| !matches!(*value, "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"))
    {
        family.push(|| {
            format!(
                "{place} has Photometric Interpretation (0028,0004) {}, but only monochrome \
                 and palette color images (MONOCHROME1, MONOCHROME2, PALETTE COLOR) shall be \
                 used (PS3.3 F.7)",
                shown(PHOTOMETRIC_INTERPRETATION, photometric)
            )
        });
    }
    if icon.contains_key(&PLANAR_CONFIGURATION) {
        family.push(|| {
            format!("{place} has Planar Configuration (0028,0006), which shall not be present (PS3.3 F.7)")
        });
    }
    for (tag, name) in [
        (BITS_ALLOCATED, "Bits Allocated (0028,0100)"),
        (BITS_STORED, "Bits Stored (0028,0101)"),
    ] {
        if let Some(bits) = unsigned(tag).filter(|bits| !matches!(bits, 1 | 8)) {
            family.push(|| format!("{place} has {name} {bits}, but shall have 1 or 8 (PS3.3 F.7)"));
        }
    }
    if let (Some(stored), Some(high)) = (unsigned(BITS_STORED), unsigned(HIGH_BIT)) {
        if stored.checked_sub(1) != Some(high) {
            family.push(|| {
                format!(
                    "{place} has High Bit (0028,0102) {high}, but shall have one less than Bits \
                     Stored {stored} (PS3.3 F.7)"
                )
            });
        }
    }
    if let Some(representation) = unsigned(PIXEL_REPRESENTATION).filter(|value| *value != 0) {
        family.push(|| {
            format!(
                "{place} has Pixel Representation (0028,0103) {representation}, but shall be \
                 unsigned, 0000H (PS3.3 F.7)"
            )
        });
    }
    check_aspect_ratio(icon, place, family);
    if photometric.as_deref() == Some("PALETTE COLOR") {
        if let Some(bits) = unsigned(BITS_ALLOCATED).filter(|bits| *bits != 8) {
            family.push(|| {
                format!(
                    "{place} uses a Palette Color Lookup Table with Bits Allocated (0028,0100) \
                     {bits}, but shall have 8 (PS3.3 F.7)"
                )
            });
        }
    }
}

/// PS3.3 F.7: Pixel Aspect Ratio, when present, is 1:1.
fn check_aspect_ratio(icon: &BTreeMap<Tag, Value>, place: &str, family: &mut Family) {
    let Some(value) = icon.get(&PIXEL_ASPECT_RATIO) else {
        return;
    };
    let values = value.texts();
    let square = matches!(
        values.as_slice(),
        [row, column] if row.parse::<u64>().is_ok_and(|row| row > 0)
            && row.parse::<u64>().ok() == column.parse::<u64>().ok()
    );
    if !square {
        family.push(|| {
            format!(
                "{place} has Pixel Aspect Ratio (0028,0034) {}, but shall have a Value of \
                 1:1 (PS3.3 F.7)",
                shown(PIXEL_ASPECT_RATIO, &values.join("\\"))
            )
        });
    }
}
