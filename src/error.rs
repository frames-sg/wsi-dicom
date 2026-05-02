use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum WsiDicomError {
    #[error("invalid DICOM export options: {reason}")]
    InvalidOptions { reason: String },
    #[error("invalid DICOM metadata: {reason}")]
    Metadata { reason: String },
    #[error("unsupported pixel data: {reason}")]
    UnsupportedPixelData { reason: String },
    #[error("unsupported export request: {reason}")]
    Unsupported { reason: String },
    #[error("failed to open source slide {path}: {message}")]
    SourceOpen { path: PathBuf, message: String },
    #[error("slide read failed: {message}")]
    SlideRead { message: String },
    #[error("JPEG 2000 encode failed: {message}")]
    Encode { message: String },
    #[error(
        "JPEG 2000 encode failed at level {level}, tile row {row}, tile column {col}: {message}"
    )]
    FrameEncode {
        level: u32,
        row: u64,
        col: u64,
        message: String,
    },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("DICOM write failed for {path}: {message}")]
    DicomWrite { path: PathBuf, message: String },
    #[error("JSON parse failed for {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}
