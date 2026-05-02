#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use dicom_object::FileMetaTableBuilder;
use signinum_j2k::J2kLosslessSamples;
use statumen::{LevelIdx, PlaneIdx, PlaneSelection, RegionRequest, SceneId, SeriesId, Slide};

mod encode;
mod error;
mod metadata;
mod options;
mod tile;
mod uid;
mod writer;

pub use error::WsiDicomError;
pub use metadata::{DicomMetadata, MetadataSource};
pub use options::{DicomExportOptions, EncodeBackendPreference, TransferSyntax};

use encode::DicomJ2kEncoder;
use tile::{optical_path_groups, prepare_tile_samples};
use uid::{deterministic_instance_path, uid_from_seed};
use writer::build_dicom_object;

pub(crate) const VL_WSI_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";

/// A validated request to export one vendor WSI into one DICOM output directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DicomExportRequest {
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub options: DicomExportOptions,
    pub metadata: MetadataSource,
}

impl DicomExportRequest {
    pub fn new(
        source_path: PathBuf,
        output_dir: PathBuf,
        options: DicomExportOptions,
    ) -> Result<Self, WsiDicomError> {
        options.validate()?;
        Ok(Self {
            source_path,
            output_dir,
            options,
            metadata: MetadataSource::Strict(DicomMetadata::default()),
        })
    }

    pub fn validate(&self) -> Result<(), WsiDicomError> {
        self.options.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomExportReport {
    pub output_dir: PathBuf,
    pub instances: Vec<DicomInstanceReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomInstanceReport {
    pub path: PathBuf,
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub transfer_syntax_uid: &'static str,
    pub level: u32,
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub frame_count: u32,
}

/// Export a statumen-readable WSI into DICOM VL Whole Slide Microscopy files.
pub fn export_dicom(request: DicomExportRequest) -> Result<DicomExportReport, WsiDicomError> {
    request.validate()?;
    if request.options.transfer_syntax != TransferSyntax::Jpeg2000Lossless {
        return Err(WsiDicomError::Unsupported {
            reason: "only JPEG 2000 Lossless transfer syntax is implemented".into(),
        });
    }
    let metadata = request.metadata.resolve()?;
    fs::create_dir_all(&request.output_dir).map_err(|source| WsiDicomError::Io {
        path: request.output_dir.clone(),
        source,
    })?;

    let slide = Slide::open(&request.source_path).map_err(|source| WsiDicomError::SourceOpen {
        path: request.source_path.clone(),
        message: source.to_string(),
    })?;

    let study_uid = metadata
        .study_instance_uid
        .clone()
        .unwrap_or_else(|| uid_from_seed(&format!("study:{}", request.source_path.display())));
    let mut instances = Vec::new();

    for (scene_idx, scene) in slide.dataset().scenes.iter().enumerate() {
        for (series_idx, series) in scene.series.iter().enumerate() {
            for (level_idx, level) in series.levels.iter().enumerate() {
                for z in 0..series.axes.z {
                    for t in 0..series.axes.t {
                        let channel_groups = optical_path_groups(series.axes.c);
                        for c in channel_groups {
                            let report = export_instance(
                                &slide,
                                &request,
                                &metadata,
                                &study_uid,
                                scene_idx,
                                series_idx,
                                level_idx as u32,
                                z,
                                c,
                                t,
                                level,
                            )?;
                            instances.push(report);
                        }
                    }
                }
            }
        }
    }

    Ok(DicomExportReport {
        output_dir: request.output_dir,
        instances,
    })
}

#[allow(clippy::too_many_arguments)]
fn export_instance(
    slide: &Slide,
    request: &DicomExportRequest,
    metadata: &DicomMetadata,
    study_uid: &str,
    scene_idx: usize,
    series_idx: usize,
    level_idx: u32,
    z: u32,
    c: u32,
    t: u32,
    level: &statumen::Level,
) -> Result<DicomInstanceReport, WsiDicomError> {
    let tile_size = request.options.tile_size;
    let (matrix_columns, matrix_rows) = level.dimensions;
    let tiles_across = matrix_columns.div_ceil(u64::from(tile_size));
    let tiles_down = matrix_rows.div_ceil(u64::from(tile_size));
    let frame_count = tiles_across
        .checked_mul(tiles_down)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| WsiDicomError::Unsupported {
            reason: "frame count exceeds u32".into(),
        })?;

    let series_uid = uid_from_seed(&format!(
        "series:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        z,
        c,
        t
    ));
    let sop_instance_uid = uid_from_seed(&format!(
        "instance:{}:{}:{}:{}:{}:{}",
        request.source_path.display(),
        scene_idx,
        series_idx,
        level_idx,
        z,
        c
    ));

    let mut fragments = Vec::with_capacity(frame_count as usize);
    let mut offsets = Vec::with_capacity(frame_count as usize);
    let mut lengths = Vec::with_capacity(frame_count as usize);
    let mut offset = 0_u64;
    let mut pixel_profile = None;
    let mut j2k_encoder = DicomJ2kEncoder::new(request.options.encode_backend);

    for row in 0..tiles_down {
        for col in 0..tiles_across {
            let x = col * u64::from(tile_size);
            let y = row * u64::from(tile_size);
            let width = (matrix_columns - x).min(u64::from(tile_size)) as u32;
            let height = (matrix_rows - y).min(u64::from(tile_size)) as u32;
            let region = slide
                .read_region(&RegionRequest {
                    scene: SceneId(scene_idx),
                    series: SeriesId(series_idx),
                    level: LevelIdx(level_idx),
                    plane: PlaneIdx(PlaneSelection { z, c, t }),
                    origin_px: (x as i64, y as i64),
                    size_px: (width, height),
                })
                .map_err(|source| WsiDicomError::SlideRead {
                    message: source.to_string(),
                })?;
            let prepared = prepare_tile_samples(&region, tile_size, tile_size)?;
            if let Some(existing) = pixel_profile {
                if existing != prepared.profile {
                    return Err(WsiDicomError::UnsupportedPixelData {
                        reason: "pixel profile changed across frames".into(),
                    });
                }
            } else {
                pixel_profile = Some(prepared.profile);
            }
            let samples = J2kLosslessSamples::new(
                &prepared.bytes,
                tile_size,
                tile_size,
                prepared.profile.components,
                prepared.profile.bits_allocated as u8,
                false,
            )
            .map_err(|source| WsiDicomError::Encode {
                message: source.to_string(),
            })?;
            let encoded = j2k_encoder.encode(samples).map_err(|err| match err {
                WsiDicomError::Encode { message } => WsiDicomError::FrameEncode {
                    level: level_idx,
                    row,
                    col,
                    message,
                },
                other => other,
            })?;
            offsets.push(offset);
            lengths.push(encoded.len() as u64);
            offset = offset
                .checked_add(encoded.len() as u64 + 8)
                .ok_or_else(|| WsiDicomError::Unsupported {
                    reason: "extended offset table overflow".into(),
                })?;
            fragments.push(even_len(encoded));
        }
    }

    let profile = pixel_profile.ok_or_else(|| WsiDicomError::Unsupported {
        reason: "slide level produced no frames".into(),
    })?;
    let path = deterministic_instance_path(&request.output_dir, level_idx, z, c, t);
    let object = build_dicom_object(
        metadata,
        study_uid,
        &series_uid,
        &sop_instance_uid,
        level_idx,
        tile_size,
        matrix_columns,
        matrix_rows,
        frame_count,
        profile,
        fragments,
        offsets,
        lengths,
    )?;
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .media_storage_sop_class_uid(VL_WSI_SOP_CLASS_UID)
                .media_storage_sop_instance_uid(&sop_instance_uid)
                .transfer_syntax(request.options.transfer_syntax.uid()),
        )
        .map_err(|err| WsiDicomError::DicomWrite {
            path: path.clone(),
            message: err.to_string(),
        })?
        .write_to_file(&path)
        .map_err(|err| WsiDicomError::DicomWrite {
            path: path.clone(),
            message: err.to_string(),
        })?;

    Ok(DicomInstanceReport {
        path,
        sop_instance_uid,
        series_instance_uid: series_uid,
        transfer_syntax_uid: request.options.transfer_syntax.uid(),
        level: level_idx,
        z,
        c,
        t,
        frame_count,
    })
}

fn even_len(mut bytes: Vec<u8>) -> Vec<u8> {
    if !bytes.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::encode::{dicom_j2k_decomposition_levels, encode_dicom_j2k_lossless};
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::{tags, uids};
    use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

    #[test]
    fn default_options_use_jpeg2000_lossless_and_auto_backend() {
        let options = DicomExportOptions::default();

        assert_eq!(options.tile_size, 512);
        assert_eq!(options.transfer_syntax.uid(), "1.2.840.10008.1.2.4.90");
        assert_eq!(options.encode_backend, EncodeBackendPreference::Auto);
    }

    #[test]
    fn export_request_rejects_zero_tile_size() {
        let err = DicomExportRequest {
            source_path: PathBuf::from("source.svs"),
            output_dir: PathBuf::from("out"),
            options: DicomExportOptions {
                tile_size: 0,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
        }
        .validate()
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("tile_size must be greater than zero"));
    }

    #[test]
    fn export_request_keeps_source_and_output_paths() {
        let request = DicomExportRequest::new(
            PathBuf::from("source.ndpi"),
            PathBuf::from("dicom-out"),
            DicomExportOptions::default(),
        )
        .unwrap();

        assert_eq!(request.source_path, PathBuf::from("source.ndpi"));
        assert_eq!(request.output_dir, PathBuf::from("dicom-out"));
    }

    #[test]
    fn auto_and_prefer_device_fall_back_to_facade_cpu_when_no_device_backend_is_enabled() {
        let bytes = vec![0; 16 * 16];
        let samples = J2kLosslessSamples::new(&bytes, 16, 16, 1, 8, false).expect("valid samples");

        let auto = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::Auto)
            .expect("auto backend should fall back to CPU");
        assert_j2k_facade_roundtrip(samples, &auto);

        let prefer = encode_dicom_j2k_lossless(samples, EncodeBackendPreference::PreferDevice)
            .expect("prefer-device backend should fall back to CPU");
        assert_j2k_facade_roundtrip(samples, &prefer);

        let require =
            encode_dicom_j2k_lossless(samples, EncodeBackendPreference::RequireDevice).unwrap_err();
        assert!(require.to_string().contains("device encode backend"));
    }

    #[test]
    fn dicom_j2k_decomposition_uses_validated_lossless_safe_profile() {
        let gray = vec![0; 128 * 128];
        let gray_samples =
            J2kLosslessSamples::new(&gray, 128, 128, 1, 8, false).expect("valid gray");
        assert_eq!(dicom_j2k_decomposition_levels(gray_samples), 0);

        let rgb = vec![0; 128 * 128 * 3];
        let rgb_samples = J2kLosslessSamples::new(&rgb, 128, 128, 3, 8, false).expect("valid rgb");
        assert_eq!(dicom_j2k_decomposition_levels(rgb_samples), 0);
    }

    #[test]
    fn dicom_j2k_cpu_encode_round_trips_gray8_tile() {
        let bytes: Vec<u8> = (0..64).map(|value| ((value * 5) & 0xFF) as u8).collect();
        let samples = J2kLosslessSamples::new(&bytes, 8, 8, 1, 8, false).expect("valid samples");

        let codestream =
            encode_dicom_j2k_lossless(samples, EncodeBackendPreference::CpuOnly).unwrap();

        assert_j2k_facade_roundtrip(samples, &codestream);
    }

    fn assert_j2k_facade_roundtrip(samples: J2kLosslessSamples<'_>, codestream: &[u8]) {
        let mut decoder = signinum_j2k::J2kDecoder::new(codestream).expect("parse encoded J2K");
        let bytes_per_sample = if samples.bit_depth <= 8 {
            1usize
        } else {
            2usize
        };
        let stride = samples.width as usize * samples.components as usize * bytes_per_sample;
        let mut decoded = vec![0; stride * samples.height as usize];
        let fmt = match (samples.components, samples.bit_depth) {
            (1, 8) => signinum_j2k::PixelFormat::Gray8,
            (3, 8) => signinum_j2k::PixelFormat::Rgb8,
            (1, 16) => signinum_j2k::PixelFormat::Gray16,
            (3, 16) => signinum_j2k::PixelFormat::Rgb16,
            _ => panic!(
                "unsupported test sample profile: components={} bit_depth={}",
                samples.components, samples.bit_depth
            ),
        };
        decoder
            .decode_into(&mut decoded, stride, fmt)
            .expect("decode encoded J2K");

        assert_eq!(decoded, samples.data);
    }

    #[test]
    fn fhir_bundle_maps_patient_specimen_service_request_and_report() {
        let bundle = serde_json::json!({
            "resourceType": "Bundle",
            "entry": [
                {
                    "resource": {
                        "resourceType": "Patient",
                        "id": "pat-1",
                        "identifier": [{"value": "MRN123"}],
                        "name": [{"family": "Doe", "given": ["Jane", "Q"]}]
                    }
                },
                {
                    "resource": {
                        "resourceType": "Specimen",
                        "identifier": [{"value": "S-42"}],
                        "type": {"text": "colon biopsy"}
                    }
                },
                {
                    "resource": {
                        "resourceType": "ServiceRequest",
                        "identifier": [{"value": "ORDER-7"}],
                        "code": {"text": "Surgical pathology"}
                    }
                },
                {
                    "resource": {
                        "resourceType": "DiagnosticReport",
                        "identifier": [{"value": "DR-9"}],
                        "code": {"text": "Final pathology report"}
                    }
                }
            ]
        });

        let metadata = DicomMetadata::from_fhir_r4_bundle(&bundle).unwrap();

        assert_eq!(metadata.patient_id.as_deref(), Some("MRN123"));
        assert_eq!(metadata.patient_name.as_deref(), Some("Doe^Jane Q"));
        assert_eq!(metadata.specimen_identifier.as_deref(), Some("S-42"));
        assert_eq!(metadata.accession_number.as_deref(), Some("ORDER-7"));
        assert_eq!(
            metadata.study_description.as_deref(),
            Some("Final pathology report")
        );
    }

    #[test]
    fn export_dicom_writes_jpeg2000_lossless_vl_wsi_instances() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.dcm");
        let out = tmp.path().join("out");
        write_source_dicom(&source);

        let report = export_dicom(DicomExportRequest {
            source_path: source,
            output_dir: out.clone(),
            options: DicomExportOptions {
                tile_size: 2,
                encode_backend: EncodeBackendPreference::PreferDevice,
                ..DicomExportOptions::default()
            },
            metadata: MetadataSource::ResearchPlaceholder,
        })
        .unwrap();

        assert_eq!(report.instances.len(), 1);
        assert_eq!(report.instances[0].frame_count, 2);
        assert_eq!(
            report.instances[0].transfer_syntax_uid,
            TransferSyntax::Jpeg2000Lossless.uid()
        );
        assert!(report.instances[0].path.starts_with(&out));

        let object = dicom_object::open_file(&report.instances[0].path).unwrap();
        assert_eq!(
            object.meta().transfer_syntax,
            TransferSyntax::Jpeg2000Lossless.uid()
        );
        assert_eq!(
            object
                .element(tags::SOP_CLASS_UID)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
        );
        assert_eq!(
            object
                .element(tags::DIMENSION_ORGANIZATION_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .as_ref(),
            "TILED_FULL"
        );
        assert_eq!(
            object
                .element(tags::NUMBER_OF_FRAMES)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_COLUMNS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            3
        );
        assert_eq!(
            object
                .element(tags::TOTAL_PIXEL_MATRIX_ROWS)
                .unwrap()
                .to_int::<u32>()
                .unwrap(),
            2
        );
        assert!(object.element(tags::EXTENDED_OFFSET_TABLE).is_ok());
        assert!(object.element(tags::EXTENDED_OFFSET_TABLE_LENGTHS).is_ok());
        assert_eq!(
            object
                .element(tags::PIXEL_DATA)
                .unwrap()
                .value()
                .fragments()
                .unwrap()
                .len(),
            2
        );
    }

    fn write_source_dicom(path: &std::path::Path) {
        let mut object = InMemDicomObject::new_empty();
        object.put(DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE,
        ));
        object.put(DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999.1",
        ));
        object.put(DataElement::new(
            tags::SERIES_INSTANCE_UID,
            VR::UI,
            "1.2.826.0.1.3680043.10.999",
        ));
        object.put(DataElement::new(
            tags::IMAGE_TYPE,
            VR::CS,
            "ORIGINAL\\PRIMARY\\VOLUME\\NONE",
        ));
        object.put(DataElement::new(
            tags::ROWS,
            VR::US,
            PrimitiveValue::from(2u16),
        ));
        object.put(DataElement::new(
            tags::COLUMNS,
            VR::US,
            PrimitiveValue::from(3u16),
        ));
        object.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_ROWS,
            VR::UL,
            PrimitiveValue::from(2u32),
        ));
        object.put(DataElement::new(
            tags::TOTAL_PIXEL_MATRIX_COLUMNS,
            VR::UL,
            PrimitiveValue::from(3u32),
        ));
        object.put(DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(1u32),
        ));
        object.put(DataElement::new(
            tags::SAMPLES_PER_PIXEL,
            VR::US,
            PrimitiveValue::from(3u16),
        ));
        object.put(DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            "RGB",
        ));
        object.put(DataElement::new(
            tags::PLANAR_CONFIGURATION,
            VR::US,
            PrimitiveValue::from(0u16),
        ));
        object.put(DataElement::new(
            tags::BITS_ALLOCATED,
            VR::US,
            PrimitiveValue::from(8u16),
        ));
        object.put(DataElement::new(
            tags::BITS_STORED,
            VR::US,
            PrimitiveValue::from(8u16),
        ));
        object.put(DataElement::new(
            tags::HIGH_BIT,
            VR::US,
            PrimitiveValue::from(7u16),
        ));
        object.put(DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0u16),
        ));
        object.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![
                255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
            ]),
        ));
        object
            .with_meta(
                FileMetaTableBuilder::new()
                    .media_storage_sop_class_uid(uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE)
                    .media_storage_sop_instance_uid("1.2.826.0.1.3680043.10.999.1")
                    .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN),
            )
            .unwrap()
            .write_to_file(path)
            .unwrap();
    }
}
