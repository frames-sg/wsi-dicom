use super::*;

#[test]
fn external_htj2k_reference_decodes_htj2k_rpcl_exported_frame_when_available() {
    let Some(reference_decoder) = find_htj2k_reference_decoder_for_test() else {
        eprintln!("skipping external HTJ2K parity smoke: grk_decompress or kdu_expand not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let frame = write_external_j2k_decoder_frame_for_test(
        tmp.path(),
        "1.2.826.0.1.3680043.10.999.93",
        TransferSyntax::Htj2kLosslessRpcl,
    );

    reference_decoder.decode(&frame.codestream_path, &frame.ppm_path);

    assert_external_decoder_ppm_matches_source_for_test(&frame.ppm_path, &frame.expected_pixels);
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
