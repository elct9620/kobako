//! Resolving a guest path beneath a directory descriptor.
//!
//! The runtime is the boundary: a path never climbs above the directory
//! descriptor it is resolved beneath, as a capability never widens, and
//! the Mount is asked what the guest may do there before the Filesystem
//! is, so a Filesystem of a third party's need check neither.
//! A `..` steps out of what the Filesystem holds rather than out of the
//! path's text, as a host resolves it, so `missing/..` names nothing and
//! a `..` out of a link is refused rather than taken from its text. A link
//! is followed only within the directory the descriptor opened, as
//! wasmtime-wasi's cap-std `Dir` follows one.
//!
//! A descriptor reaches its directory wherever it moved. Beneath a Mount
//! with a Policy it reaches the directory again from the Mount's root
//! instead, so the Policy judges the path the directory stands at now.

use std::sync::Arc;

use kobako_runtime::filesystem::{FileType, Filesystem, FilesystemError, Metadata};

use super::errno::{Answer, Errno};
use crate::descriptors::{Identity, OpenDirectory};

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub(crate) const PATH_MAX: usize = 4096;
pub(crate) const NAME_MAX: usize = 255;

/// A guest path resolved beneath a directory: the directory it starts
/// from, the path relative to it, and whether it ended in `/` or `/.`,
/// which only a directory satisfies. Beneath a Mount with a Policy the
/// directory is the Mount's root, so the path is the one the Policy
/// judges.
pub(crate) struct ResolvedPath {
    pub(crate) base: Arc<dyn Filesystem>,
    pub(crate) path: String,
    pub(crate) names_directory: bool,
}

impl ResolvedPath {
    /// The path, refused as `NOTDIR` when it must name a directory and
    /// `lookup` finds anything else there: `metadata` for a call that
    /// follows a link at the end, `symlink_metadata` for one that acts on
    /// the link itself.
    pub(crate) fn check_directory(
        self,
        lookup: fn(&(dyn Filesystem + 'static), &str) -> Result<Metadata, FilesystemError>,
    ) -> Answer<(Arc<dyn Filesystem>, String)> {
        if self.names_directory {
            match lookup(self.base.as_ref(), &self.path) {
                Ok(metadata) if metadata.file_type != FileType::Directory => {
                    return Err(Errno::Notdir.into())
                }
                Ok(_) | Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok((self.base, self.path))
    }

    /// The path with a link standing at its end followed within the
    /// directory `dir` opened, for a call following it. A link pointing
    /// at nothing there is `LOOP` to a call that `creates`, since what it
    /// would create could lie outside `dir`, and `NOENT` otherwise.
    pub(crate) fn follow_last(self, dir: &OpenDirectory, creates: bool) -> Answer<Self> {
        if !bounded(dir) || self.path.is_empty() {
            return Ok(self);
        }
        match self.base.symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type == FileType::Symlink => {}
            _ => return Ok(self),
        }
        let path = match self.base.canonicalize_beneath("", &self.path) {
            Ok(found) => found,
            Err(FilesystemError::NotFound) if creates => return Err(Errno::Loop.into()),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { path, ..self })
    }
}

/// Whether a descriptor's links are kept within its own directory, rather
/// than within the Mount's root, which the Filesystem keeps them within
/// already, or nowhere, beneath a Mount that follows no link at all.
fn bounded(dir: &OpenDirectory) -> bool {
    !dir.preopen && dir.mount.follows_links()
}

/// Where a call through `dir` starts, and the path of `dir` from there:
/// the directory itself, or the Mount's root beneath a Mount following no
/// link, which is one with a Policy.
fn reached(dir: &OpenDirectory) -> Answer<(Arc<dyn Filesystem>, &str)> {
    if dir.mount.follows_links() {
        return Ok((dir.handle.clone(), ""));
    }
    confirm(dir)?;
    Ok((dir.mount.filesystem().clone(), &dir.path))
}

/// The directory `dir` stands for, and its path from there, for a call on
/// the directory itself.
pub(crate) fn directory_of(dir: &OpenDirectory) -> Answer<(Arc<dyn Filesystem>, String)> {
    let (base, path) = reached(dir)?;
    Ok((base, path.to_string()))
}

/// `path` with the links on its way to its last name followed within the
/// directory `dir` opened, so none leads the call out of it. A way the
/// Filesystem finds nothing on stays as given, for the call to answer.
fn within(dir: &OpenDirectory, base: &dyn Filesystem, path: String) -> Answer<String> {
    if !bounded(dir) {
        return Ok(path);
    }
    let Some((way, name)) = path.rsplit_once('/') else {
        return Ok(path);
    };
    match base.canonicalize_beneath("", way) {
        Ok(found) if found.is_empty() => Ok(name.to_string()),
        Ok(found) => Ok(format!("{found}/{name}")),
        Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => Ok(path),
        Err(err) => Err(err.into()),
    }
}

/// `path`, beneath `dir`. An empty path names nothing; an absolute path or
/// a `..` above `dir` itself is not the guest's to reach; a `..` or a final `.`
/// after a component that is missing, no directory, or a link is `NOENT`,
/// `NOTDIR`, or `LOOP`. A
/// name past `NAME_MAX`, or a resolved path the Filesystem could not be
/// handed within `PATH_MAX`, is `NAMETOOLONG`.
pub(crate) fn resolve(dir: &OpenDirectory, path: &str) -> Answer<ResolvedPath> {
    let (base, start) = reached(dir)?;
    if path.is_empty() {
        return Err(Errno::Noent.into());
    }
    if path.starts_with('/') {
        return Err(Errno::Notcapable.into());
    }
    let mut components: Vec<&str> = start.split('/').filter(|c| !c.is_empty()).collect();
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
                step_out_of(base.as_ref(), &components.join("/"))?;
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
        step_out_of(base.as_ref(), &path)?;
    }
    Ok(ResolvedPath {
        path: within(dir, base.as_ref(), path)?,
        base,
        names_directory: matches!(last, "" | "."),
    })
}

/// `dir` still stands at its path, `NOENT` once another directory or
/// nothing does, as a POSIX descriptor never reaches another directory.
/// A Mount's root never moves; any other directory opened with no
/// identity cannot be told from another, so it is reached no more.
fn confirm(dir: &OpenDirectory) -> Answer {
    if dir.preopen {
        return Ok(());
    }
    let Some(identity) = dir.identity else {
        return Err(Errno::Noent.into());
    };
    match dir.mount.filesystem().metadata(&dir.path) {
        Ok(found) if Identity::of(&found) == Some(identity) => Ok(()),
        Ok(_) | Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => {
            Err(Errno::Noent.into())
        }
        Err(err) => Err(err.into()),
    }
}

/// A `..` leaves `path` only when it is a directory the Filesystem holds.
/// A host leaves a link by the parent of where it points, which no guest
/// is told, so a link there is refused as `LOOP`, as `openat2(2)`
/// refuses a link it may not follow.
fn step_out_of(filesystem: &dyn Filesystem, path: &str) -> Answer {
    match filesystem.symlink_metadata(path) {
        Ok(metadata) if metadata.file_type == FileType::Directory => Ok(()),
        Ok(metadata) if metadata.file_type == FileType::Symlink => Err(Errno::Loop.into()),
        Ok(_) => Err(Errno::Notdir.into()),
        Err(err) => Err(err.into()),
    }
}
