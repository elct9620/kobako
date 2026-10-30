//! The calls a guest makes on paths beneath its Mounts, answered as the
//! VFS above a filesystem answers them, so an engine only carries each
//! call's arguments in and its answer out.
//!
//! A path never climbs above the directory it is resolved beneath, as a
//! capability never widens, and the Mount judges each open and each change
//! before the Filesystem is asked, so a Filesystem of a third party's
//! checks neither.

mod dentry;
mod file;
mod filesystem;
mod inode;
#[cfg(test)]
pub(crate) mod kern_path;
mod namei;
mod openat2;
mod path;
mod pipe;
mod rwsem;
mod seqlock;
mod stat;
mod times;
mod utimes;

use crate::filesystem::FilesystemError;
use crate::mount::Denial;

pub(crate) use namei::{
    d_get_link, d_getattr, d_open, lookup_one, lookup_slow, start_renaming, vfs_create, vfs_link,
    vfs_mkdir, vfs_rename, vfs_rmdir, vfs_unlink, vfs_whiteout, RenameFlag,
};
pub(crate) use rwsem::{Inline, RwSemaphore, RwSemaphoreGuard};

pub use dentry::Dentry;
pub use file::File;
pub use filesystem::Filesystem;
pub use inode::{Identity, Inode};
pub use namei::{
    do_file_open, filename_mkdirat, filename_renameat2, filename_rmdir, filename_unlinkat,
};
pub use openat2::OpenHow;
pub use path::Path;
pub use pipe::{PipeReader, PipeWriter};
pub use rwsem::Blocking;
pub use stat::{do_readlinkat, vfs_fstatat};
pub use utimes::do_utimes;

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
    /// beneath, escaping it as `openat2`'s `RESOLVE_BENEATH` refuses.
    Escape,
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
