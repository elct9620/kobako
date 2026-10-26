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

use kobako_runtime::filesystem::{FileType, Filesystem, FilesystemError, Metadata};

use super::errno::{Answer, Errno};
use crate::descriptors::{Identity, OpenDirectory};

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
    /// `lookup` finds anything else there: `metadata` for a call that
    /// follows a link at the end, `symlink_metadata` for one that acts on
    /// the link itself.
    pub(crate) fn check_directory(
        self,
        filesystem: &(dyn Filesystem + 'static),
        lookup: fn(&(dyn Filesystem + 'static), &str) -> Result<Metadata, FilesystemError>,
    ) -> Answer<String> {
        if self.names_directory {
            match lookup(filesystem, &self.path) {
                Ok(metadata) if metadata.file_type != FileType::Directory => {
                    return Err(Errno::Notdir.into())
                }
                Ok(_) | Err(FilesystemError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(self.path)
    }

    /// The path with a link standing at its end followed within the
    /// directory `dir` opened, for a call following it. A link pointing
    /// at nothing there is `LOOP` to a call that `creates`, since what it
    /// would create could lie outside `dir`, and `NOENT` otherwise.
    pub(crate) fn follow_last(self, dir: &OpenDirectory, creates: bool) -> Answer<Self> {
        let Some(base) = bounded_base(dir) else {
            return Ok(self);
        };
        let filesystem = dir.mount.filesystem();
        let Some(rest) = beneath_base(base, &self.path) else {
            return Ok(self);
        };
        match filesystem.symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type == FileType::Symlink => {}
            _ => return Ok(self),
        }
        let path = match filesystem.canonicalize_beneath(base, rest) {
            Ok(found) => join(base, &found),
            Err(FilesystemError::NotFound) if creates => return Err(Errno::Loop.into()),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { path, ..self })
    }
}

/// The directory a descriptor's links are kept within, or `None` where
/// the Filesystem keeps them within the Mount's root already, or a Mount
/// follows no link at all.
fn bounded_base(dir: &OpenDirectory) -> Option<&str> {
    (!dir.path.is_empty() && dir.mount.follows_links()).then_some(dir.path.as_str())
}

/// What `path` names beneath `base`, or `None` for `base` itself.
fn beneath_base<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    path.strip_prefix(base)?.strip_prefix('/')
}

fn join(base: &str, path: &str) -> String {
    if path.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{path}")
    }
}

/// `path` with the links on its way to its last name followed within the
/// directory `dir` opened, so none leads the call out of it. A way the
/// Filesystem finds nothing on stays as given, for the call to answer.
fn within(dir: &OpenDirectory, path: String) -> Answer<String> {
    let Some(base) = bounded_base(dir) else {
        return Ok(path);
    };
    let Some((way, name)) = beneath_base(base, &path).and_then(|rest| rest.rsplit_once('/')) else {
        return Ok(path);
    };
    match dir.mount.filesystem().canonicalize_beneath(base, way) {
        Ok(found) => Ok(join(&join(base, &found), name)),
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
        path: within(dir, path)?,
        names_directory: matches!(last, "" | "."),
    })
}

/// `dir` still stands at its path, `NOENT` once another directory or
/// nothing does, as a POSIX descriptor never reaches a directory made in
/// its place. A directory opened with no identity follows its path alone.
pub(crate) fn confirm(dir: &OpenDirectory) -> Answer {
    let Some(identity) = dir.identity else {
        return Ok(());
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
