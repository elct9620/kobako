//! What a name stands for, apart from where it stands, as Linux's
//! `struct inode` is what a dentry is instantiated with.

use std::any::Any;
use std::sync::{Arc, OnceLock};

use crate::filesystem::{FileType, InodeOperations, Metadata};

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
}

/// One entry of a Filesystem, as a lookup finds it for the runtime to record
/// on the dentry naming it: what it is, and what answers for it, as Linux's
/// inode carries `i_mode`, `i_ino`, and `i_op`.
pub struct Inode {
    /// What kind of entry it is; `None` for one its Filesystem answers for
    /// by path.
    kind: Option<FileType>,
    /// `None` where the Filesystem names no inode number.
    identity: Option<Identity>,
    ops: Arc<dyn InodeOperations>,
    /// What the Filesystem keeps on the entry once it needs it, set once,
    /// as Linux's `i_private`, reached the same however its operations are
    /// wrapped.
    private: OnceLock<Arc<dyn Any + Send + Sync>>,
}

impl Inode {
    /// The entry `metadata` reports, answered for by `ops`.
    pub fn new(metadata: &Metadata, ops: Arc<dyn InodeOperations>) -> Self {
        Self {
            kind: Some(metadata.file_type),
            identity: Identity::of(metadata),
            ops,
            private: OnceLock::new(),
        }
    }

    /// An entry `ops` answers for by the path each call names, so the name
    /// always means what stands there now, and a lookup need not ask what
    /// that is.
    pub fn by_path(ops: Arc<dyn InodeOperations>) -> Self {
        Self {
            kind: None,
            identity: None,
            ops,
            private: OnceLock::new(),
        }
    }

    /// This entry answered for by `ops`, as a Filesystem wrapping another
    /// wraps each entry that one's lookup finds.
    pub fn with_ops(&self, ops: Arc<dyn InodeOperations>) -> Self {
        Self {
            kind: self.kind,
            identity: self.identity,
            ops,
            private: OnceLock::new(),
        }
    }

    /// The root of a Filesystem, which a lookup never finds.
    pub(super) fn root(ops: Arc<dyn InodeOperations>) -> Self {
        Self {
            kind: Some(FileType::Directory),
            identity: None,
            ops,
            private: OnceLock::new(),
        }
    }

    /// Whether its Filesystem answers for it by the path each call names.
    pub fn answers_by_path(&self) -> bool {
        self.kind.is_none()
    }

    pub fn kind(&self) -> Option<FileType> {
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
