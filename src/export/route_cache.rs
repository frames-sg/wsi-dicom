use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
    pub(super) level: u32,
    pub(super) tile_size: u32,
    pub(super) transfer_syntax: TransferSyntax,
    pub(super) route_scope_frames: u64,
}

static AUTO_METAL_INPUT_ROUTE_CACHE: OnceLock<
    Mutex<HashMap<AutoMetalInputRouteCacheKey, AutoLosslessJ2kRouteDecision>>,
> = OnceLock::new();

#[derive(Debug, Default)]
struct AutoMetalInputRouteCacheState {
    loaded_path: Option<PathBuf>,
    dirty: bool,
}

static AUTO_METAL_INPUT_ROUTE_CACHE_STATE: OnceLock<Mutex<AutoMetalInputRouteCacheState>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct PersistentAutoMetalInputRouteCacheEntry {
    source_path: PathBuf,
    level: u32,
    tile_size: u32,
    transfer_syntax_uid: String,
    #[serde(default)]
    route_scope_frames: u64,
    #[serde(default)]
    route: Option<AutoLosslessJ2kRouteDecision>,
}

fn auto_metal_input_route_cache(
) -> &'static Mutex<HashMap<AutoMetalInputRouteCacheKey, AutoLosslessJ2kRouteDecision>> {
    AUTO_METAL_INPUT_ROUTE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn auto_metal_input_route_cache_state() -> &'static Mutex<AutoMetalInputRouteCacheState> {
    AUTO_METAL_INPUT_ROUTE_CACHE_STATE
        .get_or_init(|| Mutex::new(AutoMetalInputRouteCacheState::default()))
}

pub(super) fn cached_auto_metal_input_decision(
    key: &AutoMetalInputRouteCacheKey,
) -> Option<AutoLosslessJ2kRouteDecision> {
    match auto_metal_input_route_cache().lock() {
        Ok(cache) => cache.get(key).copied(),
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
            cache.insert(key.clone(), route);
        }
        Err(_) => {
            eprintln!("wsi-dicom: auto Metal input route cache mutex is poisoned");
            return;
        }
    }
    match auto_metal_input_route_cache_state().lock() {
        Ok(mut state) => {
            state.dirty = true;
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
        .clear();
}

#[cfg(all(test, feature = "metal", target_os = "macos"))]
pub(super) fn clear_auto_metal_input_route_cache_state_for_tests() {
    *auto_metal_input_route_cache_state()
        .lock()
        .expect("auto Metal input route cache state mutex poisoned") =
        AutoMetalInputRouteCacheState::default();
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
    {
        let state =
            auto_metal_input_route_cache_state()
                .lock()
                .map_err(|_| Error::Unsupported {
                    reason: "auto Metal input route cache state mutex is poisoned".into(),
                })?;
        if state.loaded_path.as_ref() == Some(&path) {
            return Ok(());
        }
    }

    let bytes = match read_route_cache_file_capped(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => {
            return Err(Error::Io { path, source });
        }
    };

    if !bytes.is_empty() {
        let entries: Vec<PersistentAutoMetalInputRouteCacheEntry> = serde_json::from_slice(&bytes)
            .map_err(|source| Error::Json {
                path: path.clone(),
                source,
            })?;
        let mut cache = auto_metal_input_route_cache()
            .lock()
            .map_err(|_| Error::Unsupported {
                reason: "auto Metal input route cache mutex is poisoned".into(),
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
            cache.insert(
                AutoMetalInputRouteCacheKey {
                    source_path: entry.source_path,
                    level: entry.level,
                    tile_size: entry.tile_size,
                    transfer_syntax,
                    route_scope_frames: entry.route_scope_frames,
                },
                route,
            );
        }
    }

    let mut state =
        auto_metal_input_route_cache_state()
            .lock()
            .map_err(|_| Error::Unsupported {
                reason: "auto Metal input route cache state mutex is poisoned".into(),
            })?;
    state.loaded_path = Some(path);
    state.dirty = false;
    Ok(())
}

pub(super) fn flush_persistent_auto_metal_input_route_cache_if_requested() -> Result<(), Error> {
    let Some(path) = persistent_auto_metal_input_route_cache_path() else {
        return Ok(());
    };
    {
        let state =
            auto_metal_input_route_cache_state()
                .lock()
                .map_err(|_| Error::Unsupported {
                    reason: "auto Metal input route cache state mutex is poisoned".into(),
                })?;
        if !state.dirty && state.loaded_path.as_ref() == Some(&path) {
            return Ok(());
        }
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }

    let mut entries: Vec<_> = auto_metal_input_route_cache()
        .lock()
        .map_err(|_| Error::Unsupported {
            reason: "auto Metal input route cache mutex is poisoned".into(),
        })?
        .iter()
        .map(|(key, route)| PersistentAutoMetalInputRouteCacheEntry {
            source_path: key.source_path.clone(),
            level: key.level,
            tile_size: key.tile_size,
            transfer_syntax_uid: key.transfer_syntax.uid().to_string(),
            route_scope_frames: key.route_scope_frames,
            route: Some(*route),
        })
        .collect();
    entries.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then(left.level.cmp(&right.level))
            .then(left.tile_size.cmp(&right.tile_size))
            .then(left.transfer_syntax_uid.cmp(&right.transfer_syntax_uid))
            .then(left.route_scope_frames.cmp(&right.route_scope_frames))
    });
    let bytes = serde_json::to_vec_pretty(&entries).map_err(|source| Error::JsonSerialize {
        message: format!("auto route cache serialization failed: {source}"),
    })?;
    fs::write(&path, bytes).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;

    let mut state =
        auto_metal_input_route_cache_state()
            .lock()
            .map_err(|_| Error::Unsupported {
                reason: "auto Metal input route cache state mutex is poisoned".into(),
            })?;
    state.loaded_path = Some(path);
    state.dirty = false;
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
