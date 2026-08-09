use super::{
    build_dicom_object, checked_dimension_index_value, dicom_file_writer,
    extended_offset_table_metadata_bytes, format_ds, functional_groups, per_frame_items,
    pixel_data_offsets_from_lengths, synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile,
    write_dicom_object_with_pixel_data, write_dicom_object_with_spooled_pixel_data,
    write_dicom_object_with_streamed_pixel_data, write_encapsulated_pixel_data_from_frames,
    write_encapsulated_pixel_data_from_spool, FrameGrid, FrameIndexSpool,
    PerFrameFunctionalGroupsPlan, PixelDataSpool, SpooledPixelDataFragment, StreamedDicomWritePlan,
    DICOM_FILE_WRITE_BUFFER_BYTES,
};
use crate::{tile::PixelProfile, DicomMetadata, Error};
use dicom_core::{DataElement, PrimitiveValue, Tag, VR};

use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use std::io::{Read, Seek, SeekFrom, Write};

mod clinical_metadata;
mod dimensions;
#[path = "functional_groups.rs"]
mod functional_group_tests;
mod pixel_data;

fn sample_object(level_idx: u32) -> InMemDicomObject {
    sample_object_with_metadata_level(DicomMetadata::research_placeholder(), level_idx)
}

fn sample_object_with_metadata(metadata: DicomMetadata) -> InMemDicomObject {
    sample_object_with_metadata_level(metadata, 0)
}

fn sample_object_with_metadata_level(metadata: DicomMetadata, level_idx: u32) -> InMemDicomObject {
    sample_object_with_metadata_level_and_offset_tables(
        metadata,
        level_idx,
        vec![0; 6],
        vec![128; 6],
    )
}

fn sample_object_with_offset_tables(offsets: Vec<u64>, lengths: Vec<u64>) -> InMemDicomObject {
    sample_object_with_metadata_level_and_offset_tables(
        DicomMetadata::research_placeholder(),
        0,
        offsets,
        lengths,
    )
}

fn sample_object_with_metadata_level_and_offset_tables(
    metadata: DicomMetadata,
    level_idx: u32,
    offsets: Vec<u64>,
    lengths: Vec<u64>,
) -> InMemDicomObject {
    let frame_count = u32::try_from(lengths.len()).unwrap();
    let icc_profile = synthetic_srgb_icc_profile().unwrap();
    let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
    params.level_idx = level_idx;
    params.frame_count = frame_count;
    let mut object = super::build_dicom_object(params).unwrap();
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
    object
}

fn sample_file_meta() -> dicom_object::FileMetaTableBuilder {
    dicom_object::FileMetaTableBuilder::new()
        .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
        .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.3")
        .transfer_syntax("1.2.840.10008.1.2.4.202")
}

fn sample_per_frame_plan(frame_count: u32) -> super::PerFrameFunctionalGroupsPlan {
    super::PerFrameFunctionalGroupsPlan::new(
        frame_count,
        super::FrameGrid {
            frame_columns: 512,
            frame_rows: 512,
            matrix_columns: u64::from(frame_count) * 512,
            matrix_rows: 512,
        },
        0.0005,
        0.0005,
    )
    .unwrap()
}

fn sample_object_with_dimensions(
    frame_columns: u32,
    frame_rows: u32,
    matrix_columns: u64,
    matrix_rows: u64,
) -> Result<InMemDicomObject, Error> {
    let icc_profile = synthetic_srgb_icc_profile().unwrap();
    let metadata = DicomMetadata::research_placeholder();
    let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
    params.frame_grid = super::FrameGrid {
        frame_columns,
        frame_rows,
        matrix_columns,
        matrix_rows,
    };
    params.frame_count = 1;
    super::build_dicom_object(params)
}

fn sample_dicom_object_params<'a>(
    metadata: &'a DicomMetadata,
    icc_profile: Option<&'a [u8]>,
) -> super::DicomObjectParams<'a> {
    super::DicomObjectParams {
        metadata,
        identifiers: super::DicomObjectIdentifiers {
            study_uid: "1.2.826.0.1.3680043.10.999.1",
            specimen_uid: metadata
                .specimen_uid
                .as_deref()
                .unwrap_or("1.2.826.0.1.3680043.10.999.7"),
            series_uid: "1.2.826.0.1.3680043.10.999.2",
            sop_instance_uid: "1.2.826.0.1.3680043.10.999.3",
            frame_of_reference_uid: "1.2.826.0.1.3680043.10.999.4",
            pyramid_uid: "1.2.826.0.1.3680043.10.999.5",
            dimension_organization_uid: "1.2.826.0.1.3680043.10.999.6",
            pyramid_label: "WSI pyramid s0 ser0 z0 c0 t0",
        },
        series_number: 7,
        instance_number: 42,
        level_idx: 0,
        frame_grid: super::FrameGrid {
            frame_columns: 512,
            frame_rows: 512,
            matrix_columns: 1024,
            matrix_rows: 1536,
        },
        frame_count: 6,
        profile: PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "RGB",
        },
        pixel_spacing_mm: Some((0.0005, 0.0005)),
        icc_profile,
        lossy_compression: super::LossyCompressionHistory::default(),
    }
}

fn sequence_items(object: &InMemDicomObject, tag: Tag) -> &[InMemDicomObject] {
    object
        .element(tag)
        .unwrap_or_else(|err| panic!("missing sequence {tag:?}: {err}"))
        .items()
        .unwrap_or_else(|| panic!("element {tag:?} is not a sequence"))
}

fn tag_str(object: &InMemDicomObject, tag: Tag) -> String {
    object
        .element(tag)
        .unwrap_or_else(|err| panic!("missing element {tag:?}: {err}"))
        .to_str()
        .unwrap_or_else(|err| panic!("element {tag:?} is not a string: {err}"))
        .into_owned()
}

fn assert_code_item(
    item: &InMemDicomObject,
    code_value: &str,
    coding_scheme: &str,
    code_meaning: &str,
) {
    assert_eq!(tag_str(item, tags::CODE_VALUE), code_value);
    assert_eq!(tag_str(item, tags::CODING_SCHEME_DESIGNATOR), coding_scheme);
    assert_eq!(tag_str(item, tags::CODE_MEANING), code_meaning);
}

fn assert_dimension_index_item(item: &InMemDicomObject, indexed_tag: Tag, dimension_uid: &str) {
    assert_eq!(
        item.element(tags::DIMENSION_INDEX_POINTER)
            .unwrap()
            .value()
            .to_tag()
            .unwrap(),
        indexed_tag
    );
    assert_eq!(
        item.element(tags::FUNCTIONAL_GROUP_POINTER)
            .unwrap()
            .value()
            .to_tag()
            .unwrap(),
        tags::PLANE_POSITION_SLIDE_SEQUENCE
    );
    assert_eq!(
        item.element(tags::DIMENSION_ORGANIZATION_UID)
            .unwrap()
            .to_str()
            .unwrap()
            .as_ref(),
        dimension_uid
    );
}

struct SeekCountingReader {
    inner: std::io::Cursor<Vec<u8>>,
    seek_count: usize,
}

struct MaxReadLenReader {
    bytes: Vec<u8>,
    position: usize,
    max_read_len: std::rc::Rc<std::cell::Cell<usize>>,
}

impl Read for MaxReadLenReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.max_read_len
            .set(self.max_read_len.get().max(buf.len()));
        if self.position >= self.bytes.len() {
            return Ok(0);
        }
        let len = buf.len().min(self.bytes.len() - self.position);
        buf[..len].copy_from_slice(&self.bytes[self.position..self.position + len]);
        self.position += len;
        Ok(len)
    }
}

impl SeekCountingReader {
    fn new(data: Vec<u8>) -> Self {
        Self {
            inner: std::io::Cursor::new(data),
            seek_count: 0,
        }
    }
}

impl Read for SeekCountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for SeekCountingReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.seek_count += 1;
        self.inner.seek(pos)
    }
}
