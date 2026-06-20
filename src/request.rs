//! Request types accepted by the public export, profile, coverage, and encode APIs.

use std::path::PathBuf;
use std::time::Duration;

use crate::{
    CodecValidation, EncodeBackendPreference, Error, ExportOptions, MetadataSource, TransferSyntax,
};

/// A validated request to export one vendor WSI into one DICOM output directory.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportRequest {
    /// Path to the source slide readable by `wsi-rs`.
    pub source_path: PathBuf,
    /// Directory where generated DICOM instances are written.
    pub output_dir: PathBuf,
    /// Export routing, encoding, ICC, and backend options.
    pub options: ExportOptions,
    /// Metadata policy used to populate required DICOM identifying fields.
    pub metadata: MetadataSource,
    /// Optional single source pyramid level to export.
    pub level_filter: Option<u32>,
}

impl ExportRequest {
    /// Build an export request with explicit metadata.
    pub fn new(
        source_path: PathBuf,
        output_dir: PathBuf,
        options: ExportOptions,
        metadata: MetadataSource,
    ) -> Result<Self, Error> {
        options.validate()?;
        Ok(Self {
            source_path,
            output_dir,
            options,
            metadata,
            level_filter: None,
        })
    }

    /// Validate request options before export.
    pub fn validate(&self) -> Result<(), Error> {
        self.options.validate()
    }
}

/// Borrowed interleaved samples and geometry for DICOM J2K/HTJ2K frame encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameSamples<'a> {
    /// Interleaved pixel bytes for the frame.
    pub data: &'a [u8],
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Samples per pixel; currently 1 for grayscale or 3 for RGB-like data.
    pub components: u8,
    /// Stored bits per sample.
    pub bit_depth: u8,
    /// Whether sample values are signed.
    pub signed: bool,
}

impl<'a> FrameSamples<'a> {
    /// Validate and describe an interleaved frame buffer.
    pub fn new(
        data: &'a [u8],
        width: u32,
        height: u32,
        components: u8,
        bit_depth: u8,
        signed: bool,
    ) -> Result<Self, Error> {
        let samples = Self {
            data,
            width,
            height,
            components,
            bit_depth,
            signed,
        };
        samples.to_j2k()?;
        Ok(samples)
    }

    pub(crate) fn to_j2k(self) -> Result<j2k::J2kLosslessSamples<'a>, Error> {
        j2k::J2kLosslessSamples::new(
            self.data,
            self.width,
            self.height,
            self.components,
            self.bit_depth,
            self.signed,
        )
        .map_err(|err| Error::UnsupportedPixelData {
            reason: err.to_string(),
        })
    }
}

/// Request to encode one already-composed tile into DICOM-ready J2K/HTJ2K frame bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct J2kFrameEncodeRequest<'a> {
    /// Interleaved samples and geometry for the frame.
    pub samples: FrameSamples<'a>,
    /// DICOM transfer syntax to encode.
    pub transfer_syntax: TransferSyntax,
    /// Runtime backend preference for the encoder.
    pub encode_backend: EncodeBackendPreference,
    /// Optional per-frame codec validation policy.
    pub codec_validation: CodecValidation,
}

impl<'a> J2kFrameEncodeRequest<'a> {
    /// Build a single-frame encode request.
    pub fn new(
        samples: FrameSamples<'a>,
        transfer_syntax: TransferSyntax,
        encode_backend: EncodeBackendPreference,
        codec_validation: CodecValidation,
    ) -> Self {
        Self {
            samples,
            transfer_syntax,
            encode_backend,
            codec_validation,
        }
    }
}

/// Request to profile frame routing for one level without writing DICOM.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RouteProfileRequest {
    /// Path to the source slide readable by `wsi-rs`.
    pub source_path: PathBuf,
    /// Export options used for route planning.
    pub options: ExportOptions,
    /// Whether to resolve the transfer syntax from source compression before profiling.
    pub source_aware_transfer_syntax: bool,
    /// Source pyramid level to profile.
    pub level: u32,
    /// Maximum frames to sample.
    pub max_frames: u64,
}

impl RouteProfileRequest {
    /// Build a route profile request for one source level.
    pub fn new(source_path: PathBuf, options: ExportOptions, level: u32, max_frames: u64) -> Self {
        Self {
            source_path,
            options,
            source_aware_transfer_syntax: true,
            level,
            max_frames,
        }
    }

    /// Set whether profiling should resolve transfer syntax from source compression.
    pub fn with_source_aware_transfer_syntax(mut self, source_aware: bool) -> Self {
        self.source_aware_transfer_syntax = source_aware;
        self
    }
}

/// Request for source-aware default transfer syntax selection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DefaultTransferSyntaxRequest {
    /// Path to the source slide readable by `wsi-rs`.
    pub source_path: PathBuf,
    /// Requested fallback DICOM tile size.
    pub tile_size: u32,
    /// Optional single source pyramid level to inspect.
    pub level_filter: Option<u32>,
    /// Optional cap on source levels inspected.
    pub max_levels: Option<u32>,
}

impl DefaultTransferSyntaxRequest {
    /// Build a default transfer syntax request for all source levels.
    pub fn new(source_path: PathBuf, tile_size: u32) -> Self {
        Self {
            source_path,
            tile_size,
            level_filter: None,
            max_levels: None,
        }
    }
}

/// Target for route coverage profiling.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteCoverageTarget {
    /// Profile one source slide.
    Source(PathBuf),
    /// Profile all supported source files under a root directory.
    Corpus(PathBuf),
}

/// Progress reporting destination for route coverage work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteProgressSink {
    /// Emit progress lines to standard error.
    Stderr,
}

/// Request to sample route coverage without writing DICOM.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RouteCoverageRequest {
    /// Source or corpus target to profile.
    pub target: RouteCoverageTarget,
    /// Export options used for route planning.
    pub options: ExportOptions,
    /// Whether to resolve the transfer syntax from source compression before profiling.
    pub source_aware_transfer_syntax: bool,
    /// Maximum frames sampled per level; `u64::MAX` requests full coverage.
    pub max_frames_per_level: u64,
    /// Optional cap on source levels inspected.
    pub max_levels: Option<u32>,
    /// Optional per-level elapsed time budget.
    pub max_level_elapsed: Option<Duration>,
    /// Optional progress sink.
    pub progress: Option<RouteProgressSink>,
    /// Maximum source files considered for corpus coverage.
    pub max_sources: usize,
    /// Maximum directory depth walked for corpus coverage.
    pub max_depth: usize,
}

impl RouteCoverageRequest {
    /// Build a route coverage request that samples one frame per level.
    pub fn new(source_path: PathBuf, options: ExportOptions) -> Self {
        Self {
            target: RouteCoverageTarget::Source(source_path),
            options,
            source_aware_transfer_syntax: true,
            max_frames_per_level: 1,
            max_levels: None,
            max_level_elapsed: None,
            progress: None,
            max_sources: 100_000,
            max_depth: 64,
        }
    }

    /// Build a corpus coverage request that samples one frame per level.
    pub fn new_corpus(source_root: PathBuf, options: ExportOptions) -> Self {
        Self {
            target: RouteCoverageTarget::Corpus(source_root),
            options,
            source_aware_transfer_syntax: true,
            max_frames_per_level: 1,
            max_levels: None,
            max_level_elapsed: None,
            progress: None,
            max_sources: 100_000,
            max_depth: 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_constructors_preserve_defaults_and_validate_options() {
        let source = PathBuf::from("slide.svs");
        let output = PathBuf::from("dicom-out");
        let options = ExportOptions::default();

        let export = ExportRequest::new(
            source.clone(),
            output.clone(),
            options.clone(),
            MetadataSource::ResearchPlaceholder,
        )
        .unwrap();
        assert_eq!(export.source_path, source);
        assert_eq!(export.output_dir, output);
        assert_eq!(export.metadata, MetadataSource::ResearchPlaceholder);
        assert_eq!(export.level_filter, None);
        export.validate().unwrap();

        let err = ExportRequest::new(
            PathBuf::from("slide.svs"),
            PathBuf::from("dicom-out"),
            ExportOptions {
                tile_size: 0,
                ..ExportOptions::default()
            },
            MetadataSource::ResearchPlaceholder,
        )
        .expect_err("zero tile size should be rejected");
        assert!(err.to_string().contains("tile_size"));

        let profile = RouteProfileRequest::new(PathBuf::from("slide.svs"), options.clone(), 2, 16);
        assert!(profile.source_aware_transfer_syntax);
        assert_eq!(profile.level, 2);
        assert_eq!(profile.max_frames, 16);

        let default_transfer = DefaultTransferSyntaxRequest::new(PathBuf::from("slide.svs"), 512);
        assert_eq!(default_transfer.tile_size, 512);
        assert_eq!(default_transfer.level_filter, None);
        assert_eq!(default_transfer.max_levels, None);

        let coverage = RouteCoverageRequest::new(PathBuf::from("slide.svs"), options.clone());
        assert_eq!(
            coverage.target,
            RouteCoverageTarget::Source(PathBuf::from("slide.svs"))
        );
        assert!(coverage.source_aware_transfer_syntax);
        assert_eq!(coverage.max_frames_per_level, 1);
        assert_eq!(coverage.max_levels, None);
        assert_eq!(coverage.max_level_elapsed, None);
        assert_eq!(coverage.progress, None);
        assert_eq!(coverage.max_sources, 100_000);
        assert_eq!(coverage.max_depth, 64);

        let corpus = RouteCoverageRequest::new_corpus(PathBuf::from("slides"), options);
        assert_eq!(
            corpus.target,
            RouteCoverageTarget::Corpus(PathBuf::from("slides"))
        );
        assert!(corpus.source_aware_transfer_syntax);
        assert_eq!(corpus.max_frames_per_level, 1);
        assert_eq!(corpus.max_levels, None);
        assert_eq!(corpus.max_level_elapsed, None);
        assert_eq!(corpus.progress, None);
        assert_eq!(corpus.max_sources, 100_000);
        assert_eq!(corpus.max_depth, 64);
    }

    #[test]
    fn frame_encode_request_constructors_validate_sample_geometry() {
        let pixels = [0_u8; 2 * 2 * 3];
        let samples = FrameSamples::new(&pixels, 2, 2, 3, 8, false).unwrap();
        assert_eq!(samples.width, 2);
        assert_eq!(samples.height, 2);
        assert_eq!(samples.components, 3);

        let request = J2kFrameEncodeRequest::new(
            samples,
            TransferSyntax::Htj2kLosslessRpcl,
            EncodeBackendPreference::CpuOnly,
            CodecValidation::RoundTrip,
        );
        assert_eq!(request.transfer_syntax, TransferSyntax::Htj2kLosslessRpcl);
        assert_eq!(request.encode_backend, EncodeBackendPreference::CpuOnly);
        assert_eq!(request.codec_validation, CodecValidation::RoundTrip);

        let err = FrameSamples::new(&pixels[..3], 2, 2, 3, 8, false)
            .expect_err("short frame data should be rejected");
        assert!(err.to_string().contains("pixel"));
    }
}
