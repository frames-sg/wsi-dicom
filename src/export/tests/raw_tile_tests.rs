//! Raw tiles larger than their tile, MONOCHROME1 tiles and tiles with fewer
//! significant bits than they occupy take the decode route; they are never
//! copied as frames.

use std::collections::HashMap;

use dicom_core::DataElement;
use wsi_rs::{PixelRect, TileEntry, TileRequest, WsiError};

use super::super::lossless_j2k_plan::native_tile_is_frame;
use super::super::raw_tile::{raw_tile_read_declined, undersized_jpeg_tile};
use super::super::{j2k_direct_htj2k, jpeg_direct_htj2k};
use super::*;

struct RawTileSpec {
    compression: Compression,
    image: (u32, u32),
    region: Option<PixelRect>,
    bits_allocated: u16,
    significant_bits: Option<u16>,
    samples_per_pixel: u16,
    photometric: EncodedTilePhotometricInterpretation,
}

impl RawTileSpec {
    fn jpeg(image: (u32, u32)) -> Self {
        Self {
            compression: Compression::Jpeg,
            image,
            region: None,
            bits_allocated: 8,
            significant_bits: None,
            samples_per_pixel: 3,
            photometric: EncodedTilePhotometricInterpretation::YbrFull422,
        }
    }

    fn gray_jpeg(image: (u32, u32)) -> Self {
        Self {
            samples_per_pixel: 1,
            photometric: EncodedTilePhotometricInterpretation::Monochrome2,
            ..Self::jpeg(image)
        }
    }

    fn j2k(image: (u32, u32), samples_per_pixel: u16, bits_allocated: u16) -> Self {
        Self {
            compression: Compression::Jp2kRgb,
            image,
            region: None,
            bits_allocated,
            significant_bits: None,
            samples_per_pixel,
            photometric: if samples_per_pixel == 1 {
                EncodedTilePhotometricInterpretation::Monochrome2
            } else {
                EncodedTilePhotometricInterpretation::Rgb
            },
        }
    }

    fn region(self, region: PixelRect) -> Self {
        Self {
            region: Some(region),
            ..self
        }
    }

    fn significant_bits(self, bits: u16) -> Self {
        Self {
            significant_bits: Some(bits),
            ..self
        }
    }

    fn photometric(self, photometric: EncodedTilePhotometricInterpretation) -> Self {
        Self {
            photometric,
            ..self
        }
    }

    fn build(self, data: Vec<u8>) -> RawCompressedTile {
        let mut builder = RawCompressedTile::builder(self.compression)
            .dimensions(self.image.0, self.image.1)
            .bits_allocated(self.bits_allocated)
            .samples_per_pixel(self.samples_per_pixel)
            .photometric_interpretation(self.photometric)
            .data(data);
        if let Some(region) = self.region {
            builder = builder.tile_region(region);
        }
        if let Some(bits) = self.significant_bits {
            builder = builder.significant_bits(bits);
        }
        builder.build().expect("valid raw tile")
    }
}

fn ybr_jpeg(width: u32, height: u32) -> Vec<u8> {
    encode_test_jpeg(width, height, [90, 120, 150])
}

fn lossless_j2k(width: u32, height: u32, components: u16, bit_depth: u8) -> Vec<u8> {
    let samples = (0..width * height * u32::from(components))
        .flat_map(|value| {
            let sample = (value * 37 + 11) as u16 & ((1u32 << bit_depth) - 1) as u16;
            if bit_depth > 8 {
                sample.to_le_bytes().to_vec()
            } else {
                vec![sample as u8]
            }
        })
        .collect::<Vec<_>>();
    let samples =
        J2kLosslessSamples::new(&samples, width, height, components, bit_depth, false).unwrap();
    encode_dicom_lossless(
        samples,
        TransferSyntax::Jpeg2000Lossless,
        EncodeBackendPreference::CpuOnly,
        CodecValidation::RoundTrip,
    )
    .unwrap()
}

fn j2k_copies(raw: &RawCompressedTile, frame: FrameExtent) -> bool {
    j2k_passthrough_frame(raw.clone(), frame, TransferSyntax::Jpeg2000Lossless)
        .unwrap()
        .is_some()
}

fn j2k_recodes(raw: &RawCompressedTile, frame: FrameExtent) -> bool {
    let profile = RawJ2kInspection::new(raw).and_then(|inspection| inspection.profile());
    j2k_direct_htj2k::frame(raw.clone(), frame, TransferSyntax::Htj2kLossless, profile).is_some()
}

fn export_for_test(
    source_path: &std::path::Path,
    output_dir: std::path::PathBuf,
    transfer_syntax: TransferSyntax,
    pixel_reencode: PixelReencodePolicy,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path: source_path.to_path_buf(),
        output_dir,
        options: ExportOptions {
            tile_size: 32,
            transfer_syntax,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            pixel_reencode,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap()
}

fn pixel_data_fragments(path: &std::path::Path) -> Vec<Vec<u8>> {
    let object = dicom_object::open_file(path).unwrap();
    object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap()
        .iter()
        .map(|fragment| dicom_fragment_jpeg_payload(fragment).to_vec())
        .collect()
}

#[test]
fn raw_jpeg_cropped_from_an_earlier_mcu_boundary_takes_the_decode_route() {
    // The 504-column right-edge tile of a 1016-column level, cut from the
    // MCU boundary 8 columns before it: the image is frame-sized, but its
    // first 8 columns belong to the previous frame.
    let edge = FrameExtent::at(1, 0, 512, 512, 1016, 512);
    let gray = || encode_test_gray_jpeg(512, 512, 90);
    let crop = RawTileSpec::gray_jpeg((512, 512))
        .region(PixelRect::new(8, 0, 504, 512))
        .build(gray());

    assert!(!raw_jpeg_matches_frame_geometry(&crop, edge));
    assert!(jpeg_direct_htj2k::frame(crop, edge, TransferSyntax::Htj2kLossless).is_none());

    // The same image starting at the tile origin is that edge frame.
    let aligned = RawTileSpec::gray_jpeg((512, 512))
        .region(PixelRect::new(0, 0, 504, 512))
        .build(gray());
    assert!(raw_jpeg_matches_frame_geometry(&aligned, edge));
    assert!(jpeg_direct_htj2k::frame(aligned, edge, TransferSyntax::Htj2kLossless).is_some());
}

#[test]
fn raw_tile_cut_from_a_larger_stored_image_takes_the_decode_route() {
    // A whole stored 1024x1024 JPEG returned for its second 512-pixel tile.
    let stored_jpeg = RawTileSpec::jpeg((1024, 1024))
        .region(PixelRect::new(512, 0, 512, 512))
        .build(ybr_jpeg(1024, 1024));
    assert!(!raw_jpeg_matches_frame_geometry(
        &stored_jpeg,
        FrameExtent::interior(512, 512)
    ));

    // A whole 4x2 JPEG 2000 codestream returned for its right 2x2 tile.
    let codestream = RawTileSpec::j2k((4, 2), 3, 8)
        .region(PixelRect::new(2, 0, 2, 2))
        .build(lossless_j2k(4, 2, 3, 8));
    let frame = FrameExtent::interior(2, 2);
    assert!(!j2k_copies(&codestream, frame));
    assert!(!j2k_recodes(&codestream, frame));

    // A frame-sized stored JPEG 2000 tile whose requested tile is its left
    // column, as for a Ventana tile cut from an overlapping stored tile: its
    // right column holds other pixels, so it cannot fill an interior frame.
    let left_column = RawTileSpec::j2k((2, 2), 3, 8)
        .region(PixelRect::new(0, 0, 1, 2))
        .build(lossless_j2k(2, 2, 3, 8));
    assert!(!j2k_copies(&left_column, frame));
    assert!(!j2k_recodes(&left_column, frame));

    let whole = RawTileSpec::j2k((2, 2), 3, 8).build(lossless_j2k(2, 2, 3, 8));
    assert!(j2k_copies(&whole, frame));
    assert!(j2k_recodes(&whole, frame));
}

#[test]
fn frame_sized_raw_tile_with_a_smaller_region_is_copied_only_as_an_edge_frame() {
    // wsi-rs returns a DICOM edge frame at its stored size, with the tile
    // region marking the part inside the image. Its padding may fill the
    // part of a TILED_FULL edge frame outside the matrix, never an interior
    // frame.
    let right_edge = FrameExtent::at(1, 0, 512, 512, 612, 512);
    let edge_jpeg = RawTileSpec::jpeg((512, 512))
        .region(PixelRect::new(0, 0, 100, 512))
        .build(ybr_jpeg(512, 512));
    assert!(raw_jpeg_matches_frame_geometry(&edge_jpeg, right_edge));
    assert!(!raw_jpeg_matches_frame_geometry(
        &edge_jpeg,
        FrameExtent::interior(512, 512)
    ));
    // A region narrower than the frame's in-matrix part leaves matrix
    // pixels uncovered.
    assert!(!raw_jpeg_matches_frame_geometry(
        &edge_jpeg,
        FrameExtent::at(1, 0, 512, 512, 712, 512)
    ));

    let bottom_edge = FrameExtent::at(0, 1, 2, 2, 2, 3);
    let edge_j2k = RawTileSpec::j2k((2, 2), 3, 8)
        .region(PixelRect::new(0, 0, 2, 1))
        .build(lossless_j2k(2, 2, 3, 8));
    assert!(j2k_copies(&edge_j2k, bottom_edge));
    assert!(j2k_recodes(&edge_j2k, bottom_edge));
    assert!(!j2k_copies(&edge_j2k, FrameExtent::interior(2, 2)));
    assert!(!j2k_recodes(&edge_j2k, FrameExtent::interior(2, 2)));
}

#[test]
fn irregular_tiles_stand_for_frames_only_on_an_exact_frame_grid() {
    let frame = FrameExtent::interior(256, 256);
    let irregular = |tile_advance, offset, extra_tiles| {
        wsi_rs::Level::new(
            (1024, 256),
            1.0,
            TileLayout::Irregular {
                tile_advance,
                extra_tiles,
                tiles: HashMap::from([((1, 0), TileEntry::new(offset, (256, 256)))]),
            },
        )
    };
    let exact = irregular((256.0, 256.0), (0.0, 0.0), (0, 0, 0, 0));
    assert!(native_tile_is_frame(&exact, (1, 0), frame));
    assert!(
        !native_tile_is_frame(&exact, (0, 0), frame),
        "no tile there"
    );
    // Overlapping Ventana tiles advance 252 pixels: tile (1, 0) starts at
    // x=252, not at the frame origin, and tile (2, 0) paints its right edge.
    let overlapping = irregular((252.0, 256.0), (0.0, 0.0), (0, 0, 1, 0));
    assert!(!native_tile_is_frame(&overlapping, (1, 0), frame));
    // An area offset shifts every tile of the area off the frame grid.
    let offset = irregular((256.0, 256.0), (3.0, 0.0), (0, 0, 1, 0));
    assert!(!native_tile_is_frame(&offset, (1, 0), frame));

    let regular = wsi_rs::Level::new(
        (1024, 256),
        1.0,
        TileLayout::Regular {
            tile_width: 256,
            tile_height: 256,
            tiles_across: 4,
            tiles_down: 1,
        },
    );
    assert!(native_tile_is_frame(&regular, (1, 0), frame));
}

#[test]
fn dicom_jpeg_edge_frames_are_copied_into_matching_edge_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let tiff = tmp.path().join("source.svs");
    let tiles = [
        encode_test_jpeg(32, 32, [180, 30, 40]),
        encode_test_jpeg(32, 32, [20, 170, 60]),
    ];
    write_tiled_jpeg_tiff(&tiff, 33, 32, 32, 32, &tiles);
    let dicom = export_for_test(
        &tiff,
        tmp.path().join("dicom"),
        TransferSyntax::JpegBaseline8Bit,
        PixelReencodePolicy::Lossless,
    );
    assert_eq!(dicom.metrics.routes.jpeg_passthrough_frames, 2);
    let dicom = dicom.instances[0].path.clone();

    // The 1-column edge tile comes back as its padded 32x32 frame.
    let edge = Slide::open(&dicom)
        .unwrap()
        .read_raw_compressed_tile(&TileRequest::new(0, 0, 0, 1, 0))
        .unwrap();
    assert_eq!((edge.width(), edge.height()), (32, 32));
    assert_eq!(edge.tile_region(), PixelRect::new(0, 0, 1, 32));

    let report = export_for_test(
        &dicom,
        tmp.path().join("out"),
        TransferSyntax::JpegBaseline8Bit,
        PixelReencodePolicy::Never,
    );
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.compressed_source_reencoded_frames, 0);
    assert_eq!(
        pixel_data_fragments(&report.instances[0].path),
        tiles.to_vec()
    );
}

#[test]
fn monochrome1_raw_tiles_are_decoded_never_copied_as_monochrome2() {
    let gray_jpeg = |photometric| {
        RawTileSpec::gray_jpeg((16, 16))
            .photometric(photometric)
            .build(encode_test_gray_jpeg(16, 16, 40))
    };
    let frame = FrameExtent::interior(16, 16);
    let monochrome2 = gray_jpeg(EncodedTilePhotometricInterpretation::Monochrome2);
    assert_eq!(
        pixel_profile_from_raw_jpeg_tile(&monochrome2)
            .unwrap()
            .photometric_interpretation,
        "MONOCHROME2"
    );
    assert!(jpeg_direct_htj2k::frame(monochrome2, frame, TransferSyntax::Htj2kLossless).is_some());
    let monochrome1 = gray_jpeg(EncodedTilePhotometricInterpretation::Monochrome1);
    let error = pixel_profile_from_raw_jpeg_tile(&monochrome1).unwrap_err();
    assert!(error.to_string().contains("MONOCHROME1"), "{error}");
    assert!(jpeg_direct_htj2k::frame(monochrome1, frame, TransferSyntax::Htj2kLossless).is_none());

    let gray_j2k = |photometric| {
        RawTileSpec::j2k((2, 2), 1, 8)
            .photometric(photometric)
            .build(lossless_j2k(2, 2, 1, 8))
    };
    let frame = FrameExtent::interior(2, 2);
    let monochrome2 = gray_j2k(EncodedTilePhotometricInterpretation::Monochrome2);
    assert!(j2k_copies(&monochrome2, frame));
    assert!(j2k_recodes(&monochrome2, frame));
    let monochrome1 = gray_j2k(EncodedTilePhotometricInterpretation::Monochrome1);
    assert!(!j2k_copies(&monochrome1, frame));
    assert!(!j2k_recodes(&monochrome1, frame));
}

#[test]
fn monochrome1_dicom_source_frames_are_decoded_not_relabelled() {
    let tmp = tempfile::tempdir().unwrap();
    let tiff = tmp.path().join("gray.svs");
    let tiles = [
        encode_test_gray_jpeg(32, 32, 40),
        encode_test_gray_jpeg(32, 32, 200),
    ];
    write_tiled_grayscale_jpeg_tiff(&tiff, 64, 32, 32, 32, &tiles);
    let monochrome2 = export_for_test(
        &tiff,
        tmp.path().join("monochrome2"),
        TransferSyntax::JpegBaseline8Bit,
        PixelReencodePolicy::Lossless,
    )
    .instances[0]
        .path
        .clone();
    let copied = export_for_test(
        &monochrome2,
        tmp.path().join("copied"),
        TransferSyntax::JpegBaseline8Bit,
        PixelReencodePolicy::Never,
    );
    assert_eq!(copied.metrics.routes.jpeg_passthrough_frames, 2);

    // The same frames labelled MONOCHROME1, in a directory of their own.
    let mut object = dicom_object::open_file(&monochrome2).unwrap();
    object.put(DataElement::new(
        tags::PHOTOMETRIC_INTERPRETATION,
        VR::CS,
        "MONOCHROME1",
    ));
    std::fs::create_dir(tmp.path().join("monochrome1")).unwrap();
    let monochrome1 = tmp.path().join("monochrome1").join("source.dcm");
    object.write_to_file(&monochrome1).unwrap();
    let raw = Slide::open(&monochrome1)
        .unwrap()
        .read_raw_compressed_tile(&TileRequest::new(0, 0, 0, 0, 0))
        .unwrap();
    assert_eq!(
        raw.photometric_interpretation(),
        EncodedTilePhotometricInterpretation::Monochrome1
    );

    // VL Whole Slide Microscopy has no MONOCHROME1: frames are decoded,
    // never copied under MONOCHROME2.
    let refused = export_dicom(ExportRequest {
        source_path: monochrome1.clone(),
        output_dir: tmp.path().join("refused"),
        options: ExportOptions {
            tile_size: 32,
            transfer_syntax: TransferSyntax::JpegBaseline8Bit,
            encode_backend: EncodeBackendPreference::CpuOnly,
            pixel_reencode: PixelReencodePolicy::Never,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: Some(0),
    })
    .unwrap_err();
    assert!(
        refused.to_string().contains("pixel_reencode=never"),
        "{refused}"
    );
    for (transfer_syntax, output) in [
        (TransferSyntax::JpegBaseline8Bit, "jpeg"),
        (TransferSyntax::Htj2kLossless, "htj2k"),
    ] {
        let report = export_for_test(
            &monochrome1,
            tmp.path().join(output),
            transfer_syntax,
            PixelReencodePolicy::Any,
        );
        let metrics = &report.metrics;
        assert_eq!(
            metrics.routes.jpeg_passthrough_frames, 0,
            "{transfer_syntax:?}"
        );
        assert_eq!(
            metrics.jpeg_direct_htj2k.jpeg_direct_htj2k_53_frames, 0,
            "{transfer_syntax:?}"
        );
        assert_eq!(metrics.routes.compressed_source_reencoded_frames, 2);
        assert_ne!(
            pixel_data_fragments(&report.instances[0].path),
            tiles.to_vec()
        );
    }
}

#[test]
fn raw_tiles_with_fewer_significant_bits_are_not_copied() {
    // wsi-dicom writes Bits Stored equal to Bits Allocated, so a copied
    // DICOM Bits Stored 12 of 16 frame would make its unused bits pixel data.
    let frame = FrameExtent::interior(2, 2);
    let codestream = lossless_j2k(2, 2, 1, 16);
    let all_bits = RawTileSpec::j2k((2, 2), 1, 16).build(codestream.clone());
    let copied = j2k_passthrough_frame(all_bits.clone(), frame, TransferSyntax::Jpeg2000Lossless)
        .unwrap()
        .expect("a 16-bit codestream of 16-bit samples is copied");
    assert_eq!(copied.profile.bits_allocated, 16);
    assert!(j2k_recodes(&all_bits, frame));
    let declared_full = RawTileSpec::j2k((2, 2), 1, 16)
        .significant_bits(16)
        .build(codestream.clone());
    assert!(j2k_copies(&declared_full, frame));

    let twelve_of_sixteen = RawTileSpec::j2k((2, 2), 1, 16)
        .significant_bits(12)
        .build(codestream);
    assert!(!j2k_copies(&twelve_of_sixteen, frame));
    assert!(!j2k_recodes(&twelve_of_sixteen, frame));

    let jpeg_frame = FrameExtent::interior(16, 16);
    let jpeg = RawTileSpec::jpeg((16, 16)).build(ybr_jpeg(16, 16));
    assert!(raw_jpeg_matches_frame_geometry(&jpeg, jpeg_frame));
    let seven_of_eight = RawTileSpec::jpeg((16, 16))
        .significant_bits(7)
        .build(ybr_jpeg(16, 16));
    assert!(!raw_jpeg_matches_frame_geometry(
        &seven_of_eight,
        jpeg_frame
    ));
}

#[test]
fn raw_tile_reads_refused_for_stored_bytes_memory_or_size_take_the_decode_route() {
    assert!(raw_tile_read_declined(&WsiError::Unsupported {
        reason: "no stored bytes hold this tile".into(),
    }));
    // A lossless crop of a JPEG too large for the read's reservation.
    assert!(raw_tile_read_declined(&WsiError::ResourceLimit {
        resource: "lossless JPEG crop DCT coefficients",
        requested: 2 << 30,
        limit: 128 << 20,
    }));
    let tile_read = |reason: &str| WsiError::TileRead {
        col: 1,
        row: 0,
        level: 0,
        reason: reason.into(),
    };
    // A stored JPEG smaller than its tile decodes, padded. A JPEG 2000
    // codestream smaller than its tile does not decode either.
    let jpeg = tile_read("TIFF JPEG tile is 32x16, smaller than its 32x32 tile");
    assert!(undersized_jpeg_tile(&jpeg));
    assert!(raw_tile_read_declined(&jpeg));
    for reason in [
        "TIFF J2K tile is 32x16, smaller than its 32x32 tile",
        "truncated tile",
    ] {
        let error = tile_read(reason);
        assert!(!undersized_jpeg_tile(&error), "{reason}");
        assert!(!raw_tile_read_declined(&error), "{reason}");
    }
}

fn lossy_compression_method(path: &std::path::Path) -> Option<String> {
    dicom_object::open_file(path)
        .unwrap()
        .element(tags::LOSSY_IMAGE_COMPRESSION_METHOD)
        .ok()
        .map(|element| element.to_str().unwrap().trim().to_string())
}

#[test]
fn padded_tiff_edge_tiles_are_copied_whole_into_edge_frames() {
    // A 40x20 level in 32x32 tiles: both tiles are edge tiles whose stored
    // image is the full 32x32 tile, padding included.
    let tmp = tempfile::tempdir().unwrap();
    let jpeg_tiff = tmp.path().join("jpeg.svs");
    let jpeg_tiles = [
        encode_test_jpeg(32, 32, [180, 30, 40]),
        encode_test_jpeg(32, 32, [20, 170, 60]),
    ];
    write_tiled_jpeg_tiff(&jpeg_tiff, 40, 20, 32, 32, &jpeg_tiles);
    let edge = Slide::open(&jpeg_tiff)
        .unwrap()
        .read_raw_compressed_tile(&TileRequest::new(0, 0, 0, 1, 0))
        .unwrap();
    assert_eq!((edge.width(), edge.height()), (32, 32));
    assert_eq!(edge.tile_region(), PixelRect::new(0, 0, 8, 20));

    let report = export_for_test(
        &jpeg_tiff,
        tmp.path().join("jpeg-out"),
        TransferSyntax::JpegBaseline8Bit,
        PixelReencodePolicy::Never,
    );
    assert_eq!(report.metrics.routes.jpeg_passthrough_frames, 2);
    assert_eq!(
        pixel_data_fragments(&report.instances[0].path),
        jpeg_tiles.to_vec()
    );

    let j2k_tiff = tmp.path().join("j2k.svs");
    let codestreams = [lossless_j2k(32, 32, 3, 8), lossless_j2k(32, 32, 3, 8)];
    write_tiled_jp2k_rgb_tiff(&j2k_tiff, 40, 20, 32, 32, &codestreams);
    let report = export_for_test(
        &j2k_tiff,
        tmp.path().join("j2k-out"),
        TransferSyntax::Jpeg2000Lossless,
        PixelReencodePolicy::Never,
    );
    assert_eq!(report.metrics.routes.j2k_passthrough_frames, 2);
    assert_eq!(report.metrics.routes.compressed_source_reencoded_frames, 0);
    let fragments = dicom_object::open_file(&report.instances[0].path)
        .unwrap()
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap()
        .iter()
        .map(|fragment| dicom_fragment_payload_without_padding(fragment).to_vec())
        .collect::<Vec<_>>();
    assert_eq!(fragments, codestreams.to_vec());
}

#[test]
fn undersized_tiff_jpeg_tiles_are_reencoded_under_the_policy_with_their_history() {
    // Each 32x32 tile stores only a 32x16 JPEG. wsi-rs refuses the raw read
    // but decodes the tile, padded.
    let tmp = tempfile::tempdir().unwrap();
    let tiff = tmp.path().join("source.svs");
    let tiles = [
        encode_test_jpeg(32, 16, [180, 30, 40]),
        encode_test_jpeg(32, 16, [20, 170, 60]),
    ];
    write_tiled_jpeg_tiff(&tiff, 64, 32, 32, 32, &tiles);
    let raw_error = Slide::open(&tiff)
        .unwrap()
        .read_raw_compressed_tile(&TileRequest::new(0, 0, 0, 1, 0))
        .unwrap_err();
    assert!(undersized_jpeg_tile(&raw_error), "{raw_error}");

    // The source JPEG stage leads the lossy history; a JPEG Baseline output
    // adds its own stage.
    for (transfer_syntax, refusing, output, history) in [
        (
            TransferSyntax::JpegBaseline8Bit,
            PixelReencodePolicy::Lossless,
            "jpeg",
            "ISO_10918_1\\ISO_10918_1",
        ),
        (
            TransferSyntax::Htj2kLossless,
            PixelReencodePolicy::Never,
            "htj2k",
            "ISO_10918_1",
        ),
    ] {
        let refused = export_dicom(ExportRequest {
            source_path: tiff.clone(),
            output_dir: tmp.path().join(format!("{output}-refused")),
            options: ExportOptions {
                tile_size: 32,
                transfer_syntax,
                encode_backend: EncodeBackendPreference::CpuOnly,
                pixel_reencode: refusing,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap_err()
        .to_string();
        assert!(
            refused.contains("pixel re-encoding fallback is disabled"),
            "{transfer_syntax:?}: {refused}"
        );

        let report = export_for_test(
            &tiff,
            tmp.path().join(output),
            transfer_syntax,
            PixelReencodePolicy::Any,
        );
        assert_eq!(report.metrics.routes.total_frames, 2);
        assert_eq!(report.metrics.routes.compressed_source_reencoded_frames, 2);
        // The source JPEG stage is measured from the stored tiles even though
        // no raw tile was read.
        let instance = &report.instances[0].path;
        assert_eq!(
            lossy_compression_method(instance).as_deref(),
            Some(history),
            "{transfer_syntax:?}"
        );
        // Measured from the location: the TIFF's declared 32x32 stored tile
        // against the stored bytes.
        let ratio = dicom_object::open_file(instance)
            .unwrap()
            .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .unwrap()
            .to_multi_float64()
            .unwrap()[0];
        let stored_bytes = tiles.iter().map(Vec::len).sum::<usize>() as f64;
        let expected = (2 * 32 * 32 * 3) as f64 / stored_bytes;
        assert!(
            (ratio - expected).abs() < 1e-6 * expected,
            "{transfer_syntax:?}: {ratio} != {expected}"
        );
    }
}

#[test]
fn undersized_tiff_j2k_tiles_fail_as_read_errors_under_every_policy() {
    // wsi-rs decodes neither the raw form nor the pixels of a JPEG 2000
    // codestream smaller than its tile, so the export names the tile instead
    // of reporting a policy refusal that permitting re-encoding cannot fix.
    let tmp = tempfile::tempdir().unwrap();
    let tiff = tmp.path().join("source.svs");
    let codestreams = [lossless_j2k(32, 32, 3, 8), lossless_j2k(32, 16, 3, 8)];
    write_tiled_jp2k_rgb_tiff(&tiff, 64, 32, 32, 32, &codestreams);
    let raw_error = Slide::open(&tiff)
        .unwrap()
        .read_raw_compressed_tile(&TileRequest::new(0, 0, 0, 1, 0))
        .unwrap_err();
    assert!(!raw_tile_read_declined(&raw_error), "{raw_error}");

    for pixel_reencode in [PixelReencodePolicy::Never, PixelReencodePolicy::Lossless] {
        let error = export_dicom(ExportRequest {
            source_path: tiff.clone(),
            output_dir: tmp.path().join(format!("{pixel_reencode:?}")),
            options: ExportOptions {
                tile_size: 32,
                transfer_syntax: TransferSyntax::Htj2kLossless,
                encode_backend: EncodeBackendPreference::CpuOnly,
                pixel_reencode,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("raw source read failed at row=0 col=1")
                && error.contains("smaller than its 32x32 tile"),
            "{pixel_reencode:?}: {error}"
        );
    }
}
