use std::path::{Path, PathBuf};

use wsi_rs::{PlaneSelection, TileRequest};

use crate::Error;

/// Complete source coordinate for one exported DICOM instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InstanceCoordinate {
    pub(crate) scene_idx: usize,
    pub(crate) series_idx: usize,
    pub(crate) level_idx: u32,
    pub(crate) z: u32,
    pub(crate) c: u32,
    pub(crate) t: u32,
}

impl InstanceCoordinate {
    pub(crate) const fn new(
        scene_idx: usize,
        series_idx: usize,
        level_idx: u32,
        z: u32,
        c: u32,
        t: u32,
    ) -> Self {
        Self {
            scene_idx,
            series_idx,
            level_idx,
            z,
            c,
            t,
        }
    }

    #[cfg(test)]
    pub(crate) const fn first_series_level(level_idx: u32) -> Self {
        Self::new(0, 0, level_idx, 0, 0, 0)
    }

    pub(crate) fn output_path(self, output_dir: &Path) -> PathBuf {
        output_dir.join(format!(
            "scene-{}-series-{}-level-{}-z{}-c{}-t{}.dcm",
            self.scene_idx, self.series_idx, self.level_idx, self.z, self.c, self.t
        ))
    }

    pub(crate) fn series_number(self) -> Result<u32, Error> {
        let one_based = self
            .series_idx
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported {
                reason: "source series index exceeds DICOM series number range".into(),
            })?;
        u32::try_from(one_based).map_err(|_| Error::Unsupported {
            reason: "source series index exceeds DICOM series number range".into(),
        })
    }

    pub(crate) fn tile_request(self, col: i64, row: i64) -> TileRequest {
        TileRequest::new(self.scene_idx, self.series_idx, self.level_idx, col, row)
            .with_plane(PlaneSelection::new(self.z, self.c, self.t))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::InstanceCoordinate;

    #[test]
    fn output_path_contains_every_instance_axis_without_padding_collisions() {
        let coordinate = InstanceCoordinate::new(12, 34, 56, 78, 90, 12345);
        assert_eq!(
            coordinate.output_path(Path::new("out")),
            Path::new("out/scene-12-series-34-level-56-z78-c90-t12345.dcm")
        );
    }

    #[test]
    fn scene_and_series_are_part_of_output_identity() {
        let base = InstanceCoordinate::new(0, 0, 1, 2, 3, 4);
        let other_scene = InstanceCoordinate::new(1, 0, 1, 2, 3, 4);
        let other_series = InstanceCoordinate::new(0, 1, 1, 2, 3, 4);
        assert_ne!(
            base.output_path(Path::new("out")),
            other_scene.output_path(Path::new("out"))
        );
        assert_ne!(
            base.output_path(Path::new("out")),
            other_series.output_path(Path::new("out"))
        );
    }
}
