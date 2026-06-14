use std::path::{Path, PathBuf};

use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::metadata::DicomMetadata;
use crate::report::{ExportMetrics, IccProfileSource, InstanceReport};
use crate::tile::PixelProfile;
use crate::uid::{deterministic_instance_path, uid_from_seed};
use crate::writer::{
    build_dicom_object, DicomObjectIdentifiers, DicomObjectParams, FrameGrid,
    LossyCompressionMetadata, PixelDataOffsetTables,
};
use crate::{Error, VL_WSI_SOP_CLASS_UID};

pub(crate) struct DicomInstanceContext {
    pub(crate) path: PathBuf,
    pub(crate) series_uid: String,
    pub(crate) sop_instance_uid: String,
    pub(crate) frame_of_reference_uid: String,
    pub(crate) pyramid_uid: String,
    pub(crate) dimension_organization_uid: String,
    pub(crate) pyramid_label: String,
    pub(crate) pixel_spacing_mm: (f64, f64),
    pub(crate) series_number: u32,
    pub(crate) level_idx: u32,
    pub(crate) z: u32,
    pub(crate) c: u32,
    pub(crate) t: u32,
}

impl DicomInstanceContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source_path: &Path,
        output_dir: &Path,
        pixel_spacing_mm: (f64, f64),
        scene_idx: usize,
        series_idx: usize,
        level_idx: u32,
        z: u32,
        c: u32,
        t: u32,
    ) -> Self {
        Self {
            path: deterministic_instance_path(output_dir, level_idx, z, c, t),
            series_uid: uid_from_seed(&format!(
                "series:{}:{}:{}:{}:{}:{}",
                source_path.display(),
                scene_idx,
                series_idx,
                z,
                c,
                t
            )),
            sop_instance_uid: uid_from_seed(&format!(
                "instance:{}:{}:{}:{}:{}:{}:{}",
                source_path.display(),
                scene_idx,
                series_idx,
                level_idx,
                z,
                c,
                t
            )),
            frame_of_reference_uid: uid_from_seed(&format!(
                "frame-of-reference:{}:{}:{}",
                source_path.display(),
                scene_idx,
                series_idx
            )),
            pyramid_uid: uid_from_seed(&format!(
                "pyramid:{}:{}:{}:{}:{}:{}",
                source_path.display(),
                scene_idx,
                series_idx,
                z,
                c,
                t
            )),
            dimension_organization_uid: uid_from_seed(&format!(
                "dimension-organization:{}:{}:{}:{}:{}:{}",
                source_path.display(),
                scene_idx,
                series_idx,
                z,
                c,
                t
            )),
            pyramid_label: format!("WSI pyramid s{scene_idx} ser{series_idx} z{z} c{c} t{t}"),
            pixel_spacing_mm,
            series_number: (series_idx + 1) as u32,
            level_idx,
            z,
            c,
            t,
        }
    }

    pub(crate) fn build_dicom_object(
        &self,
        params: InstanceDicomObjectParams<'_>,
    ) -> Result<InMemDicomObject, Error> {
        build_dicom_object(DicomObjectParams {
            metadata: params.metadata,
            identifiers: DicomObjectIdentifiers {
                study_uid: params.study_uid,
                series_uid: &self.series_uid,
                sop_instance_uid: &self.sop_instance_uid,
                frame_of_reference_uid: &self.frame_of_reference_uid,
                pyramid_uid: &self.pyramid_uid,
                dimension_organization_uid: &self.dimension_organization_uid,
                pyramid_label: &self.pyramid_label,
            },
            series_number: self.series_number,
            instance_number: params.instance_number,
            level_idx: self.level_idx,
            frame_grid: params.frame_grid,
            frame_count: params.frame_count,
            profile: params.profile,
            pixel_spacing_mm: Some(self.pixel_spacing_mm),
            pixel_data_offsets: params.pixel_data_offsets,
            icc_profile: params.icc_profile,
            lossy_compression: params.lossy_compression,
        })
    }

    pub(crate) fn file_meta(&self, transfer_syntax_uid: &'static str) -> FileMetaTableBuilder {
        FileMetaTableBuilder::new()
            .media_storage_sop_class_uid(VL_WSI_SOP_CLASS_UID)
            .media_storage_sop_instance_uid(&self.sop_instance_uid)
            .transfer_syntax(transfer_syntax_uid)
    }

    pub(crate) fn report(
        &self,
        transfer_syntax_uid: &'static str,
        frame_count: u32,
        icc_profile_source: IccProfileSource,
        metrics: ExportMetrics,
    ) -> InstanceReport {
        InstanceReport {
            path: self.path.clone(),
            sop_instance_uid: self.sop_instance_uid.clone(),
            series_instance_uid: self.series_uid.clone(),
            transfer_syntax_uid,
            icc_profile_source,
            level: self.level_idx,
            z: self.z,
            c: self.c,
            t: self.t,
            frame_count,
            metrics,
        }
    }
}

pub(crate) struct InstanceDicomObjectParams<'a> {
    pub(crate) metadata: &'a DicomMetadata,
    pub(crate) study_uid: &'a str,
    pub(crate) instance_number: u32,
    pub(crate) frame_grid: FrameGrid,
    pub(crate) frame_count: u32,
    pub(crate) profile: PixelProfile,
    pub(crate) pixel_data_offsets: PixelDataOffsetTables,
    pub(crate) icc_profile: Option<&'a [u8]>,
    pub(crate) lossy_compression: Option<LossyCompressionMetadata>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::DicomInstanceContext;
    use crate::options::ExportOptions;
    use crate::report::{ExportMetrics, IccProfileSource, InstanceReport};
    use crate::uid::{deterministic_instance_path, uid_from_seed};

    #[test]
    fn dicom_instance_context_captures_stable_ids_paths_and_report_fields() {
        let source = PathBuf::from("/tmp/source.dcm");
        let output = PathBuf::from("/tmp/out");
        let context =
            DicomInstanceContext::new(&source, &output, (0.0005, 0.0005), 1, 2, 3, 4, 5, 6);

        assert_eq!(
            context.path,
            deterministic_instance_path(&output, 3, 4, 5, 6)
        );
        let actual_uids = [
            context.series_uid.as_str(),
            context.sop_instance_uid.as_str(),
            context.frame_of_reference_uid.as_str(),
            context.pyramid_uid.as_str(),
            context.dimension_organization_uid.as_str(),
        ];
        let expected_uids = [
            uid_from_seed("series:/tmp/source.dcm:1:2:4:5:6"),
            uid_from_seed("instance:/tmp/source.dcm:1:2:3:4:5:6"),
            uid_from_seed("frame-of-reference:/tmp/source.dcm:1:2"),
            uid_from_seed("pyramid:/tmp/source.dcm:1:2:4:5:6"),
            uid_from_seed("dimension-organization:/tmp/source.dcm:1:2:4:5:6"),
        ];
        assert_eq!(actual_uids, expected_uids.each_ref().map(String::as_str));
        assert_eq!(context.pyramid_label, "WSI pyramid s1 ser2 z4 c5 t6");
        assert_eq!(context.pixel_spacing_mm, (0.0005, 0.0005));
        assert_eq!(context.series_number, 3);

        let report = context.report(
            ExportOptions::default().transfer_syntax.uid(),
            7,
            IccProfileSource::SynthesizedSrgb,
            ExportMetrics::default(),
        );
        assert_eq!(
            report,
            InstanceReport {
                path: context.path,
                sop_instance_uid: context.sop_instance_uid,
                series_instance_uid: context.series_uid,
                transfer_syntax_uid: ExportOptions::default().transfer_syntax.uid(),
                icc_profile_source: IccProfileSource::SynthesizedSrgb,
                level: 3,
                z: 4,
                c: 5,
                t: 6,
                frame_count: 7,
                metrics: ExportMetrics::default(),
            }
        );
    }

    #[test]
    fn dicom_instance_context_uid_changes_when_only_timepoint_changes() {
        let source = PathBuf::from("/tmp/source.dcm");
        let output = PathBuf::from("/tmp/out");
        let t0 = DicomInstanceContext::new(&source, &output, (0.0005, 0.0005), 0, 0, 0, 0, 0, 0);
        let t1 = DicomInstanceContext::new(&source, &output, (0.0005, 0.0005), 0, 0, 0, 0, 0, 1);

        assert_ne!(t0.sop_instance_uid, t1.sop_instance_uid);
        assert_ne!(t0.path, t1.path);
    }
}
