use std::io::Write;
use std::path::Path;

use signinum_jpeg::{JpegBackend, JpegSamples, JpegSubsampling};

pub(crate) fn find_command_for_test(name: &str) -> Option<String> {
    std::env::var_os("PATH")
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
        .map(|path| path.to_string_lossy().into_owned())
}

pub(crate) fn read_binary_ppm_for_test(path: &Path) -> (u32, u32, Vec<u8>) {
    let bytes = std::fs::read(path).expect("read PPM");
    let mut cursor = 0usize;
    let magic = read_netpbm_token_for_test(&bytes, &mut cursor);
    assert_eq!(magic, "P6");
    let width = read_netpbm_token_for_test(&bytes, &mut cursor)
        .parse::<u32>()
        .expect("PPM width");
    let height = read_netpbm_token_for_test(&bytes, &mut cursor)
        .parse::<u32>()
        .expect("PPM height");
    let max_value = read_netpbm_token_for_test(&bytes, &mut cursor)
        .parse::<u32>()
        .expect("PPM max value");
    assert_eq!(max_value, 255);
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let expected_len = (width as usize) * (height as usize) * 3;
    assert_eq!(bytes.len() - cursor, expected_len);
    (width, height, bytes[cursor..].to_vec())
}

pub(crate) fn encode_test_jpeg(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let pixels = vec![rgb; (width * height) as usize]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    signinum_jpeg::encode_jpeg_baseline(
        JpegSamples::Rgb8 {
            data: &pixels,
            width,
            height,
        },
        signinum_jpeg::JpegEncodeOptions {
            quality: 90,
            subsampling: JpegSubsampling::Ybr422,
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
    let cod_offset = codestream
        .windows(2)
        .position(|window| window == [0xFF, 0x52])
        .expect("COD marker");
    assert_eq!(codestream[cod_offset + 5], 0x02);
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x50]));
    assert!(codestream.windows(2).any(|window| window == [0xFF, 0x55]));
}

pub(crate) fn write_tiled_jpeg_tiff(
    path: &Path,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tiles: &[Vec<u8>],
) {
    write_tiled_compressed_tiff(path, width, height, tile_width, tile_height, 7, 6, tiles);
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
    tiles: &[Vec<u8>],
) {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    let first_ifd_pos = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());

    let mut tile_offsets = Vec::with_capacity(tiles.len());
    let mut tile_byte_counts = Vec::with_capacity(tiles.len());
    for tile in tiles {
        tile_offsets.push(buf.len() as u32);
        tile_byte_counts.push(tile.len() as u32);
        buf.extend_from_slice(tile);
    }

    let tile_offsets_array_offset = buf.len() as u32;
    for value in &tile_offsets {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    let tile_byte_counts_array_offset = buf.len() as u32;
    for value in &tile_byte_counts {
        buf.extend_from_slice(&value.to_le_bytes());
    }
    let x_resolution_offset = buf.len() as u32;
    buf.extend_from_slice(&40_000u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    let y_resolution_offset = buf.len() as u32;
    buf.extend_from_slice(&40_000u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());

    let ifd_offset = buf.len() as u32;
    buf[first_ifd_pos..first_ifd_pos + 4].copy_from_slice(&ifd_offset.to_le_bytes());
    let mut tags = vec![
        tiff_tag(256, 4, 1, width.to_le_bytes()),
        tiff_tag(257, 4, 1, height.to_le_bytes()),
        tiff_tag(258, 3, 1, tiff_short_value(8)),
        tiff_tag(259, 3, 1, tiff_short_value(compression)),
        tiff_tag(262, 3, 1, tiff_short_value(photometric)),
        tiff_tag(277, 3, 1, tiff_short_value(3)),
        tiff_tag(282, 5, 1, x_resolution_offset.to_le_bytes()),
        tiff_tag(283, 5, 1, y_resolution_offset.to_le_bytes()),
        tiff_tag(296, 3, 1, tiff_short_value(3)),
        tiff_tag(322, 4, 1, tile_width.to_le_bytes()),
        tiff_tag(323, 4, 1, tile_height.to_le_bytes()),
        tiff_tag(
            324,
            4,
            tile_offsets.len() as u32,
            if tile_offsets.len() == 1 {
                tile_offsets[0].to_le_bytes()
            } else {
                tile_offsets_array_offset.to_le_bytes()
            },
        ),
        tiff_tag(
            325,
            4,
            tile_byte_counts.len() as u32,
            if tile_byte_counts.len() == 1 {
                tile_byte_counts[0].to_le_bytes()
            } else {
                tile_byte_counts_array_offset.to_le_bytes()
            },
        ),
    ];
    tags.sort_by_key(|tag| tag.0);

    buf.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in tags {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&value);
    }
    buf.extend_from_slice(&0u32.to_le_bytes());

    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&buf).unwrap();
    file.flush().unwrap();
}

pub(crate) fn tiff_short_value(value: u16) -> [u8; 4] {
    let mut bytes = [0u8; 4];
    bytes[..2].copy_from_slice(&value.to_le_bytes());
    bytes
}

pub(crate) fn tiff_tag(tag: u16, typ: u16, count: u32, value: [u8; 4]) -> (u16, u16, u32, [u8; 4]) {
    (tag, typ, count, value)
}

fn read_netpbm_token_for_test(bytes: &[u8], cursor: &mut usize) -> String {
    loop {
        while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor >= bytes.len() || bytes[*cursor] != b'#' {
            break;
        }
        while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
            *cursor += 1;
        }
    }
    let start = *cursor;
    while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    std::str::from_utf8(&bytes[start..*cursor])
        .expect("PPM token is UTF-8")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{find_command_for_test, read_binary_ppm_for_test};

    #[test]
    fn read_binary_ppm_for_test_skips_comments_and_reads_pixels() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sample.ppm");
        std::fs::write(&path, b"P6\n# comment\n2 1\n255\n\x01\x02\x03\x04\x05\x06")
            .expect("write PPM");

        let (width, height, pixels) = read_binary_ppm_for_test(&path);

        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn find_command_for_test_returns_none_for_missing_command() {
        assert!(find_command_for_test("wsi-dicom-command-that-should-not-exist").is_none());
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
