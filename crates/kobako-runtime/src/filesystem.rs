//! The Filesystem behind a filesystem Grant: what answers every call a
//! guest makes beneath that Grant's path.
//!
//! A runtime owns the descriptors, the access the Grant allows, and path
//! resolution; a `Filesystem` only answers for the files and directories
//! themselves, so a third party brings one without implementing any of
//! that. Every path a runtime hands over is relative to the Filesystem's
//! own root, its components joined by `/`, with no `.` or `..` left and
//! no leading `/`; the empty path is the root.

mod directory;
mod memory;
mod overlay;
mod path;
mod temporary;

use std::sync::Arc;

pub use directory::Directory;
pub use memory::Memory;
pub use overlay::Overlay;
pub use temporary::Temporary;

/// The files and directories behind a filesystem Grant. One may wrap
/// another and answer by delegating, so it sees every call on the way
/// through.
pub trait Filesystem: Send + Sync {
    /// Open the file at `path` as `options` asks, creating or truncating it
    /// where they say to.
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError>;

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError>;

    /// The entries of the directory at `path`, without `.` or `..`.
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError>;

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError>;

    /// Remove the empty directory at `path`.
    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError>;

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError>;

    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError>;

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

    fn metadata(&self) -> Result<Metadata, FilesystemError>;

    /// Truncate or extend the file to `len` bytes, extending with zeros.
    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError>;

    /// Make what was written durable. The default has nothing to flush,
    /// which is the truthful answer of a file with no durability of its
    /// own.
    fn sync(&mut self) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// How `Filesystem::open` opens a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

/// What a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub kind: Kind,
    /// The length in bytes; zero for a directory.
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: Kind,
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
    ReadOnly,
    InvalidInput,
    Unsupported,
    /// Any other failure, with what the Filesystem knows of it.
    Other(String),
}
