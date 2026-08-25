use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use wsi_dicom::{
    AnnotationCoordinateSpace, CodecValidation, ExportOptions, JpegDirectHtj2kProfile,
    TransferSyntax,
};

use super::mapping::GuiColorManagement;
use super::worker::GuiRunResult;

pub(crate) struct WsiDicomGui {
    pub(super) source_path: Option<PathBuf>,
    pub(super) output_dir: Option<PathBuf>,
    pub(super) metadata_path: Option<PathBuf>,
    pub(super) research_placeholder: bool,
    pub(super) convert_annotations: bool,
    pub(super) annotation_geojson_path: Option<PathBuf>,
    pub(super) annotation_mapping_path: Option<PathBuf>,
    pub(super) annotation_target_ann: bool,
    pub(super) annotation_target_seg: bool,
    pub(super) annotation_target_sr: bool,
    pub(super) annotation_coordinate_space: AnnotationCoordinateSpace,
    pub(super) transfer_syntax: TransferSyntax,
    pub(super) jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    pub(super) color_management: GuiColorManagement,
    pub(super) codec_validation: CodecValidation,
    pub(super) tile_size: u32,
    pub(super) jpeg_quality: u8,
    pub(super) overwrite: bool,
    pub(super) validate_after_export: bool,
    pub(super) validation_strict: bool,
    pub(super) htj2k_decoder: String,
    pub(super) running: bool,
    pub(super) receiver: Option<Receiver<GuiRunResult>>,
    pub(super) status: String,
    pub(super) report_json: String,
}

impl Default for WsiDicomGui {
    fn default() -> Self {
        let options = ExportOptions::default();
        Self {
            source_path: None,
            output_dir: None,
            metadata_path: None,
            research_placeholder: false,
            convert_annotations: false,
            annotation_geojson_path: None,
            annotation_mapping_path: None,
            annotation_target_ann: true,
            annotation_target_seg: false,
            annotation_target_sr: false,
            annotation_coordinate_space: AnnotationCoordinateSpace::Level0Pixels,
            transfer_syntax: options.transfer_syntax,
            jpeg_direct_htj2k_profile: options.jpeg_direct_htj2k_profile,
            color_management: GuiColorManagement::SourceOrSrgb,
            codec_validation: options.codec_validation,
            tile_size: options.tile_size,
            jpeg_quality: options.jpeg_quality,
            overwrite: options.overwrite,
            validate_after_export: true,
            validation_strict: false,
            htj2k_decoder: String::new(),
            running: false,
            receiver: None,
            status: "Select a source slide and output directory.".to_string(),
            report_json: String::new(),
        }
    }
}
