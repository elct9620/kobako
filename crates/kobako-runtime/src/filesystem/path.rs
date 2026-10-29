//! Paths beneath a Filesystem's root, components joined by `/` and the
//! empty path being the root: the files a Host App seeds a Tmpfs with,
//! and the positions an Overlay keeps its own records by.

/// The directory holding `path`; the root holds every top-level entry.
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The last name of `path`, within its parent.
pub(crate) fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The path of `name` within the directory `dir`; an empty `name` is `dir`
/// itself.
pub(crate) fn join(dir: &str, name: &str) -> String {
    match (dir, name) {
        ("", name) => name.to_string(),
        (dir, "") => dir.to_string(),
        (dir, name) => format!("{dir}/{name}"),
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

/// A path a Host App seeds a Filesystem with, in the form a runtime hands
/// over: no leading `/`, and no empty or `.` component.
pub(crate) fn normalize_seed(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}
