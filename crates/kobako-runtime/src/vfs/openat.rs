//! Opening a path, as Linux's `openat2` opens one.

use super::namei::{resolve, Resolved};
use super::path::{Identity, Path};
use super::seen::seen_file;
use super::Error;
use crate::filesystem::{Access, File, FileType, FilesystemError, OpenOptions};

/// How to open a path, as Linux's `struct open_how` carries `openat2`'s
/// flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenHow {
    pub read: bool,
    pub write: bool,
    /// Create a file where nothing stands, as `O_CREAT` does.
    pub create: bool,
    /// Refuse what already stands there, as `O_EXCL` does.
    pub exclusive: bool,
    /// Empty the file once it is open, as `O_TRUNC` does.
    pub truncate: bool,
    /// Refuse anything but a directory, as `O_DIRECTORY` does.
    pub directory: bool,
    /// Follow a link at the end of the path.
    pub follow: bool,
}

/// What an open reached.
pub enum Opened {
    Directory(Path),
    File(Box<dyn File>),
}

/// Open `path` beneath `start` as `how` asks. `room` says whether the
/// caller has a descriptor left to hand the guest; without one the open
/// answers `TooManyOpenFiles` once the path is known to stand.
pub fn open(start: &Path, path: &str, how: &OpenHow, room: bool) -> Result<Opened, Error> {
    // A trailing `/` resolves a link standing there, as Linux's `open` does.
    let follow = how.follow || super::namei::names_directory(path);
    let mut resolved = resolve(start, path, follow, how.create)?;
    // A path ending in `/` asks for a directory, which cannot be created
    // as a file.
    if path.ends_with('/') && how.create {
        return Err(FilesystemError::IsADirectory.into());
    }
    // The walk followed every link the call asked it to, so a link at the
    // end now is one swapped in since, and is opened as itself.
    resolved.check_directory()?;
    if !room {
        return Err(FilesystemError::TooManyOpenFiles.into());
    }
    let (existing, identity) = match resolved.found() {
        Ok(metadata) => (Some(metadata.file_type), Identity::of(&metadata)),
        Err(FilesystemError::NotFound) => (None, None),
        Err(err) => return Err(err.into()),
    };
    refuse_what_stands(existing, how)?;
    // Creating changes the Mount only where nothing stands yet, so the
    // Mount judges the open once, knowing what it does.
    let changes = how.write || how.truncate || (how.create && existing.is_none());
    resolved.check(if changes {
        Access::ReadWrite
    } else {
        Access::Read
    })?;
    match existing {
        Some(FileType::Directory) => open_directory(start, resolved, identity),
        Some(_) | None => {
            let options = OpenOptions {
                read: how.read,
                write: how.write,
                // A file already there is opened as it is, so a host that
                // opens only to create never asks for more than the guest.
                create: how.create && existing.is_none(),
                create_new: how.create && how.exclusive,
                truncate: how.truncate,
                nofollow: true,
            };
            let file = resolved.base.open(&resolved.child(), &options)?;
            Ok(Opened::File(seen_file(file, start.profile())))
        }
    }
}

/// What stands at the path answers before any access is judged, as Linux's
/// `open` does on a read-only mount.
fn refuse_what_stands(existing: Option<FileType>, how: &OpenHow) -> Result<(), Error> {
    let refusal = match existing {
        None if !how.create => Some(FilesystemError::NotFound),
        Some(_) if how.create && how.exclusive => Some(FilesystemError::AlreadyExists),
        Some(FileType::Directory) if how.write || how.truncate || how.create => {
            Some(FilesystemError::IsADirectory)
        }
        Some(FileType::Directory) | None => None,
        Some(_) if how.directory => Some(FilesystemError::NotADirectory),
        Some(_) => None,
    };
    match refusal {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

fn open_directory(
    start: &Path,
    resolved: Resolved,
    identity: Option<Identity>,
) -> Result<Opened, Error> {
    let (dir, dentry) = match resolved.name.as_str() {
        "" => (resolved.base.clone(), resolved.dentry.clone()),
        _ => {
            let dentry = resolved.child();
            (resolved.base.open_dir(&dentry)?, dentry)
        }
    };
    Ok(Opened::Directory(start.opened(dir, dentry, identity)))
}
