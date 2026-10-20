//! Paths as a Filesystem receives them: relative to its root, components
//! joined by `/`, the empty path being the root.

use super::{FilesystemError, Kind};

/// The directory holding `path`; the root holds every top-level entry.
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
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

/// Why an entry of `kind` cannot move beneath itself: a directory cannot
/// hold itself, and a file stands where the new path needs a directory,
/// as `rename(2)` answers each.
pub(crate) fn beneath_itself(kind: Kind) -> FilesystemError {
    match kind {
        Kind::Directory => FilesystemError::InvalidInput,
        _ => FilesystemError::NotADirectory,
    }
}

/// A path a Host App seeds a Filesystem with, in the form a runtime hands
/// over: no leading `/`, and no empty or `.` component.
pub(crate) fn seeded(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}
