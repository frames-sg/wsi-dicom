use dicom_core::value::{DataSetSequence, PixelFragmentSequence};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

use crate::tile::PixelProfile;
use crate::{DicomMetadata, WsiDicomError, VL_WSI_SOP_CLASS_UID};

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_dicom_object(
    metadata: &DicomMetadata,
    study_uid: &str,
    series_uid: &str,
    sop_instance_uid: &str,
    level_idx: u32,
    tile_size: u32,
    matrix_columns: u64,
    matrix_rows: u64,
    frame_count: u32,
    profile: PixelProfile,
    fragments: Vec<Vec<u8>>,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
) -> Result<InMemDicomObject, WsiDicomError> {
    let mut object = InMemDicomObject::new_empty();
    put_str(
        &mut object,
        tags::SOP_CLASS_UID,
        VR::UI,
        VL_WSI_SOP_CLASS_UID,
    );
    put_str(
        &mut object,
        tags::SOP_INSTANCE_UID,
        VR::UI,
        sop_instance_uid,
    );
    put_str(&mut object, tags::STUDY_INSTANCE_UID, VR::UI, study_uid);
    put_str(&mut object, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    put_str(&mut object, tags::MODALITY, VR::CS, "SM");
    put_str(
        &mut object,
        tags::IMAGE_TYPE,
        VR::CS,
        "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
    );
    put_str(
        &mut object,
        tags::PATIENT_NAME,
        VR::PN,
        metadata.patient_name.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::PATIENT_ID,
        VR::LO,
        metadata.patient_id.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::ACCESSION_NUMBER,
        VR::SH,
        metadata.accession_number.as_deref().unwrap_or_default(),
    );
    put_str(
        &mut object,
        tags::STUDY_ID,
        VR::SH,
        metadata.study_id.as_deref().unwrap_or("1"),
    );
    put_str(
        &mut object,
        tags::STUDY_DESCRIPTION,
        VR::LO,
        metadata.study_description.as_deref().unwrap_or_default(),
    );
    put_u16(&mut object, tags::ROWS, tile_size as u16);
    put_u16(&mut object, tags::COLUMNS, tile_size as u16);
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_COLUMNS,
        matrix_columns as u32,
    );
    put_u32(
        &mut object,
        tags::TOTAL_PIXEL_MATRIX_ROWS,
        matrix_rows as u32,
    );
    put_str(
        &mut object,
        tags::NUMBER_OF_FRAMES,
        VR::IS,
        &frame_count.to_string(),
    );
    put_u16(
        &mut object,
        tags::SAMPLES_PER_PIXEL,
        profile.components as u16,
    );
    put_str(
        &mut object,
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        profile.photometric_interpretation,
    );
    if profile.components > 1 {
        put_u16(&mut object, tags::PLANAR_CONFIGURATION, 0);
    }
    put_u16(&mut object, tags::BITS_ALLOCATED, profile.bits_allocated);
    put_u16(&mut object, tags::BITS_STORED, profile.bits_allocated);
    put_u16(&mut object, tags::HIGH_BIT, profile.bits_allocated - 1);
    put_u16(&mut object, tags::PIXEL_REPRESENTATION, 0);
    put_str(
        &mut object,
        tags::DIMENSION_ORGANIZATION_TYPE,
        VR::CS,
        "TILED_FULL",
    );
    put_u16(&mut object, tags::NUMBER_OF_OPTICAL_PATHS, 1);
    put_u32(&mut object, tags::TOTAL_PIXEL_MATRIX_FOCAL_PLANES, 1);
    put_str(&mut object, tags::SPECIMEN_LABEL_IN_IMAGE, VR::CS, "NO");
    put_u32(&mut object, tags::SERIES_NUMBER, level_idx + 1);
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE,
        VR::OV,
        PrimitiveValue::U64(offsets.into()),
    ));
    object.put(DataElement::new(
        tags::EXTENDED_OFFSET_TABLE_LENGTHS,
        VR::OV,
        PrimitiveValue::U64(lengths.into()),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::OPTICAL_PATH_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(vec![optical_path_item()]),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
        VR::SQ,
        DataSetSequence::from(per_frame_items(frame_count, tile_size, matrix_columns)?),
    ));
    object.put(DataElement::<InMemDicomObject>::new(
        tags::PIXEL_DATA,
        VR::OB,
        PixelFragmentSequence::new_fragments(fragments),
    ));
    Ok(object)
}

fn optical_path_item() -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::OPTICAL_PATH_IDENTIFIER, VR::SH, "0");
    put_str(
        &mut item,
        tags::OPTICAL_PATH_DESCRIPTION,
        VR::ST,
        "Default optical path",
    );
    item
}

fn per_frame_items(
    frame_count: u32,
    tile_size: u32,
    matrix_columns: u64,
) -> Result<Vec<InMemDicomObject>, WsiDicomError> {
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let mut items = Vec::with_capacity(frame_count as usize);
    for frame in 0..frame_count {
        let row = u64::from(frame) / tiles_across;
        let col = u64::from(frame) % tiles_across;
        let mut position = InMemDicomObject::new_empty();
        position.put(DataElement::new(
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from((col * u64::from(tile_size) + 1) as i32),
        ));
        position.put(DataElement::new(
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from((row * u64::from(tile_size) + 1) as i32),
        ));
        let mut item = InMemDicomObject::new_empty();
        item.put(DataElement::<InMemDicomObject>::new(
            tags::PLANE_POSITION_SLIDE_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![position]),
        ));
        items.push(item);
    }
    Ok(items)
}

fn put_str(object: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    object.put(DataElement::new(tag, vr, value));
}

fn put_u16(object: &mut InMemDicomObject, tag: Tag, value: u16) {
    object.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
}

fn put_u32(object: &mut InMemDicomObject, tag: Tag, value: u32) {
    object.put(DataElement::new(tag, VR::UL, PrimitiveValue::from(value)));
}
