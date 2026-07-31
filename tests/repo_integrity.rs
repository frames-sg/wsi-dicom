#[test]
fn public_api_compiles_through_owned_types_and_prelude() {
    use wsi_dicom::prelude::{
        Error as PreludeError, Export as PreludeExport, ExportOptions as PreludeExportOptions,
        FrameSamples as PreludeFrameSamples, MetadataSource as PreludeMetadataSource,
        TransferSyntax as PreludeTransferSyntax,
    };

    let _export = PreludeExport::from_slide("slide.ndpi")
        .to_directory("out")
        .with_metadata(PreludeMetadataSource::ResearchPlaceholder)
        .max_instance_metadata_bytes(256 * 1024 * 1024)
        .max_total_metadata_bytes(1024 * 1024 * 1024);
    assert_eq!(
        PreludeExportOptions::default().transfer_syntax,
        PreludeTransferSyntax::Htj2kLosslessRpcl
    );
    let samples = PreludeFrameSamples::new(&[0], 1, 1, 1, 8, false).expect("valid sample");
    assert_eq!(samples.data, &[0]);
    let _ = std::any::type_name::<PreludeError>();
}
