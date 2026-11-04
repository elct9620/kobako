//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use super::cred::Cred;
use super::namei::{filename_lookup, retry_estale};
use super::path::Start;
use super::FilesystemError;

/// What a path names, as Linux's file types `FT_*` name each kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileType {
    RegFile,
    Dir,
    /// A link, as only a stat that does not follow it and a listing report one.
    Symlink,
    /// A FIFO a host holds, or a pipe of the runtime's own.
    Fifo,
    Sock,
    Blkdev,
    /// A character device a host holds, or a whiteout, as Linux makes one.
    Chrdev,
    /// What a platform names no kind for.
    Unknown,
}

/// Whether `kind` is a device, a FIFO, or a socket, as Linux's `special_file`
/// names what an open for writing takes no write access to its mount for.
pub(crate) fn special_file(kind: FileType) -> bool {
    matches!(
        kind,
        FileType::Chrdev | FileType::Blkdev | FileType::Fifo | FileType::Sock
    )
}

/// What a Filesystem reports of an entry, as Linux's `struct kstat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Kstat {
    /// What the entry is, as the `S_IFMT` bits of `mode` say; the runtime
    /// keeps no permission bits, so the kind stands alone.
    pub file_type: FileType,
    /// The length in bytes, of a directory what its Filesystem reports.
    pub size: u64,
    /// The device and inode numbers naming what the path holds, as
    /// `st_dev` and `st_ino` do. An `ino` of zero reports none, so such a
    /// directory cannot be told from one made in its place.
    pub dev: u64,
    pub ino: u64,
    /// How many names the entry has, as `st_nlink` counts; one where the
    /// Filesystem keeps no count, as Linux starts every inode.
    pub nlink: u64,
    /// When the entry was last read, written, or changed, as `st_atime`,
    /// `st_mtime`, and `st_ctime` record; `None` where the Filesystem keeps
    /// no such time.
    pub atime: Option<SystemTime>,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
}

impl Kstat {
    /// What an entry is and its size, reporting no identity or time.
    pub fn new(file_type: FileType, size: u64) -> Self {
        Self {
            file_type,
            size,
            dev: 0,
            ino: 0,
            nlink: 1,
            atime: None,
            mtime: None,
            ctime: None,
        }
    }

    /// The same, naming `dev` and `ino` as what the path holds.
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {
        Self { dev, ino, ..self }
    }

    /// The same, counting `nlink` names.
    pub fn with_nlink(self, nlink: u64) -> Self {
        Self { nlink, ..self }
    }

    /// The same, recording when the entry was last read, written, and changed.
    pub fn with_times(
        self,
        atime: Option<SystemTime>,
        mtime: Option<SystemTime>,
        ctime: Option<SystemTime>,
    ) -> Self {
        Self {
            atime,
            mtime,
            ctime,
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
    start: &impl Start,
    path: &str,
    follow: bool,
) -> Result<Kstat, FilesystemError> {
    retry_estale(
        || filename_lookup(cred, start, path, follow),
        |nd| nd.getattr(),
    )
}

/// Where the link at `path` beneath `start` points, as Linux's `do_readlinkat`
/// asks the security hook only of a link, and answers anything else invalid.
/// It takes the name with `LOOKUP_EMPTY`, so an empty one names `start`
/// itself, and anything but a link there is not found. The caller's buffer is
/// the ABI's to judge, as only its syscall has one.
pub fn do_readlinkat(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
) -> Result<String, FilesystemError> {
    let walk = || match path.is_empty() {
        true => Ok((
            start.path_init()?.dentry().clone(),
            FilesystemError::NotFound,
        )),
        false => Ok((
            filename_lookup(cred, start, path, false)?.path()?,
            FilesystemError::InvalidInput,
        )),
    };
    retry_estale(walk, |(target, refusal)| {
        if target.kind()? != FileType::Symlink {
            return Err(refusal);
        }
        cred.security_inode_readlink(&target)?;
        target.get_link()
    })
}
