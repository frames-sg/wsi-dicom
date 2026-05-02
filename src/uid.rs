use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub(crate) fn deterministic_instance_path(
    out_dir: &Path,
    level: u32,
    z: u32,
    c: u32,
    t: u32,
) -> PathBuf {
    out_dir.join(format!("level-{level:04}-z{z:04}-c{c:04}-t{t:04}.dcm"))
}

pub(crate) fn uid_from_seed(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    format!("2.25.{}", u128::from_be_bytes(bytes))
}
