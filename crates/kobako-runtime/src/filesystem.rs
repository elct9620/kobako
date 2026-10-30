//! The Filesystem behind a Mount: what answers every call a
//! guest makes beneath that Mount's path.
//!
//! A runtime owns the descriptors and path resolution, and the Mount
//! decides access; an `Inode` only answers for the files and directories
//! themselves, so a third party brings one without implementing any of
//! that. A runtime asks a directory about one name at a time, carried by
//! the `Dentry` the VFS made for it, as Linux hands a filesystem's
//! `inode_operations` a dentry: never `.`, `..`, or a name holding `/`.

mod bind;
mod overlay;
mod path;
mod tmpfs;
#[cfg(test)]
pub(crate) mod wrap;

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

pub use crate::vfs::Blocking;
pub use bind::Bind;
pub use overlay::Overlay;
pub use tmpfs::{SeedError, Tmpfs};

use crate::vfs::{Dentry, Inode};

/// What the guest may do beneath a Mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// What one name in a directory turned out to be, as `->lookup` answers.
pub enum Lookup {
    /// The entry standing at the name, for the runtime to record on the
    /// dentry, as `d_splice_alias` records it.
    Found(Inode),
    /// Nothing stands at the name, as a negative dentry records.
    Absent,
    /// The entry the name reaches is stored under this other spelling, as
    /// `d_add_ci` is handed the case-exact name, so a name a host reaches by
    /// more than one spelling has one dentry, one lock, and one spelling a
    /// Policy judges.
    Spelled(String),
}

/// What one entry of a Filesystem answers: a directory for the names in it,
/// a link for where it points, and any entry for itself, as Linux's
/// `inode_operations` hang from each inode. An operation an entry does not
/// have answers `None`, as a NULL in Linux's table, and the runtime answers
/// for it as Linux's VFS does. One Filesystem may wrap another and answer by
/// delegating, wrapping each entry its `lookup` finds, so it sees every call
/// on the way through. A directory keeps its identity its own while it is
/// open, as an open descriptor keeps a host's inode from being reused.
///
/// The runtime walks a guest's path one name at a time and follows a link
/// itself, as the VFS above a filesystem does, so a link never leads a call
/// out of the directory a descriptor opened. Every Invocation reaches the
/// same Filesystem; one that should start afresh is handed in its place for
/// that Invocation.
pub trait InodeOperations: Any + Send + Sync {
    /// What this entry, which `dentry` names, reports of itself, as
    /// `getattr` is handed the path it answers for. A link reports itself;
    /// the runtime keeps its size from the guest.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError>;

    /// What stands at the name `dentry` carries in this directory. The
    /// runtime asks once for a dentry naming nothing yet, and an entry
    /// without it is no directory to walk through.
    fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        None
    }

    /// Whether `dentry`, a name in this directory the runtime still holds,
    /// names what it recorded, as `d_revalidate` answers; `false` has the
    /// runtime drop it and take the name afresh. A Filesystem changed only
    /// through the runtime need not have it, as Linux's need no `d_op`.
    fn d_revalidate(&self, _dentry: &Dentry) -> Option<Result<bool, FilesystemError>> {
        None
    }

    /// Where this link, which `dentry` names, points, as `get_link` answers,
    /// for the runtime to follow; a guest never reads it. Anything else
    /// without it refuses as `readlinkat` does.
    fn get_link(&self, _dentry: &Dentry) -> Option<Result<String, FilesystemError>> {
        None
    }

    /// Open this entry, which `dentry` names, as `options` asks, truncating
    /// a file where they say to, as `f_op->open` makes the open file. A
    /// directory opens to be listed; without it, listing one answers
    /// `NotADirectory`. The runtime refuses a link before it asks.
    fn open(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        None
    }

    /// Make the file `dentry` names in this directory and open it as
    /// `options` asks, as `->create` does. Without it the runtime answers
    /// `PermissionDenied`, as Linux's VFS answers EACCES.
    fn create(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        None
    }

    /// Make the directory `dentry` names in this one. Without it, and
    /// without `rmdir`, `unlink`, or `rename`, the runtime answers
    /// `NotPermitted`, as Linux's VFS answers EPERM.
    fn mkdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Remove the empty directory `dentry` names in this one.
    fn rmdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Remove the file `dentry` names in this directory.
    fn unlink(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Move the entry `from` names in this directory to `to` in `to_dir`,
    /// refusing what rename(2) refuses at `to`. The runtime has already
    /// checked what stands at `from` and never hands a move onto its own
    /// name, as Linux's VFS does before `inode_operations.rename`. It hands
    /// only a `to_dir` of the same Mount, so it is of this Filesystem's own
    /// type, and one that is not answers `CrossesDevices`.
    fn rename(
        &self,
        _from: &Dentry,
        _to_dir: &dyn InodeOperations,
        _to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        None
    }
}

/// One open file. The runtime keeps its position, so every read and write
/// names the offset it starts at.
pub trait FileOperations: Send {
    /// Read from `offset` into `buf`. A file without it refuses as Linux's
    /// `vfs_read` refuses one without `f_op->read`.
    fn read(&mut self, _buf: &mut [u8], _offset: u64) -> Result<usize, FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// Write `buf` at `offset`. A file without it refuses as Linux's
    /// `vfs_write` refuses one without `f_op->write`.
    fn write(&mut self, _buf: &[u8], _offset: u64) -> Result<usize, FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// Write `buf` at the file's end as it stands when the write lands,
    /// answering the offset it landed at and how much was written. It
    /// keeps another writer's append from landing over this one, as Linux
    /// takes an append under the file's lock, so a file that cannot read
    /// its end and write there in one step answers `Unsupported`.
    fn append(&mut self, _buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// What the open file is, and its length, even once its name is gone,
    /// as a host answers `fstat` through the descriptor it holds.
    fn getattr(&self) -> Result<Metadata, FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// Truncate or extend the file to `len` bytes, extending with zeros, as
    /// `ftruncate` reaches through the file a host holds.
    fn truncate(&mut self, _len: u64) -> Result<(), FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// The entries of this directory, which `dentry` names, without `.`
    /// or `..`, as `iterate_shared` lists the file it is handed. A file
    /// without it answers `NotADirectory`, as Linux's `iterate_dir` does.
    fn iterate_shared(&mut self, _dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        Err(FilesystemError::NotADirectory)
    }

    /// Make what was written durable. The default has nothing to flush,
    /// which is the truthful answer of a file with no durability of its
    /// own.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Release the file once the guest closes it, answering whether what
    /// it held was kept. The default has nothing to release; a file the
    /// guest never closes is dropped instead.
    fn release(self: Box<Self>) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// How `InodeOperations::open` opens a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Create the file when it does not exist.
    pub create: bool,
    /// Create the file, refusing one that already exists.
    pub create_new: bool,
    /// Empty the file once it is open.
    pub truncate: bool,
    /// Open a directory, to list it, as `O_DIRECTORY` does.
    pub directory: bool,
}

/// What a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileType {
    File,
    Directory,
    /// A link, as only `symlink_metadata` and a listing report one.
    Symlink,
    /// Anything else a host holds, such as a FIFO, a socket, or a device,
    /// which no Filesystem opens.
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
    /// How many names the entry has, as `st_nlink` counts; `None` where
    /// the Filesystem keeps no count.
    pub nlink: Option<u64>,
    /// When the entry was last read, written, or changed, as `st_atime`,
    /// `st_mtime`, and `st_ctime` record; `None` where the Filesystem keeps
    /// no such time.
    pub accessed: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub changed: Option<SystemTime>,
}

impl Metadata {
    /// Metadata reporting no identity, link count, or time.
    pub fn new(file_type: FileType, len: u64) -> Self {
        Self {
            file_type,
            len,
            dev: 0,
            ino: 0,
            nlink: None,
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
        Self {
            nlink: Some(nlink),
            ..self
        }
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

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
}

impl DirEntry {
    /// The entry `name` in its directory, naming a `file_type`.
    pub fn new(name: impl Into<String>, file_type: FileType) -> Self {
        Self {
            name: name.into(),
            file_type,
        }
    }
}

/// Why a Filesystem call failed. The runtime hands the guest the matching
/// error code.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilesystemError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    PermissionDenied,
    ReadOnlyFilesystem,
    InvalidInput,
    Unsupported,
    /// A change past what the Filesystem can hold.
    StorageFull,
    /// A file grown past the largest the Filesystem keeps.
    FileTooLarge,
    /// A move that cannot happen in one step, as between two devices.
    CrossesDevices,
    /// A call that would wait on a lock its own thread holds.
    Deadlock,
    /// A symbolic link where the call asked to follow none.
    FilesystemLoop,
    /// A call only a privileged caller may make, as apart from one the
    /// file's permissions deny.
    NotPermitted,
    BadDescriptor,
    ResourceBusy,
    /// Bytes that are no valid character where the call needs one.
    IllegalSequence,
    Interrupted,
    TooManyOpenFiles,
    /// A name too long for the Filesystem.
    InvalidFilename,
    OutOfMemory,
    NotASocket,
    /// A value too large for the type the call answers in.
    ValueTooLarge,
    NotSeekable,
    /// Any other failure, with what the Filesystem knows of it.
    Other(String),
}

/// Each failure under the POSIX error name it stands for. `ENOTSUP` comes
/// before `EOPNOTSUPP`, a name some hosts give the same error, so
/// `Unsupported` answers to the first.
const POSIX_NAMES: [(&str, FilesystemError); 26] = [
    ("ENOENT", FilesystemError::NotFound),
    ("EEXIST", FilesystemError::AlreadyExists),
    ("ENOTDIR", FilesystemError::NotADirectory),
    ("EISDIR", FilesystemError::IsADirectory),
    ("ENOTEMPTY", FilesystemError::DirectoryNotEmpty),
    ("EACCES", FilesystemError::PermissionDenied),
    ("EROFS", FilesystemError::ReadOnlyFilesystem),
    ("EINVAL", FilesystemError::InvalidInput),
    ("ENOTSUP", FilesystemError::Unsupported),
    ("EOPNOTSUPP", FilesystemError::Unsupported),
    ("ENOSPC", FilesystemError::StorageFull),
    ("EFBIG", FilesystemError::FileTooLarge),
    ("EXDEV", FilesystemError::CrossesDevices),
    ("EDEADLK", FilesystemError::Deadlock),
    ("ELOOP", FilesystemError::FilesystemLoop),
    ("EPERM", FilesystemError::NotPermitted),
    ("EBADF", FilesystemError::BadDescriptor),
    ("EBUSY", FilesystemError::ResourceBusy),
    ("EILSEQ", FilesystemError::IllegalSequence),
    ("EINTR", FilesystemError::Interrupted),
    ("EMFILE", FilesystemError::TooManyOpenFiles),
    ("ENAMETOOLONG", FilesystemError::InvalidFilename),
    ("ENOMEM", FilesystemError::OutOfMemory),
    ("ENOTSOCK", FilesystemError::NotASocket),
    ("EOVERFLOW", FilesystemError::ValueTooLarge),
    ("ESPIPE", FilesystemError::NotSeekable),
];

impl FilesystemError {
    /// The POSIX error name this failure stands for, as `ENOENT`. `Other`
    /// has none.
    pub fn posix_name(&self) -> Option<&'static str> {
        POSIX_NAMES
            .iter()
            .find(|(_, error)| error == self)
            .map(|(name, _)| *name)
    }

    /// The failure a POSIX error name stands for, if it is one of these.
    pub fn from_posix_name(name: &str) -> Option<Self> {
        POSIX_NAMES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, error)| error.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_posix_name_answers_back_to_its_failure() {
        for (name, error) in &POSIX_NAMES {
            assert_eq!(
                FilesystemError::from_posix_name(name)
                    .as_ref()
                    .and_then(FilesystemError::posix_name)
                    .and_then(FilesystemError::from_posix_name),
                Some(error.clone()),
                "{name} through FilesystemError::from_posix_name and posix_name must name {error:?} again"
            );
        }
        assert_eq!(
            (
                FilesystemError::Unsupported.posix_name(),
                FilesystemError::Other("x".into()).posix_name()
            ),
            (Some("ENOTSUP"), None),
            "Unsupported through FilesystemError::posix_name must answer ENOTSUP, and Other none"
        );
    }
}
