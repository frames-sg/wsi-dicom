use crate::Error;

pub(crate) const JPEG_BASELINE_METHOD: &str = "ISO_10918_1";
pub(crate) const JPEG_2000_METHOD: &str = "ISO_15444_1";
pub(crate) const HTJ2K_METHOD: &str = "ISO_15444_15";

pub(crate) fn method_for_lossy_transfer_syntax(uid: &str) -> Option<&'static str> {
    match uid.trim_end_matches('\0') {
        "1.2.840.10008.1.2.4.50" => Some(JPEG_BASELINE_METHOD),
        "1.2.840.10008.1.2.4.91" => Some(JPEG_2000_METHOD),
        "1.2.840.10008.1.2.4.203" => Some(HTJ2K_METHOD),
        _ => None,
    }
}

pub(crate) fn uncompressed_pixel_bytes(
    width: u64,
    height: u64,
    components: u64,
    bits_allocated: u16,
) -> Result<u64, Error> {
    let bytes_per_sample = u64::from(bits_allocated).div_ceil(8);
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(components))
        .and_then(|samples| samples.checked_mul(bytes_per_sample))
        .ok_or_else(|| Error::Metadata {
            reason: "lossy compression uncompressed byte count overflow".into(),
        })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LossyCompressionHistory {
    stages: Vec<LossyCompressionStage>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LossyCompressionStage {
    method: String,
    ratio: f64,
}

impl LossyCompressionHistory {
    pub(crate) fn from_byte_counts(
        method: impl Into<String>,
        uncompressed_bytes: u64,
        compressed_bytes: u64,
    ) -> Result<Self, Error> {
        let mut history = Self::default();
        history.push_byte_counts(method, uncompressed_bytes, compressed_bytes)?;
        Ok(history)
    }

    pub(crate) fn push_byte_counts(
        &mut self,
        method: impl Into<String>,
        uncompressed_bytes: u64,
        compressed_bytes: u64,
    ) -> Result<(), Error> {
        if compressed_bytes == 0 {
            return Err(Error::Metadata {
                reason:
                    "lossy compression history cannot estimate a ratio from zero compressed bytes"
                        .into(),
            });
        }
        self.push_declared(method, uncompressed_bytes as f64 / compressed_bytes as f64)
    }

    pub(crate) fn push_declared(
        &mut self,
        method: impl Into<String>,
        ratio: f64,
    ) -> Result<(), Error> {
        let method = method.into();
        if method.is_empty()
            || method.len() > 16
            || !method.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_' || byte == b' '
            })
        {
            return Err(Error::Metadata {
                reason: "lossy compression history method must be a valid scalar DICOM CS value"
                    .into(),
            });
        }
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(Error::Metadata {
                reason: "lossy compression history ratio must be finite and greater than zero"
                    .into(),
            });
        }
        self.stages.push(LossyCompressionStage { method, ratio });
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub(crate) fn stages(&self) -> &[LossyCompressionStage] {
        &self.stages
    }

    pub(crate) fn append(&mut self, mut subsequent: Self) {
        self.stages.append(&mut subsequent.stages);
    }
}

impl LossyCompressionStage {
    pub(crate) fn method(&self) -> &str {
        &self.method
    }

    pub(crate) fn ratio(&self) -> f64 {
        self.ratio
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LossyCompressionByteCounts {
    method: &'static str,
    uncompressed_bytes: u64,
    compressed_bytes: u64,
}

impl LossyCompressionByteCounts {
    pub(crate) fn new(
        method: &'static str,
        uncompressed_bytes: u64,
        compressed_bytes: u64,
    ) -> Result<Self, Error> {
        if compressed_bytes == 0 {
            return Err(Error::Metadata {
                reason:
                    "lossy compression history cannot estimate a ratio from zero compressed bytes"
                        .into(),
            });
        }
        Ok(Self {
            method,
            uncompressed_bytes,
            compressed_bytes,
        })
    }

    pub(crate) fn from_raw_tile(
        method: &'static str,
        raw: &wsi_rs::RawCompressedTile,
    ) -> Result<Self, Error> {
        let uncompressed_bytes = uncompressed_pixel_bytes(
            u64::from(raw.width()),
            u64::from(raw.height()),
            u64::from(raw.samples_per_pixel()),
            raw.bits_allocated(),
        )?;
        let compressed_bytes = u64::try_from(raw.data().len()).map_err(|_| Error::Metadata {
            reason: "lossy compressed tile byte count exceeds u64".into(),
        })?;
        Self::new(method, uncompressed_bytes, compressed_bytes)
    }
}

#[derive(Debug, Default)]
pub(crate) struct LossyCompressionAccumulator {
    stages: Vec<LossyCompressionByteCounts>,
}

impl LossyCompressionAccumulator {
    pub(crate) fn observe(&mut self, observed: &LossyCompressionByteCounts) -> Result<(), Error> {
        if let Some(stage) = self
            .stages
            .iter_mut()
            .find(|stage| stage.method == observed.method)
        {
            stage.uncompressed_bytes = stage
                .uncompressed_bytes
                .checked_add(observed.uncompressed_bytes)
                .ok_or_else(|| Error::Metadata {
                    reason: "lossy uncompressed byte count overflow".into(),
                })?;
            stage.compressed_bytes = stage
                .compressed_bytes
                .checked_add(observed.compressed_bytes)
                .ok_or_else(|| Error::Metadata {
                    reason: "lossy compressed byte count overflow".into(),
                })?;
        } else {
            self.stages.push(observed.clone());
        }
        Ok(())
    }

    pub(crate) fn observe_bytes(
        &mut self,
        method: &'static str,
        uncompressed_bytes: u64,
        compressed_bytes: u64,
    ) -> Result<(), Error> {
        self.observe(&LossyCompressionByteCounts::new(
            method,
            uncompressed_bytes,
            compressed_bytes,
        )?)
    }

    pub(crate) fn observe_encoded_frame(
        &mut self,
        method: &'static str,
        uncompressed_bytes: u64,
        encoded: &[u8],
    ) -> Result<(), Error> {
        let compressed_bytes = u64::try_from(encoded.len()).map_err(|_| Error::Metadata {
            reason: "lossy encoded frame byte count exceeds u64".into(),
        })?;
        self.observe_bytes(method, uncompressed_bytes, compressed_bytes)
    }

    pub(crate) fn into_history(self) -> Result<LossyCompressionHistory, Error> {
        let mut history = LossyCompressionHistory::default();
        for stage in self.stages {
            history.push_byte_counts(
                stage.method,
                stage.uncompressed_bytes,
                stage.compressed_bytes,
            )?;
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_history_rejects_invalid_dicom_method_and_ratio_values() {
        let mut history = LossyCompressionHistory::default();

        assert!(history.push_declared("lowercase", 2.0).is_err());
        assert!(history
            .push_declared("METHOD_NAME_LONGER_THAN_16", 2.0)
            .is_err());
        assert!(history.push_declared(JPEG_BASELINE_METHOD, 0.0).is_err());
        history.push_declared(JPEG_BASELINE_METHOD, 2.0).unwrap();
    }
}
