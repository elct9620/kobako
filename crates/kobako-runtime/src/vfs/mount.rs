//! One Filesystem mounted, as Linux's `vfsmount`: what a walk stands on and
//! what it allows, and nothing of where a guest finds it.

use std::sync::Arc;

use super::super_block::SuperBlock;

/// What the guest may do beneath a Mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// Why a call was refused before it reached the Filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    /// A change beneath a mount allowing only reads.
    ReadOnly,
    /// A security hook said no.
    Refused,
}

/// A Filesystem instance as the VFS walks it, as `vfsmount` holds its super
/// block and `mnt_flags`.
pub struct VfsMount {
    sb: Arc<SuperBlock>,
    read_only: bool,
}

impl VfsMount {
    /// `sb` mounted, allowing only reads where `read_only` says.
    pub fn new(sb: Arc<SuperBlock>, read_only: bool) -> Arc<Self> {
        Arc::new(Self { sb, read_only })
    }

    /// `sb` mounted for the runtime's own use, as `kern_mount` mounts one no
    /// guest path reaches.
    pub(crate) fn kern_mount(sb: Arc<SuperBlock>) -> Arc<Self> {
        Self::new(sb, false)
    }

    /// The Filesystem instance mounted, as `mnt_sb` names it.
    pub fn sb(&self) -> &Arc<SuperBlock> {
        &self.sb
    }

    /// Whether a change beneath it is refused.
    pub fn is_readonly(&self) -> bool {
        self.read_only
    }

    /// Refuse a change beneath it where it allows only reads, as
    /// `mnt_want_write` does before any security hook is asked.
    pub(crate) fn mnt_want_write(&self) -> Result<(), Denial> {
        match self.read_only {
            true => Err(Denial::ReadOnly),
            false => Ok(()),
        }
    }
}
