#[test]
fn public_api_compiles_through_owned_types_and_prelude() {
    use wsi_dicom::prelude::{
        ColorManagement as PreludeColorManagement, Error as PreludeError, Export as PreludeExport,
        ExportOptions as PreludeExportOptions, FrameSamples as PreludeFrameSamples,
        MetadataSource as PreludeMetadataSource,
        SpecimenIdentifierIssuer as PreludeSpecimenIdentifierIssuer,
        TransferSyntax as PreludeTransferSyntax, UniversalEntityIdType as PreludeIssuerType,
    };

    let _export = PreludeExport::from_slide("slide.ndpi")
        .to_directory("out")
        .with_metadata(PreludeMetadataSource::ResearchPlaceholder)
        .color_management(PreludeColorManagement::SourceOrSrgb)
        .max_instance_metadata_bytes(256 * 1024 * 1024)
        .max_total_metadata_bytes(1024 * 1024 * 1024);
    assert_eq!(
        PreludeExportOptions::default().transfer_syntax,
        PreludeTransferSyntax::Htj2kLosslessRpcl
    );
    let samples = PreludeFrameSamples::new(&[0], 1, 1, 1, 8, false).expect("valid sample");
    assert_eq!(samples.data, &[0]);
    let mut issuer = PreludeSpecimenIdentifierIssuer::default();
    issuer.universal_entity_id = Some("https://hospital.example/specimens".into());
    issuer.universal_entity_id_type = Some(PreludeIssuerType::Uri);
    assert_eq!(
        issuer.universal_entity_id_type,
        Some(PreludeIssuerType::Uri)
    );
    let _ = std::any::type_name::<PreludeError>();
}

#[test]
fn shared_application_metadata_input_enforces_one_source() {
    use wsi_dicom::{ExportWorkflowStage, MetadataInput};

    let conflict = MetadataInput::from_parts(Some("metadata.json".into()), true)
        .expect_err("metadata path and placeholder must conflict");
    assert_eq!(conflict.stage(), ExportWorkflowStage::Metadata);

    let missing =
        MetadataInput::from_parts(None, false).expect_err("one metadata source is required");
    assert_eq!(missing.stage(), ExportWorkflowStage::Metadata);

    assert!(matches!(
        MetadataInput::from_parts(None, true).unwrap(),
        MetadataInput::ResearchPlaceholder
    ));
}
