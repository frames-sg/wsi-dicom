use moxcms::{ColorProfile, DataColorSpace, ParsingOptions, ProfileClass};

use crate::Error;

const ICC_HEADER_BYTES: usize = 128;
const ICC_PROFILE_CLASS: std::ops::Range<usize> = 12..16;
const ICC_INPUT_COLOR_SPACE: std::ops::Range<usize> = 16..20;
const ICC_PROFILE_CONNECTION_SPACE: std::ops::Range<usize> = 20..24;
const ICC_CREATION_DATETIME: std::ops::Range<usize> = 24..36;
const ICC_SIGNATURE: std::ops::Range<usize> = 36..40;

pub(crate) fn validate_dicom_icc_profile(bytes: &[u8]) -> Result<(), Error> {
    let profile = encoded_profile_bytes(bytes)?;
    if &profile[ICC_SIGNATURE] != b"acsp" {
        return Err(invalid_icc("header signature must be 'acsp'"));
    }
    if &profile[ICC_PROFILE_CLASS] != b"scnr" {
        return Err(invalid_icc(
            "profile/device class must be input device ('scnr')",
        ));
    }
    if &profile[ICC_INPUT_COLOR_SPACE] != b"RGB " {
        return Err(invalid_icc("input color space must be RGB"));
    }
    if !matches!(&profile[ICC_PROFILE_CONNECTION_SPACE], b"XYZ " | b"Lab ") {
        return Err(invalid_icc("PCS must be XYZ or Lab"));
    }

    let parsed = ColorProfile::new_from_slice_with_options(profile, parsing_options(profile))
        .map_err(|err| Error::Metadata {
            reason: format!("DICOM ICC Profile is malformed: {err}"),
        })?;
    if parsed.profile_class != ProfileClass::InputDevice
        || parsed.color_space != DataColorSpace::Rgb
        || !matches!(parsed.pcs, DataColorSpace::Xyz | DataColorSpace::Lab)
    {
        return Err(invalid_icc(
            "parsed class, input color space, or PCS is not DICOM-conformant",
        ));
    }
    Ok(())
}

/// Whether a matrix/TRC profile has the primaries of the space a Color Space
/// (0028,2002) Defined Term names (PS3.3 C.11.15.1.2). `None` when that cannot
/// be decided: an unreadable or table-based profile, or another term.
pub(crate) fn defines_color_space(bytes: &[u8], term: &str) -> Option<bool> {
    let reference = match term {
        "SRGB" => ColorProfile::new_srgb(),
        "ADOBERGB" => ColorProfile::new_adobe_rgb(),
        "ROMMRGB" => ColorProfile::new_pro_photo_rgb(),
        "DISPLAYP3" => ColorProfile::new_display_p3(),
        _ => return None,
    };
    let profile = encoded_profile_bytes(bytes).ok()?;
    let parsed =
        ColorProfile::new_from_slice_with_options(profile, parsing_options(profile)).ok()?;
    // An AToB table, when present, is the transform; the colorants are not.
    let tables = [
        &parsed.lut_a_to_b_perceptual,
        &parsed.lut_a_to_b_colorimetric,
        &parsed.lut_a_to_b_saturation,
    ];
    if tables.iter().any(|table| table.is_some()) || parsed.red_trc.is_none() {
        return None;
    }
    // PCS-relative (D50) colorants of profiles of one space agree to the
    // s15Fixed16 rounding and chromatic adaptation differences between
    // vendors; distinct spaces differ by more than 0.05.
    let close = |a: moxcms::Xyzd, b: moxcms::Xyzd| {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.z - b.z).abs() < 0.01
    };
    Some(
        close(parsed.red_colorant, reference.red_colorant)
            && close(parsed.green_colorant, reference.green_colorant)
            && close(parsed.blue_colorant, reference.blue_colorant),
    )
}

pub(crate) fn synthetic_srgb_icc_profile() -> Result<Vec<u8>, Error> {
    encode_synthetic_input_profile("sRGB", ColorProfile::new_srgb())
}

/// Whether `bytes` is the sRGB profile this crate synthesizes.
pub(crate) fn is_synthetic_srgb_icc_profile(bytes: &[u8]) -> bool {
    static SRGB: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    SRGB.get_or_init(|| synthetic_srgb_icc_profile().ok())
        .as_deref()
        .is_some_and(|srgb| srgb == bytes)
}

pub(crate) fn synthetic_display_p3_icc_profile() -> Result<Vec<u8>, Error> {
    encode_synthetic_input_profile("Display P3", ColorProfile::new_display_p3())
}

fn encode_synthetic_input_profile(
    name: &'static str,
    mut profile: ColorProfile,
) -> Result<Vec<u8>, Error> {
    profile.profile_class = ProfileClass::InputDevice;
    let mut bytes = profile.encode().map_err(|err| Error::Metadata {
        reason: format!("failed to generate synthetic {name} ICC profile: {err}"),
    })?;
    stabilize_synthetic_icc_profile(&mut bytes);
    validate_dicom_icc_profile(&bytes)?;
    Ok(bytes)
}

// moxcms rejects profiles of 10 MB or more by default. Conformant scanner
// input profiles exceed that (the Leica GT450 CLUT profile is 13 MB), so the
// limits are bounded by the encoded bytes instead: a CLUT cannot have more
// grid nodes than the profile has bytes.
fn parsing_options(profile: &[u8]) -> ParsingOptions {
    let defaults = ParsingOptions::default();
    ParsingOptions {
        max_profile_size: defaults
            .max_profile_size
            .max(profile.len().saturating_add(1)),
        max_allowed_clut_size: defaults.max_allowed_clut_size.max(profile.len()),
        ..defaults
    }
}

fn encoded_profile_bytes(bytes: &[u8]) -> Result<&[u8], Error> {
    if bytes.len() < ICC_HEADER_BYTES {
        return Err(invalid_icc("profile is shorter than the 128-byte header"));
    }
    let declared_size = usize::try_from(u32::from_be_bytes(
        bytes[..4].try_into().expect("four-byte ICC size header"),
    ))
    .expect("u32 fits usize on supported platforms");
    if declared_size < ICC_HEADER_BYTES {
        return Err(invalid_icc(
            "declared profile size is smaller than the header",
        ));
    }
    if declared_size > bytes.len() {
        return Err(invalid_icc("declared profile size exceeds available bytes"));
    }
    let padding = &bytes[declared_size..];
    if padding.len() > 1 || padding.first().is_some_and(|byte| *byte != 0) {
        return Err(invalid_icc(
            "bytes after the declared profile size are not DICOM value padding",
        ));
    }
    Ok(&bytes[..declared_size])
}

fn stabilize_synthetic_icc_profile(profile: &mut [u8]) {
    const FIXED_CREATION_DATETIME: [u8; 12] = [
        0x07, 0xE8, // 2024
        0x00, 0x01, // January
        0x00, 0x01, // Day 1
        0x00, 0x00, // Hour 0
        0x00, 0x00, // Minute 0
        0x00, 0x00, // Second 0
    ];
    if let Some(created_at) = profile.get_mut(ICC_CREATION_DATETIME) {
        created_at.copy_from_slice(&FIXED_CREATION_DATETIME);
    }
}

fn invalid_icc(reason: &str) -> Error {
    Error::Metadata {
        reason: format!("DICOM ICC Profile {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_profiles_have_dicom_input_device_headers() {
        for profile in [
            synthetic_srgb_icc_profile().unwrap(),
            synthetic_display_p3_icc_profile().unwrap(),
        ] {
            validate_dicom_icc_profile(&profile).unwrap();
            assert_eq!(&profile[ICC_PROFILE_CLASS], b"scnr");
            assert_eq!(&profile[ICC_INPUT_COLOR_SPACE], b"RGB ");
            assert!(matches!(
                &profile[ICC_PROFILE_CONNECTION_SPACE],
                b"XYZ " | b"Lab "
            ));
        }
    }

    #[test]
    fn color_space_terms_match_only_their_own_primaries() {
        let srgb = synthetic_srgb_icc_profile().unwrap();
        let display_p3 = synthetic_display_p3_icc_profile().unwrap();
        let adobe = ColorProfile::new_adobe_rgb().encode().unwrap();
        let romm = ColorProfile::new_pro_photo_rgb().encode().unwrap();
        for (profile, term) in [
            (&srgb, "SRGB"),
            (&display_p3, "DISPLAYP3"),
            (&adobe, "ADOBERGB"),
            (&romm, "ROMMRGB"),
        ] {
            for other in ["SRGB", "DISPLAYP3", "ADOBERGB", "ROMMRGB"] {
                assert_eq!(
                    defines_color_space(profile, other),
                    Some(other == term),
                    "{term} profile against {other}"
                );
            }
        }
        assert_eq!(defines_color_space(&srgb, "SCANNER"), None);
        assert_eq!(defines_color_space(&srgb[..64], "SRGB"), None);
    }

    #[test]
    fn profiles_larger_than_ten_megabytes_are_accepted() {
        let mut profile = synthetic_srgb_icc_profile().unwrap();
        profile.resize(13 * 1024 * 1024, 0);
        let declared = u32::try_from(profile.len()).unwrap().to_be_bytes();
        profile[..4].copy_from_slice(&declared);
        validate_dicom_icc_profile(&profile).unwrap();
    }

    #[test]
    fn dicom_icc_validation_rejects_each_normative_header_violation() {
        let valid = synthetic_srgb_icc_profile().unwrap();
        for (range, replacement, expected) in [
            (ICC_PROFILE_CLASS, b"mntr".as_slice(), "scnr"),
            (ICC_INPUT_COLOR_SPACE, b"GRAY".as_slice(), "RGB"),
            (ICC_PROFILE_CONNECTION_SPACE, b"CMYK".as_slice(), "PCS"),
            (ICC_SIGNATURE, b"nope".as_slice(), "acsp"),
        ] {
            let mut invalid = valid.clone();
            invalid[range].copy_from_slice(replacement);
            let error = validate_dicom_icc_profile(&invalid).unwrap_err();
            assert!(
                error.to_string().contains(expected),
                "unexpected error: {error}"
            );
        }

        let mut invalid_size = valid;
        invalid_size[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(validate_dicom_icc_profile(&invalid_size)
            .unwrap_err()
            .to_string()
            .contains("size"));
    }
}
