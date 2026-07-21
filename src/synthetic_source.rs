use std::path::Path;

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::Error;

pub(crate) fn deterministic_rgb_pixels(width: u32, height: u32) -> Result<Vec<u8>, Error> {
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| Error::Unsupported {
            reason: "synthetic RGB source dimensions overflow platform limits".into(),
        })?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(byte_len)
        .map_err(|_| Error::Unsupported {
            reason: "synthetic RGB source exceeds available memory".into(),
        })?;
    for y in 0..height {
        for x in 0..width {
            pixels.push((x.wrapping_mul(37).wrapping_add(y.wrapping_mul(11))) as u8);
            pixels.push((x.wrapping_mul(17).wrapping_add(y.wrapping_mul(29))) as u8);
            pixels.push((x.wrapping_mul(7).wrapping_add(y.wrapping_mul(43))) as u8);
        }
    }
    Ok(pixels)
}

pub(crate) fn write_rgb_source_dicom(
    path: &Path,
    sop_instance_uid: &str,
    series_instance_uid: &str,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<(), Error> {
    let rows = u16::try_from(height).map_err(|_| Error::Unsupported {
        reason: "RGB source height exceeds DICOM Rows range".into(),
    })?;
    let columns = u16::try_from(width).map_err(|_| Error::Unsupported {
        reason: "RGB source width exceeds DICOM Columns range".into(),
    })?;
    let expected_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| Error::Unsupported {
            reason: "RGB source dimensions overflow platform limits".into(),
        })?;
    if pixels.len() != expected_len {
        return Err(Error::DicomWrite {
            path: path.to_path_buf(),
            message: format!(
                "RGB source pixel buffer has {} byte(s), expected {expected_len}",
                pixels.len()
            ),
        });
    }

    let mut object = InMemDicomObject::new_empty();
    object.put(DataElement::new(
        tags::SOP_CLASS_UID,
        VR::UI,
        uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
    ));
    object.put(DataElement::new(
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    ));
    object.put(DataElement::new(
        tags::SERIES_INSTANCE_UID,
        VR::UI,
        series_instance_uid,
    ));
    object.put(DataElement::new(
        tags::IMAGE_TYPE,
        VR::CS,
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
    ));
    object.put(DataElement::new(
        tags::ROWS,
        VR::US,
        PrimitiveValue::from(rows),
    ));
    object.put(DataElement::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(columns),
    ));
    object.put(DataElement::new(
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        VR::UL,
        PrimitiveValue::from(height),
    ));
    object.put(DataElement::new(
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        VR::UL,
        PrimitiveValue::from(width),
    ));
    object.put(DataElement::new(
        tags::PIXEL_SPACING,
        VR::DS,
        "0.0005\\0.0005",
    ));
    object.put(DataElement::new(
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        PrimitiveValue::from(1u32),
    ));
    object.put(DataElement::new(
        tags::SAMPLES_PER_PIXEL,
        VR::US,
        PrimitiveValue::from(3u16),
    ));
    object.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "RGB",
    ));
    object.put(DataElement::new(
        tags::PLANAR_CONFIGURATION,
        VR::US,
        PrimitiveValue::from(0u16),
    ));
    object.put(DataElement::new(
        tags::BITS_ALLOCATED,
        VR::US,
        PrimitiveValue::from(8u16),
    ));
    object.put(DataElement::new(
        tags::BITS_STORED,
        VR::US,
        PrimitiveValue::from(8u16),
    ));
    object.put(DataElement::new(
        tags::HIGH_BIT,
        VR::US,
        PrimitiveValue::from(7u16),
    ));
    object.put(DataElement::new(
        tags::PIXEL_REPRESENTATION,
        VR::US,
        PrimitiveValue::from(0u16),
    ));
    object.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        PrimitiveValue::from(pixels),
    ));
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                .media_storage_sop_instance_uid(sop_instance_uid)
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
        )
        .map_err(|source| Error::DicomWrite {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?
        .write_to_file(path)
        .map_err(|source| Error::DicomWrite {
            path: path.to_path_buf(),
            message: source.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_rgb_geometry_overflow_fails_before_allocation_or_write() {
        assert!(deterministic_rgb_pixels(u32::MAX, u32::MAX).is_err());

        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("oversized.dcm");
        let error = write_rgb_source_dicom(
            &path,
            "1.2.826.0.1.3680043.10.999.1",
            "1.2.826.0.1.3680043.10.999.2",
            u32::from(u16::MAX) + 1,
            1,
            Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Columns"));
        assert!(!path.exists());
    }
}
