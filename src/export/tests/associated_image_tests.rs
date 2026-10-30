use super::*;
use crate::report::ImageFlavor;
use crate::test_support::write_aperio_svs_with_associated_images;
use crate::validation::{validate_dicom_path, ValidationOptions, ValidationStatus};
use crate::{AssociatedImageExclusions, AssociatedImageKind, PixelReencodePolicy};

fn export_fixture(
    options: ExportOptions,
    unnamed_extra: bool,
    level_filter: Option<u32>,
) -> (tempfile::TempDir, PathBuf, Result<ExportReport, Error>) {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("associated.svs");
    write_aperio_svs_with_associated_images(&source, unnamed_extra);
    let result = export_dicom(ExportRequest {
        source_path: source.clone(),
        output_dir: tmp.path().join("out"),
        options,
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter,
    });
    (tmp, source, result)
}

fn flavors(report: &ExportReport) -> Vec<ImageFlavor> {
    report
        .instances
        .iter()
        .map(|instance| instance.flavor)
        .collect()
}

#[test]
fn associated_images_export_as_label_overview_and_thumbnail_instances() {
    let (_tmp, _source, report) = export_fixture(ExportOptions::default(), false, None);
    let report = report.unwrap();

    assert_eq!(
        flavors(&report),
        [
            ImageFlavor::Volume,
            ImageFlavor::Label,
            ImageFlavor::Overview,
            ImageFlavor::Thumbnail
        ]
    );
    assert!(report.skipped_associated_images.is_empty());
    let level_zero = dicom_object::open_file(&report.instances[0].path).unwrap();
    for (flavor, name, image_type, shows_label, has_frame_of_reference) in [
        (
            ImageFlavor::Label,
            "label",
            "ORIGINAL\\PRIMARY\\LABEL\\NONE",
            "YES",
            false,
        ),
        (
            ImageFlavor::Overview,
            "macro",
            "ORIGINAL\\PRIMARY\\OVERVIEW\\NONE",
            "YES",
            false,
        ),
        (
            ImageFlavor::Thumbnail,
            "thumbnail",
            "DERIVED\\PRIMARY\\THUMBNAIL\\RESAMPLED",
            "NO",
            true,
        ),
    ] {
        let instance = instance(&report, flavor);
        assert_eq!(instance.associated_image.as_deref(), Some(name));
        assert_eq!(instance.frame_count, 1);
        let object = dicom_object::open_file(&instance.path).unwrap();
        assert_eq!(text(&object, tags::IMAGE_TYPE), image_type);
        assert_eq!(text(&object, tags::NUMBER_OF_FRAMES), "1");
        assert_eq!(text(&object, tags::SPECIMEN_LABEL_IN_IMAGE), shows_label);
        assert_eq!(text(&object, tags::BURNED_IN_ANNOTATION), shows_label);
        assert_eq!(
            object.element(tags::FRAME_OF_REFERENCE_UID).is_ok(),
            has_frame_of_reference,
            "{name}"
        );
        assert!(object.element(tags::PYRAMID_UID).is_err(), "{name}");
        assert!(object.element(tags::IMAGED_VOLUME_WIDTH).is_err(), "{name}");
        assert!(object.element(tags::DIMENSION_INDEX_SEQUENCE).is_err());
        assert!(object
            .element(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .is_err());
        assert_eq!(
            text(&object, tags::SERIES_INSTANCE_UID),
            text(&level_zero, tags::SERIES_INSTANCE_UID)
        );
        assert_eq!(
            object.meta().transfer_syntax.trim_end_matches('\0'),
            TransferSyntax::Htj2kLosslessRpcl.uid()
        );
    }

    let label = dicom_object::open_file(&instance(&report, ImageFlavor::Label).path).unwrap();
    assert_eq!(text(&label, tags::BARCODE_VALUE), "");
    assert_eq!(text(&label, tags::LABEL_TEXT), "");
    assert_eq!(text(&label, tags::LOSSY_IMAGE_COMPRESSION), "00");

    let overview = dicom_object::open_file(&instance(&report, ImageFlavor::Overview).path).unwrap();
    assert!(overview.element(tags::BARCODE_VALUE).is_err());
    assert_eq!(text(&overview, tags::LOSSY_IMAGE_COMPRESSION), "01");
    assert_eq!(
        text(&overview, tags::LOSSY_IMAGE_COMPRESSION_METHOD),
        "ISO_10918_1"
    );

    let thumbnail =
        dicom_object::open_file(&instance(&report, ImageFlavor::Thumbnail).path).unwrap();
    assert_eq!(text(&thumbnail, tags::LOSSY_IMAGE_COMPRESSION), "00");
    let source_images = thumbnail
        .element(tags::SOURCE_IMAGE_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    assert_eq!(
        text(&source_images[0], tags::REFERENCED_SOP_INSTANCE_UID),
        text(&level_zero, tags::SOP_INSTANCE_UID)
    );
    let shared = thumbnail
        .element(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    let measures = shared[0]
        .element(tags::PIXEL_MEASURES_SEQUENCE)
        .unwrap()
        .items()
        .unwrap();
    // Level zero is 64x48 at 0.0005 mm; the 16x12 thumbnail spans it.
    assert_eq!(
        measures[0]
            .element(tags::PIXEL_SPACING)
            .unwrap()
            .to_multi_float64()
            .unwrap(),
        [0.002, 0.002]
    );
}

#[test]
fn associated_images_read_back_through_wsi_rs_with_identical_pixels() {
    let (_tmp, source, report) = export_fixture(ExportOptions::default(), false, None);
    let report = report.unwrap();
    let source = Slide::open(&source).unwrap();
    let output = Slide::open(&report.output_dir).unwrap();

    let mut names = output
        .dataset()
        .associated_images
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["label", "macro", "thumbnail"]);
    for name in names {
        let written = output.read_associated(&name).unwrap();
        let original = source.read_associated(&name).unwrap();
        assert_eq!(
            (written.width(), written.height(), written.channels()),
            (original.width(), original.height(), original.channels()),
            "{name}"
        );
        assert_eq!(written.data().as_u8(), original.data().as_u8(), "{name}");
    }
}

#[test]
fn associated_instances_pass_intrinsic_validation() {
    let (_tmp, _source, report) = export_fixture(ExportOptions::default(), false, None);
    let report = report.unwrap();
    let validation =
        validate_dicom_path(&report.output_dir, &ValidationOptions::default()).unwrap();

    let failures = validation
        .checks
        .iter()
        .filter(|check| check.name.starts_with("intrinsic-"))
        .filter(|check| check.status == ValidationStatus::Failed)
        .map(|check| format!("{}: {}", check.name, check.message))
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(validation.files.len(), 4);
}

#[test]
fn excluded_kinds_and_level_filters_write_no_associated_instances() {
    let options = ExportOptions {
        exclude_associated: AssociatedImageExclusions::NONE
            .with(AssociatedImageKind::Label)
            .with(AssociatedImageKind::Overview),
        ..ExportOptions::default()
    };
    let (_tmp, _source, report) = export_fixture(options, false, None);
    assert_eq!(
        flavors(&report.unwrap()),
        [ImageFlavor::Volume, ImageFlavor::Thumbnail]
    );

    let options = ExportOptions {
        exclude_associated: AssociatedImageExclusions::ALL,
        ..ExportOptions::default()
    };
    let (_tmp, _source, report) = export_fixture(options, false, None);
    assert_eq!(flavors(&report.unwrap()), [ImageFlavor::Volume]);

    let (_tmp, _source, report) = export_fixture(ExportOptions::default(), false, Some(0));
    let report = report.unwrap();
    assert_eq!(flavors(&report), [ImageFlavor::Volume]);
    assert!(report.skipped_associated_images.is_empty());
}

#[test]
fn associated_images_without_a_dicom_flavour_are_reported_as_skipped() {
    let (_tmp, _source, report) = export_fixture(ExportOptions::default(), true, None);
    let report = report.unwrap();

    assert_eq!(report.instances.len(), 4);
    assert_eq!(report.skipped_associated_images.len(), 1);
    let skipped = &report.skipped_associated_images[0];
    assert_eq!(skipped.name, "image_4");
    assert!(
        skipped.reason.contains("no LABEL, OVERVIEW or THUMBNAIL"),
        "{}",
        skipped.reason
    );
}

#[test]
fn lossy_requests_encode_associated_images_losslessly_unless_any() {
    let (_tmp, _source, report) = export_fixture(passthrough_jpeg_options(), false, None);
    let report = report.unwrap();
    let overview = instance(&report, ImageFlavor::Overview);
    assert_eq!(
        overview.transfer_syntax_uid,
        TransferSyntax::JpegLosslessSv1.uid()
    );
    let object = dicom_object::open_file(&overview.path).unwrap();
    assert_eq!(
        text(&object, tags::LOSSY_IMAGE_COMPRESSION_METHOD),
        "ISO_10918_1"
    );
    assert_eq!(
        overview.metrics.routes.compressed_source_reencoded_frames,
        1
    );
    assert_eq!(
        instance(&report, ImageFlavor::Label)
            .metrics
            .routes
            .compressed_source_reencoded_frames,
        0
    );

    let mut options = passthrough_jpeg_options();
    options.pixel_reencode = PixelReencodePolicy::Any;
    let (_tmp, _source, report) = export_fixture(options, false, None);
    let report = report.unwrap();
    let overview = instance(&report, ImageFlavor::Overview);
    assert_eq!(
        overview.transfer_syntax_uid,
        TransferSyntax::JpegBaseline8Bit.uid()
    );
    let object = dicom_object::open_file(&overview.path).unwrap();
    assert_eq!(
        text(&object, tags::PHOTOMETRIC_INTERPRETATION),
        "YBR_FULL_422"
    );
    assert_eq!(
        text(&object, tags::LOSSY_IMAGE_COMPRESSION_METHOD),
        "ISO_10918_1\\ISO_10918_1"
    );
}

#[test]
fn dicom_associated_image_preserves_its_own_declared_lossy_history() {
    let tmp = tempfile::tempdir().unwrap();
    let source_dir = tmp.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let volume = source_dir.join("volume.dcm");
    let thumbnail = source_dir.join("thumbnail.dcm");
    write_source_dicom_with_dimensions(&volume, "1.2.826.0.1.3680043.10.999.911", 4, 4);
    write_source_dicom_with_dimensions(&thumbnail, "1.2.826.0.1.3680043.10.999.912", 2, 2);
    let mut source_thumbnail = dicom_object::open_file(&thumbnail).unwrap();
    source_thumbnail.put(dicom_core::DataElement::new(
        tags::IMAGE_TYPE,
        VR::CS,
        "DERIVED\\PRIMARY\\THUMBNAIL\\RESAMPLED",
    ));
    declare_lossy_history_for_test(&mut source_thumbnail, "ISO_10918_1", "6.25");
    source_thumbnail.write_to_file(&thumbnail).unwrap();

    let report = export_dicom(ExportRequest {
        source_path: source_dir,
        output_dir: tmp.path().join("out"),
        options: ExportOptions {
            transfer_syntax: TransferSyntax::Jpeg2000Lossless,
            encode_backend: EncodeBackendPreference::CpuOnly,
            tile_size: 2,
            ..ExportOptions::default()
        },
        color_management: ColorManagement::SourceOrSrgb,
        metadata: MetadataSource::ResearchPlaceholder,
        level_filter: None,
    })
    .unwrap();

    let output = dicom_object::open_file(&instance(&report, ImageFlavor::Thumbnail).path).unwrap();
    assert_eq!(text(&output, tags::LOSSY_IMAGE_COMPRESSION), "01");
    assert_eq!(
        text(&output, tags::LOSSY_IMAGE_COMPRESSION_METHOD),
        "ISO_10918_1"
    );
    assert_eq!(
        output
            .element(tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .unwrap()
            .to_float64()
            .unwrap(),
        6.25
    );
}

#[test]
fn never_policy_refuses_a_compressed_associated_image_unless_excluded() {
    let mut options = passthrough_jpeg_options();
    options.pixel_reencode = PixelReencodePolicy::Never;
    let (_tmp, _source, report) = export_fixture(options.clone(), false, None);
    let error = report.unwrap_err().to_string();
    assert!(error.contains("associated image 'macro'"), "{error}");
    assert!(error.contains("exclude it"), "{error}");

    options.exclude_associated =
        AssociatedImageExclusions::NONE.with(AssociatedImageKind::Overview);
    let (_tmp, _source, report) = export_fixture(options, false, None);
    assert_eq!(
        flavors(&report.unwrap()),
        [
            ImageFlavor::Volume,
            ImageFlavor::Label,
            ImageFlavor::Thumbnail
        ]
    );
}
