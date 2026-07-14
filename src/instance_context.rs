use std::path::{Path, PathBuf};

use dicom_object::{FileMetaTableBuilder, InMemDicomObject};

use crate::coordinate::InstanceCoordinate;
use crate::metadata::DicomMetadata;
use crate::report::{ExportMetrics, IccProfileSource, InstanceReport};
use crate::tile::PixelProfile;
use crate::uid::DicomExportIdentity;
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
    pub(crate) coordinate: InstanceCoordinate,
}

impl DicomInstanceContext {
    pub(crate) fn new(
        identity: &DicomExportIdentity,
        output_dir: &Path,
        pixel_spacing_mm: (f64, f64),
        coordinate: InstanceCoordinate,
    ) -> Result<Self, Error> {
        Ok(Self {
            path: coordinate.output_path(output_dir),
            series_uid: identity.uid(&format!(
                "series:{}:{}:{}:{}:{}",
                coordinate.scene_idx,
                coordinate.series_idx,
                coordinate.z,
                coordinate.c,
                coordinate.t
            )),
            sop_instance_uid: identity.uid(&format!(
                "instance:{}:{}:{}:{}:{}:{}",
                coordinate.scene_idx,
                coordinate.series_idx,
                coordinate.level_idx,
                coordinate.z,
                coordinate.c,
                coordinate.t
            )),
            frame_of_reference_uid: identity.uid(&format!(
                "frame-of-reference:{}:{}",
                coordinate.scene_idx, coordinate.series_idx
            )),
            pyramid_uid: identity.uid(&format!(
                "pyramid:{}:{}:{}:{}:{}",
                coordinate.scene_idx,
                coordinate.series_idx,
                coordinate.z,
                coordinate.c,
                coordinate.t
            )),
            dimension_organization_uid: identity.uid(&format!(
                "dimension-organization:{}:{}:{}:{}:{}",
                coordinate.scene_idx,
                coordinate.series_idx,
                coordinate.z,
                coordinate.c,
                coordinate.t
            )),
            pyramid_label: format!(
                "WSI pyramid s{} ser{} z{} c{} t{}",
                coordinate.scene_idx,
                coordinate.series_idx,
                coordinate.z,
                coordinate.c,
                coordinate.t
            ),
            pixel_spacing_mm,
            series_number: coordinate.series_number()?,
            coordinate,
        })
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
            level_idx: self.coordinate.level_idx,
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
            scene: self.coordinate.scene_idx,
            series: self.coordinate.series_idx,
            level: self.coordinate.level_idx,
            z: self.coordinate.z,
            c: self.coordinate.c,
            t: self.coordinate.t,
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
    use crate::coordinate::InstanceCoordinate;
    use crate::options::ExportOptions;
    use crate::report::{ExportMetrics, IccProfileSource, InstanceReport};
    use crate::uid::DicomExportIdentity;

    #[test]
    fn dicom_instance_context_captures_stable_ids_paths_and_report_fields() {
        let output = PathBuf::from("/tmp/out");
        let coordinate = InstanceCoordinate::new(1, 2, 3, 4, 5, 6);
        let identity = DicomExportIdentity::from_seed("1.2.3".into(), "test-seed".into());
        let context =
            DicomInstanceContext::new(&identity, &output, (0.0005, 0.0005), coordinate).unwrap();

        assert_eq!(context.path, coordinate.output_path(&output));
        let actual_uids = [
            context.series_uid.as_str(),
            context.sop_instance_uid.as_str(),
            context.frame_of_reference_uid.as_str(),
            context.pyramid_uid.as_str(),
            context.dimension_organization_uid.as_str(),
        ];
        let expected_uids = [
            identity.uid("series:1:2:4:5:6"),
            identity.uid("instance:1:2:3:4:5:6"),
            identity.uid("frame-of-reference:1:2"),
            identity.uid("pyramid:1:2:4:5:6"),
            identity.uid("dimension-organization:1:2:4:5:6"),
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
                scene: 1,
                series: 2,
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
        let output = PathBuf::from("/tmp/out");
        let identity = DicomExportIdentity::from_seed("1.2.3".into(), "test-seed".into());
        let t0 = DicomInstanceContext::new(
            &identity,
            &output,
            (0.0005, 0.0005),
            InstanceCoordinate::new(0, 0, 0, 0, 0, 0),
        )
        .unwrap();
        let t1 = DicomInstanceContext::new(
            &identity,
            &output,
            (0.0005, 0.0005),
            InstanceCoordinate::new(0, 0, 0, 0, 0, 1),
        )
        .unwrap();

        assert_ne!(t0.sop_instance_uid, t1.sop_instance_uid);
        assert_ne!(t0.path, t1.path);
    }
}
