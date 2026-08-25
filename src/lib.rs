#![deny(unsafe_code)]
#![warn(missing_docs)]

//! DICOM whole-slide export for `wsi-rs` datasets.
//!
//! The crate facade intentionally stays small: public types and functions are
//! re-exported from focused internal modules, while implementation details live
//! behind crate-private module boundaries.
//!
//! Optional `cuda` and `metal` features enable acceleration plumbing where the
//! corresponding platform runtime is available. The
//! `bench-internals` feature exposes unstable helpers for the repository's
//! benchmark harness only.

mod annotation_export;
mod api;
mod application;
#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub mod bench_support;
mod calibration;
mod coordinate;
mod diagnostics;
mod encode;
mod error;
mod export;
mod icc;
mod instance_context;
mod lossy;
mod metadata;
#[cfg(all(feature = "metal", target_os = "macos"))]
#[allow(unsafe_code)]
mod metal_interop;
mod options;
mod passthrough;
mod report;
mod request;
mod routing;
mod synthetic_source;
mod tile;
#[doc(hidden)]
pub mod time;
mod uid;
mod validation;
mod writer;

#[cfg(test)]
mod test_support;

pub use annotation_export::{
    export_qupath_annotations, AnnotationCoordinateSpace, AnnotationDiagnosticReport,
    AnnotationExportReport, AnnotationInstanceReport, AnnotationTarget, QuPathAnnotationOptions,
};
pub use api::Export;
pub use application::{
    run_export_workflow, ExportWorkflowError, ExportWorkflowProgress, ExportWorkflowProgressEvent,
    ExportWorkflowReport, ExportWorkflowRequest, ExportWorkflowStage, MetadataInput,
};
pub use calibration::{
    create_icc_calibration_bundle, ColorManagement, IccCalibrationRegistry, IccConflictPolicy,
    IccProfile, ScannerIdentity, ICC_CALIBRATION_REGISTRY_MAX_BYTES, ICC_PROFILE_MAX_BYTES,
};
pub use diagnostics::{run_dicom_self_test, SelfTestOptions, SelfTestReport};
pub use error::Error;
pub use export::default_transfer_syntax_for_source;
pub use export::{
    encode_dicom_j2k_frame, export_dicom, profile_corpus_route_coverage,
    profile_dicom_route_corpus_coverage, profile_dicom_route_coverage, profile_dicom_routes,
    profile_slide_route_coverage,
};
pub use metadata::{
    DicomMetadata, MetadataSource, SpecimenIdentifierIssuer, UniversalEntityIdType,
    METADATA_JSON_MAX_BYTES,
};
pub use options::{
    CodecValidation, EncodeBackendPreference, ExportOptions, ExportPreset, JpegDirectHtj2kProfile,
    TransferSyntax, UidPolicy,
};
pub use report::{
    EncodedFrame, ExportMetrics, ExportReport, GpuEncodeMetrics, IccConflictDecision,
    IccProfileSource, InstanceReport, JpegDirectHtj2kMetrics, RouteCorpusCoverageFailure,
    RouteCorpusCoverageReport, RouteCounters, RouteCoverageReport, RouteProfileReport,
    WriteTimings,
};
pub use request::{
    CorpusRouteCoverageRequest, DefaultTransferSyntaxRequest, ExportRequest, FrameSamples,
    J2kFrameEncodeRequest, RouteCoverageRequest, RouteCoverageTarget, RouteProfileRequest,
    RouteProgressSink, SlideRouteCoverageRequest,
};
pub use validation::{
    doctor_dicom_environment, validate_dicom_path, DoctorOptions, DoctorReport, DoctorStatus,
    DoctorTool, ValidationCheck, ValidationOptions, ValidationReport, ValidationStatus,
};

pub mod prelude {
    //! Common imports for applications using `wsi-dicom`.

    pub use crate::{
        CodecValidation, ColorManagement, DefaultTransferSyntaxRequest, Error, Export,
        ExportOptions, ExportPreset, ExportReport, ExportRequest, FrameSamples,
        IccCalibrationRegistry, IccConflictDecision, IccConflictPolicy, IccProfile,
        IccProfileSource, J2kFrameEncodeRequest, JpegDirectHtj2kProfile, MetadataSource,
        ScannerIdentity, SpecimenIdentifierIssuer, TransferSyntax, UidPolicy,
        UniversalEntityIdType, ValidationOptions,
    };
}

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
