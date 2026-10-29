//! A file made with no name and named once whole, as tmpfs's
//! `shmem_tmpfile` and `linkat` let overlayfs copy a file up unseen, and
//! the times set on what it copies, as `utimensat` sets them.

use std::time::SystemTime;

use super::super::FilesystemError;
use super::{lock, Content, Node, Tmpfs, TmpfsFile};

impl Tmpfs {
    /// An empty file with no name yet, as `O_TMPFILE` opens one. Dropped
    /// unnamed, it gives its room back.
    pub(crate) fn tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        Ok(TmpfsFile {
            data: Content::create(&self.tree.space)?,
        })
    }

    /// Name `file` `name` in this directory, as `simple_link` names a file
    /// `O_TMPFILE` opened once `vfs_link` found the name free.
    pub(crate) fn link(&self, file: &TmpfsFile, name: &str) -> Result<(), FilesystemError> {
        self.dir.state().insert(name, Node::File(file.data.clone()));
        Ok(())
    }

    /// Set this directory's times, as `utimensat` does.
    pub(crate) fn set_times(&self, accessed: Option<SystemTime>, modified: Option<SystemTime>) {
        self.dir.state().times.set(accessed, modified);
    }
}

impl TmpfsFile {
    /// Set this file's times, as `futimens` does.
    pub(crate) fn set_times(&self, accessed: Option<SystemTime>, modified: Option<SystemTime>) {
        lock(&self.data).times.set(accessed, modified);
    }
}
