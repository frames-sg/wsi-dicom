mod encoding;
mod frame_index;
mod functional_groups;
mod object_construction;
mod persistence;
mod pixel_data;

#[cfg(test)]
use encoding::format_ds;
#[cfg(test)]
use frame_index::FrameIndexSpool;
#[cfg(test)]
use functional_groups::{checked_dimension_index_value, per_frame_items};
pub(crate) use functional_groups::{FrameGrid, PerFrameFunctionalGroupsPlan};
pub(crate) use object_construction::{
    build_dicom_object, synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile,
    DicomObjectIdentifiers, DicomObjectParams, LossyCompressionMetadata,
};
#[cfg(any(test, feature = "bench-internals"))]
pub(crate) use pixel_data::pixel_data_offsets_from_lengths;
#[cfg(test)]
use pixel_data::{
    dicom_file_writer, write_dicom_object_with_pixel_data, DICOM_FILE_WRITE_BUFFER_BYTES,
};
pub(crate) use pixel_data::{
    extended_offset_table_metadata_bytes, unique_spool_path,
    write_dicom_object_with_streamed_pixel_data, BufferedPixelDataSink, PixelDataSink,
    PixelDataSpool, StreamedDicomWritePlan,
};
#[cfg(test)]
pub(crate) use pixel_data::{
    write_dicom_object_with_spooled_pixel_data, write_encapsulated_pixel_data_from_frames,
    write_encapsulated_pixel_data_from_spool, SpooledPixelDataFragment,
};

#[cfg(test)]
mod tests {
    use super::{
        format_ds, synthetic_display_p3_icc_profile, synthetic_srgb_icc_profile,
        write_encapsulated_pixel_data_from_frames, write_encapsulated_pixel_data_from_spool,
        SpooledPixelDataFragment,
    };
    use crate::{tile::PixelProfile, DicomMetadata, Error};
    use dicom_core::{DataElement, PrimitiveValue, Tag, VR};

    #[test]
    fn per_frame_functional_groups_stream_with_an_exact_budget() {
        let plan = super::PerFrameFunctionalGroupsPlan::new(
            100_000,
            super::FrameGrid {
                frame_columns: 1,
                frame_rows: 1,
                matrix_columns: 100_000,
                matrix_rows: 1,
            },
            0.0005,
            0.0005,
        )
        .unwrap();
        let estimate = plan.encoded_len().unwrap();

        let mut rejected = Vec::new();
        let error = plan.write_to(&mut rejected, estimate - 1).unwrap_err();
        assert!(error.to_string().contains("metadata"));
        assert!(rejected.is_empty(), "preflight must precede output");

        let written = plan.write_to(&mut std::io::sink(), estimate).unwrap();
        assert_eq!(written, estimate);
    }

    #[test]
    fn per_frame_metadata_rejects_an_impossible_plan_from_its_structural_lower_bound() {
        let plan = super::PerFrameFunctionalGroupsPlan::new(
            u32::MAX,
            super::FrameGrid {
                frame_columns: 1,
                frame_rows: 1,
                matrix_columns: u64::from(u32::MAX),
                matrix_rows: 1,
            },
            0.0005,
            0.0005,
        )
        .unwrap();
        let budget = 256 * 1024 * 1024;

        assert!(plan.minimum_encoded_len().unwrap() > budget);
        let error = plan.encoded_len_with_limit(budget).unwrap_err();

        assert!(matches!(error, Error::InvalidOptions { .. }));
        assert!(error.to_string().contains("metadata"));
    }

    #[test]
    fn frame_index_spool_round_trips_large_odd_and_even_tables() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("frame-index.tmp");
        let mut spool = super::FrameIndexSpool::create(path.clone()).unwrap();
        let mut next_offset = 0u64;
        for frame in 0..32_770u64 {
            let raw_len = 127 + frame % 2;
            spool.push(frame * 1024, next_offset, raw_len).unwrap();
            next_offset += 8 + (raw_len + raw_len % 2);
        }
        assert_eq!(spool.len(), 32_770);

        let mut seen = 0u64;
        spool
            .replay(|record| {
                let expected_raw_len = 127 + seen % 2;
                assert_eq!(record.source_offset, seen * 1024);
                assert_eq!(record.raw_len, expected_raw_len);
                seen += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, 32_770);
        drop(spool);
        assert!(!path.exists());
    }
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::InMemDicomObject;
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn dicom_file_writer_uses_large_buffer_for_pixel_data_streams() {
        let tmp = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(tmp.path().join("buffered.dcm")).unwrap();
        let writer = super::dicom_file_writer(file);

        assert!(writer.capacity() >= 1024 * 1024);
    }

    #[test]
    fn synthetic_icc_profiles_are_stable_across_instances() {
        const FIXED_CREATION_DATETIME: [u8; 12] = [
            0x07, 0xE8, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let srgb_a = synthetic_srgb_icc_profile().unwrap();
        let srgb_b = synthetic_srgb_icc_profile().unwrap();
        let display_p3_a = synthetic_display_p3_icc_profile().unwrap();
        let display_p3_b = synthetic_display_p3_icc_profile().unwrap();

        assert_eq!(srgb_a, srgb_b);
        assert_eq!(display_p3_a, display_p3_b);
        assert_eq!(&srgb_a[24..36], &FIXED_CREATION_DATETIME);
        assert_eq!(&display_p3_a[24..36], &FIXED_CREATION_DATETIME);
    }

    #[test]
    fn overwrite_failure_leaves_existing_output_bytes_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("existing.dcm");
        std::fs::write(&path, b"existing output").unwrap();

        let err = super::write_dicom_object_with_pixel_data(
            &path,
            sample_object_with_offset_tables(vec![0], vec![3]),
            sample_file_meta(),
            true,
            |file| {
                file.write_all(b"partial")?;
                Err(std::io::Error::other("intentional pixel data failure"))
            },
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("intentional pixel data failure"),
            "unexpected error: {err}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"existing output");
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_rejects_symlink_output_path() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.dcm");
        let link = tmp.path().join("link.dcm");
        std::fs::write(&target, b"target output").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = super::write_dicom_object_with_pixel_data(
            &link,
            sample_object_with_offset_tables(vec![0], vec![0]),
            sample_file_meta(),
            true,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("symlink output path"),
            "unexpected error: {err}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"target output");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn spooled_pixel_data_writer_appends_encapsulated_fragments_with_padding() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_path = tmp.path().join("frames.bin");
        let mut spool = std::fs::File::create(&spool_path).unwrap();
        spool.write_all(&[1, 2, 3, 0, 4, 5]).unwrap();
        drop(spool);

        let mut spool = std::fs::File::open(&spool_path).unwrap();
        let fragments = [
            SpooledPixelDataFragment {
                spool_offset: 0,
                padded_len: 4,
            },
            SpooledPixelDataFragment {
                spool_offset: 4,
                padded_len: 2,
            },
        ];
        let mut out = Vec::new();

        write_encapsulated_pixel_data_from_spool(&mut out, &mut spool, &fragments).unwrap();

        assert_eq!(
            out,
            vec![
                0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF,
                0x00, 0xE0, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00,
                1, 2, 3, 0, 0xFE, 0xFF, 0x00, 0xE0, 0x02, 0x00, 0x00, 0x00, 4, 5, 0xFE, 0xFF, 0xDD,
                0xE0, 0x00, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn spooled_pixel_data_writer_reads_fragments_sequentially() {
        let mut spool = SeekCountingReader::new(vec![1, 2, 3, 0, 4, 5]);
        let fragments = [
            SpooledPixelDataFragment {
                spool_offset: 0,
                padded_len: 4,
            },
            SpooledPixelDataFragment {
                spool_offset: 4,
                padded_len: 2,
            },
        ];
        let mut out = Vec::new();

        write_encapsulated_pixel_data_from_spool(&mut out, &mut spool, &fragments).unwrap();

        assert_eq!(spool.seek_count, 0);
        assert!(out.ends_with(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]));
    }

    #[test]
    fn direct_pixel_data_writer_matches_spooled_output() {
        let tmp = tempfile::tempdir().unwrap();
        let spool_path = tmp.path().join("frames.bin");
        let mut spool = std::fs::File::create(&spool_path).unwrap();
        spool.write_all(&[1, 2, 3, 0, 4, 5]).unwrap();
        drop(spool);

        let mut spool = std::fs::File::open(&spool_path).unwrap();
        let fragments = [
            SpooledPixelDataFragment {
                spool_offset: 0,
                padded_len: 4,
            },
            SpooledPixelDataFragment {
                spool_offset: 4,
                padded_len: 2,
            },
        ];
        let mut spooled = Vec::new();
        write_encapsulated_pixel_data_from_spool(&mut spooled, &mut spool, &fragments).unwrap();

        let frames = [vec![1, 2, 3], vec![4, 5]];
        let lengths = frames
            .iter()
            .map(|frame| frame.len() as u64)
            .collect::<Vec<_>>();
        let mut direct = Vec::new();
        write_encapsulated_pixel_data_from_frames(&mut direct, &lengths, |idx, output| {
            output.write_all(&frames[idx])
        })
        .unwrap();

        assert_eq!(direct, spooled);
    }

    #[test]
    fn pixel_data_spool_records_padded_extended_offsets_and_raw_lengths() {
        let tmp = tempfile::tempdir().unwrap();
        let mut spool = super::PixelDataSpool::create(tmp.path().join("frames.bin"), 2).unwrap();

        spool.push_frame(&[1, 2, 3]).unwrap();
        spool.push_frame(&[4, 5]).unwrap();

        let mut offsets = Vec::new();
        let mut lengths = Vec::new();
        spool
            .index
            .replay(|record| {
                offsets.push(record.extended_offset);
                lengths.push(record.raw_len);
                Ok(())
            })
            .unwrap();
        assert_eq!(offsets, vec![0, 12]);
        assert_eq!(lengths, vec![3, 2]);
    }

    #[test]
    fn streamed_pixel_data_writer_matches_spooled_output_and_patches_offset_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let frames = [vec![1, 2, 3], vec![4, 5]];
        let lengths = frames
            .iter()
            .map(|frame| frame.len() as u64)
            .collect::<Vec<_>>();
        let offsets = super::pixel_data_offsets_from_lengths(&lengths).unwrap();

        let mut spool =
            super::PixelDataSpool::create(tmp.path().join("frames.bin"), frames.len()).unwrap();
        for frame in &frames {
            spool.push_frame(frame).unwrap();
        }
        let spooled_path = tmp.path().join("spooled.dcm");
        super::write_dicom_object_with_spooled_pixel_data(
            &spooled_path,
            sample_object_with_offset_tables(offsets.clone(), lengths.clone()),
            sample_file_meta(),
            false,
            &mut spool,
        )
        .unwrap();

        let streamed_path = tmp.path().join("streamed.dcm");
        let report = super::write_dicom_object_with_streamed_pixel_data(
            &streamed_path,
            super::StreamedDicomWritePlan {
                object: sample_object_with_offset_tables(
                    vec![0; frames.len()],
                    vec![0; frames.len()],
                ),
                meta: sample_file_meta(),
                overwrite: false,
                per_frame_plan: sample_per_frame_plan(frames.len() as u32),
                max_instance_metadata_bytes: u64::MAX,
                frame_count: frames.len(),
            },
            |writer| {
                for frame in &frames {
                    writer.push_frame(frame)?;
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(report.frame_count, frames.len());
        let streamed = dicom_object::open_file(streamed_path).unwrap();
        assert_eq!(
            streamed
                .element(tags::EXTENDED_OFFSET_TABLE)
                .unwrap()
                .to_multi_int::<u64>()
                .unwrap(),
            offsets
        );
        assert_eq!(
            streamed
                .element(tags::EXTENDED_OFFSET_TABLE_LENGTHS)
                .unwrap()
                .to_multi_int::<u64>()
                .unwrap(),
            lengths
        );
        assert_eq!(
            streamed
                .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
                .unwrap()
                .items()
                .unwrap()
                .len(),
            frames.len()
        );
        assert!(spooled_path.exists());
    }

    #[test]
    fn streamed_pixel_data_writer_copies_reader_frame_in_chunks() {
        let tmp = tempfile::tempdir().unwrap();
        let frame_len = super::DICOM_FILE_WRITE_BUFFER_BYTES + 17;
        let frame = (0..frame_len)
            .map(|value| (value % 251) as u8)
            .collect::<Vec<_>>();
        let max_read_len = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let mut reader = MaxReadLenReader {
            bytes: frame.clone(),
            position: 0,
            max_read_len: max_read_len.clone(),
        };

        let streamed_path = tmp.path().join("streamed-reader.dcm");
        let report = super::write_dicom_object_with_streamed_pixel_data(
            &streamed_path,
            super::StreamedDicomWritePlan {
                object: sample_object_with_offset_tables(vec![0], vec![0]),
                meta: sample_file_meta(),
                overwrite: false,
                per_frame_plan: sample_per_frame_plan(1),
                max_instance_metadata_bytes: u64::MAX,
                frame_count: 1,
            },
            |writer| writer.push_frame_from_reader(frame.len() as u64, &mut reader),
        )
        .unwrap();

        assert_eq!(report.frame_count, 1);
        assert_eq!(reader.position, frame.len());
        assert!(max_read_len.get() <= super::DICOM_FILE_WRITE_BUFFER_BYTES);
        assert!(max_read_len.get() < frame.len());
    }

    #[test]
    fn streamed_pixel_data_writer_rejects_wrong_frame_count() {
        let tmp = tempfile::tempdir().unwrap();
        let err = super::write_dicom_object_with_streamed_pixel_data(
            &tmp.path().join("streamed.dcm"),
            super::StreamedDicomWritePlan {
                object: sample_object_with_offset_tables(vec![0; 2], vec![0; 2]),
                meta: sample_file_meta(),
                overwrite: false,
                per_frame_plan: sample_per_frame_plan(2),
                max_instance_metadata_bytes: u64::MAX,
                frame_count: 2,
            },
            |writer| writer.push_frame(&[1, 2, 3]),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("expected 2"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decimal_string_formatting_stays_within_dicom_limit() {
        assert_eq!(format_ds(0.0002528), "0.0002528");
        assert!(format_ds(123_456.789_123_456).len() <= 16);
    }

    #[test]
    fn pyramid_resampled_level_metadata_is_grouped_and_labeled() {
        let object = sample_object(1);

        assert_eq!(
            object
                .element(tags::IMAGE_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "DERIVED\\PRIMARY\\VOLUME\\RESAMPLED"
        );
        assert_eq!(
            object
                .element(tags::PYRAMID_UID)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "1.2.826.0.1.3680043.10.999.5"
        );
        assert_eq!(
            object
                .element(tags::PYRAMID_LABEL)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "WSI pyramid s0 ser0 z0 c0 t0"
        );
        assert_eq!(
            object
                .element(tags::SERIES_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            7
        );
        assert_eq!(
            object
                .element(tags::INSTANCE_NUMBER)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            42
        );
        assert_eq!(
            object
                .element(tags::ACQUISITION_DATE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "19700101"
        );
        assert_eq!(
            object
                .element(tags::ACQUISITION_TIME)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "000000"
        );
        assert!(object.element(tags::PIXEL_SPACING).is_err());
    }

    #[test]
    fn vl_wsi_multiframe_metadata_contains_required_shared_and_dimension_sequences() {
        let object = sample_object(0);
        let image_type = object
            .element(tags::IMAGE_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .into_owned();

        let shared = sequence_items(&object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(shared.len(), 1);
        let pixel_measures = sequence_items(&shared[0], tags::PIXEL_MEASURES_SEQUENCE);
        assert_eq!(pixel_measures.len(), 1);
        assert_eq!(
            pixel_measures[0]
                .element(tags::PIXEL_SPACING)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.0005\\0.0005"
        );
        assert_eq!(
            pixel_measures[0]
                .element(tags::SLICE_THICKNESS)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.001"
        );
        let frame_type = sequence_items(
            &shared[0],
            tags::WHOLE_SLIDE_MICROSCOPY_IMAGE_FRAME_TYPE_SEQUENCE,
        );
        assert_eq!(frame_type.len(), 1);
        assert_eq!(
            frame_type[0]
                .element(tags::FRAME_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            image_type.as_str()
        );

        let dimension_organization = sequence_items(&object, tags::DIMENSION_ORGANIZATION_SEQUENCE);
        assert_eq!(dimension_organization.len(), 1);
        let dimension_uid = dimension_organization[0]
            .element(tags::DIMENSION_ORGANIZATION_UID)
            .unwrap()
            .to_str()
            .unwrap();

        let dimension_index = sequence_items(&object, tags::DIMENSION_INDEX_SEQUENCE);
        assert_eq!(dimension_index.len(), 2);
        assert_dimension_index_item(
            &dimension_index[0],
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            dimension_uid.as_ref(),
        );
        assert_dimension_index_item(
            &dimension_index[1],
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            dimension_uid.as_ref(),
        );

        let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(per_frame.len(), 6);
        let frame_content = sequence_items(&per_frame[0], tags::FRAME_CONTENT_SEQUENCE);
        assert_eq!(frame_content.len(), 1);
        assert_eq!(
            frame_content[0]
                .element(tags::DIMENSION_INDEX_VALUES)
                .unwrap()
                .to_multi_int::<u32>()
                .unwrap(),
            vec![1, 1]
        );
        let frame_position = sequence_items(&per_frame[5], tags::PLANE_POSITION_SLIDE_SEQUENCE);
        assert_eq!(frame_position.len(), 1);
        assert_eq!(
            frame_position[0]
                .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            513
        );
        assert_eq!(
            frame_position[0]
                .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            1025
        );
        assert_eq!(
            frame_position[0]
                .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.256"
        );
        assert_eq!(
            frame_position[0]
                .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0.512"
        );
        assert_eq!(
            frame_position[0]
                .element(tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
    }

    #[test]
    fn vl_wsi_research_placeholder_contains_required_conformance_metadata() {
        let object = sample_object_with_metadata(DicomMetadata::research_placeholder());

        assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "");
        assert_eq!(tag_str(&object, tags::PATIENT_SEX), "");
        assert_eq!(tag_str(&object, tags::STUDY_DATE), "19700101");
        assert_eq!(tag_str(&object, tags::STUDY_TIME), "000000");
        assert_eq!(tag_str(&object, tags::REFERRING_PHYSICIAN_NAME), "");
        assert!(object.element(tags::LATERALITY).is_err());
        assert_eq!(
            tag_str(&object, tags::POSITION_REFERENCE_INDICATOR),
            "SLIDE_CORNER"
        );
        assert_eq!(tag_str(&object, tags::MANUFACTURER), "wsi-dicom");
        assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "wsi-dicom");
        assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "RESEARCH");
        assert_eq!(
            tag_str(&object, tags::SOFTWARE_VERSIONS),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(tag_str(&object, tags::CONTENT_DATE), "19700101");
        assert_eq!(tag_str(&object, tags::CONTENT_TIME), "000000");
        assert_eq!(
            tag_str(&object, tags::ACQUISITION_DATE_TIME),
            "19700101000000"
        );
        assert_eq!(
            tag_str(&object, tags::CONTAINER_IDENTIFIER),
            "RESEARCH-CONTAINER"
        );
        assert_eq!(tag_str(&object, tags::VOLUMETRIC_PROPERTIES), "VOLUME");
        assert_eq!(tag_str(&object, tags::BURNED_IN_ANNOTATION), "NO");
        assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "AUTO");
        assert_eq!(tag_str(&object, tags::EXTENDED_DEPTH_OF_FIELD), "NO");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_WIDTH), "0.512");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_HEIGHT), "0.768");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "1");

        assert_eq!(
            sequence_items(&object, tags::ACQUISITION_CONTEXT_SEQUENCE).len(),
            0
        );
        assert_eq!(
            sequence_items(&object, tags::ISSUER_OF_THE_CONTAINER_IDENTIFIER_SEQUENCE).len(),
            0
        );
        let container_type = sequence_items(&object, tags::CONTAINER_TYPE_CODE_SEQUENCE);
        assert_eq!(container_type.len(), 1);
        assert_code_item(&container_type[0], "433466003", "SCT", "Microscope slide");

        let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
        assert_eq!(specimen.len(), 1);
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER),
            "RESEARCH-SPECIMEN"
        );
        assert!(!tag_str(&specimen[0], tags::SPECIMEN_UID).is_empty());
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
            "Research placeholder specimen"
        );
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_DETAILED_DESCRIPTION),
            "Research placeholder specimen"
        );

        let optical_path = sequence_items(&object, tags::OPTICAL_PATH_SEQUENCE);
        assert_eq!(optical_path.len(), 1);
        let illumination_type =
            sequence_items(&optical_path[0], tags::ILLUMINATION_TYPE_CODE_SEQUENCE);
        assert_eq!(illumination_type.len(), 1);
        assert_code_item(
            &illumination_type[0],
            "111744",
            "DCM",
            "Brightfield illumination",
        );
        let illumination_color =
            sequence_items(&optical_path[0], tags::ILLUMINATION_COLOR_CODE_SEQUENCE);
        assert_eq!(illumination_color.len(), 1);
        assert_code_item(&illumination_color[0], "371251000", "SCT", "White");
        assert!(optical_path[0].element(tags::ICC_PROFILE).is_ok());
    }

    #[test]
    fn vl_wsi_strict_metadata_overrides_conformance_defaults() {
        let metadata: DicomMetadata = serde_json::from_value(serde_json::json!({
            "patient_name": "REAL^PATIENT",
            "patient_id": "P-123",
            "patient_birth_date": "19650504",
            "patient_sex": "F",
            "study_date": "20260504",
            "study_time": "142233",
            "referring_physician_name": "REFERRING^DOC",
            "laterality": "L",
            "manufacturer": "ScannerCo",
            "manufacturer_model_name": "Model X",
            "device_serial_number": "SN123",
            "software_versions": "9.8.7",
            "content_date": "20260504",
            "content_time": "142300",
            "acquisition_date_time": "20260504142233",
            "container_identifier": "SLIDE-123",
            "specimen_identifier": "SPEC-123",
            "specimen_description": "H&E section",
            "imaged_volume_depth_mm": 0.004,
            "focus_method": "MANUAL"
        }))
        .unwrap();
        let object = sample_object_with_metadata(metadata);

        assert_eq!(tag_str(&object, tags::PATIENT_BIRTH_DATE), "19650504");
        assert_eq!(tag_str(&object, tags::PATIENT_SEX), "F");
        assert_eq!(tag_str(&object, tags::STUDY_DATE), "20260504");
        assert_eq!(tag_str(&object, tags::STUDY_TIME), "142233");
        assert_eq!(
            tag_str(&object, tags::REFERRING_PHYSICIAN_NAME),
            "REFERRING^DOC"
        );
        assert_eq!(tag_str(&object, tags::LATERALITY), "L");
        assert_eq!(tag_str(&object, tags::MANUFACTURER), "ScannerCo");
        assert_eq!(tag_str(&object, tags::MANUFACTURER_MODEL_NAME), "Model X");
        assert_eq!(tag_str(&object, tags::DEVICE_SERIAL_NUMBER), "SN123");
        assert_eq!(tag_str(&object, tags::SOFTWARE_VERSIONS), "9.8.7");
        assert_eq!(tag_str(&object, tags::CONTENT_DATE), "20260504");
        assert_eq!(tag_str(&object, tags::CONTENT_TIME), "142300");
        assert_eq!(
            tag_str(&object, tags::ACQUISITION_DATE_TIME),
            "20260504142233"
        );
        assert_eq!(tag_str(&object, tags::CONTAINER_IDENTIFIER), "SLIDE-123");
        assert_eq!(tag_str(&object, tags::IMAGED_VOLUME_DEPTH), "4");
        assert_eq!(tag_str(&object, tags::FOCUS_METHOD), "MANUAL");

        let specimen = sequence_items(&object, tags::SPECIMEN_DESCRIPTION_SEQUENCE);
        assert_eq!(tag_str(&specimen[0], tags::SPECIMEN_IDENTIFIER), "SPEC-123");
        assert_eq!(
            tag_str(&specimen[0], tags::SPECIMEN_SHORT_DESCRIPTION),
            "H&E section"
        );
    }

    #[test]
    fn vl_wsi_declares_utf8_only_when_caller_metadata_requires_it() {
        let ascii = sample_object_with_metadata(DicomMetadata::research_placeholder());
        assert!(ascii.element(tags::SPECIFIC_CHARACTER_SET).is_err());

        let mut unicode = DicomMetadata::research_placeholder();
        unicode.patient_name = Some("山田^太郎".to_string());
        unicode.study_description = Some("Cafe\u{301} 病理".to_string());
        let unicode = sample_object_with_metadata(unicode);
        assert_eq!(
            tag_str(&unicode, tags::SPECIFIC_CHARACTER_SET),
            "ISO_IR 192"
        );
        assert_eq!(tag_str(&unicode, tags::PATIENT_NAME), "山田^太郎");
        assert_eq!(
            tag_str(&unicode, tags::STUDY_DESCRIPTION),
            "Cafe\u{301} 病理"
        );
    }

    #[test]
    fn vl_wsi_object_contains_tiled_full_origin_orientation_and_representative_frame() {
        let object = sample_object(0);

        let origin = sequence_items(&object, tags::TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE);
        assert_eq!(origin.len(), 1);
        assert_eq!(
            origin[0]
                .element(tags::X_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
        assert_eq!(
            origin[0]
                .element(tags::Y_OFFSET_IN_SLIDE_COORDINATE_SYSTEM)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "0"
        );
        assert_eq!(
            object
                .element(tags::IMAGE_ORIENTATION_SLIDE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "1\\0\\0\\0\\1\\0"
        );
        assert_eq!(
            object
                .element(tags::REPRESENTATIVE_FRAME_NUMBER)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            1
        );
        assert_eq!(
            object
                .element(tags::LOSSY_IMAGE_COMPRESSION)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "00"
        );
    }

    #[test]
    fn vl_wsi_volume_requires_pixel_spacing_for_pixel_measures() {
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, None);
        params.pixel_spacing_mm = None;
        let err = super::build_dicom_object(params).unwrap_err();

        assert!(
            err.to_string().contains("pixel spacing"),
            "unexpected error: {err}"
        );
    }

    fn sample_object(level_idx: u32) -> InMemDicomObject {
        sample_object_with_metadata_level(DicomMetadata::research_placeholder(), level_idx)
    }

    fn sample_object_with_metadata(metadata: DicomMetadata) -> InMemDicomObject {
        sample_object_with_metadata_level(metadata, 0)
    }

    fn sample_object_with_metadata_level(
        metadata: DicomMetadata,
        level_idx: u32,
    ) -> InMemDicomObject {
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

    #[test]
    fn vl_wsi_rejects_dimensions_that_exceed_dicom_attribute_ranges() {
        let rows_err =
            sample_object_with_dimensions(512, u32::from(u16::MAX) + 1, 512, 512).unwrap_err();
        assert!(
            rows_err.to_string().contains("Rows exceeds US range"),
            "unexpected error: {rows_err}"
        );

        let columns_err =
            sample_object_with_dimensions(u32::from(u16::MAX) + 1, 512, 512, 512).unwrap_err();
        assert!(
            columns_err.to_string().contains("Columns exceeds US range"),
            "unexpected error: {columns_err}"
        );

        let matrix_columns_err =
            sample_object_with_dimensions(512, 512, u64::from(u32::MAX) + 1, 512).unwrap_err();
        assert!(
            matrix_columns_err
                .to_string()
                .contains("Total Pixel Matrix Columns exceeds UL range"),
            "unexpected error: {matrix_columns_err}"
        );

        let matrix_rows_err =
            sample_object_with_dimensions(512, 512, 512, u64::from(u32::MAX) + 1).unwrap_err();
        assert!(
            matrix_rows_err
                .to_string()
                .contains("Total Pixel Matrix Rows exceeds UL range"),
            "unexpected error: {matrix_rows_err}"
        );
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

    #[test]
    fn vl_wsi_rectangular_frames_write_rows_columns_and_positions() {
        let icc_profile = synthetic_srgb_icc_profile().unwrap();
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, Some(&icc_profile));
        params.frame_grid = super::FrameGrid {
            frame_columns: 64,
            frame_rows: 8,
            matrix_columns: 130,
            matrix_rows: 31,
        };
        params.frame_count = 12;
        params.profile = PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_FULL_422",
        };
        params.pixel_spacing_mm = Some((0.0005, 0.00025));
        let object = super::build_dicom_object(params).unwrap();

        assert_eq!(
            object.element(tags::ROWS).unwrap().to_int::<u16>().unwrap(),
            8
        );
        assert_eq!(
            object
                .element(tags::COLUMNS)
                .unwrap()
                .to_int::<u16>()
                .unwrap(),
            64
        );
        let per_frame = sequence_items(&object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE);
        assert_eq!(per_frame.len(), 12);
        let frame_4_position = sequence_items(&per_frame[4], tags::PLANE_POSITION_SLIDE_SEQUENCE);
        assert_eq!(
            frame_4_position[0]
                .element(tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            65
        );
        assert_eq!(
            frame_4_position[0]
                .element(tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX)
                .unwrap()
                .to_int::<i32>()
                .unwrap(),
            9
        );
    }

    #[test]
    fn vl_wsi_rejects_per_frame_positions_outside_sl_range() {
        let metadata = DicomMetadata::research_placeholder();
        let mut params = sample_dicom_object_params(&metadata, None);
        params.frame_grid = super::FrameGrid {
            frame_columns: u16::MAX as u32,
            frame_rows: 1,
            matrix_columns: 2_147_516_416,
            matrix_rows: 1,
        };
        params.frame_count = 32_770;
        params.profile = PixelProfile {
            components: 3,
            bits_allocated: 8,
            photometric_interpretation: "YBR_FULL_422",
        };
        params.pixel_spacing_mm = Some((0.0005, 0.00025));
        let err = super::build_dicom_object(params).unwrap_err();

        assert!(
            err.to_string().contains("position exceeds SL range"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn per_frame_items_reject_invalid_frame_grid_values() {
        let err = super::per_frame_items(
            1,
            super::FrameGrid {
                frame_columns: 0,
                frame_rows: 512,
                matrix_columns: 512,
                matrix_rows: 512,
            },
            0.00025,
            0.00025,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("non-zero frame dimensions"),
            "unexpected error: {err}"
        );

        let err = super::checked_dimension_index_value(u64::from(u32::MAX), "column")
            .expect_err("one-based dimension index should exceed UL range");
        assert!(
            err.to_string()
                .contains("column dimension index exceeds UL range"),
            "unexpected error: {err}"
        );
    }

    fn sample_dicom_object_params<'a>(
        metadata: &'a DicomMetadata,
        icc_profile: Option<&'a [u8]>,
    ) -> super::DicomObjectParams<'a> {
        super::DicomObjectParams {
            metadata,
            identifiers: super::DicomObjectIdentifiers {
                study_uid: "1.2.826.0.1.3680043.10.999.1",
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
            lossy_compression: None,
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
}
