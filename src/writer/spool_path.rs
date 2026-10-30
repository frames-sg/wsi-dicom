//! Unique sibling paths for transient pixel-data files.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn unique_spool_path(output_path: &Path) -> PathBuf {
    transient_path(output_path, "pixeldata")
}

pub(super) fn frame_index_spool_path(data_or_output_path: &Path) -> PathBuf {
    transient_path(data_or_output_path, "frameindex")
}

fn transient_path(path: &Path, kind: &str) -> PathBuf {
    let counter = SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("dcm");
    path.with_extension(format!(
        "{extension}.{kind}.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}
