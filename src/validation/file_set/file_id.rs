//! File IDs (PS3.10 8.2 and 8.5) and their resolution inside the File-set
//! root. A File ID that would leave the root, by a parent or absolute
//! component or through a symbolic link, is refused before any file it names
//! is opened: the DICOMDIR is untrusted input.

use std::path::{Component, Path, PathBuf};

use dicom_core::Tag;

use crate::validation::disclosure::shown;

/// Referenced File ID (0004,1500), whose Components messages show as the
/// File-set Descriptor File ID's (0004,1141): Table E.1-1 lists neither.
const FILE_ID: Tag = Tag(0x0004, 0x1500);

/// PS3.10 8.2: at most eight Components per File ID.
const MAX_COMPONENTS: usize = 8;
/// PS3.10 8.2: one to eight characters per Component.
const MAX_COMPONENT_CHARS: usize = 8;

/// PS3.10 8.5: uppercase A-Z, 0-9 and underscore.
pub(super) fn in_repertoire(character: char) -> bool {
    character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
}

/// The PS3.10 8.2 and 8.5 defects of `components`, each a sentence.
pub(super) fn defects(components: &[String]) -> Vec<String> {
    let mut defects = Vec::new();
    if components.is_empty() || components.len() > MAX_COMPONENTS {
        defects.push(format!(
            "has {} Components, but a File ID has one to {MAX_COMPONENTS} (PS3.10 8.2)",
            components.len()
        ));
    }
    for component in components {
        let length = component.chars().count();
        if length == 0 || length > MAX_COMPONENT_CHARS {
            defects.push(format!(
                "Component {} has {length} characters, but a Component has one to \
                 {MAX_COMPONENT_CHARS} (PS3.10 8.2)",
                shown(FILE_ID, component)
            ));
        }
        let outside = component
            .chars()
            .enumerate()
            .find(|(_, c)| !in_repertoire(*c));
        if let Some((index, character)) = outside {
            let shown = shown(FILE_ID, component);
            defects.push(format!(
                "Component {shown} contains {}, outside the File ID repertoire of uppercase \
                 A-Z, 0-9 and _ (PS3.10 8.5)",
                shown.character(character, index + 1)
            ));
        }
    }
    defects
}

/// Why a File ID does not name a regular file inside the root. `Escape` is a
/// security finding: the File ID or a link leads outside the File-set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unresolved {
    Escape(String),
    Missing(String),
}

impl Unresolved {
    pub(super) fn message(&self) -> &str {
        match self {
            Self::Escape(message) | Self::Missing(message) => message,
        }
    }
}

/// The canonical path of the regular file `components` names under `root`,
/// whose canonical form is `canonical_root`. Each Component must be one
/// plain path segment, each intermediate a directory and the last a regular
/// file, none of them a symbolic link, and the canonical result must lie
/// inside the root.
pub(super) fn resolve(
    root: &Path,
    canonical_root: &Path,
    components: &[String],
) -> Result<PathBuf, Unresolved> {
    if components.is_empty() {
        return Err(Unresolved::Missing("names no file".into()));
    }
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        if !is_plain_segment(component) {
            return Err(Unresolved::Escape(format!(
                "refused: Component {} is not a plain path segment, so the File ID could \
                 leave the File-set root (path traversal; PS3.10 8.6 forbids referencing Files \
                 outside the File-set)",
                shown(FILE_ID, component)
            )));
        }
        path.push(component);
        let last = index + 1 == components.len();
        let metadata = std::fs::symlink_metadata(&path).map_err(|err| {
            Unresolved::Missing(format!(
                "names {}, which cannot be read: {err}",
                path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Unresolved::Escape(format!(
                "refused: {} is a symbolic link, which could lead outside the File-set root \
                 (PS3.10 8.6 forbids referencing Files outside the File-set)",
                path.display()
            )));
        }
        if !last && !metadata.is_dir() {
            return Err(Unresolved::Missing(format!(
                "names {} as a directory, but it is not one",
                path.display()
            )));
        }
        if last && !metadata.is_file() {
            return Err(Unresolved::Missing(format!(
                "names {}, which is not a regular file",
                path.display()
            )));
        }
    }
    let canonical = path.canonicalize().map_err(|err| {
        Unresolved::Missing(format!(
            "names {}, which cannot be resolved: {err}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(canonical_root) {
        return Err(Unresolved::Escape(format!(
            "refused: {} resolves to {}, outside the File-set root {} (PS3.10 8.6)",
            path.display(),
            canonical.display(),
            canonical_root.display()
        )));
    }
    Ok(canonical)
}

/// Whether `component` is exactly one normal path segment: not empty, not
/// `.` or `..`, without a separator, drive prefix or NUL.
fn is_plain_segment(component: &str) -> bool {
    if component.is_empty() || component.contains(['/', '\\', '\0', ':']) {
        return false;
    }
    let mut components = Path::new(component).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}
