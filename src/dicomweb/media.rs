//! Media types (PS3.18 8.7) and the Bulk Data media types of each Transfer
//! Syntax (Tables 8.7.3-4 and 8.7.3-5).

use dicom_dictionary_std::uids;

/// A parsed `type/subtype *(; name=value)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaType {
    /// Lowercase `type/subtype`; type, subtype and parameter names are
    /// case-insensitive (PS3.18 8.7).
    pub(crate) essence: String,
    /// Parameters in order, names lowercase, quoted values unquoted.
    pub(crate) parameters: Vec<(String, String)>,
}

impl MediaType {
    /// Parses one media type (RFC 7231 3.1.1.1).
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut rest = text.trim();
        let end = rest.find(';').unwrap_or(rest.len());
        let essence = rest[..end].trim().to_ascii_lowercase();
        let valid = essence
            .split_once('/')
            .is_some_and(|(kind, subtype)| is_token(kind) && is_token(subtype));
        if !valid {
            return Err(format!("{text:?} is not a media type (type/subtype)"));
        }
        rest = &rest[end..];
        let mut parameters = Vec::new();
        while let Some(after) = rest.strip_prefix(';') {
            let after = after.trim_start();
            if after.is_empty() {
                break;
            }
            let equals = after
                .find('=')
                .ok_or_else(|| format!("parameter {after:?} of {text:?} has no value"))?;
            let name = after[..equals].trim().to_ascii_lowercase();
            if !is_token(&name) {
                return Err(format!(
                    "parameter name {name:?} of {text:?} is not a token"
                ));
            }
            let (value, remainder) = parameter_value(after[equals + 1..].trim_start())
                .ok_or_else(|| format!("parameter {name} of {text:?} has a malformed value"))?;
            parameters.push((name, value));
            rest = remainder.trim_start();
            if !rest.is_empty() && !rest.starts_with(';') {
                return Err(format!("{text:?} has text after a parameter value"));
            }
        }
        Ok(Self {
            essence,
            parameters,
        })
    }

    /// The first value of parameter `name` (lowercase).
    pub(crate) fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    /// How many times parameter `name` appears.
    pub(crate) fn count(&self, name: &str) -> usize {
        self.parameters
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .count()
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// A token or quoted-string, and the text after it.
fn parameter_value(text: &str) -> Option<(String, &str)> {
    if let Some(quoted) = text.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = quoted.char_indices();
        while let Some((index, character)) = chars.next() {
            match character {
                '"' => return Some((value, &quoted[index + 1..])),
                '\\' => value.push(chars.next()?.1),
                other => value.push(other),
            }
        }
        return None;
    }
    let end = text.find([';', ' ', '\t']).unwrap_or(text.len());
    let value = &text[..end];
    // A boundary may hold characters a token does not (8.6.1.2.1 bchar).
    (!value.is_empty() && !value.contains([',', '"'])).then(|| (value.to_string(), &text[end..]))
}

/// One row of Table 8.7.3-5 (or 8.7.3-4): a Bulk Data media type and the
/// Transfer Syntaxes it carries, the first the default.
pub(crate) struct BulkMediaType {
    pub(crate) essence: &'static str,
    pub(crate) syntaxes: &'static [&'static str],
}

/// Table 8.7.3-4 (application/octet-stream) and Table 8.7.3-5 for images.
pub(crate) const BULK_MEDIA_TYPES: &[BulkMediaType] = &[
    BulkMediaType {
        essence: "application/octet-stream",
        syntaxes: &[
            uids::EXPLICIT_VR_LITTLE_ENDIAN,
            uids::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
        ],
    },
    BulkMediaType {
        essence: "image/jpeg",
        syntaxes: &[
            uids::JPEG_LOSSLESS_SV1,
            uids::JPEG_BASELINE8_BIT,
            uids::JPEG_EXTENDED12_BIT,
            uids::JPEG_LOSSLESS,
        ],
    },
    BulkMediaType {
        essence: "application/x-deflate",
        syntaxes: &[uids::DEFLATED_IMAGE_FRAME_COMPRESSION],
    },
    BulkMediaType {
        essence: "image/dicom-rle",
        syntaxes: &[uids::RLE_LOSSLESS],
    },
    BulkMediaType {
        essence: "image/jls",
        syntaxes: &[uids::JPEGLS_LOSSLESS, uids::JPEGLS_NEAR_LOSSLESS],
    },
    BulkMediaType {
        essence: "image/jp2",
        syntaxes: &[uids::JPEG2000_LOSSLESS, uids::JPEG2000],
    },
    BulkMediaType {
        essence: "image/jpx",
        syntaxes: &[uids::JPEG2000MC_LOSSLESS, uids::JPEG2000MC],
    },
    BulkMediaType {
        essence: "image/jphc",
        syntaxes: &[uids::HTJ2K_LOSSLESS, uids::HTJ2K_LOSSLESS_RPCL, uids::HTJ2K],
    },
    BulkMediaType {
        essence: "image/jxl",
        syntaxes: &[
            uids::JPEGXL_LOSSLESS,
            uids::JPEGXLJPEG_RECOMPRESSION,
            uids::JPEGXL,
        ],
    },
];

/// The Table 8.7.3-4 or 8.7.3-5 row of a media type.
pub(crate) fn bulk_media_type(essence: &str) -> Option<&'static BulkMediaType> {
    BULK_MEDIA_TYPES.iter().find(|row| row.essence == essence)
}

/// The media type Table 8.7.3-5 assigns to a Transfer Syntax.
pub(crate) fn media_type_of(syntax: &str) -> Option<&'static str> {
    BULK_MEDIA_TYPES
        .iter()
        .find(|row| row.syntaxes.contains(&syntax))
        .map(|row| row.essence)
}

/// An Accept value naming every image Bulk Data media type of Table
/// 8.7.3-5 with any Transfer Syntax, preferred over uncompressed bytes.
pub(crate) fn any_frame_accept() -> String {
    let mut ranges: Vec<String> = BULK_MEDIA_TYPES
        .iter()
        .skip(1)
        .map(|row| {
            format!(
                "multipart/related; type=\"{}\"; transfer-syntax=*",
                row.essence
            )
        })
        .collect();
    ranges.push(
        "multipart/related; type=\"application/octet-stream\"; transfer-syntax=*; q=0.5".into(),
    );
    ranges.join(", ")
}

#[cfg(test)]
mod tests {
    use super::{any_frame_accept, bulk_media_type, media_type_of, MediaType};

    #[test]
    fn parameters_parse_quoted_and_token_values() {
        let parsed = MediaType::parse(
            "Multipart/Related; type=\"image/jpeg\"; Boundary=abc'()+_,-./:=?; transfer-syntax=1.2.840.10008.1.2.4.50",
        );
        // A comma ends a token value; the boundary must then be quoted.
        assert!(parsed.is_err());
        let parsed = MediaType::parse(
            "Multipart/Related; type=\"image/jpeg\"; Boundary=\"a b:c\"; transfer-syntax=1.2.840.10008.1.2.4.50",
        )
        .unwrap();
        assert_eq!(parsed.essence, "multipart/related");
        assert_eq!(parsed.parameter("type"), Some("image/jpeg"));
        assert_eq!(parsed.parameter("boundary"), Some("a b:c"));
        assert_eq!(
            parsed.parameter("transfer-syntax"),
            Some("1.2.840.10008.1.2.4.50")
        );
        for bad in ["image", "image/", "image/jpeg; q", "image/jpeg; a=\"x"] {
            assert!(MediaType::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn table_8_7_3_5_maps_syntaxes_and_defaults() {
        assert_eq!(media_type_of("1.2.840.10008.1.2.4.50"), Some("image/jpeg"));
        assert_eq!(media_type_of("1.2.840.10008.1.2.4.202"), Some("image/jphc"));
        assert_eq!(
            bulk_media_type("image/jpeg").unwrap().syntaxes[0],
            "1.2.840.10008.1.2.4.70"
        );
        assert_eq!(
            bulk_media_type("application/octet-stream")
                .unwrap()
                .syntaxes[0],
            "1.2.840.10008.1.2.1"
        );
        assert!(any_frame_accept().contains("type=\"image/jxl\"; transfer-syntax=*"));
    }
}
