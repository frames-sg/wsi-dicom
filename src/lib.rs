#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! DICOM whole-slide export for `wsi-rs` datasets.
//!
//! The crate facade intentionally stays small: public types and functions are
//! re-exported from focused internal modules, while implementation details live
//! behind crate-private module boundaries.
//!
//! Optional `cuda`, `metal`, and aggregate `gpu` features enable acceleration
//! plumbing where the corresponding platform runtime is available. The
//! `bench-internals` feature exposes unstable helpers for the repository's
//! benchmark harness only.

mod api;
#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub mod bench_support;
mod defaults;
mod diagnostics;
mod encode;
mod error;
mod export;
mod instance_context;
mod metadata;
mod options;
mod passthrough;
mod report;
mod request;
mod routing;
mod tile;
mod uid;
mod validation;
mod writer;

#[cfg(test)]
mod test_support;

pub use api::Export;
pub use defaults::default_transfer_syntax_for_source;
pub use diagnostics::{run_dicom_self_test, SelfTestOptions, SelfTestReport};
pub use error::Error;
pub use export::{
    encode_dicom_j2k_frame, export_dicom, profile_dicom_route_corpus_coverage,
    profile_dicom_route_coverage, profile_dicom_routes,
};
pub use metadata::{DicomMetadata, MetadataSource, METADATA_JSON_MAX_BYTES};
pub use options::{
    CodecValidation, EncodeBackendPreference, ExportOptions, ExportPreset, IccProfilePolicy,
    JpegDirectHtj2kProfile, TransferSyntax,
};
pub use report::{
    EncodedFrame, ExportMetrics, ExportReport, GpuEncodeMetrics, IccProfileSource, InstanceReport,
    JpegDirectHtj2kMetrics, RouteCorpusCoverageFailure, RouteCorpusCoverageReport, RouteCounters,
    RouteCoverageReport, RouteProfileReport, WriteTimings,
};
pub use request::{
    DefaultTransferSyntaxRequest, ExportRequest, FrameSamples, J2kFrameEncodeRequest,
    RouteCoverageRequest, RouteCoverageTarget, RouteProfileRequest, RouteProgressSink,
};
pub use validation::{
    doctor_dicom_environment, validate_dicom_path, DoctorOptions, DoctorReport, DoctorStatus,
    DoctorTool, ValidationCheck, ValidationOptions, ValidationReport, ValidationStatus,
};

pub mod prelude {
    //! Common imports for applications using `wsi-dicom`.

    pub use crate::{
        CodecValidation, DefaultTransferSyntaxRequest, Error, Export, ExportOptions, ExportPreset,
        ExportReport, ExportRequest, FrameSamples, IccProfilePolicy, IccProfileSource,
        J2kFrameEncodeRequest, JpegDirectHtj2kProfile, MetadataSource, TransferSyntax,
        ValidationOptions,
    };
}

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
