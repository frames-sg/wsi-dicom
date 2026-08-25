use super::*;

#[test]
fn extended_offset_table_metadata_rejects_a_value_larger_than_u32_vl() {
    let bytes_per_entry = u32::try_from(std::mem::size_of::<u64>()).unwrap();
    let largest_frame_count = u32::MAX / bytes_per_entry;

    assert!(super::extended_offset_table_metadata_bytes(largest_frame_count).is_ok());
    let error = super::extended_offset_table_metadata_bytes(largest_frame_count + 1).unwrap_err();

    assert!(matches!(error, Error::InvalidOptions { .. }));
    assert!(error.to_string().contains("element length"));
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
            0x00, 0xE0, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0x00, 0xE0, 0x04, 0x00, 0x00, 0x00, 1,
            2, 3, 0, 0xFE, 0xFF, 0x00, 0xE0, 0x02, 0x00, 0x00, 0x00, 4, 5, 0xFE, 0xFF, 0xDD, 0xE0,
            0x00, 0x00, 0x00, 0x00,
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
            object: sample_object_with_offset_tables(vec![0; frames.len()], vec![0; frames.len()]),
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
fn single_frame_streamed_writer_uses_the_compatible_basic_offset_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("single-frame.dcm");
    let frame = vec![1, 2, 3];

    super::write_dicom_object_with_streamed_pixel_data(
        &path,
        super::StreamedDicomWritePlan {
            object: sample_object_with_offset_tables(vec![0], vec![0]),
            meta: sample_file_meta(),
            overwrite: false,
            per_frame_plan: sample_per_frame_plan(1),
            max_instance_metadata_bytes: u64::MAX,
            frame_count: 1,
        },
        |writer| writer.push_frame(&frame),
    )
    .unwrap();

    let object = dicom_object::open_file(path).unwrap();
    assert!(object.element(tags::EXTENDED_OFFSET_TABLE).is_err());
    assert!(object.element(tags::EXTENDED_OFFSET_TABLE_LENGTHS).is_err());
    assert_eq!(
        object
            .element(tags::PIXEL_DATA)
            .unwrap()
            .value()
            .offset_table()
            .unwrap(),
        &[0]
    );
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
fn streamed_pixel_data_writer_rejects_declared_frame_length_mismatches() {
    for (case, declared_len, actual_bytes) in
        [("short", 3u64, &[1, 2][..]), ("long", 2u64, &[1, 2, 3][..])]
    {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(format!("{case}.dcm"));
        let error = super::write_dicom_object_with_streamed_pixel_data(
            &path,
            super::StreamedDicomWritePlan {
                object: sample_object_with_offset_tables(vec![0], vec![0]),
                meta: sample_file_meta(),
                overwrite: false,
                per_frame_plan: sample_per_frame_plan(1),
                max_instance_metadata_bytes: u64::MAX,
                frame_count: 1,
            },
            |writer| writer.push_frame_with(declared_len, |output| output.write_all(actual_bytes)),
        )
        .unwrap_err();

        assert!(error.to_string().contains("declared frame length"));
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}
