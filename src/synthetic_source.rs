use std::path::Path;

use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::Error;

pub(crate) fn deterministic_rgb_pixels(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for y in 0..height {
        for x in 0..width {
            pixels.push((x * 37 + y * 11) as u8);
            pixels.push((x * 17 + y * 29) as u8);
            pixels.push((x * 7 + y * 43) as u8);
        }
    }
    pixels
}

pub(crate) fn write_rgb_source_dicom(
    path: &Path,
    sop_instance_uid: &str,
    series_instance_uid: &str,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<(), Error> {
    let expected_len = (width as usize) * (height as usize) * 3;
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
        PrimitiveValue::from(height as u16),
    ));
    object.put(DataElement::new(
        tags::COLUMNS,
        VR::US,
        PrimitiveValue::from(width as u16),
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
