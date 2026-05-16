use std::path::Path;

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
}
