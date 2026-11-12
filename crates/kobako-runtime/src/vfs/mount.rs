//! One Filesystem mounted, as Linux's `vfsmount`: what a walk stands on and
//! what it allows, and nothing of a path it is found at.

use std::sync::Arc;

use super::dentry::Dentry;
use super::super_block::{SuperBlock, MADE_ROOT};
use super::Errno;

/// A Filesystem instance as the VFS walks it, as `vfsmount` holds its root,
/// its super block, and `mnt_flags`.
pub struct VfsMount {
    /// Where every walk on it starts, as `mnt_root` holds the super block's
    /// root.
    root: Arc<Dentry>,
    sb: Arc<SuperBlock>,
    read_only: bool,
}

impl VfsMount {
    /// `sb` mounted at its root, allowing only reads where `read_only` says.
    pub(crate) fn new(sb: Arc<SuperBlock>, read_only: bool) -> Arc<Self> {
        Arc::new(Self {
            root: sb.root().expect(MADE_ROOT).clone(),
            sb,
            read_only,
        })
    }

    /// The dentry this mount stands on, as `mnt_root`.
    pub(crate) fn root(&self) -> &Arc<Dentry> {
        &self.root
    }

    /// `sb` mounted for the runtime's own use, as `kern_mount` mounts one no
    /// path reaches.
    pub(crate) fn kern_mount(sb: Arc<SuperBlock>) -> Arc<Self> {
        Self::new(sb, false)
    }

    /// The Filesystem instance mounted, as `mnt_sb` names it.
    pub fn sb(&self) -> &Arc<SuperBlock> {
        &self.sb
    }

    /// Whether a change beneath it is refused, by the mount or by its super
    /// block, as `__mnt_is_readonly` asks both.
    pub fn is_readonly(&self) -> bool {
        self.read_only || self.sb.rdonly()
    }

    /// Refuse a change beneath it where it allows only reads, as
    /// `mnt_want_write` does before any security hook is asked.
    pub(crate) fn mnt_want_write(&self) -> Result<(), Errno> {
        match self.is_readonly() {
            true => Err(Errno::ReadOnlyFilesystem),
            false => Ok(()),
        }
    }
}
