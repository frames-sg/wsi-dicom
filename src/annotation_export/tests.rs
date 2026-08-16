use super::*;
use crate::test_support::{encode_test_jpeg, write_tiled_jpeg_tiff};
use crate::{ColorManagement, Export, TransferSyntax};
use wsi_dicom_annotations::{AnnotationDocument, DicomAnnotationContext, SegmentationDocument};

const MAPPING: &str = include_str!("../../examples/qupath-neoplasm-mapping-v1.json");

const GEOJSON: &str = r#"
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "id": "2.25.701",
    "geometry": {
      "type": "Polygon",
      "coordinates": [[[1,1],[6,1],[6,6],[1,6],[1,1]]]
    },
    "properties": {
      "name": "Tumor region",
      "classification": {"name": "Tumor"}
    }
  }]
}
"#;

#[test]
fn combined_export_creates_verified_ann_and_seg_for_the_new_wsi() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("case.svs");
    let jpeg = encode_test_jpeg(8, 8, [120, 50, 80]);
    write_tiled_jpeg_tiff(&source, 8, 8, 8, 8, &[jpeg]);
    let geojson = temporary.path().join("case.geojson");
    let mapping = temporary.path().join("mapping.json");
    std::fs::write(&geojson, GEOJSON).unwrap();
    std::fs::write(&mapping, MAPPING).unwrap();
    let output = temporary.path().join("dicom-output");

    let report = Export::from_slide(&source)
        .to_directory(&output)
        .with_research_placeholder_metadata()
        .color_management(ColorManagement::SourceOrSrgb)
        .transfer_syntax(TransferSyntax::JpegBaseline8Bit)
        .tile_size(8)
        .run_with_qupath_annotations(QuPathAnnotationOptions::new(
            &geojson,
            &mapping,
            vec![AnnotationTarget::Ann, AnnotationTarget::Seg],
        ))
        .unwrap();

    let annotations = report.annotations.as_ref().unwrap();
    assert_eq!(annotations.feature_count, 1);
    assert_eq!(annotations.instances.len(), 2);
    assert!(annotations.output_dir.join("manifest.json").is_file());
    assert!(annotations.instances.iter().all(|item| item.path.is_file()));
    let context = DicomAnnotationContext::from_source(&annotations.source_wsi).unwrap();
    let ann = AnnotationDocument::read_ann(
        &annotations
            .instances
            .iter()
            .find(|item| item.target == AnnotationTarget::Ann)
            .unwrap()
            .path,
        &context,
    )
    .unwrap();
    let seg = SegmentationDocument::read_seg(
        &annotations
            .instances
            .iter()
            .find(|item| item.target == AnnotationTarget::Seg)
            .unwrap()
            .path,
        &context,
    )
    .unwrap();
    assert_eq!(ann.groups().len(), 1);
    assert_eq!(seg.segments().len(), 1);
}

#[test]
fn annotation_source_selection_rejects_ambiguous_level_zero_planes() {
    let report = ExportReport {
        instances: vec![InstanceReport::default(), InstanceReport::default()],
        ..ExportReport::default()
    };

    let error =
        select_annotation_source(&report, AnnotationCoordinateSpace::Level0Pixels).unwrap_err();

    assert!(error.to_string().contains("unambiguous level-zero"));
}

#[test]
fn duplicate_annotation_targets_fail_before_input_files_are_read() {
    let options = QuPathAnnotationOptions::new(
        "missing.geojson",
        "missing-mapping.json",
        vec![AnnotationTarget::Ann, AnnotationTarget::Ann],
    );

    let error = match options.prepare() {
        Ok(_) => panic!("duplicate targets unexpectedly passed"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("targets must be unique"));
}
