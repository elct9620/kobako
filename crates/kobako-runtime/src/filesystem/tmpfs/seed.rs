//! The paths a Host App seeds a Tmpfs with, components joined by `/` and the
//! empty path being the root.

use super::SeedError;

/// The last name of `path`, within its parent.
pub(super) fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Every directory above `path`, outermost first, the root left out.
pub(super) fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(end, _)| &path[..end])
}

/// `path` in the form a Tmpfs holds it: no leading `/`, and no empty or
/// `.` component. The root, and a `..` that would leave the Tmpfs, are
/// refused.
pub(super) fn normalize(path: &str) -> Result<String, SeedError> {
    let path = path
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        return Err(SeedError::Root);
    }
    if path.split('/').any(|name| name == "..") {
        return Err(SeedError::DotDot(path));
    }
    Ok(path)
}
