#![forbid(unsafe_code)]

//! DICOM whole-slide export for `statumen` datasets.
//!
//! The crate facade intentionally stays small: public types and functions are
//! re-exported from focused internal modules, while implementation details live
//! behind crate-private module boundaries.

mod api;
#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub mod bench_support;
mod defaults;
mod encode;
mod error;
mod export;
mod instance_context;
mod metadata;
mod options;
mod passthrough;
mod profile;
mod report;
mod request;
mod routing;
mod tile;
mod uid;
mod writer;

#[cfg(test)]
mod test_support;

#[cfg(any(feature = "cuda", all(feature = "metal", target_os = "macos")))]
mod gpu;

pub use api::DicomExport;
pub use defaults::default_transfer_syntax_for_source;
pub use error::WsiDicomError;
pub use export::{encode_dicom_j2k_frame, export_dicom};
pub use metadata::{DicomMetadata, MetadataSource};
pub use options::{CodecValidation, DicomExportOptions, EncodeBackendPreference, TransferSyntax};
pub use profile::{
    profile_dicom_route_corpus_coverage, profile_dicom_route_coverage, profile_dicom_routes,
};
pub use report::{
    duration_as_reported_micros, DicomEncodedFrame, DicomExportMetrics, DicomExportReport,
    DicomInstanceReport, DicomRouteCorpusCoverageFailure, DicomRouteCorpusCoverageReport,
    DicomRouteCoverageReport, DicomRouteProfileReport,
};
pub use request::{
    DefaultTransferSyntaxRequest, DicomExportRequest, DicomJ2kFrameEncodeRequest,
    DicomRouteCorpusCoverageProgress, DicomRouteCorpusCoverageRequest, DicomRouteCoverageProgress,
    DicomRouteCoverageRequest, DicomRouteProfileRequest,
};

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
