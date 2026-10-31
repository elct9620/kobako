//! The Filesystem kinds kobako ships, each built on the VFS as a Linux
//! filesystem is: what answers every call a guest makes beneath a Mount.
//!
//! A third party implements the same tables to bring its own, so they are
//! named here too; the VFS, which calls them, owns them.

mod bind;
mod overlay;
mod path;
mod tmpfs;
#[cfg(test)]
pub(crate) mod wrap;

pub use bind::Bind;
pub use overlay::Overlay;
pub use tmpfs::{SeedError, Tmpfs};

pub use crate::vfs::{
    DentryOperations, DirEntry, FileOperations, FileType, FilesystemError, Iattr, InodeOperations,
    Kiocb, Lookup, Metadata, OpenOptions, SetTime,
};
