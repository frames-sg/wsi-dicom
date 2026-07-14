use std::path::PathBuf;

/// Error type returned by public `wsi-dicom` APIs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Export or validation options are internally inconsistent.
    #[error("invalid DICOM export options: {reason}")]
    InvalidOptions {
        /// Human-readable reason safe to show to users.
        reason: String,
    },
    /// Metadata is missing required fields or cannot be mapped safely.
    #[error("invalid DICOM metadata: {reason}")]
    Metadata {
        /// Human-readable reason safe to show to users.
        reason: String,
    },
    /// Source or prepared pixel data cannot be represented by the requested output.
    #[error("unsupported pixel data: {reason}")]
    UnsupportedPixelData {
        /// Human-readable reason safe to show to users.
        reason: String,
    },
    /// The requested route, transfer syntax, or backend is unsupported.
    #[error("unsupported export request: {reason}")]
    Unsupported {
        /// Human-readable reason safe to show to users.
        reason: String,
    },
    /// The source slide could not be opened.
    #[error("failed to open source slide {path}: {message}")]
    SourceOpen {
        /// Source path that failed to open.
        path: PathBuf,
        /// Underlying open error message.
        message: String,
    },
    /// DICOM export identity generation failed.
    #[error("failed to generate DICOM export identity: {reason}")]
    Identity {
        /// Human-readable identity generation failure.
        reason: String,
    },
    /// The source slide failed while reading pixels or metadata.
    #[error("slide read failed: {message}")]
    SlideRead {
        /// Underlying read error message.
        message: String,
    },
    /// JPEG 2000 or HTJ2K encoding failed before a frame location was available.
    #[error("JPEG 2000 encode failed: {message}")]
    Encode {
        /// Underlying encode error message.
        message: String,
    },
    /// JPEG 2000 or HTJ2K encoding failed for one DICOM frame.
    #[error(
        "JPEG 2000 encode failed at level {level}, tile row {row}, tile column {col}: {message}"
    )]
    FrameEncode {
        /// Source pyramid level being encoded.
        level: u32,
        /// DICOM tile row being encoded.
        row: u64,
        /// DICOM tile column being encoded.
        col: u64,
        /// Underlying encode error message.
        message: String,
    },
    /// Filesystem I/O failed at a known path.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path involved in the failed I/O operation.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// DICOM object construction or writing failed.
    #[error("DICOM write failed for {path}: {message}")]
    DicomWrite {
        /// Destination path being written.
        path: PathBuf,
        /// Underlying DICOM writer message.
        message: String,
    },
    /// JSON parsing failed at a known path.
    #[error("JSON parse failed for {path}: {source}")]
    Json {
        /// JSON path being read.
        path: PathBuf,
        /// Underlying JSON parser error.
        source: serde_json::Error,
    },
    /// JSON serialization failed.
    #[error("JSON serialization failed: {message}")]
    JsonSerialize {
        /// Serialization error message.
        message: String,
    },
    /// A staged export could not be committed or rolled back completely.
    #[error("export transaction requires recovery at {recovery_path}: {reason}")]
    ExportTransaction {
        /// Directory containing the durable manifest, staged files, and backups.
        recovery_path: PathBuf,
        /// Human-readable commit or rollback failure details.
        reason: String,
    },
    /// External DICOM validation failed.
    #[error("DICOM validation failed: {reason}")]
    Validation {
        /// Human-readable validation failure reason.
        reason: String,
    },
}
