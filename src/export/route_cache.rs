use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::routing::transfer_syntax_from_uid;
use crate::{Error, TransferSyntax};

pub(super) const WSI_DICOM_AUTO_ROUTE_CACHE_ENV: &str = "WSI_DICOM_AUTO_ROUTE_CACHE";
const ROUTE_CACHE_JSON_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AutoLosslessJ2kRouteDecision {
    Undecided,
    CpuOnly,
    CpuInputDeviceEncode,
    GpuInputDeviceEncode,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct AutoMetalInputRouteCacheKey {
    pub(super) source_path: PathBuf,
    pub(super) scene_idx: usize,
    pub(super) series_idx: usize,
    pub(super) level: u32,
    pub(super) z: u32,
    pub(super) c: u32,
    pub(super) t: u32,
    pub(super) tile_size: u32,
    pub(super) transfer_syntax: TransferSyntax,
    pub(super) route_scope_frames: u64,
}

#[derive(Debug, Default)]
struct AutoMetalInputRouteCache {
    entries: HashMap<AutoMetalInputRouteCacheKey, AutoLosslessJ2kRouteDecision>,
    loaded_path: Option<PathBuf>,
    dirty: bool,
}

static AUTO_METAL_INPUT_ROUTE_CACHE: OnceLock<Mutex<AutoMetalInputRouteCache>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct PersistentAutoMetalInputRouteCacheEntry {
    source_path: PathBuf,
    #[serde(default)]
    scene_idx: usize,
    #[serde(default)]
    series_idx: usize,
    level: u32,
    #[serde(default)]
    z: u32,
    #[serde(default)]
    c: u32,
    #[serde(default)]
    t: u32,
    tile_size: u32,
    transfer_syntax_uid: String,
    #[serde(default)]
    route_scope_frames: u64,
    #[serde(default)]
    route: Option<AutoLosslessJ2kRouteDecision>,
}

fn auto_metal_input_route_cache() -> &'static Mutex<AutoMetalInputRouteCache> {
    AUTO_METAL_INPUT_ROUTE_CACHE.get_or_init(|| Mutex::new(AutoMetalInputRouteCache::default()))
}

pub(super) fn cached_auto_metal_input_decision(
    key: &AutoMetalInputRouteCacheKey,
) -> Option<AutoLosslessJ2kRouteDecision> {
    match auto_metal_input_route_cache().lock() {
        Ok(cache) => cache.entries.get(key).copied(),
        Err(_) => {
            eprintln!("wsi-dicom: auto Metal input route cache mutex is poisoned");
            None
        }
    }
}

pub(super) fn store_cached_auto_metal_input_decision(
    key: &AutoMetalInputRouteCacheKey,
    route: AutoLosslessJ2kRouteDecision,
) {
    if route == AutoLosslessJ2kRouteDecision::Undecided {
        return;
    }
    match auto_metal_input_route_cache().lock() {
        Ok(mut cache) => {
            cache.entries.insert(key.clone(), route);
            cache.dirty = true;
        }
        Err(_) => {
            eprintln!("wsi-dicom: auto Metal input route cache state mutex is poisoned");
        }
    }
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
pub(super) fn clear_auto_metal_input_route_cache_for_tests() {
    auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache mutex poisoned")
        .entries
        .clear();
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
pub(super) fn clear_auto_metal_input_route_cache_state_for_tests() {
    *auto_metal_input_route_cache()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned") =
        AutoMetalInputRouteCache::default();
}

fn persistent_auto_metal_input_route_cache_path() -> Option<PathBuf> {
    std::env::var_os(WSI_DICOM_AUTO_ROUTE_CACHE_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

pub(super) fn load_persistent_auto_metal_input_route_cache_if_requested() -> Result<(), Error> {
    let Some(path) = persistent_auto_metal_input_route_cache_path() else {
        return Ok(());
    };
    let mut cache = auto_metal_input_route_cache()
        .lock()
        .map_err(|_| Error::Unsupported {
            reason: "auto Metal input route cache mutex is poisoned".into(),
        })?;
    if cache.loaded_path.as_ref() == Some(&path) {
        return Ok(());
    }
    if cache.dirty {
        return Err(Error::Unsupported {
            reason: "auto Metal input route cache path changed while unsaved decisions remain"
                .into(),
        });
    }

    let bytes = match read_route_cache_file_capped(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => {
            return Err(Error::Io { path, source });
        }
    };

    let mut loaded_entries = HashMap::new();
    if !bytes.is_empty() {
        let entries: Vec<PersistentAutoMetalInputRouteCacheEntry> = serde_json::from_slice(&bytes)
            .map_err(|source| Error::Json {
                path: path.clone(),
                source,
            })?;
        for entry in entries {
            let Some(route) = entry
                .route
                .filter(|route| *route != AutoLosslessJ2kRouteDecision::Undecided)
            else {
                continue;
            };
            let transfer_syntax =
                transfer_syntax_from_uid(&entry.transfer_syntax_uid).ok_or_else(|| {
                    Error::Unsupported {
                        reason: format!(
                            "auto route cache {} contains unsupported transfer syntax UID {}",
                            path.display(),
                            entry.transfer_syntax_uid
                        ),
                    }
                })?;
            loaded_entries.insert(
                AutoMetalInputRouteCacheKey {
                    source_path: entry.source_path,
                    scene_idx: entry.scene_idx,
                    series_idx: entry.series_idx,
                    level: entry.level,
                    z: entry.z,
                    c: entry.c,
                    t: entry.t,
                    tile_size: entry.tile_size,
                    transfer_syntax,
                    route_scope_frames: entry.route_scope_frames,
                },
                route,
            );
        }
    }

    cache.entries = loaded_entries;
    cache.loaded_path = Some(path);
    cache.dirty = false;
    Ok(())
}

pub(super) fn flush_persistent_auto_metal_input_route_cache_if_requested() -> Result<(), Error> {
    let Some(path) = persistent_auto_metal_input_route_cache_path() else {
        return Ok(());
    };
    let mut cache = auto_metal_input_route_cache()
        .lock()
        .map_err(|_| Error::Unsupported {
            reason: "auto Metal input route cache mutex is poisoned".into(),
        })?;
    if !cache.dirty && cache.loaded_path.as_ref() == Some(&path) {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }

    reject_symlink_route_cache_path(&path)?;

    let mut entries: Vec<_> = cache
        .entries
        .iter()
        .map(|(key, route)| PersistentAutoMetalInputRouteCacheEntry {
            source_path: key.source_path.clone(),
            scene_idx: key.scene_idx,
            series_idx: key.series_idx,
            level: key.level,
            z: key.z,
            c: key.c,
            t: key.t,
            tile_size: key.tile_size,
            transfer_syntax_uid: key.transfer_syntax.uid().to_string(),
            route_scope_frames: key.route_scope_frames,
            route: Some(*route),
        })
        .collect();
    entries.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then(left.scene_idx.cmp(&right.scene_idx))
            .then(left.series_idx.cmp(&right.series_idx))
            .then(left.level.cmp(&right.level))
            .then(left.z.cmp(&right.z))
            .then(left.c.cmp(&right.c))
            .then(left.t.cmp(&right.t))
            .then(left.tile_size.cmp(&right.tile_size))
            .then(left.transfer_syntax_uid.cmp(&right.transfer_syntax_uid))
            .then(left.route_scope_frames.cmp(&right.route_scope_frames))
    });
    let bytes = serde_json::to_vec_pretty(&entries).map_err(|source| Error::JsonSerialize {
        message: format!("auto route cache serialization failed: {source}"),
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ROUTE_CACHE_JSON_MAX_BYTES {
        return Err(Error::Unsupported {
            reason: format!(
                "auto route cache serialization exceeds {ROUTE_CACHE_JSON_MAX_BYTES} byte limit"
            ),
        });
    }
    atomic_write_route_cache(&path, &bytes)?;

    cache.loaded_path = Some(path);
    cache.dirty = false;
    Ok(())
}

fn reject_symlink_route_cache_path(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Io {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "auto route cache path must not be a symbolic link",
            ),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn atomic_write_route_cache(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".wsi-dicom-route-cache-")
        .tempfile_in(parent)
        .map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temporary.write_all(bytes).map_err(|source| Error::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.flush().map_err(|source| Error::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.as_file().sync_all().map_err(|source| Error::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.persist(path).map_err(|error| Error::Io {
        path: path.to_path_buf(),
        source: error.error,
    })?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    Ok(())
}

fn read_route_cache_file_capped(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    use std::io::Read;

    let file = fs::File::open(path)?;
    let mut limited = file.take(ROUTE_CACHE_JSON_MAX_BYTES.saturating_add(1));
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > ROUTE_CACHE_JSON_MAX_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "auto route cache exceeds {} byte limit",
                ROUTE_CACHE_JSON_MAX_BYTES
            ),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::reject_symlink_route_cache_path;

    #[cfg(unix)]
    #[test]
    fn route_cache_path_rejects_symlinks_without_touching_the_target() {
        let temp = tempfile::tempdir().expect("create temporary directory");
        let target = temp.path().join("target.json");
        let link = temp.path().join("cache.json");
        std::fs::write(&target, b"trusted").expect("write target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let error = reject_symlink_route_cache_path(&link).expect_err("reject symlink");

        assert!(error.to_string().contains("symbolic link"));
        assert_eq!(std::fs::read(&target).expect("read target"), b"trusted");
        assert!(std::fs::symlink_metadata(&link)
            .expect("read symlink metadata")
            .file_type()
            .is_symlink());
    }
}
