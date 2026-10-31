//! The calls made on paths beneath a mount, answered as the VFS above a
//! filesystem answers them, so an ABI above only carries each call's arguments
//! in and its answer out.
//!
//! A path never climbs above the directory it is resolved beneath, as a
//! capability never widens, and a read-only mount and the security hooks
//! answer each open and each change before the Filesystem is asked, so a
//! Filesystem of a third party's checks neither.

mod attr;
mod cred;
mod dentry;
mod errno;
mod file;
mod inode;
#[cfg(test)]
pub(crate) mod kern_path;
mod mount;
mod namei;
mod openat2;
mod path;
mod seqlock;
mod stat;
mod super_block;
mod utimes;

pub(crate) use namei::{
    atomic_open, lookup_one, lookup_slow, start_renaming, vfs_link, vfs_mkdir, vfs_rename,
    vfs_rmdir, vfs_unlink, vfs_whiteout, RenameFlag,
};

pub use attr::{Iattr, SetTime};
pub use cred::{Cred, SecurityOperations};
pub use dentry::{Dentry, DentryOperations};
pub use errno::FilesystemError;
pub use file::{
    DirContext, DirEmit, DirEntry, FMode, File, FileOperations, Kiocb, PollMask, Whence,
};
pub use inode::{Identity, Inode, InodeOperations, Lookup};
pub use mount::{Access, Denial, VfsMount};
pub use namei::{
    do_file_open, filename_linkat, filename_mkdirat, filename_renameat2, filename_rmdir,
    filename_symlinkat, filename_unlinkat,
};
pub use openat2::{OpenHow, OpenOptions};
pub use path::Path;
pub use stat::{do_readlinkat, vfs_fstatat, FileType, Metadata};
pub use super_block::SuperBlock;
pub use utimes::{do_utimes, vfs_utimes};

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
    /// The path is absolute, climbs above the directory it is resolved
    /// beneath, or holds a link that does either, escaping it as `openat2`'s
    /// `RESOLVE_BENEATH` refuses.
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
