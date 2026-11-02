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
    atomic_open, d_revalidate, lookup_one_qstr_excl, may_create_dentry, start_renaming, vfs_mkdir,
    vfs_rename, vfs_rmdir, vfs_unlink,
};

pub(crate) use attr::notify_change;
pub use attr::{Iattr, SetTime};
pub use cred::{Cred, SecurityOperations};
pub use dentry::{Dentry, DentryOperations};
pub use errno::FilesystemError;
pub use file::{
    verify_dirent_name, DirContext, DirEmit, DirEntry, FMode, File, FileOperations, Kiocb,
    PollMask, Whence,
};
pub use inode::{Identity, Inode, InodeOperations, Lookup, Mask, RenameFlag};
pub use mount::{Access, VfsMount};
pub use namei::{
    do_file_open, filename_linkat, filename_mkdirat, filename_renameat2, filename_rmdir,
    filename_symlinkat, filename_unlinkat,
};
pub use openat2::{OpenHow, OpenOptions};
pub use path::Path;
pub use stat::{do_readlinkat, vfs_fstatat, FileType, Metadata};
pub use super_block::SuperBlock;
pub use utimes::{do_utimes, do_utimes_fd};

/// Linux's limits on a path, its terminating NUL counted, and on one name
/// within it.
pub const PATH_MAX: usize = 4096;
pub const NAME_MAX: usize = 255;
