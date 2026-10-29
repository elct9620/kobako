//! Paths beneath a Filesystem's root, components joined by `/` and the
//! empty path being the root: the files a Host App seeds a Tmpfs with.

/// The last name of `path`, within its parent.
pub(crate) fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Every directory above `path`, outermost first, the root left out.
pub(crate) fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(end, _)| &path[..end])
}

/// A path a Host App seeds a Filesystem with, in the form a runtime hands
/// over: no leading `/`, and no empty or `.` component.
pub(crate) fn normalize_seed(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}
