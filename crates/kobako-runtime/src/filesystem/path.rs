//! Paths as a Filesystem receives them: relative to its root, components
//! joined by `/`, the empty path being the root.

use super::{FileType, FilesystemError};

/// The directory holding `path`; the root holds every top-level entry.
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The last name of `path`, within its parent.
pub(crate) fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The path of `name` within the directory `dir`.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The names of `path`, outermost first; the root has none.
pub(crate) fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|name| !name.is_empty())
}

/// Every directory above `path`, outermost first, the root left out.
pub(crate) fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(end, _)| &path[..end])
}

/// Whether `path` lies beneath `dir`, `dir` itself left out.
pub(crate) fn is_beneath(path: &str, dir: &str) -> bool {
    dir.is_empty() && !path.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Why an entry of `file_type` cannot move beneath itself: a directory
/// cannot hold itself, and a file stands where the new path needs a
/// directory, as `rename(2)` answers each.
pub(crate) fn beneath_itself(file_type: FileType) -> FilesystemError {
    match file_type {
        FileType::Directory => FilesystemError::InvalidInput,
        _ => FilesystemError::NotADirectory,
    }
}

/// A path a Host App seeds a Filesystem with, in the form a runtime hands
/// over: no leading `/`, and no empty or `.` component.
pub(crate) fn normalize_seed(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}
