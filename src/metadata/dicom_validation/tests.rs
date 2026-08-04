use super::DicomMetadata;

#[test]
fn writer_metadata_validation_rejects_invalid_vr_values() {
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_instance_uid = Some("1..2".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_instance_uid"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_date = Some("2026-06-14".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_date"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.patient_sex = Some("female".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("patient_sex"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_description = Some("A".repeat(65));
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_description"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.patient_name = Some("BAD\u{0007}NAME".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("patient_name"));

    for delimiter_or_control in ['\\', '\0', '\t', '\r', '\n', '\u{001b}'] {
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_description = Some(format!("before{delimiter_or_control}after"));
        assert!(metadata
            .validate_for_export()
            .unwrap_err()
            .to_string()
            .contains("study_description"));
    }
}

#[test]
fn export_metadata_validation_checks_depth_before_encoding() {
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.imaged_volume_depth_mm = Some(0.001);
    metadata.validate_for_export().unwrap();

    for invalid in [
        0.0,
        -0.001,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        f64::MAX,
    ] {
        metadata.imaged_volume_depth_mm = Some(invalid);
        let error = metadata.validate_for_export().unwrap_err();
        assert!(
            error.to_string().contains("imaged_volume_depth_mm"),
            "unexpected error for {invalid:?}: {error}"
        );
    }
}

#[test]
fn export_metadata_validation_accepts_unicode_and_person_name_groups() {
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.patient_name = Some("Yamada^Tarou=山田^太郎=やまだ^たろう".to_string());
    metadata.study_description = Some("Cafe\u{301} 病理".to_string());
    metadata.validate_for_export().unwrap();

    metadata.patient_name = Some("a^b^c^d^e^f".to_string());
    assert!(metadata.validate_for_export().is_err());

    metadata.patient_name = Some("a=b=c=d".to_string());
    assert!(metadata.validate_for_export().is_err());
}

#[test]
fn writer_metadata_validation_accepts_semantic_da_tm_dt_values() {
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.patient_birth_date = Some("20240229".to_string());
    metadata.study_date = Some("20260614".to_string());
    metadata.study_time = Some("235959.123456".to_string());
    metadata.content_time = Some("00".to_string());
    metadata.acquisition_date_time = Some("20240229235959.123456+0530".to_string());

    metadata.validated_for_writer().unwrap();
}

#[test]
fn writer_metadata_validation_rejects_invalid_semantic_da_tm_dt_values() {
    let mut metadata = DicomMetadata::research_placeholder();
    metadata.patient_birth_date = Some("20230229".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("patient_birth_date"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_date = Some("20261301".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_date"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_time = Some("240000".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_time"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.content_time = Some("235960".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("content_time"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.study_time = Some("1200.1".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("study_time"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.acquisition_date_time = Some("20260229235959".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("acquisition_date_time"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.acquisition_date_time = Some("20260614235959.1234567".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("acquisition_date_time"));

    let mut metadata = DicomMetadata::research_placeholder();
    metadata.acquisition_date_time = Some("20260614235959+1401".to_string());
    assert!(metadata
        .validated_for_writer()
        .unwrap_err()
        .to_string()
        .contains("acquisition_date_time"));
}
