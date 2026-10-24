//! The Filesystem behind a Mount: what answers every call a
//! guest makes beneath that Mount's path.
//!
//! A runtime owns the descriptors and path resolution, and the Mount
//! decides access; a `Filesystem` only answers for the files and directories
//! themselves, so a third party brings one without implementing any of
//! that. Every path a runtime hands over is relative to the Filesystem's
//! own root, its components joined by `/`, with no `.` or `..` left and
//! no leading `/`; the empty path is the root, which a runtime never asks
//! to remove or move.

mod directory;
mod memory;
mod overlay;
mod path;
mod temporary;

use std::sync::Arc;

pub use directory::Directory;
pub use memory::{Memory, SeedError};
pub use overlay::{Blocking, Overlay};
pub use temporary::Temporary;

/// What the guest may do beneath a Mount, or an Overlay in one of its
/// Layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// The files and directories behind a Mount. One may wrap
/// another and answer by delegating, so it sees every call on the way
/// through.
pub trait Filesystem: Send + Sync {
    /// Open the file at `path` as `options` asks, creating or truncating it
    /// where they say to.
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError>;

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError>;

    /// What `path` names without following a link standing there, as
    /// `lstat` answers. A link is reported as one and nothing of where it
    /// points: no size, and no way to read it. The default, for a
    /// Filesystem holding no links, is `metadata`.
    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.metadata(path)
    }

    /// The entries of the directory at `path`, without `.` or `..`.
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError>;

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError>;

    /// Remove the empty directory at `path`.
    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError>;

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError>;

    /// Move the entry at `from` to `to`, refusing one `rename` does not
    /// allow; a move onto its own path changes nothing and is allowed.
    /// The Filesystem checks what stands at `from` as it moves it, under
    /// the same lock, so nothing replaces it in between.
    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError>;

    /// The Filesystem a new Invocation starts from in this one's place, or
    /// `None` when every Invocation shares this one. The default shares.
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        None
    }
}

/// One open file. The runtime keeps its position, so every read and write
/// names the offset it starts at.
pub trait File: Send {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError>;

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError>;

    /// Write `buf` at the file's end as it stands when the write lands,
    /// answering the offset it landed at and how much was written. The
    /// default reads the length, then writes there: two steps a file
    /// another writer shares should take as one, as Linux takes an append
    /// under the file's lock.
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        let at = self.metadata()?.len;
        Ok((at, self.write_at(buf, at)?))
    }

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

/// How `Filesystem::open` opens a file.
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
    /// Refuse a symbolic link standing at the path, as `O_NOFOLLOW` does;
    /// a Filesystem holding no links has none to refuse.
    pub nofollow: bool,
}

/// What `Filesystem::rename` may move, as `renameat2` flags ask more of
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    pub file_type: FileType,
    /// The length in bytes; zero for a directory or a link.
    pub len: u64,
}

impl Metadata {
    pub fn new(file_type: FileType, len: u64) -> Self {
        Self { file_type, len }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
}

impl DirEntry {
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
