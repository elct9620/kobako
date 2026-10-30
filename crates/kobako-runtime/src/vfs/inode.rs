//! What a name stands for, apart from where it stands, as Linux's
//! `struct inode` is what a dentry is instantiated with.

use crate::filesystem::{FileType, Metadata};

/// What a directory is, apart from where it stands: the device and inode
/// numbers its Filesystem names it by, as Linux reads `i_ino` and `s_dev`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    dev: u64,
    ino: u64,
}

/// One entry of a Filesystem, as a lookup finds it and hands it to the
/// dentry naming it through `Dentry::d_add`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inode {
    /// What kind of entry it is, as Linux reads `i_mode`; `None` for an
    /// entry its Filesystem answers for by path.
    kind: Option<FileType>,
    /// `None` where the Filesystem names no inode number.
    identity: Option<Identity>,
}

impl Inode {
    /// An entry its Filesystem answers for by the path each call names, so
    /// the name always means what stands there now, and a lookup need not
    /// ask what that is.
    pub fn by_path() -> Self {
        Self {
            kind: None,
            identity: None,
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
}

/// The entry `metadata` reports, its inode of zero naming no identity.
impl From<&Metadata> for Inode {
    fn from(metadata: &Metadata) -> Self {
        Self {
            kind: Some(metadata.file_type),
            identity: (metadata.ino != 0).then_some(Identity {
                dev: metadata.dev,
                ino: metadata.ino,
            }),
        }
    }
}
