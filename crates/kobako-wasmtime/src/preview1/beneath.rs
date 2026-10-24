//! Resolving a guest path beneath a directory descriptor.
//!
//! The runtime is the boundary: a path never climbs above the directory
//! descriptor it is resolved beneath, as a capability never widens, and
//! the Mount is asked what the guest may do there before the Filesystem
//! is, so a Filesystem of a third party's need check neither.
//! A `..` steps out of what the Filesystem holds rather than out of the
//! path's text, as a host resolves it, so `missing/..` names nothing.

use kobako_runtime::filesystem::{FileType, Filesystem, FilesystemError};

use super::errno::{Answer, Errno};
use crate::descriptors::OpenDirectory;

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub(crate) const PATH_MAX: usize = 4096;
pub(crate) const NAME_MAX: usize = 255;

/// A guest path resolved beneath a directory: relative to the mount's
/// root, and whether it ended in `/` or `/.`, which only a directory
/// satisfies.
pub(crate) struct ResolvedPath {
    pub(crate) path: String,
    pub(crate) names_directory: bool,
}

impl ResolvedPath {
    /// The path, refused as `NOTDIR` when it must name a directory and
    /// names a file instead.
    pub(crate) fn check_directory(self, filesystem: &dyn Filesystem) -> Answer<String> {
        if self.names_directory {
            match filesystem.metadata(&self.path) {
                Ok(metadata) if metadata.file_type != FileType::Directory => {
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
/// a `..` above `dir` itself is not the guest's to reach; a `..` or a final `.`
/// after a component that is missing or no directory is `NOENT` or
/// `NOTDIR`. A
/// name past `NAME_MAX`, or a resolved path the Filesystem could not be
/// handed within `PATH_MAX`, is `NAMETOOLONG`.
pub(crate) fn resolve(dir: &OpenDirectory, path: &str) -> Answer<ResolvedPath> {
    confirm(dir)?;
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
                if components.len() <= opened {
                    return Err(Errno::Notcapable.into());
                }
                step_out_of(dir.mount.filesystem().as_ref(), &components.join("/"))?;
                components.pop();
            }
            component if component.len() > NAME_MAX => return Err(Errno::Nametoolong.into()),
            component => components.push(component),
        }
    }
    let path = components.join("/");
    if path.len() >= PATH_MAX {
        return Err(Errno::Nametoolong.into());
    }
    // A final `.` resolves through the name before it, as a `..` does.
    if last == "." && components.len() > opened {
        step_out_of(dir.mount.filesystem().as_ref(), &path)?;
    }
    Ok(ResolvedPath {
        path,
        names_directory: matches!(last, "" | "."),
    })
}

/// `dir` still stands at its path, `NOENT` once another directory or
/// nothing does, as a POSIX descriptor never reaches a directory made in
/// its place. A directory opened with no identity follows its path alone.
pub(crate) fn confirm(dir: &OpenDirectory) -> Answer {
    if dir.identity.1 == 0 {
        return Ok(());
    }
    match dir.mount.filesystem().metadata(&dir.path) {
        Ok(found) if (found.dev, found.ino) == dir.identity => Ok(()),
        Ok(_) | Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
            Err(Errno::Noent.into())
        }
        Err(err) => Err(err.into()),
    }
}

/// A `..` leaves `path` only when it is a directory the Filesystem holds.
fn step_out_of(filesystem: &dyn Filesystem, path: &str) -> Answer {
    match filesystem.metadata(path) {
        Ok(metadata) if metadata.file_type == FileType::Directory => Ok(()),
        Ok(_) => Err(Errno::Notdir.into()),
        Err(err) => Err(err.into()),
    }
}
