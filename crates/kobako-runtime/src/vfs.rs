//! The calls a guest makes on paths beneath its Mounts, answered as the
//! VFS above a filesystem answers them, so an engine only carries each
//! call's arguments in and its answer out.
//!
//! A path never climbs above the directory it is resolved beneath, as a
//! capability never widens, and the Mount judges each open and each change
//! before the Filesystem is asked, so a Filesystem of a third party's
//! checks neither.

mod dentry;
mod entry;
mod filesystem;
mod namei;
mod openat;
mod path;
mod seen;
mod stat;
#[cfg(test)]
pub(crate) mod through;

use crate::filesystem::FilesystemError;
use crate::mount::Denial;

pub use dentry::Dentry;
pub use entry::{create_dir, remove_dir, remove_file, rename};
pub use filesystem::Filesystem;
pub use openat::{open, OpenHow, Opened};
pub use path::Path;
pub use stat::{metadata, read_link, set_times, symlink_metadata};

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub const PATH_MAX: usize = 4096;
pub const NAME_MAX: usize = 255;

/// Why a call on a path failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// What the Filesystem, or the path itself, answered.
    Filesystem(FilesystemError),
    /// The Mount refused it.
    Denied(Denial),
    /// The path is absolute, or climbs above the directory it is resolved
    /// beneath, which no capability the guest holds reaches.
    NotCapable,
}

impl From<FilesystemError> for Error {
    fn from(error: FilesystemError) -> Self {
        Self::Filesystem(error)
    }
}

impl From<Denial> for Error {
    fn from(denial: Denial) -> Self {
        Self::Denied(denial)
    }
}
