//! Portable, verified ICC calibration profiles and scanner registries.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::icc::validate_dicom_icc_profile;
use crate::metadata::DicomMetadata;
use crate::Error;

/// Maximum accepted calibration registry JSON size (1 MiB).
pub const ICC_CALIBRATION_REGISTRY_MAX_BYTES: u64 = 1024 * 1024;
/// Maximum accepted ICC profile size (16 MiB).
pub const ICC_PROFILE_MAX_BYTES: u64 = 16 * 1024 * 1024;
const ICC_CALIBRATION_REGISTRY_MAX_ENTRIES: usize = 256;
const ICC_CALIBRATION_SCHEMA_VERSION: u32 = 1;

/// Exact scanner identity used for governed calibration matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannerIdentity {
    manufacturer: String,
    model_name: String,
    device_serial_number: String,
}

impl ScannerIdentity {
    /// Create a scanner identity after trimming outer whitespace.
    pub fn new(
        manufacturer: impl Into<String>,
        model_name: impl Into<String>,
        device_serial_number: impl Into<String>,
    ) -> Result<Self, Error> {
        Ok(Self {
            manufacturer: required_trimmed("manufacturer", manufacturer.into())?,
            model_name: required_trimmed("model_name", model_name.into())?,
            device_serial_number: required_trimmed(
                "device_serial_number",
                device_serial_number.into(),
            )?,
        })
    }

    /// Governed DICOM Manufacturer value.
    pub fn manufacturer(&self) -> &str {
        &self.manufacturer
    }

    /// Governed DICOM Manufacturer's Model Name value.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Governed DICOM Device Serial Number value.
    pub fn device_serial_number(&self) -> &str {
        &self.device_serial_number
    }

    pub(crate) fn from_metadata(metadata: &DicomMetadata) -> Result<Self, Error> {
        Self::new(
            required_metadata_field("manufacturer", metadata.manufacturer.as_deref())?,
            required_metadata_field(
                "manufacturer_model_name",
                metadata.manufacturer_model_name.as_deref(),
            )?,
            required_metadata_field(
                "device_serial_number",
                metadata.device_serial_number.as_deref(),
            )?,
        )
    }
}

/// Verified DICOM input-device ICC profile retained entirely in memory.
#[derive(Clone, PartialEq, Eq)]
pub struct IccProfile {
    id: String,
    bytes: Vec<u8>,
    sha256: String,
}

impl fmt::Debug for IccProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IccProfile")
            .field("id", &self.id)
            .field("byte_len", &self.bytes.len())
            .field("sha256", &self.sha256)
            .finish()
    }
}

impl IccProfile {
    /// Validate an in-memory ICC profile and associate it with a governed ID.
    pub fn from_bytes(id: impl Into<String>, bytes: Vec<u8>) -> Result<Self, Error> {
        let id = validate_calibration_id(id.into())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ICC_PROFILE_MAX_BYTES {
            return Err(calibration_error(format!(
                "ICC profile exceeds the {ICC_PROFILE_MAX_BYTES}-byte limit"
            )));
        }
        validate_dicom_icc_profile(&bytes)?;
        let sha256 = sha256_hex(&bytes);
        Ok(Self { id, bytes, sha256 })
    }

    /// Read and validate an ICC profile without retaining its local path.
    pub fn from_file(id: impl Into<String>, path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let bytes = read_bounded_file(path, ICC_PROFILE_MAX_BYTES, "ICC profile")?;
        Self::from_bytes(id, bytes)
    }

    /// Governed calibration or explicit-profile identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Verified profile bytes suitable for DICOM embedding.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Lowercase SHA-256 digest of the verified profile bytes.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Resolution policy when a configured ICC profile differs from a source profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum IccConflictPolicy {
    /// Fail before creating export staging or output state.
    Fail,
    /// Embed the configured calibration or explicit profile.
    PreferConfigured,
    /// Retain the source profile.
    PreferSource,
}

/// Explicit color-management configuration required for every export.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorManagement {
    /// Require an ICC profile from governed source metadata or embedded JPEG data.
    RequireSource,
    /// Preserve a source profile, or use a deterministic synthetic sRGB input profile.
    SourceOrSrgb,
    /// Preserve a source profile, or use a deterministic synthetic Display P3 input profile.
    SourceOrDisplayP3,
    /// Match a verified portable calibration registry against governed scanner metadata.
    Calibration {
        /// Loaded registry containing verified profile bytes.
        registry: IccCalibrationRegistry,
        /// Resolution policy for a differing source profile.
        conflict: IccConflictPolicy,
    },
    /// Use one verified profile supplied directly by the caller.
    ExplicitProfile {
        /// Verified profile and governed identifier.
        profile: IccProfile,
        /// Resolution policy for a differing source profile.
        conflict: IccConflictPolicy,
    },
}

/// Portable scanner-to-profile registry with verified profile bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct IccCalibrationRegistry {
    calibrations: Vec<Calibration>,
}

impl fmt::Debug for IccCalibrationRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IccCalibrationRegistry")
            .field("calibration_count", &self.calibrations.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Calibration {
    scanner: ScannerIdentity,
    profile: IccProfile,
}

impl IccCalibrationRegistry {
    /// Load a schema-v1 registry and eagerly retain checksum-verified profile bytes.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let bytes = read_bounded_file(
            path,
            ICC_CALIBRATION_REGISTRY_MAX_BYTES,
            "ICC calibration registry",
        )?;
        let raw: RegistryDocument =
            serde_json::from_slice(&bytes).map_err(|source| Error::Json {
                path: path.to_path_buf(),
                source,
            })?;
        if raw.schema_version != ICC_CALIBRATION_SCHEMA_VERSION {
            return Err(calibration_error(format!(
                "unsupported calibration registry schema_version {}; expected {ICC_CALIBRATION_SCHEMA_VERSION}",
                raw.schema_version
            )));
        }
        if raw.calibrations.len() > ICC_CALIBRATION_REGISTRY_MAX_ENTRIES {
            return Err(calibration_error(format!(
                "calibration registry contains {} entries; limit is {ICC_CALIBRATION_REGISTRY_MAX_ENTRIES}",
                raw.calibrations.len()
            )));
        }

        let registry_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let canonical_registry_dir =
            fs::canonicalize(registry_dir).map_err(|source| Error::Io {
                path: registry_dir.to_path_buf(),
                source,
            })?;
        let mut ids = HashSet::new();
        let mut scanners = HashSet::new();
        let mut calibrations = Vec::new();
        calibrations
            .try_reserve(raw.calibrations.len())
            .map_err(|_| calibration_error("calibration registry exceeds available memory"))?;

        for raw_calibration in raw.calibrations {
            let id = validate_calibration_id(raw_calibration.id)?;
            let scanner = ScannerIdentity::new(
                raw_calibration.scanner.manufacturer,
                raw_calibration.scanner.model_name,
                raw_calibration.scanner.device_serial_number,
            )?;
            if !ids.insert(id.clone()) {
                return Err(calibration_error(format!(
                    "duplicate calibration id '{id}'"
                )));
            }
            if !scanners.insert(scanner.clone()) {
                return Err(calibration_error(format!(
                    "duplicate scanner tuple for manufacturer='{}', model_name='{}', device_serial_number='{}'",
                    scanner.manufacturer, scanner.model_name, scanner.device_serial_number
                )));
            }
            validate_sha256(&raw_calibration.sha256)?;
            let relative_profile = validate_relative_profile_path(&raw_calibration.icc_profile)?;
            let profile_path = registry_dir.join(relative_profile);
            let canonical_profile =
                fs::canonicalize(&profile_path).map_err(|source| Error::Io {
                    path: profile_path.clone(),
                    source,
                })?;
            if !canonical_profile.starts_with(&canonical_registry_dir) {
                return Err(calibration_error(format!(
                    "ICC profile path '{}' resolves outside the registry directory",
                    raw_calibration.icc_profile
                )));
            }
            let profile = IccProfile::from_file(id, &canonical_profile)?;
            if profile.sha256 != raw_calibration.sha256 {
                return Err(calibration_error(format!(
                    "ICC profile checksum mismatch for calibration '{}': expected {}, got {}",
                    profile.id, raw_calibration.sha256, profile.sha256
                )));
            }
            calibrations.push(Calibration { scanner, profile });
        }

        Ok(Self { calibrations })
    }

    /// Find a calibration by exact, case-sensitive scanner identity.
    pub fn match_scanner(&self, scanner: &ScannerIdentity) -> Option<&IccProfile> {
        self.calibrations
            .iter()
            .find(|calibration| &calibration.scanner == scanner)
            .map(|calibration| &calibration.profile)
    }

    pub(crate) fn match_metadata(&self, metadata: &DicomMetadata) -> Result<&IccProfile, Error> {
        let scanner = ScannerIdentity::from_metadata(metadata)?;
        self.match_scanner(&scanner).ok_or_else(|| {
            calibration_error(format!(
                "no calibration matches manufacturer='{}', model_name='{}', device_serial_number='{}'",
                scanner.manufacturer, scanner.model_name, scanner.device_serial_number
            ))
        })
    }
}

/// Package an existing vendor- or target-generated ICC profile into a portable bundle.
///
/// This operation validates and copies an existing input-device profile. It does not derive a
/// calibration from slide pixels.
pub fn create_icc_calibration_bundle(
    icc_path: impl AsRef<Path>,
    id: impl Into<String>,
    scanner: &ScannerIdentity,
    output_dir: impl AsRef<Path>,
) -> Result<(), Error> {
    let profile = IccProfile::from_file(id, icc_path)?;
    let output_dir = output_dir.as_ref();
    if output_dir.exists() {
        return Err(Error::Io {
            path: output_dir.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "calibration output directory already exists",
            ),
        });
    }
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| Error::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let staging = tempfile::Builder::new()
        .prefix(".wsi-dicom-calibration-")
        .tempdir_in(parent)
        .map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    let profiles_dir = staging.path().join("profiles");
    fs::create_dir(&profiles_dir).map_err(|source| Error::Io {
        path: profiles_dir.clone(),
        source,
    })?;
    let bundled_profile = profiles_dir.join("profile.icc");
    fs::write(&bundled_profile, profile.bytes()).map_err(|source| Error::Io {
        path: bundled_profile,
        source,
    })?;
    let registry = RegistryDocument {
        schema_version: ICC_CALIBRATION_SCHEMA_VERSION,
        calibrations: vec![RegistryCalibration {
            id: profile.id().to_string(),
            scanner: scanner.clone(),
            icc_profile: "profiles/profile.icc".to_string(),
            sha256: profile.sha256().to_string(),
        }],
    };
    let mut registry_bytes =
        serde_json::to_vec_pretty(&registry).map_err(|err| Error::JsonSerialize {
            message: format!("cannot serialize ICC calibration registry: {err}"),
        })?;
    registry_bytes.push(b'\n');
    let registry_path = staging.path().join("registry.json");
    fs::write(&registry_path, registry_bytes).map_err(|source| Error::Io {
        path: registry_path,
        source,
    })?;
    fs::rename(staging.path(), output_dir).map_err(|source| Error::Io {
        path: output_dir.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    schema_version: u32,
    calibrations: Vec<RegistryCalibration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryCalibration {
    id: String,
    scanner: ScannerIdentity,
    icc_profile: String,
    sha256: String,
}

fn validate_relative_profile_path(value: &str) -> Result<PathBuf, Error> {
    let path = Path::new(value);
    let has_windows_drive_prefix = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || has_windows_drive_prefix
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(calibration_error(format!(
            "ICC profile path '{value}' must be a non-empty portable relative path using forward slashes without parent traversal"
        )));
    }
    Ok(path.to_path_buf())
}

fn validate_calibration_id(value: String) -> Result<String, Error> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(calibration_error("calibration id must not be empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(calibration_error(
            "calibration id must not contain control characters",
        ));
    }
    Ok(value)
}

fn validate_sha256(value: &str) -> Result<(), Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(calibration_error(
            "calibration sha256 must contain exactly 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn required_trimmed(field: &str, value: String) -> Result<String, Error> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(calibration_error(format!(
            "scanner {field} must not be empty"
        )));
    }
    Ok(value)
}

fn required_metadata_field<'a>(field: &str, value: Option<&'a str>) -> Result<&'a str, Error> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| calibration_error(format!("calibration matching requires DICOM {field}")))
}

fn read_bounded_file(path: &Path, limit: u64, kind: &str) -> Result<Vec<u8>, Error> {
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(calibration_error(format!(
            "{kind} at {} exceeds the {limit}-byte limit",
            path.display()
        )));
    }
    Ok(bytes)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn calibration_error(reason: impl Into<String>) -> Error {
    Error::Metadata {
        reason: format!("ICC calibration: {}", reason.into()),
    }
}
