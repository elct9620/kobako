//! What a name stands for, apart from where it stands, as Linux's
//! `struct inode` is what a dentry is instantiated with.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::filesystem::{FileType, FilesystemError, InodeOperations, Metadata};
use crate::locking::{RwSemaphore, RwSemaphoreGuard};

/// What a directory is, apart from where it stands: the device and inode
/// numbers its Filesystem names it by, as Linux reads `i_ino` and `s_dev`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    /// The identity `metadata` names, `None` where its inode of zero reports
    /// none.
    pub fn of(metadata: &Metadata) -> Option<Self> {
        (metadata.ino != 0).then_some(Self {
            dev: metadata.dev,
            ino: metadata.ino,
        })
    }

    /// `metadata`, naming this identity.
    pub(crate) fn onto(self, metadata: Metadata) -> Metadata {
        metadata.with_identity(self.dev, self.ino)
    }
}

/// One entry of a Filesystem, as a lookup finds it for the runtime to record
/// on the dentry naming it: what it is, and what answers for it, as Linux's
/// inode carries `i_mode`, `i_ino`, and `i_op`.
pub struct Inode {
    /// What kind of entry it is.
    kind: FileType,
    /// `None` where the Filesystem names no inode number.
    identity: Option<Identity>,
    ops: Arc<dyn InodeOperations>,
    /// What the Filesystem keeps on the entry once it needs it, set once,
    /// as Linux's `i_private`, reached the same however its operations are
    /// wrapped.
    private: OnceLock<Arc<dyn Any + Send + Sync>>,
    /// The lock calls on the entry run under, as Linux's `i_rwsem`.
    rwsem: Arc<RwSemaphore>,
    /// Whether the directory was removed or replaced through the VFS, as
    /// Linux sets `S_DEAD` in `i_flags`.
    dead: AtomicBool,
}

impl Inode {
    /// The entry `metadata` reports, answered for by `ops`.
    pub fn new(metadata: &Metadata, ops: Arc<dyn InodeOperations>) -> Self {
        Self::of_kind(metadata.file_type, Identity::of(metadata), ops)
    }

    /// This entry answered for by `ops`, as a Filesystem wrapping another
    /// wraps each entry that one's lookup finds.
    pub fn with_ops(&self, ops: Arc<dyn InodeOperations>) -> Self {
        Self::of_kind(self.kind, self.identity, ops)
    }

    /// The root of a Filesystem, which a lookup never finds.
    pub(super) fn root(ops: Arc<dyn InodeOperations>) -> Self {
        Self::of_kind(FileType::Directory, None, ops)
    }

    fn of_kind(kind: FileType, identity: Option<Identity>, ops: Arc<dyn InodeOperations>) -> Self {
        Self {
            kind,
            identity,
            ops,
            private: OnceLock::new(),
            rwsem: Arc::default(),
            dead: AtomicBool::new(false),
        }
    }

    /// Hold it shared, as Linux's `inode_lock_shared` holds the directory it
    /// looks a name up in, so no change through the VFS lands between the
    /// name's dentry and what the Filesystem answers for it.
    pub(super) fn lock_shared(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        Ok(self.rwsem.down_read()?)
    }

    /// Hold it exclusive, as Linux's `inode_lock` holds a directory while a
    /// name in it is made, removed, or moved.
    pub(super) fn lock(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        Ok(self.rwsem.down_write()?)
    }

    /// Whether the directory was removed or replaced, as `IS_DEADDIR`.
    pub(super) fn is_deaddir(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    /// The directory was removed or replaced, so nothing is made in it any
    /// more.
    pub(super) fn mark_dead(&self) {
        self.dead.store(true, Ordering::SeqCst);
    }

    pub fn kind(&self) -> FileType {
        self.kind
    }

    pub fn identity(&self) -> Option<Identity> {
        self.identity
    }

    /// What answers for the entry, as Linux's `i_op`.
    pub fn ops(&self) -> &Arc<dyn InodeOperations> {
        &self.ops
    }

    /// What the Filesystem keeps on the entry, set once, as Linux's
    /// `i_private`.
    pub fn private(&self) -> &OnceLock<Arc<dyn Any + Send + Sync>> {
        &self.private
    }
}
