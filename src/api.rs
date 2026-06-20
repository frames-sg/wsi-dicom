//! Builder-first Rust API for DICOM export.

use std::path::PathBuf;

use crate::{
    default_transfer_syntax_for_source, export_dicom, CodecValidation,
    DefaultTransferSyntaxRequest, EncodeBackendPreference, Error, ExportOptions, ExportReport,
    ExportRequest, IccProfilePolicy, JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferSyntaxSelection {
    SourceAware,
    Explicit,
}

/// Builder for exporting one wsi-rs-readable whole-slide image to DICOM VL WSI.
#[derive(Debug, Clone)]
#[must_use = "Export is a builder; call run() to execute the export"]
pub struct Export {
    source_path: PathBuf,
    output_dir: Option<PathBuf>,
    options: ExportOptions,
    metadata: Option<MetadataSource>,
    level_filter: Option<u32>,
    transfer_syntax: TransferSyntaxSelection,
}

impl Export {
    /// Start an export builder from a source slide path.
    ///
    /// The builder resolves a source-aware transfer syntax in `build_request`
    /// or `run` until `with_options` or `transfer_syntax` makes the transfer
    /// syntax explicit. Direct `export_dicom` calls use the request options as
    /// supplied and do not perform this source-aware resolution step.
    #[must_use = "call run() or build_request() on the returned Export builder"]
    pub fn from_slide(source_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            output_dir: None,
            options: ExportOptions::default(),
            metadata: None,
            level_filter: None,
            transfer_syntax: TransferSyntaxSelection::SourceAware,
        }
    }

    /// Set the output directory for generated DICOM instances.
    #[must_use = "builder methods return the updated Export"]
    pub fn to_directory(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(output_dir.into());
        self
    }

    /// Replace export options and treat their transfer syntax as explicit.
    #[must_use = "builder methods return the updated Export"]
    pub fn with_options(mut self, options: ExportOptions) -> Self {
        self.options = options;
        self.transfer_syntax = TransferSyntaxSelection::Explicit;
        self
    }

    /// Replace metadata used for DICOM conformance fields.
    #[must_use = "builder methods return the updated Export"]
    pub fn with_metadata(mut self, metadata: MetadataSource) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Use deterministic non-clinical placeholder metadata for research exports.
    #[must_use = "builder methods return the updated Export"]
    pub fn with_research_placeholder_metadata(mut self) -> Self {
        self.metadata = Some(MetadataSource::ResearchPlaceholder);
        self
    }

    /// Export only one source pyramid level.
    #[must_use = "builder methods return the updated Export"]
    pub fn level(mut self, level: u32) -> Self {
        self.level_filter = Some(level);
        self
    }

    /// Explicitly set the output transfer syntax.
    #[must_use = "builder methods return the updated Export"]
    pub fn transfer_syntax(mut self, transfer_syntax: TransferSyntax) -> Self {
        self.options.transfer_syntax = transfer_syntax;
        self.options.jpeg_direct_htj2k_profile =
            JpegDirectHtj2kProfile::default_for_transfer_syntax(transfer_syntax);
        self.transfer_syntax = TransferSyntaxSelection::Explicit;
        self
    }

    /// Select the direct JPEG-to-HTJ2K coefficient path for HTJ2K exports.
    #[must_use = "builder methods return the updated Export"]
    pub fn jpeg_direct_htj2k_profile(mut self, profile: JpegDirectHtj2kProfile) -> Self {
        self.options.jpeg_direct_htj2k_profile = profile;
        self
    }

    /// Set the DICOM frame tile size in pixels.
    #[must_use = "builder methods return the updated Export"]
    pub fn tile_size(mut self, tile_size: u32) -> Self {
        self.options.tile_size = tile_size;
        self
    }

    /// Set JPEG Baseline fallback quality.
    #[must_use = "builder methods return the updated Export"]
    pub fn jpeg_quality(mut self, jpeg_quality: u8) -> Self {
        self.options.jpeg_quality = jpeg_quality;
        self
    }

    /// Set the ICC profile policy for missing source color metadata.
    #[must_use = "builder methods return the updated Export"]
    pub fn icc_profile_policy(mut self, policy: IccProfilePolicy) -> Self {
        self.options.icc_profile_policy = policy;
        self
    }

    /// Set the runtime encode backend preference.
    #[must_use = "builder methods return the updated Export"]
    pub fn encode_backend(mut self, backend: EncodeBackendPreference) -> Self {
        self.options.encode_backend = backend;
        self
    }

    /// Set the runtime codec validation policy.
    #[must_use = "builder methods return the updated Export"]
    pub fn codec_validation(mut self, validation: CodecValidation) -> Self {
        self.options.codec_validation = validation;
        self
    }

    /// Allow or disable device-backed source tile decode.
    #[must_use = "builder methods return the updated Export"]
    pub fn source_device_decode(mut self, source_device_decode: bool) -> Self {
        self.options.source_device_decode = source_device_decode;
        self
    }

    /// Override JPEG 2000 decomposition levels.
    #[must_use = "builder methods return the updated Export"]
    pub fn j2k_decomposition_levels(mut self, levels: Option<u8>) -> Self {
        self.options.j2k_decomposition_levels = levels;
        self
    }

    /// Set the optional GPU encode in-flight tile cap.
    #[must_use = "builder methods return the updated Export"]
    pub fn gpu_encode_inflight_tiles(mut self, tiles: Option<usize>) -> Self {
        self.options.gpu_encode_inflight_tiles = tiles;
        self
    }

    /// Set the optional GPU encode memory budget in MiB.
    #[must_use = "builder methods return the updated Export"]
    pub fn gpu_encode_memory_mib(mut self, memory_mib: Option<u64>) -> Self {
        self.options.gpu_encode_memory_mib = memory_mib;
        self
    }

    /// Set the optional GPU pipeline depth.
    #[must_use = "builder methods return the updated Export"]
    pub fn gpu_pipeline_depth(mut self, depth: Option<usize>) -> Self {
        self.options.gpu_pipeline_depth = depth;
        self
    }

    /// Set the optional maximum rows per GPU row batch.
    #[must_use = "builder methods return the updated Export"]
    pub fn gpu_row_batch_rows(mut self, rows: Option<usize>) -> Self {
        self.options.gpu_row_batch_rows = rows;
        self
    }

    /// Set the optional target tile count per GPU row batch.
    #[must_use = "builder methods return the updated Export"]
    pub fn gpu_row_batch_target_tiles(mut self, tiles: Option<usize>) -> Self {
        self.options.gpu_row_batch_target_tiles = tiles;
        self
    }

    /// Resolve the transfer syntax from the source at run time.
    #[must_use = "builder methods return the updated Export"]
    pub fn source_aware_transfer_syntax(mut self) -> Self {
        self.transfer_syntax = TransferSyntaxSelection::SourceAware;
        self
    }

    /// Convert the builder into an export request, resolving source-aware defaults.
    pub fn build_request(mut self) -> Result<ExportRequest, Error> {
        let output_dir = self
            .output_dir
            .take()
            .ok_or_else(|| Error::InvalidOptions {
                reason: "output directory must be configured with to_directory".into(),
            })?;
        if self.transfer_syntax == TransferSyntaxSelection::SourceAware {
            self.options.transfer_syntax =
                default_transfer_syntax_for_source(DefaultTransferSyntaxRequest {
                    source_path: self.source_path.clone(),
                    tile_size: self.options.tile_size,
                    level_filter: self.level_filter,
                    max_levels: None,
                })?;
            self.options.jpeg_direct_htj2k_profile =
                JpegDirectHtj2kProfile::default_for_transfer_syntax(self.options.transfer_syntax);
        }
        let metadata = self.metadata.take().ok_or_else(|| Error::Metadata {
            reason: "export metadata must be provided with with_metadata or with_research_placeholder_metadata".into(),
        })?;
        self.options.validate()?;
        Ok(ExportRequest {
            source_path: self.source_path,
            output_dir,
            options: self.options,
            metadata,
            level_filter: self.level_filter,
        })
    }

    /// Run the export and return a report.
    pub fn run(self) -> Result<ExportReport, Error> {
        export_dicom(self.build_request()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        CodecValidation, EncodeBackendPreference, Export, IccProfilePolicy, JpegDirectHtj2kProfile,
        MetadataSource, TransferSyntax,
    };

    #[test]
    fn htj2k_transfer_syntax_defaults_to_97_profile() {
        let request = Export::from_slide("source.ndpi")
            .to_directory("dicom-out")
            .with_research_placeholder_metadata()
            .transfer_syntax(TransferSyntax::Htj2k)
            .build_request()
            .unwrap();

        assert_eq!(request.options.transfer_syntax, TransferSyntax::Htj2k);
        assert_eq!(
            request.options.jpeg_direct_htj2k_profile,
            JpegDirectHtj2kProfile::Lossy97
        );
    }

    #[test]
    fn builder_option_setters_flow_into_request() {
        let request = Export::from_slide("source.ndpi")
            .to_directory("dicom-out")
            .with_metadata(MetadataSource::ResearchPlaceholder)
            .transfer_syntax(TransferSyntax::Htj2kLossless)
            .tile_size(256)
            .jpeg_quality(80)
            .icc_profile_policy(IccProfilePolicy::OmitIfMissing)
            .encode_backend(EncodeBackendPreference::CpuOnly)
            .codec_validation(CodecValidation::RoundTrip)
            .source_device_decode(true)
            .j2k_decomposition_levels(Some(3))
            .gpu_encode_inflight_tiles(Some(8))
            .gpu_encode_memory_mib(Some(4096))
            .gpu_pipeline_depth(Some(3))
            .gpu_row_batch_rows(Some(6))
            .gpu_row_batch_target_tiles(Some(96))
            .build_request()
            .unwrap();

        assert_eq!(request.options.tile_size, 256);
        assert_eq!(request.options.jpeg_quality, 80);
        assert_eq!(
            request.options.icc_profile_policy,
            IccProfilePolicy::OmitIfMissing
        );
        assert_eq!(
            request.options.encode_backend,
            EncodeBackendPreference::CpuOnly
        );
        assert_eq!(request.options.codec_validation, CodecValidation::RoundTrip);
        assert!(request.options.source_device_decode);
        assert_eq!(request.options.j2k_decomposition_levels, Some(3));
        assert_eq!(request.options.gpu_encode_inflight_tiles, Some(8));
        assert_eq!(request.options.gpu_encode_memory_mib, Some(4096));
        assert_eq!(request.options.gpu_pipeline_depth, Some(3));
        assert_eq!(request.options.gpu_row_batch_rows, Some(6));
        assert_eq!(request.options.gpu_row_batch_target_tiles, Some(96));
    }

    #[test]
    fn builder_requires_explicit_metadata() {
        let err = Export::from_slide("source.ndpi")
            .to_directory("dicom-out")
            .transfer_syntax(TransferSyntax::Htj2kLossless)
            .build_request()
            .expect_err("metadata policy must be explicit");

        assert!(err.to_string().contains("metadata"));
    }
}
