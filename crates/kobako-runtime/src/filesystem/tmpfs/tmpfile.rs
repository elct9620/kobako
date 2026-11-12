//! A file made with no name and named once whole, as tmpfs's
//! `shmem_tmpfile` and `linkat` let overlayfs copy a file up unseen, and
//! the times set on what it copies.

use super::super::{Errno, Iattr};
use super::{lock, Content, Node, Tmpfs, TmpfsFile};

impl Tmpfs {
    /// An empty file with no name yet, as `O_TMPFILE` opens one. Dropped
    /// unnamed, it gives its room back.
    pub(crate) fn tmpfile(&self) -> Result<TmpfsFile, Errno> {
        Ok(TmpfsFile {
            data: Content::create(&self.tree)?,
        })
    }

    /// Name `file` `name` in this directory, as `simple_link` names a file
    /// `O_TMPFILE` opened once `vfs_link` found the name free.
    pub(crate) fn link(&self, file: &TmpfsFile, name: &str) -> Result<(), Errno> {
        self.dir.state().insert(name, Node::File(file.data.clone()));
        Ok(())
    }
}

impl TmpfsFile {
    /// Set the times `attr` names on a file with no name yet, as
    /// `shmem_setattr` takes them where overlayfs sets the temporary file's
    /// times before naming it.
    pub(crate) fn setattr(&self, attr: &Iattr) {
        lock(&self.data).times.apply(attr);
    }
}
