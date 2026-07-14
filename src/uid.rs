use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::metadata::DicomMetadata;
use crate::options::{ExportOptions, UidPolicy};
use crate::Error;

pub(crate) struct DicomExportIdentity {
    study_uid: String,
    generation_seed: String,
}

impl DicomExportIdentity {
    pub(crate) fn for_export(
        source_path: &Path,
        options: &ExportOptions,
        metadata: &DicomMetadata,
        level_filter: Option<u32>,
    ) -> Result<Self, Error> {
        let generation_seed = match options.uid_policy {
            UidPolicy::Fresh => fresh_generation_seed()?,
            UidPolicy::Deterministic => {
                deterministic_generation_seed(source_path, options, metadata, level_filter)?
            }
        };
        let study_uid = metadata
            .study_instance_uid
            .clone()
            .unwrap_or_else(|| uid_from_seed(&format!("study:{generation_seed}")));
        Ok(Self {
            study_uid,
            generation_seed,
        })
    }

    #[cfg(any(test, feature = "bench-internals"))]
    pub(crate) fn from_seed(study_uid: String, generation_seed: String) -> Self {
        Self {
            study_uid,
            generation_seed,
        }
    }

    pub(crate) fn study_uid(&self) -> &str {
        &self.study_uid
    }

    pub(crate) fn uid(&self, role_and_coordinate: &str) -> String {
        uid_from_seed(&format!("{}:{role_and_coordinate}", self.generation_seed))
    }
}

fn fresh_generation_seed() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|err| Error::Identity {
        reason: format!("operating-system random source failed: {err}"),
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn deterministic_generation_seed(
    source_path: &Path,
    options: &ExportOptions,
    metadata: &DicomMetadata,
    level_filter: Option<u32>,
) -> Result<String, Error> {
    let mut source = File::open(source_path).map_err(|err| Error::Identity {
        reason: format!(
            "cannot open source {} for deterministic hashing: {err}",
            source_path.display()
        ),
    })?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = source.read(&mut buffer).map_err(|err| Error::Identity {
            reason: format!(
                "cannot hash source {} for deterministic identity: {err}",
                source_path.display()
            ),
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let configuration = serde_json::to_vec(&DeterministicUidInputs {
        metadata,
        level_filter,
        tile_size: options.tile_size,
        transfer_syntax: options.transfer_syntax,
        jpeg_direct_htj2k_profile: options.jpeg_direct_htj2k_profile,
        jpeg_quality: options.jpeg_quality,
        icc_profile_policy: options.icc_profile_policy,
        encode_backend: options.encode_backend,
        source_device_decode: options.source_device_decode,
        j2k_decomposition_levels: options.j2k_decomposition_levels,
    })
    .map_err(|err| Error::Identity {
        reason: format!("cannot serialize deterministic identity inputs: {err}"),
    })?;
    digest.update([0]);
    digest.update(configuration);
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Serialize)]
struct DeterministicUidInputs<'a> {
    metadata: &'a DicomMetadata,
    level_filter: Option<u32>,
    tile_size: u32,
    transfer_syntax: crate::options::TransferSyntax,
    jpeg_direct_htj2k_profile: crate::options::JpegDirectHtj2kProfile,
    jpeg_quality: u8,
    icc_profile_policy: crate::options::IccProfilePolicy,
    encode_backend: crate::options::EncodeBackendPreference,
    source_device_decode: bool,
    j2k_decomposition_levels: Option<u8>,
}

pub(crate) fn uid_from_seed(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    format!("2.25.{}", u128::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_identity_changes_between_exports_and_preserves_supplied_study_uid() {
        let source = tempfile::NamedTempFile::new().unwrap();
        let options = ExportOptions::default();
        let mut metadata = DicomMetadata::research_placeholder();
        metadata.study_instance_uid = Some("1.2.826.0.1.3680043.10.999.7".into());

        let first =
            DicomExportIdentity::for_export(source.path(), &options, &metadata, None).unwrap();
        let second =
            DicomExportIdentity::for_export(source.path(), &options, &metadata, None).unwrap();

        assert_eq!(first.study_uid(), "1.2.826.0.1.3680043.10.999.7");
        assert_eq!(second.study_uid(), first.study_uid());
        assert_ne!(first.uid("instance:0"), second.uid("instance:0"));
    }

    #[test]
    fn deterministic_identity_tracks_source_content_and_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.bin");
        std::fs::write(&source, b"pixels-a").unwrap();
        let options = ExportOptions {
            uid_policy: UidPolicy::Deterministic,
            ..ExportOptions::default()
        };
        let metadata = DicomMetadata::research_placeholder();

        let first = DicomExportIdentity::for_export(&source, &options, &metadata, None).unwrap();
        let repeated = DicomExportIdentity::for_export(&source, &options, &metadata, None).unwrap();
        let mut overwrite_options = options.clone();
        overwrite_options.overwrite = true;
        overwrite_options.codec_validation = crate::options::CodecValidation::RoundTrip;
        let operational_change =
            DicomExportIdentity::for_export(&source, &overwrite_options, &metadata, None).unwrap();
        let level_change =
            DicomExportIdentity::for_export(&source, &options, &metadata, Some(1)).unwrap();
        std::fs::write(&source, b"pixels-b").unwrap();
        let changed = DicomExportIdentity::for_export(&source, &options, &metadata, None).unwrap();

        assert_eq!(first.uid("instance:0"), repeated.uid("instance:0"));
        assert_eq!(
            first.uid("instance:0"),
            operational_change.uid("instance:0")
        );
        assert_ne!(first.uid("instance:0"), level_change.uid("instance:0"));
        assert_ne!(first.uid("instance:0"), changed.uid("instance:0"));
    }
}
