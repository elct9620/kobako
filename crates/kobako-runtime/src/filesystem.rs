//! The Filesystem kinds kobako ships, each built on the VFS as a Linux
//! filesystem is: what answers every call a guest makes beneath a Mount.
//!
//! A third party implements the same tables to bring its own, so they are
//! named here too; the VFS, which calls them, owns them.

mod anon_inodes;
mod bind;
mod overlay;
mod path;
mod pipe;
mod rdir;
mod tmpfs;
#[cfg(test)]
pub(crate) mod wrap;

pub(crate) use anon_inodes::anon_inode_getfile;
pub use bind::Bind;
pub use overlay::Overlay;
pub use pipe::{create_pipe_files, PipeInodeInfo};
pub use rdir::Rdir;
pub use tmpfs::{SeedError, Tmpfs};

pub use crate::vfs::{
    Dentry, DentryOperations, DirContext, DirEntry, FMode, FileOperations, FileSystemType,
    FileType, FilesystemError, Finish, Iattr, Inode, InodeOperations, Kiocb, Kstat, Lookup, Mask,
    OpenHow, PollMask, RenameFlag, SetTime, SuperBlock as Filesystem, Whence,
};
