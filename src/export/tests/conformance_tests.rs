use super::*;
use crate::report::ImageFlavor;
use crate::test_support::{
    encode_test_jpeg, write_aperio_svs_with_associated_images, write_tiled_jpeg_tiff,
};
use crate::validation::{validate_dicom_path, ValidationOptions, ValidationStatus};
use crate::ConformanceLevel;

fn export(
    source: &std::path::Path,
    out: std::path::PathBuf,
    conformance: ConformanceLevel,
) -> ExportReport {
    export_dicom(ExportRequest {
        source_path: source.to_path_buf(),
        output_dir: out,
        options: ExportOptions {
            conformance,
            ..passthrough_jpeg_options()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap()
}

fn open(report: &ExportReport, flavor: ImageFlavor) -> dicom_object::DefaultDicomObject {
    dicom_object::open_file(&instance(report, flavor).path).unwrap()
}

fn optical_path(object: &dicom_object::InMemDicomObject) -> &dicom_object::InMemDicomObject {
    &object
        .element(tags::OPTICAL_PATH_SEQUENCE)
        .unwrap()
        .items()
        .unwrap()[0]
}

#[test]
fn optional_conformance_writes_truthful_optional_attributes() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("associated.svs");
    write_aperio_svs_with_associated_images(&source, false);
    let report = export(&source, tmp.path().join("out"), ConformanceLevel::Optional);

    let volume = open(&report, ImageFlavor::Volume);
    for (tag, value) in [
        (tags::TILES_OVERLAP, "NONE"),
        (tags::QUALITY_CONTROL_IMAGE, "NO"),
        (tags::RECOGNIZABLE_VISUAL_FEATURES, "NO"),
        (tags::SERIES_DATE, "19700101"),
    ] {
        assert_eq!(text(&volume, tag), value, "{tag}");
    }
    assert_eq!(text(optical_path(&volume), tags::COLOR_SPACE), "SRGB");
    // The fixture declares AppMag = 20.
    assert_eq!(
        text(optical_path(&volume), tags::OBJECTIVE_LENS_POWER),
        "20"
    );
    assert!(volume.element(tags::PYRAMID_LABEL).is_ok());
    let label = open(&report, ImageFlavor::Label);
    assert_eq!(text(&label, tags::RECOGNIZABLE_VISUAL_FEATURES), "YES");
    assert!(optical_path(&label)
        .element(tags::OBJECTIVE_LENS_POWER)
        .is_err());
    let thumbnail = open(&report, ImageFlavor::Thumbnail);
    assert!(thumbnail
        .element(tags::RECOGNIZABLE_VISUAL_FEATURES)
        .is_err());
    assert_eq!(
        text(optical_path(&thumbnail), tags::OBJECTIVE_LENS_POWER),
        "20"
    );
}

#[test]
fn required_conformance_writes_only_the_pyramid_with_required_attributes() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("associated.svs");
    write_aperio_svs_with_associated_images(&source, false);
    let report = export(&source, tmp.path().join("out"), ConformanceLevel::Required);

    assert!(report
        .instances
        .iter()
        .all(|instance| instance.flavor == ImageFlavor::Volume));
    let volume = open(&report, ImageFlavor::Volume);
    for tag in [
        tags::PYRAMID_UID,
        tags::PYRAMID_LABEL,
        tags::REPRESENTATIVE_FRAME_NUMBER,
        tags::ACQUISITION_DATE,
        tags::STUDY_DESCRIPTION,
        tags::TILES_OVERLAP,
        tags::RECOGNIZABLE_VISUAL_FEATURES,
    ] {
        assert!(volume.element(tag).is_err(), "{tag} is optional");
    }
    for tag in [tags::OPTICAL_PATH_DESCRIPTION, tags::OBJECTIVE_LENS_POWER] {
        assert!(optical_path(&volume).element(tag).is_err(), "{tag}");
    }

    // What the IOD requires is all present; the optional items are reported
    // as absent rather than failed.
    let validation =
        validate_dicom_path(&report.output_dir, &ValidationOptions::default()).unwrap();
    for check in validation
        .checks
        .iter()
        .filter(|check| check.name.contains("2026c-iod-"))
    {
        assert_eq!(
            check.status,
            ValidationStatus::Passed,
            "{}: {}",
            check.name,
            check.message
        );
    }
    let absent = &validation.optional[0].absent;
    assert!(
        absent
            .iter()
            .any(|item| item.starts_with("Tiles Overlap (0048,0304)")),
        "{absent:?}"
    );
    assert!(absent
        .iter()
        .any(|item| item.starts_with("Multi-Resolution Pyramid Module")));
    // Nothing optional is written, judged by the generated requirements,
    // except the Type 3 choices of encoding: TILED_FULL, which makes frame
    // positions implicit, and the frame index of encapsulated Pixel Data.
    for object in &validation.optional {
        let written: Vec<&String> = object
            .present
            .iter()
            .filter(|item| {
                !item.starts_with("Dimension Organization Type (0020,9311)")
                    && !item.starts_with("Extended Offset Table (7FE0,0001)")
            })
            .collect();
        assert!(written.is_empty(), "{}: {written:?}", object.path.display());
    }

    let required = ValidationOptions {
        conformance: ConformanceLevel::Required,
        ..ValidationOptions::default()
    };
    let validation = validate_dicom_path(&report.output_dir, &required).unwrap();
    assert!(validation.optional.is_empty());

    // Readers group the levels without the optional Pyramid UID.
    let reread = wsi_rs::Slide::open(&report.output_dir).unwrap();
    let original = wsi_rs::Slide::open(&source).unwrap();
    let request = wsi_rs::TileRequest::new(0, 0, 0, 0, 0);
    assert_eq!(
        reread.dataset().scenes[0].series[0].levels.len(),
        original.dataset().scenes[0].series[0].levels.len()
    );
    assert!(reread.read_tile(&request).is_ok());
}

#[test]
fn optional_conformance_separates_what_the_object_encodes() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("associated.svs");
    write_aperio_svs_with_associated_images(&source, false);
    let report = export(&source, tmp.path().join("out"), ConformanceLevel::Optional);
    let volume = instance(&report, ImageFlavor::Volume);

    let validation = validate_dicom_path(&volume.path, &ValidationOptions::default()).unwrap();

    let optional = &validation.optional[0];
    assert!(!optional.absent.is_empty());
    for encoded in ["Tiles Overlap", "Pyramid Label"] {
        assert!(!optional.absent.iter().any(|item| item.starts_with(encoded)));
        assert!(
            optional
                .present
                .iter()
                .any(|item| item.starts_with(encoded)),
            "{encoded}: {:?}",
            optional.present
        );
    }
}

#[test]
fn required_conformance_pads_absent_tiles_instead_of_tiled_sparse() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("sparse.tiff");
    let tiles = [
        Vec::new(),
        encode_test_jpeg(8, 8, [160, 20, 40]),
        Vec::new(),
        encode_test_jpeg(8, 8, [20, 40, 160]),
    ];
    write_tiled_jpeg_tiff(&source, 16, 16, 8, 8, &tiles);
    let mut organization = Vec::new();
    for conformance in [ConformanceLevel::Optional, ConformanceLevel::Required] {
        let report = export_dicom(ExportRequest {
            source_path: source.clone(),
            output_dir: tmp.path().join(format!("out-{conformance:?}")),
            options: ExportOptions {
                tile_size: 8,
                transfer_syntax: TransferSyntax::JpegLosslessSv1,
                encode_backend: EncodeBackendPreference::CpuOnly,
                conformance,
                ..ExportOptions::default()
            },
            color_management: ColorManagement::SourceOrSrgb,
            metadata: MetadataSource::ResearchPlaceholder,
            level_filter: Some(0),
        })
        .unwrap();
        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        organization.push((
            text(&object, tags::DIMENSION_ORGANIZATION_TYPE),
            report.instances[0].frame_count,
            object
                .element(tags::RECOMMENDED_ABSENT_PIXEL_CIE_LAB_VALUE)
                .ok()
                .map(|element| element.to_multi_int::<u16>().unwrap()),
        ));
    }
    assert_eq!(
        organization,
        [
            ("TILED_SPARSE".to_string(), 2, Some(vec![0, 0x8080, 0x8080])),
            ("TILED_FULL".to_string(), 4, None),
        ]
    );
}
