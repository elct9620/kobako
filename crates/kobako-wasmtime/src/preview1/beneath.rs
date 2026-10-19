//! Resolving a guest path beneath a directory descriptor.
//!
//! The runtime is the boundary: a path never climbs above its Mount's
//! root, and the Mount is asked what the guest may do there before the
//! Filesystem is, so a Filesystem of a third party's need check neither.
//! A `..` steps out of what the Filesystem holds rather than out of the
//! path's text, as a host resolves it, so `missing/..` names nothing.

use kobako_runtime::filesystem::{Filesystem, FilesystemError, Kind};

use super::errno::{Answer, Errno};
use crate::descriptors::Directory;

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub(crate) const PATH_MAX: usize = 4096;
pub(crate) const NAME_MAX: usize = 255;

/// A guest path resolved beneath a directory: relative to the mount's
/// root, and whether it ended in `/` or `/.`, which only a directory
/// satisfies.
pub(crate) struct Resolved {
    pub(crate) path: String,
    pub(crate) names_directory: bool,
}

impl Resolved {
    /// The path, refused as `NOTDIR` when it must name a directory and
    /// names a file instead.
    pub(crate) fn directory_checked(self, filesystem: &dyn Filesystem) -> Answer<String> {
        if self.names_directory {
            match filesystem.metadata(&self.path) {
                Ok(metadata) if metadata.kind != Kind::Directory => {
                    return Err(Errno::Notdir.into())
                }
                Ok(_) | Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(self.path)
    }
}

/// `path`, beneath `dir`. An empty path names nothing; an absolute path or
/// a `..` above the root is not the guest's to reach; a `..` after a
/// component that is missing or no directory is `NOENT` or `NOTDIR`. A
/// name past `NAME_MAX`, or a resolved path the Filesystem could not be
/// handed within `PATH_MAX`, is `NAMETOOLONG`.
pub(crate) fn resolve(dir: &Directory, path: &str) -> Answer<Resolved> {
    if path.is_empty() {
        return Err(Errno::Noent.into());
    }
    if path.starts_with('/') {
        return Err(Errno::Notcapable.into());
    }
    let mut components: Vec<&str> = dir.path.split('/').filter(|c| !c.is_empty()).collect();
    let opened = components.len();
    let mut last = "";
    for component in path.split('/') {
        last = component;
        match component {
            "" | "." => {}
            ".." => {
                if components.len() > opened {
                    step_out_of(dir.mount.filesystem().as_ref(), &components.join("/"))?;
                }
                components.pop().ok_or(Errno::Notcapable)?;
            }
            component if component.len() > NAME_MAX => return Err(Errno::Nametoolong.into()),
            component => components.push(component),
        }
    }
    let path = components.join("/");
    if path.len() >= PATH_MAX {
        return Err(Errno::Nametoolong.into());
    }
    Ok(Resolved {
        path,
        names_directory: matches!(last, "" | "."),
    })
}

/// A `..` leaves `path` only when it is a directory the Filesystem holds.
fn step_out_of(filesystem: &dyn Filesystem, path: &str) -> Answer {
    match filesystem.metadata(path) {
        Ok(metadata) if metadata.kind == Kind::Directory => Ok(()),
        Ok(_) => Err(Errno::Notdir.into()),
        Err(err) => Err(err.into()),
    }
}
