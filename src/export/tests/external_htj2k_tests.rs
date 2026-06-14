use super::*;

#[test]
fn external_htj2k_reference_decodes_htj2k_rpcl_exported_frame_when_available() {
    let Some(reference_decoder) = find_htj2k_reference_decoder_for_test() else {
        eprintln!("skipping external HTJ2K parity smoke: grk_decompress or kdu_expand not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.dcm");
    let out = tmp.path().join("out");
    let expected = vec![
        255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
    ];
    write_source_dicom_with_pixels(
        &source,
        "1.2.826.0.1.3680043.10.999.93",
        3,
        2,
        expected.clone(),
    );

    let report = export_dicom(ExportRequest {
        source_path: source,
        output_dir: out,
        options: ExportOptions {
            tile_size: 3,
            transfer_syntax: TransferSyntax::Htj2kLosslessRpcl,
            encode_backend: EncodeBackendPreference::CpuOnly,
            codec_validation: CodecValidation::Disabled,
            source_device_decode: false,
            ..ExportOptions::default()
        },
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();
    let object = dicom_object::open_file(&report.instances[0].path).unwrap();
    let fragments = object
        .element(tags::PIXEL_DATA)
        .unwrap()
        .value()
        .fragments()
        .unwrap();
    assert_eq!(fragments.len(), 1);

    let codestream_path = tmp.path().join("frame.j2k");
    let ppm_path = tmp.path().join("frame.ppm");
    std::fs::write(
        &codestream_path,
        dicom_fragment_payload_without_padding(&fragments[0]),
    )
    .unwrap();
    reference_decoder.decode(&codestream_path, &ppm_path);

    let decoded = read_binary_ppm_for_test(&ppm_path);

    assert_eq!(decoded.0, 3);
    assert_eq!(decoded.1, 3);
    assert_eq!(&decoded.2[..expected.len()], expected.as_slice());
    assert_eq!(&decoded.2[expected.len()..], &[0; 9]);
}

enum Htj2kReferenceDecoder {
    Grok(String),
    Kakadu(String),
}

impl Htj2kReferenceDecoder {
    fn decode(&self, codestream_path: &std::path::Path, ppm_path: &std::path::Path) {
        let (command, args): (&str, &[&str]) = match self {
            Self::Grok(command) => (command.as_str(), &["-i", "-o"]),
            Self::Kakadu(command) => (command.as_str(), &["-i", "-o"]),
        };
        let status = std::process::Command::new(command)
            .arg(args[0])
            .arg(codestream_path)
            .arg(args[1])
            .arg(ppm_path)
            .status()
            .unwrap();
        assert!(status.success(), "{command} failed with {status}");
    }
}

fn find_htj2k_reference_decoder_for_test() -> Option<Htj2kReferenceDecoder> {
    find_command_for_test("grk_decompress")
        .map(Htj2kReferenceDecoder::Grok)
        .or_else(|| find_command_for_test("kdu_expand").map(Htj2kReferenceDecoder::Kakadu))
}
