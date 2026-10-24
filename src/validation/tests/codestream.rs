//! The codestream rule on exported files, through the validation entry point.

use super::*;
use dicom_core::value::Value;

const CODESTREAM_RULE: &str = "intrinsic-wsi-dicom-codestream";

fn codestream_check(path: &Path) -> super::super::ValidationCheck {
    let report = validate_dicom_path_with_runner(
        path,
        &ValidationOptions {
            profile: ValidationProfile::Core,
            ..ValidationOptions::default()
        },
        &FakeRunner::default(),
    )
    .expect("validation report");
    report
        .checks
        .into_iter()
        .find(|check| check.name == CODESTREAM_RULE)
        .expect("a codestream check")
}

#[test]
fn exported_frames_of_every_transfer_syntax_pass_the_codestream_rule() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for (name, transfer_syntax, geometry) in [
        ("jpeg", TransferSyntax::JpegBaseline8Bit, (32, 32, 16)),
        ("sv1", TransferSyntax::JpegLosslessSv1, (32, 32, 16)),
        ("j2k", TransferSyntax::Jpeg2000Lossless, (32, 32, 16)),
        ("htj2k", TransferSyntax::Htj2kLossless, (32, 32, 16)),
        ("rpcl", TransferSyntax::Htj2kLosslessRpcl, (256, 256, 128)),
    ] {
        let path = export_color_wsi(tmp.path(), name, geometry, transfer_syntax);
        let check = codestream_check(&path);
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{name}: {}",
            check.message
        );
    }
}

#[test]
fn a_patched_exported_frame_fails_the_codestream_rule_with_its_frame_number() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = export_color_wsi(
        tmp.path(),
        "patched",
        (32, 32, 16),
        TransferSyntax::Jpeg2000Lossless,
    );
    let mut object = dicom_object::open_file(&path).unwrap();
    let pixel_data = object.take(tags::PIXEL_DATA).expect("Pixel Data");
    let Value::PixelSequence(sequence) = pixel_data.into_value() else {
        panic!("encapsulated Pixel Data");
    };
    let (offset_table, mut fragments) = sequence.into_parts();
    // Frame 3's SIZ Rsiz claims Part 2 capabilities; lengths are unchanged.
    fragments[2][6] |= 0x80;
    object.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OB,
        dicom_core::value::PixelFragmentSequence::new(offset_table, fragments),
    ));
    object.write_to_file(&path).unwrap();

    let check = codestream_check(&path);
    assert_eq!(check.status, ValidationStatus::Failed, "{}", check.message);
    assert!(
        check.message.contains(
            "1 violation: Frame 3: SIZ Rsiz 8000H declares JPEG 2000 Part 2 capabilities"
        ),
        "{}",
        check.message
    );
}
