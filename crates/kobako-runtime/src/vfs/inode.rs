//! What a name stands for, apart from where it stands, as Linux's
//! `struct inode` is what a dentry is instantiated with.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use super::dentry::{Dentry, DentryOperations};
use super::file::FileOperations;
use super::{FileType, FilesystemError, Iattr, Metadata, OpenOptions};
use crate::locking::{RwSemaphore, RwSemaphoreGuard};

/// What one name in a directory turned out to be, as `->lookup` answers.
pub enum Lookup {
    /// The entry standing at the name, for the runtime to record on the
    /// dentry, as `d_splice_alias` records it.
    Found(Inode),
    /// Nothing stands at the name, as a negative dentry records.
    Absent,
    /// The entry the name reaches is stored under this other spelling, as
    /// `d_add_ci` is handed the case-exact name, so a name a host reaches by
    /// more than one spelling has one dentry, one lock, and one spelling a
    /// Policy judges.
    Spelled(String),
}

/// What one entry of a Filesystem answers: a directory for the names in it,
/// a link for where it points, and any entry for itself, as Linux's
/// `inode_operations` hang from each inode. An operation an entry does not
/// have answers `None`, as a NULL in Linux's table, and the runtime answers
/// for it as Linux's VFS does. One Filesystem may wrap another and answer by
/// delegating, wrapping each entry its `lookup` finds, so it sees every call
/// on the way through. A directory keeps its identity its own while it is
/// open, as an open descriptor keeps a host's inode from being reused.
///
/// The VFS walks a path one name at a time and follows a link
/// itself, as the VFS above a filesystem does, so a link never leads a call
/// out of the directory a descriptor opened. Every Invocation reaches the
/// same Filesystem; one that should start afresh is handed in its place for
/// that Invocation.
pub trait InodeOperations: Any + Send + Sync {
    /// What this entry, which `dentry` names, reports of itself, as
    /// `getattr` is handed the path it answers for, even once its name is
    /// gone and only an open file holds it, as `fstat` asks the same. A link
    /// reports itself.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError>;

    /// Apply `attr` to this entry, even once its name is gone and only an
    /// open file holds it, as `ftruncate` and `utimensat` reach `->setattr`.
    /// An entry without it refuses a size as invalid input and times as
    /// unsupported.
    fn setattr(&self, _dentry: &Dentry, _attr: &Iattr) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// What stands at the name `dentry` carries in this directory. The
    /// runtime asks once for a dentry naming nothing yet, and an entry
    /// without it is no directory to walk through.
    fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        None
    }

    /// What every dentry of this Filesystem answers through, asked of its
    /// root once, as a driver's `fill_super` calls `set_default_d_op`. A
    /// Filesystem changed only through the runtime need not have one.
    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        None
    }

    /// Where this link, which `dentry` names, points, as `get_link` answers,
    /// for the VFS to follow. Anything else
    /// without it refuses as `readlinkat` does.
    fn get_link(&self, _dentry: &Dentry) -> Option<Result<String, FilesystemError>> {
        None
    }

    /// Open this entry, which `dentry` names, as `options` asks, truncating
    /// a file where they say to, as `f_op->open` makes the open file. A
    /// directory opens to be listed; without it, listing one answers
    /// `NotADirectory`. The runtime refuses a link before it asks.
    fn open(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        None
    }

    /// Make the file `dentry` names in this directory, refusing one that
    /// already stands, and open it as `options` asks, in one step as
    /// `->atomic_open` does. Without it the runtime answers `PermissionDenied`,
    /// as Linux's VFS answers EACCES.
    fn atomic_open(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        None
    }

    /// Make the directory `dentry` names in this one. Without it, and
    /// without `rmdir`, `unlink`, or `rename`, the runtime answers
    /// `NotPermitted`, as Linux's VFS answers EPERM.
    fn mkdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Give the entry `old` names another name, the one `dentry` carries in
    /// this directory. Without it the runtime answers `NotPermitted`, as
    /// Linux's `vfs_link` answers EPERM.
    fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Make the link `dentry` names in this directory, pointing at `target`.
    /// Without it the runtime answers `NotPermitted`, as Linux's
    /// `vfs_symlink` answers EPERM.
    fn symlink(&self, _dentry: &Dentry, _target: &str) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Remove the empty directory `dentry` names in this one.
    fn rmdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Remove the file `dentry` names in this directory.
    fn unlink(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        None
    }

    /// Move the entry `from` names in this directory to `to` in `to_dir`,
    /// refusing what rename(2) refuses at `to`. The runtime has already
    /// checked what stands at `from` and never hands a move onto its own
    /// name, as Linux's VFS does before `inode_operations.rename`. It hands
    /// only a `to_dir` of the same Mount, so it is of this Filesystem's own
    /// type, and one that is not answers `CrossesDevices`.
    fn rename(
        &self,
        _from: &Dentry,
        _to_dir: &dyn InodeOperations,
        _to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        None
    }
}

/// What a directory is, apart from where it stands: the device and inode
/// numbers its Filesystem names it by, as Linux reads `i_ino` and `s_dev`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    /// The identity `metadata` names, `None` where its inode of zero reports
    /// none.
    pub fn of(metadata: &Metadata) -> Option<Self> {
        Self::from_numbers(metadata.dev, metadata.ino)
    }

    /// The identity `dev` and `ino` name, `None` where an inode of zero
    /// reports none.
    pub(crate) fn from_numbers(dev: u64, ino: u64) -> Option<Self> {
        (ino != 0).then_some(Self { dev, ino })
    }

    /// `metadata`, naming this identity.
    pub fn onto(self, metadata: Metadata) -> Metadata {
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
