use moxcms::{ColorProfile, DataColorSpace, ProfileClass};

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

    let parsed = ColorProfile::new_from_slice(profile).map_err(|err| Error::Metadata {
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

pub(crate) fn synthetic_srgb_icc_profile() -> Result<Vec<u8>, Error> {
    encode_synthetic_input_profile("sRGB", ColorProfile::new_srgb())
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
