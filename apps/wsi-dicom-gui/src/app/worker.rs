use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};

use wsi_dicom::{
    run_export_workflow, AnnotationTarget, CodecValidation, EncodeBackendPreference, ExportOptions,
    ExportWorkflowRequest, JpegDirectHtj2kProfile, MetadataInput, QuPathAnnotationOptions,
    TransferSyntax, ValidationOptions,
};

use super::mapping::GuiColorManagement;
use super::WsiDicomGui;

impl WsiDicomGui {
    pub(super) fn start_export(&mut self) {
        let Some(source_path) = self.source_path.clone() else {
            self.status = "Choose a source slide first.".to_string();
            return;
        };
        let Some(output_dir) = self.output_dir.clone() else {
            self.status = "Choose an output directory first.".to_string();
            return;
        };
        let annotations = match self.selected_annotation_options() {
            Ok(annotations) => annotations,
            Err(message) => {
                self.status = message;
                return;
            }
        };
        let request = GuiRunRequest {
            source_path,
            output_dir,
            metadata_path: self.metadata_path.clone(),
            research_placeholder: self.research_placeholder,
            annotations,
            transfer_syntax: self.transfer_syntax,
            jpeg_direct_htj2k_profile: self.jpeg_direct_htj2k_profile,
            color_management: self.color_management,
            codec_validation: self.codec_validation,
            tile_size: self.tile_size,
            jpeg_quality: self.jpeg_quality,
            overwrite: self.overwrite,
            validate_after_export: self.validate_after_export,
            validation_strict: self.validation_strict,
            htj2k_decoder: (!self.htj2k_decoder.trim().is_empty())
                .then(|| self.htj2k_decoder.trim().to_string()),
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(run_export(request));
        });
        self.receiver = Some(receiver);
        self.running = true;
        self.status = "Export running...".to_string();
        self.report_json.clear();
    }

    pub(super) fn selected_annotation_options(
        &self,
    ) -> Result<Option<QuPathAnnotationOptions>, String> {
        if !self.convert_annotations {
            return Ok(None);
        }
        let geojson = self
            .annotation_geojson_path
            .clone()
            .ok_or_else(|| "Choose a QuPath GeoJSON annotation file.".to_string())?;
        let mapping = self
            .annotation_mapping_path
            .clone()
            .ok_or_else(|| "Choose a DICOM annotation mapping file.".to_string())?;
        let mut targets = Vec::new();
        if self.annotation_target_ann {
            targets.push(AnnotationTarget::Ann);
        }
        if self.annotation_target_seg {
            targets.push(AnnotationTarget::Seg);
        }
        if self.annotation_target_sr {
            targets.push(AnnotationTarget::Sr);
        }
        if targets.is_empty() {
            return Err("Select at least one annotation target: ANN, SEG, or SR.".to_string());
        }
        let mut options = QuPathAnnotationOptions::new(geojson, mapping, targets);
        options.coordinate_space = self.annotation_coordinate_space;
        Ok(Some(options))
    }

    pub(super) fn poll_worker(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.running = false;
                self.receiver = None;
                self.status = "Export failed.".to_string();
                self.report_json =
                    "Export worker stopped without returning a result. Review application logs and retry."
                        .to_string();
                return;
            }
        };
        self.running = false;
        self.receiver = None;
        match result {
            Ok(report) => {
                self.status = report.summary;
                self.report_json = report.json;
            }
            Err(message) => {
                self.status = "Export failed.".to_string();
                self.report_json = message;
            }
        }
    }
}
struct GuiRunRequest {
    source_path: PathBuf,
    output_dir: PathBuf,
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
    annotations: Option<QuPathAnnotationOptions>,
    transfer_syntax: TransferSyntax,
    jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    color_management: GuiColorManagement,
    codec_validation: CodecValidation,
    tile_size: u32,
    jpeg_quality: u8,
    overwrite: bool,
    validate_after_export: bool,
    validation_strict: bool,
    htj2k_decoder: Option<String>,
}

pub(super) struct GuiRunReport {
    summary: String,
    json: String,
}

pub(super) type GuiRunResult = Result<GuiRunReport, String>;

fn run_export(request: GuiRunRequest) -> GuiRunResult {
    let metadata = MetadataInput::from_parts(request.metadata_path, request.research_placeholder)
        .map_err(|error| error.to_string())?;
    let mut options = ExportOptions::default();
    options.tile_size = request.tile_size;
    options.transfer_syntax = request.transfer_syntax;
    options.jpeg_direct_htj2k_profile = request.jpeg_direct_htj2k_profile;
    options.jpeg_quality = request.jpeg_quality;
    options.overwrite = request.overwrite;
    options.codec_validation = request.codec_validation;
    options.encode_backend = EncodeBackendPreference::Auto;
    if options.transfer_syntax != TransferSyntax::Htj2k {
        options.jpeg_direct_htj2k_profile =
            JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
    }
    let mut workflow = ExportWorkflowRequest::new(
        request.source_path,
        request.output_dir,
        options,
        request.color_management.into(),
        metadata,
    );
    workflow.annotations = request.annotations;
    workflow.validation = if request.validate_after_export {
        let mut validation = ValidationOptions::default();
        validation.strict = request.validation_strict;
        validation.htj2k_decoder = request.htj2k_decoder;
        Some(validation)
    } else {
        None
    };
    let report = run_export_workflow(workflow).map_err(|error| error.to_string())?;
    let json = report.to_pretty_json().map_err(|error| error.to_string())?;
    let summary = report.summary();
    Ok(GuiRunReport { summary, json })
}
