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

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub use crate::vfs::Blocking;
pub use bind::Bind;
pub use overlay::Overlay;
pub use tmpfs::{SeedError, Tmpfs};

use crate::vfs::Dentry;

/// What the guest may do beneath a Mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// The files and directories beneath one directory, as the `*at` calls
/// reach them from a directory descriptor: a Mount's root, or a directory
/// opened beneath it, which answers as an Inode of its own. One may
/// wrap another and answer by delegating, so it sees every call on the
/// way through, and wraps each directory it opens as well. A directory it
/// opens keeps its identity its own while it is open, as an open
/// descriptor keeps a host's inode from being reused.
///
/// The runtime walks a guest's path one name at a time and follows a link
/// itself, as the VFS above a filesystem does, so a link never leads a
/// call out of the directory a descriptor opened. Five methods keep a
/// guarantee the runtime gives the guest, so each Filesystem answers them
/// itself and a wrapper forwards them: `open`, `symlink_metadata`,
/// `open_dir`, and `read_link` keep a link from being followed anywhere but
/// where the runtime follows it, and `stored_name` keeps a Policy judging the
/// spelling the host stores. Every Invocation reaches the same Filesystem;
/// one that should start afresh is handed in its place for that
/// Invocation.
pub trait Inode: Any + Send + Sync {
    /// Open the file `dentry` names as `options` asks, creating or
    /// truncating it where they say to; `FilesystemLoop` for a link
    /// standing there, as `O_NOFOLLOW` refuses one, since the runtime
    /// follows a link itself.
    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError>;

    /// The directory `dentry` names in this one, as `O_DIRECTORY` opens
    /// one: `NotADirectory` where anything else stands, and
    /// `FilesystemLoop` for a link standing there, as `O_NOFOLLOW` refuses
    /// one, since the runtime follows a link itself.
    fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError>;

    /// What this directory reports of itself, as `fstat` answers.
    fn metadata(&self) -> Result<Metadata, FilesystemError>;

    /// What `dentry` names without following a link standing there, as
    /// `lstat` answers, since the runtime follows a link itself. A link is
    /// reported as one and nothing of where it points: no size, and no way
    /// to read it.
    fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError>;

    /// The name `dentry` carries spelled as this directory stores it, so a
    /// name a host reaches by more than one spelling is judged by one; a
    /// name it does not hold stays as given. A Filesystem matching names
    /// exactly answers the name. Only a Policy asks, as an LSM asks a
    /// filesystem for what it judges by, so no other caller pays for it.
    fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError>;

    /// Where the link `dentry` names points, as `readlinkat` answers, for
    /// the runtime to follow; a guest never reads it. `InvalidInput` where
    /// anything else stands, as `readlinkat` refuses one, which is a
    /// Filesystem holding no links's answer for every name it holds.
    fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError>;

    /// The entries of this directory, without `.` or `..`.
    fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError>;

    fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError>;

    /// Remove the empty directory `dentry` names.
    fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError>;

    fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError>;

    /// Move the entry `from` names to `to` in `to_dir`, refusing one
    /// `rename` does not allow; a move onto its own name changes nothing
    /// and is allowed. A runtime hands only a `to_dir` of the same Mount,
    /// so it is of this Filesystem's own type, and one that is not answers
    /// `CrossesDevices`. The Filesystem checks what stands at `from` as it
    /// moves it, under the same lock, so nothing replaces it in between.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn Inode,
        to: &Dentry,
        rename: Rename,
    ) -> Result<(), FilesystemError>;
}

/// One open file. The runtime keeps its position, so every read and write
/// names the offset it starts at.
pub trait File: Send {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError>;

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError>;

    /// Write `buf` at the file's end as it stands when the write lands,
    /// answering the offset it landed at and how much was written. It
    /// keeps another writer's append from landing over this one, as Linux
    /// takes an append under the file's lock, so a file that cannot read
    /// its end and write there in one step answers `Unsupported`.
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError>;

    fn metadata(&self) -> Result<Metadata, FilesystemError>;

    /// Truncate or extend the file to `len` bytes, extending with zeros.
    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError>;

    /// Make what was written durable. The default has nothing to flush,
    /// which is the truthful answer of a file with no durability of its
    /// own.
    fn sync(&mut self) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Release the file once the guest closes it, answering whether what
    /// it held was kept. The default has nothing to release; a file the
    /// guest never closes is dropped instead.
    fn close(self: Box<Self>) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// How `Inode::open` opens a file.
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
}

/// What `Inode::rename` may move, as `renameat2` flags ask more of
/// a rename than its two paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rename {
    /// Whatever stands at the source.
    #[default]
    Any,
    /// Only a file. A directory is refused as `CrossesDevices`, so a
    /// caller copies what is beneath it one entry at a time instead.
    File,
    /// Only a directory, as a trailing `/` asks. A file is refused as
    /// `NotADirectory`.
    Directory,
}

impl Rename {
    /// Refuse moving an entry of `file_type` this rename may not move.
    pub fn check(self, file_type: FileType) -> Result<(), FilesystemError> {
        match (self, file_type) {
            (Self::File, FileType::Directory) => Err(FilesystemError::CrossesDevices),
            (Self::Directory, file_type) if file_type != FileType::Directory => {
                Err(FilesystemError::NotADirectory)
            }
            _ => Ok(()),
        }
    }
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
    /// The length in bytes; zero for a directory or a link.
    pub len: u64,
    /// The device and inode numbers naming what the path holds, as
    /// `st_dev` and `st_ino` do. An `ino` of zero reports none, so a
    /// descriptor of such a directory follows its path alone.
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
