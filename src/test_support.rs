use std::path::Path;

use j2k_jpeg::{JpegBackend, JpegSamples, JpegSubsampling};

pub(crate) fn find_command_for_test(name: &str) -> Option<String> {
    let command = std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(name))
                .find(|path| path.is_file())
        })
        .or_else(|| {
            let staged = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("dicom3tools-mac")
                .join(name);
            staged.is_file().then_some(staged)
        })
        .map(|path| path.to_string_lossy().into_owned());
    if command.is_none() && external_tool_is_required(name) {
        panic!(
            "required external conformance tool `{name}` was not found; checked PATH and target/dicom3tools-mac"
        );
    }
    command
}

fn external_tool_is_required(name: &str) -> bool {
    std::env::var("WSI_DICOM_REQUIRED_EXTERNAL_TOOLS")
        .ok()
        .is_some_and(|required| external_tool_list_contains(&required, name))
}

fn external_tool_list_contains(required: &str, name: &str) -> bool {
    required
        .split(',')
        .map(str::trim)
        .any(|required_name| required_name == name)
}

pub(crate) fn encode_test_jpeg(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    encode_test_jpeg_with_subsampling(width, height, rgb, JpegSubsampling::Ybr422)
}

pub(crate) fn encode_test_jpeg_with_subsampling(
    width: u32,
    height: u32,
    rgb: [u8; 3],
    subsampling: JpegSubsampling,
) -> Vec<u8> {
    let pixels = vec![rgb; (width * height) as usize]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    j2k_jpeg::encode_jpeg_baseline(
        JpegSamples::Rgb8 {
            data: &pixels,
            width,
            height,
        },
        j2k_jpeg::JpegEncodeOptions {
            quality: 90,
            subsampling,
            restart_interval: None,
            backend: JpegBackend::Cpu,
        },
    )
    .unwrap()
    .data
}

pub(crate) fn encode_test_gray_jpeg(width: u32, height: u32, value: u8) -> Vec<u8> {
    let pixels = vec![value; (width * height) as usize];
    j2k_jpeg::encode_jpeg_baseline(
        JpegSamples::Gray8 {
            data: &pixels,
            width,
            height,
        },
        j2k_jpeg::JpegEncodeOptions {
            quality: 90,
            subsampling: JpegSubsampling::Gray,
            restart_interval: None,
            backend: JpegBackend::Cpu,
        },
    )
    .unwrap()
    .data
}

pub(crate) fn dicom_fragment_payload_without_padding(fragment: &[u8]) -> &[u8] {
    if fragment.len().is_multiple_of(2) && fragment.last() == Some(&0) {
        &fragment[..fragment.len() - 1]
    } else {
        fragment
    }
}

pub(crate) fn assert_htj2k_rpcl_codestream(codestream: &[u8]) {
    let cod_offset = j2k_cod_marker_offset(codestream);
    assert_eq!(codestream[cod_offset + 5], 0x02);
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
}

pub(crate) fn j2k_cod_marker_offset(codestream: &[u8]) -> usize {
    codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker")
}

pub(crate) fn write_tiled_jpeg_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(path, width, height, tile_width, tile_height, 7, 6, 3, tiles);
}

pub(crate) fn write_tiled_grayscale_jpeg_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(path, width, height, tile_width, tile_height, 7, 1, 1, tiles);
}

pub(crate) fn write_tiled_jp2k_rgb_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(
        path,
        width,
        height,
        tile_width,
        tile_height,
        33004,
        2,
        3,
        tiles,
    );
}

pub(crate) fn write_tiled_jp2k_ycbcr_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(
        path,
        width,
        height,
        tile_width,
        tile_height,
        33005,
        6,
        3,
        tiles,
    );
}

pub(crate) fn write_tiled_aperio_jp2k_ycbcr_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(
        path,
        width,
        height,
        tile_width,
        tile_height,
        33003,
        6,
        3,
        tiles,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_tiled_compressed_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    compression: u16,
    photometric: u16,
    samples_per_pixel: u16,
    tiles: &[Vec<u8>],
) {
    write_tiled_tiff_with_bits(
        path,
        (width, height, tile_width, tile_height),
        compression,
        photometric,
        samples_per_pixel,
        8,
        tiles,
    );
}

/// Uncompressed tiled 16-bit grayscale TIFF; each tile holds little-endian samples.
pub(crate) fn write_tiled_uncompressed_gray16_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_tiff_with_bits(
        path,
        (width, height, tile_width, tile_height),
        1,
        1,
        1,
        16,
        tiles,
    );
}

fn write_tiled_tiff_with_bits(
    path: &Path,
    (width, height, tile_width, tile_height): (u32, u32, u32, u32),
    compression: u16,
    photometric: u16,
    samples_per_pixel: u16,
    bits_per_sample: u16,
    tiles: &[Vec<u8>],
) {
    write_multi_image_tiff(
        path,
        &[TiffImageSpec {
            width,
            height,
            compression,
            photometric,
            samples_per_pixel,
            bits_per_sample,
            pixels_per_cm: Some(40_000),
            layout: TiffChunkLayout::Tiled {
                tile_width,
                tile_height,
            },
            chunks: tiles,
            description: None,
            subfile_type: None,
        }],
    );
}

/// Storage of one synthetic TIFF image.
#[derive(Clone, Copy)]
pub(crate) enum TiffChunkLayout {
    Tiled { tile_width: u32, tile_height: u32 },
    Stripped { rows_per_strip: u32 },
}

/// One image (IFD) of a synthetic multi-image TIFF.
pub(crate) struct TiffImageSpec<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) compression: u16,
    pub(crate) photometric: u16,
    pub(crate) samples_per_pixel: u16,
    pub(crate) bits_per_sample: u16,
    pub(crate) pixels_per_cm: Option<u32>,
    pub(crate) layout: TiffChunkLayout,
    pub(crate) chunks: &'a [Vec<u8>],
    pub(crate) description: Option<&'a str>,
    pub(crate) subfile_type: Option<u32>,
}

/// Write a little-endian TIFF with one IFD per image, in order.
pub(crate) fn write_multi_image_tiff(path: &Path, images: &[TiffImageSpec<'_>]) {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    let mut next_ifd_pointer = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());
    for image in images {
        let offsets = image
            .chunks
            .iter()
            .map(|chunk| {
                let offset = buf.len() as u32;
                buf.extend_from_slice(chunk);
                offset
            })
            .collect::<Vec<_>>();
        let counts = image
            .chunks
            .iter()
            .map(|chunk| chunk.len() as u32)
            .collect::<Vec<_>>();
        let array_value = |buf: &mut Vec<u8>, values: &[u32]| {
            if values.len() == 1 {
                return values[0].to_le_bytes();
            }
            let offset = buf.len() as u32;
            for value in values {
                buf.extend_from_slice(&value.to_le_bytes());
            }
            offset.to_le_bytes()
        };
        let offsets_value = array_value(&mut buf, &offsets);
        let counts_value = array_value(&mut buf, &counts);
        let description = image.description.map(|description| {
            let offset = buf.len() as u32;
            buf.extend_from_slice(description.as_bytes());
            buf.push(0);
            (offset, description.len() as u32 + 1)
        });
        let mut tags = vec![
            tiff_tag(256, 4, 1, image.width.to_le_bytes()),
            tiff_tag(257, 4, 1, image.height.to_le_bytes()),
            tiff_tag(258, 3, 1, tiff_short_value(image.bits_per_sample)),
            tiff_tag(259, 3, 1, tiff_short_value(image.compression)),
            tiff_tag(262, 3, 1, tiff_short_value(image.photometric)),
            tiff_tag(277, 3, 1, tiff_short_value(image.samples_per_pixel)),
            tiff_tag(284, 3, 1, tiff_short_value(1)),
        ];
        if let Some(resolution) = image.pixels_per_cm {
            let offset = buf.len() as u32;
            buf.extend_from_slice(&resolution.to_le_bytes());
            buf.extend_from_slice(&1_u32.to_le_bytes());
            tags.extend([
                tiff_tag(282, 5, 1, offset.to_le_bytes()),
                tiff_tag(283, 5, 1, offset.to_le_bytes()),
                tiff_tag(296, 3, 1, tiff_short_value(3)),
            ]);
        }
        if let Some(subfile_type) = image.subfile_type {
            tags.push(tiff_tag(254, 4, 1, subfile_type.to_le_bytes()));
        }
        if let Some((offset, len)) = description {
            tags.push(tiff_tag(270, 2, len, offset.to_le_bytes()));
        }
        let count = offsets.len() as u32;
        match image.layout {
            TiffChunkLayout::Tiled {
                tile_width,
                tile_height,
            } => tags.extend([
                tiff_tag(322, 4, 1, tile_width.to_le_bytes()),
                tiff_tag(323, 4, 1, tile_height.to_le_bytes()),
                tiff_tag(324, 4, count, offsets_value),
                tiff_tag(325, 4, count, counts_value),
            ]),
            TiffChunkLayout::Stripped { rows_per_strip } => tags.extend([
                tiff_tag(273, 4, count, offsets_value),
                tiff_tag(278, 4, 1, rows_per_strip.to_le_bytes()),
                tiff_tag(279, 4, count, counts_value),
            ]),
        }
        tags.sort_by_key(|tag| tag.0);
        if buf.len() % 2 != 0 {
            buf.push(0);
        }
        let ifd_offset = buf.len() as u32;
        buf[next_ifd_pointer..next_ifd_pointer + 4].copy_from_slice(&ifd_offset.to_le_bytes());
        buf.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        for (tag, typ, count, value) in tags {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&value);
        }
        next_ifd_pointer = buf.len();
        buf.extend_from_slice(&0u32.to_le_bytes());
    }
    std::fs::write(path, buf).unwrap();
}

/// Interleaved 8-bit RGB pixels that differ per pixel and per `seed`.
pub(crate) fn patterned_rgb(width: u32, height: u32, seed: u8) -> Vec<u8> {
    (0..width * height)
        .flat_map(|pixel| {
            let x = (pixel % width) as u8;
            let y = (pixel / width) as u8;
            [
                x.wrapping_mul(7).wrapping_add(seed),
                y.wrapping_mul(11).wrapping_add(seed),
                x.wrapping_add(y).wrapping_mul(3),
            ]
        })
        .collect()
}

/// Aperio-style SVS with a 64x48 JPEG-tiled level (MPP 0.5) and associated
/// images: an uncompressed 16x12 thumbnail (the second IFD), an uncompressed
/// 20x10 label (SubfileType 1) and a 32x16 JPEG macro (SubfileType 9). With
/// `unnamed_extra`, a further stripped image that names no associated kind.
pub(crate) fn write_aperio_svs_with_associated_images(path: &Path, unnamed_extra: bool) {
    let tiles = (0..4)
        .map(|index| encode_test_jpeg(32, 32, [40 + index * 30, 90, 160 - index * 20]))
        .collect::<Vec<_>>();
    let thumbnail = [patterned_rgb(16, 12, 1)];
    let label = [patterned_rgb(20, 10, 2)];
    let macro_jpeg = [encode_test_jpeg(32, 16, [200, 120, 60])];
    let extra = [patterned_rgb(8, 8, 3)];
    let stripped = |width, height, compression, photometric, chunks, subfile_type| TiffImageSpec {
        width,
        height,
        compression,
        photometric,
        samples_per_pixel: 3,
        bits_per_sample: 8,
        pixels_per_cm: None,
        layout: TiffChunkLayout::Stripped {
            rows_per_strip: height,
        },
        chunks,
        description: None,
        subfile_type,
    };
    let mut images = vec![
        TiffImageSpec {
            width: 64,
            height: 48,
            compression: 7,
            photometric: 6,
            samples_per_pixel: 3,
            bits_per_sample: 8,
            pixels_per_cm: None,
            layout: TiffChunkLayout::Tiled {
                tile_width: 32,
                tile_height: 32,
            },
            chunks: &tiles,
            description: Some("Aperio Image Library v12.0.15|AppMag = 20|MPP = 0.5"),
            subfile_type: None,
        },
        stripped(16, 12, 1, 2, &thumbnail[..], None),
        stripped(20, 10, 1, 2, &label[..], Some(1)),
        stripped(32, 16, 7, 6, &macro_jpeg[..], Some(9)),
    ];
    if unnamed_extra {
        images.push(stripped(8, 8, 1, 2, &extra[..], Some(0)));
    }
    write_multi_image_tiff(path, &images);
}

pub(crate) fn tiff_short_value(value: u16) -> [u8; 4] {
    let mut bytes = [0u8; 4];
    bytes[..2].copy_from_slice(&value.to_le_bytes());
    bytes
}

pub(crate) fn tiff_tag(tag: u16, typ: u16, count: u32, value: [u8; 4]) -> (u16, u16, u32, [u8; 4]) {
    (tag, typ, count, value)
}

pub(crate) fn decode_j2k_frame_for_test(
    codestream: &[u8],
    width: u32,
    height: u32,
    components: u8,
    bits_allocated: u16,
) -> Vec<u8> {
    let fmt = match (components, bits_allocated) {
        (1, 8) => j2k::PixelFormat::Gray8,
        (3, 8) => j2k::PixelFormat::Rgb8,
        (1, 16) => j2k::PixelFormat::Gray16,
        (3, 16) => j2k::PixelFormat::Rgb16,
        other => panic!("unsupported frame profile: {other:?}"),
    };
    let bytes_per_sample = if bits_allocated <= 8 { 1usize } else { 2usize };
    let stride = width as usize * components as usize * bytes_per_sample;
    let mut decoder = j2k::J2kDecoder::new(codestream).unwrap_or_else(|err| {
        if codestream.last() == Some(&0) {
            j2k::J2kDecoder::new(&codestream[..codestream.len() - 1])
                .unwrap_or_else(|_| panic!("parse frame: {err}"))
        } else {
            panic!("parse frame: {err}");
        }
    });
    let mut decoded = vec![0; stride * height as usize];
    decoder.decode_into(&mut decoded, stride, fmt).unwrap();
    decoded
}

#[cfg(test)]
mod tests {
    use super::find_command_for_test;

    #[test]
    fn find_command_for_test_returns_none_for_missing_command() {
        assert!(find_command_for_test("wsi-dicom-command-that-should-not-exist").is_none());
    }

    #[test]
    fn external_tool_requirement_parser_matches_complete_names_only() {
        let required = "dciodvfy, dcentvfy";
        assert!(super::external_tool_list_contains(required, "dciodvfy"));
        assert!(super::external_tool_list_contains(required, "dcentvfy"));
        assert!(!super::external_tool_list_contains(required, "dciod"));
    }

    #[test]
    fn encode_test_jpeg_emits_jpeg_baseline_bytes() {
        let jpeg = super::encode_test_jpeg(2, 1, [1, 2, 3]);

        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        assert!(jpeg.ends_with(&[0xFF, 0xD9]));
    }

    #[test]
    fn dicom_fragment_payload_without_padding_trims_only_even_zero_padding() {
        assert_eq!(
            super::dicom_fragment_payload_without_padding(&[1, 2, 3, 0]),
            &[1, 2, 3]
        );
        assert_eq!(
            super::dicom_fragment_payload_without_padding(&[1, 2, 3, 4]),
            &[1, 2, 3, 4]
        );
        assert_eq!(
            super::dicom_fragment_payload_without_padding(&[1, 2, 0]),
            &[1, 2, 0]
        );
    }
}
