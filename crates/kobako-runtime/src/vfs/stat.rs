//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use super::cred::Cred;
use super::namei::{d_get_link, filename_lookup, names_directory};
use super::path::Path;
use super::{Error, FilesystemError};

/// What a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileType {
    File,
    Directory,
    /// A link, as only a stat that does not follow it and a listing report one.
    Symlink,
    /// A FIFO a host holds, or a pipe of the runtime's own.
    Fifo,
    Socket,
    BlockDevice,
    /// A character device a host holds, or a whiteout, as Linux makes one.
    CharDevice,
    /// What a platform names no kind for, or a name stored under another
    /// spelling.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    pub file_type: FileType,
    /// The length in bytes; zero for a directory.
    pub len: u64,
    /// The device and inode numbers naming what the path holds, as
    /// `st_dev` and `st_ino` do. An `ino` of zero reports none, so such a
    /// directory cannot be told from one made in its place, and beneath a
    /// Policy a descriptor of it reaches nothing.
    pub dev: u64,
    pub ino: u64,
    /// How many names the entry has, as `st_nlink` counts; one where the
    /// Filesystem keeps no count, as Linux starts every inode.
    pub nlink: u64,
    /// When the entry was last read, written, or changed, as `st_atime`,
    /// `st_mtime`, and `st_ctime` record; `None` where the Filesystem keeps
    /// no such time.
    pub accessed: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub changed: Option<SystemTime>,
}

impl Metadata {
    /// Metadata named once, reporting no identity or time.
    pub fn new(file_type: FileType, len: u64) -> Self {
        Self {
            file_type,
            len,
            dev: 0,
            ino: 0,
            nlink: 1,
            accessed: None,
            modified: None,
            changed: None,
        }
    }

    /// This metadata naming `dev` and `ino` as what the path holds.
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {
        Self { dev, ino, ..self }
    }

    /// This metadata counting `nlink` names.
    pub fn with_nlink(self, nlink: u64) -> Self {
        Self { nlink, ..self }
    }

    /// This metadata recording when the entry was last read, written, and
    /// changed.
    pub fn with_times(
        self,
        accessed: Option<SystemTime>,
        modified: Option<SystemTime>,
        changed: Option<SystemTime>,
    ) -> Self {
        Self {
            accessed,
            modified,
            changed,
            ..self
        }
    }

    /// A device number for a Filesystem with none of its own, as each
    /// tmpfs mount takes one: no earlier call returned it, and its top bit,
    /// which no host's `st_dev` sets, keeps it apart from a host's.
    pub fn unique_device() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        (1 << 63) | NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

/// What `path` beneath `start` names, as Linux's `vfs_fstatat` answers: a
/// link at its end followed where `follow` asks, and otherwise reported as
/// itself unless a trailing `/` asks for the directory it leads to, as
/// `AT_SYMLINK_NOFOLLOW` leaves one.
pub fn vfs_fstatat(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    follow: bool,
) -> Result<Metadata, Error> {
    let follow = follow || names_directory(path);
    let mut nd = filename_lookup(cred, start, path, follow, false)?;
    // A link the walk did not follow at the end is one swapped in since,
    // so it is reported as itself rather than followed.
    nd.check_directory()?;
    Ok(nd.getattr()?)
}

/// Where the link at `path` beneath `start` points, as Linux's `do_readlinkat`
/// asks the security hook only of a link, and answers anything else invalid.
pub fn do_readlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<String, Error> {
    let mut nd = filename_lookup(cred, start, path, false, false)?;
    nd.check_directory()?;
    if nd.kind()? != FileType::Symlink {
        return Err(FilesystemError::InvalidInput.into());
    }
    cred.security_inode_readlink()?;
    Ok(d_get_link(&*nd.walked()?)?)
}
