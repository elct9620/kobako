//! What a name stands for, apart from where it stands, as Linux's
//! `struct inode` is what a dentry is instantiated with.

use std::any::Any;
use std::ops::BitOr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use super::dentry::Dentry;
use super::file::FileOperations;
use super::{Errno, FileType, Iattr, Kstat, OpenHow};
use crate::locking::{RwSemaphore, RwSemaphoreGuard};

/// What an `atomic_open` did with the name it was asked to open, as Linux's
/// `->atomic_open` ends in `finish_open` or `finish_no_open`. `created` says
/// whether it made the file, as `FMODE_CREATED` does: one it found standing is
/// opened as any open of an entry that stood, and one it made asks for nothing
/// but the open.
#[non_exhaustive]
pub enum Finish {
    /// It opened the file too, as `finish_open` does.
    Open {
        file: Box<dyn FileOperations>,
        created: bool,
    },
    /// It found the entry stored under the other spelling `name` and opened
    /// it, as `finish_open` is handed the dentry `d_add_ci` returned, for the
    /// VFS to record `inode` on that spelling's dentry.
    Spelled {
        file: Box<dyn FileOperations>,
        name: String,
        inode: Arc<Inode>,
    },
    /// It left the open to the VFS, as `finish_no_open` hands the dentry back.
    NoOpen { created: bool },
    /// It looked the name up and left the rest to the VFS, as `finish_no_open`
    /// hands back the dentry a lookup returned, for the VFS to record as it
    /// records what `lookup` answers.
    Lookup(Lookup),
}

/// What one name in a directory turned out to be, as `->lookup` answers.
pub enum Lookup {
    /// The entry standing at the name, for the runtime to record on the
    /// dentry, as `d_splice_alias` records it. A directory handed anew under
    /// the identity of one in use is that one, as `iget5_locked` finds it, so
    /// it keeps its one dentry and its lock.
    Found(Arc<Inode>),
    /// Nothing stands at the name, as a negative dentry records.
    Absent,
    /// The entry the name reaches, stored under the other spelling `name`, as
    /// `d_add_ci` is handed the inode found and the case-exact name, so a name
    /// a host reaches by more than one spelling has one dentry, one lock, and
    /// one spelling a security hook judges.
    Spelled { name: String, inode: Arc<Inode> },
}

/// What one entry of a Filesystem answers: a directory for the names in it,
/// a link for where it points, and any entry for itself, as Linux's
/// `inode_operations` hang from each inode. An operation an entry does not
/// have answers by default as Linux's VFS answers a NULL in its table. One
/// Filesystem may wrap another and answer by
/// delegating, wrapping each entry its `lookup` finds, so it sees every call
/// on the way through. It forwards every operation the one beneath keeps, or
/// the default answers in that one's place. A directory keeps its identity its
/// own while it is open, as an open descriptor keeps a host's inode from being
/// reused.
///
/// The VFS walks a path one name at a time and follows a link
/// itself, as the VFS above a filesystem does, so a link never leads a call
/// out of the directory a descriptor opened. Every call hands the dentry it
/// is about; a Filesystem answering by path reads `Dentry::path_raw`, so a
/// directory moved through the runtime is asked at its new path. A call
/// making, removing, or moving a name runs under its directory's lock.
pub trait InodeOperations: Any + Send + Sync {
    /// Whether a call may do what `mask` asks of this entry, which `dentry`
    /// names, as `->permission` answers `inode_permission`. The VFS asks where
    /// Linux does and holds no mode to judge by, so an entry without it allows
    /// the call, and the backend answers once it is made, as FUSE leaves a
    /// server to without `default_permissions`.
    fn permission(&self, _dentry: &Dentry, _mask: Mask) -> Result<(), Errno> {
        Ok(())
    }

    /// What this entry, which `dentry` names, reports of itself, as
    /// `getattr` is handed the path it answers for, even once its name is
    /// gone and only an open file holds it, as `fstat` asks the same. A
    /// Filesystem keeps what that answer takes on the `Inode`. A link reports
    /// itself.
    fn getattr(&self, dentry: &Dentry) -> Result<Kstat, Errno>;

    /// Apply `attr` to this entry, even once its name is gone and only an
    /// open file holds it, as `ftruncate`, `open` with `O_TRUNC`, and
    /// `utimensat` reach `->setattr`. One whose `open` already truncated
    /// answers success where `attr.open` says so. An entry without it refuses a
    /// size as invalid input and times as unsupported.
    fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Result<(), Errno> {
        match attr.size {
            Some(_) => Err(Errno::InvalidInput),
            None => Err(Errno::Unsupported),
        }
    }

    /// What stands at the name `dentry` carries in this directory. The
    /// runtime asks once for a dentry naming nothing yet, and an entry
    /// without it is no directory to walk through. A directory found answers as
    /// an `Inode::new` carrying its identity, or a Policy cannot tell it from
    /// one made in its place.
    fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, Errno> {
        Err(Errno::NotADirectory)
    }

    /// Where this link, which `dentry` names, points, as `get_link` answers,
    /// for the VFS to follow. Anything else refuses as `readlinkat` does.
    fn get_link(&self, _dentry: &Dentry) -> Result<String, Errno> {
        Err(Errno::InvalidInput)
    }

    /// Open this entry, which `dentry` names, as `how` asks, as `f_op->open`
    /// makes the open file reading its `f_flags`. The VFS empties a file an
    /// open asks to truncate through `setattr` afterwards, and refuses a link
    /// before it asks. An entry without it opens as Linux's opens with no
    /// `f_op->open`, answering each call on the open file as `FileOperations`
    /// does by default.
    fn open(&self, _dentry: &Dentry, _how: &OpenHow) -> Result<Box<dyn FileOperations>, Errno> {
        Ok(Box::new(EmptyFops))
    }

    /// Make the file `dentry` names in this directory and record it on `dentry`,
    /// as `->create` calls `d_instantiate`. A directory without it answers
    /// EACCES, as Linux's VFS does.
    fn create(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::PermissionDenied)
    }

    /// Look up, make, and open the last name of an open in one step, as
    /// `->atomic_open` takes `open_flag` for a backend that does it at once. The
    /// VFS asks it for a `dentry` still in lookup, which is looked up first, and
    /// for a negative one an open may make; `how` asks to make a file only
    /// where the open may, and for a directory where the name ends in a slash,
    /// as Linux adds `O_DIRECTORY` for `LOOKUP_DIRECTORY`. A directory without it looks the name up, then makes
    /// the file through `create` and leaves the open to the VFS, which asks
    /// `may_open` and the security modules first, as Linux's `lookup_open` calls
    /// `->lookup`, then `->create`, and `do_open` opens what it made.
    fn atomic_open(&self, dentry: &Dentry, how: &OpenHow) -> Result<Finish, Errno> {
        if dentry.in_lookup() {
            let found = self.lookup(dentry)?;
            if !how.create || !matches!(found, Lookup::Absent) {
                return Ok(Finish::Lookup(found));
            }
        }
        if !how.create {
            return Ok(Finish::NoOpen { created: false });
        }
        self.create(dentry)?;
        Ok(Finish::NoOpen { created: true })
    }

    /// Make the directory `dentry` names in this one, recording it on `dentry`
    /// where it can, as `->mkdir` calls `d_instantiate`. One that records nothing
    /// answers for the name at the next walk, through its `revalidate` or by
    /// keeping no name, as Linux leaves a negative dentry to its Filesystem. A
    /// directory without it, or without any change below, answers EPERM, as
    /// Linux's VFS does.
    fn mkdir(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }

    /// Give the entry `old` names another name, the one `dentry` carries in
    /// this directory. A directory without it answers EPERM, as Linux's VFS
    /// does.
    fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }

    /// Make the link `dentry` names in this directory, pointing at `target`.
    /// A directory without it answers EPERM, as Linux's VFS does.
    fn symlink(&self, _dentry: &Dentry, _target: &str) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }

    /// Remove the empty directory `dentry` names in this one. A directory
    /// without it answers EPERM, as Linux's VFS does.
    fn rmdir(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }

    /// Remove the file `dentry` names in this directory. A directory without
    /// it answers EPERM, as Linux's VFS does.
    fn unlink(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }

    /// Move the entry `from` names in this directory to `to` in `to_dir`,
    /// refusing what rename(2) refuses at `to`. The runtime has already
    /// checked what stands at `from` and never hands a move onto its own
    /// name, as Linux's VFS does before `inode_operations.rename`. It hands
    /// only a `to_dir` of the same mount, so no two instances meet here, as
    /// `filename_renameat2` answers EXDEV first. One that takes no `flag`
    /// refuses any as invalid input, as Linux's filesystems refuse a `renameat2`
    /// flag they do not know. A directory without it answers EPERM, as
    /// Linux's VFS does.
    fn rename(
        &self,
        _from: &Dentry,
        _to_dir: &dyn InodeOperations,
        _to: &Dentry,
        _flag: Option<RenameFlag>,
    ) -> Result<(), Errno> {
        Err(Errno::NotPermitted)
    }
}

/// What a call asks of an entry, as Linux's `MAY_*` bits in the mask
/// `inode_permission` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask(u8);

impl Mask {
    /// Asking nothing, as an `acc_mode` of 0.
    pub(crate) const NONE: Self = Self(0);
    /// Passing through a directory, as `MAY_EXEC`.
    pub const EXEC: Self = Self(1);
    /// Changing it, as `MAY_WRITE`.
    pub const WRITE: Self = Self(1 << 1);
    /// Reading it, as `MAY_READ`.
    pub const READ: Self = Self(1 << 2);
    /// Opening it, as `MAY_OPEN`.
    pub const OPEN: Self = Self(1 << 3);
    /// Changing it only at its end, as `MAY_APPEND`, which comes with `WRITE`
    /// so a module can tell an append from any other write.
    pub const APPEND: Self = Self(1 << 4);

    /// Whether this holds all of `mask`.
    pub fn contains(self, mask: Self) -> bool {
        self.0 & mask.0 == mask.0
    }
}

impl BitOr for Mask {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What a move asks of its target, as `renameat2`'s flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenameFlag {
    /// Refuse the move when anything stands at the target.
    NoReplace,
    /// Swap the source and the target, both of which must stand.
    Exchange,
    /// Leave a whiteout where the source stood.
    Whiteout,
}

/// What an entry without `open` opens as, as Linux's `empty_fops` is a file with
/// no operations.
pub(super) struct EmptyFops;

impl FileOperations for EmptyFops {}

/// What an entry is, apart from where it stands: the device and inode
/// numbers its Filesystem names it by, as Linux reads `i_ino` and `s_dev`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    dev: u64,
    ino: u64,
}

impl Identity {
    /// The identity `stat` names, `None` where its inode of zero reports none.
    pub fn of(stat: &Kstat) -> Option<Self> {
        Self::from_numbers(stat.dev, stat.ino)
    }

    /// The identity `dev` and `ino` name, `None` where an inode of zero
    /// reports none.
    pub(crate) fn from_numbers(dev: u64, ino: u64) -> Option<Self> {
        (ino != 0).then_some(Self { dev, ino })
    }

    /// `stat`, naming this identity.
    pub fn onto(self, stat: Kstat) -> Kstat {
        stat.with_identity(self.dev, self.ino)
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
    /// The dentry a directory stands at, as Linux keeps a directory's one
    /// dentry in `i_dentry`.
    dentry: Mutex<Weak<Dentry>>,
}

impl Inode {
    /// The entry `stat` reports, answered for by `ops`.
    pub fn new(stat: &Kstat, ops: Arc<dyn InodeOperations>) -> Arc<Self> {
        Self::of_kind(stat.file_type, Identity::of(stat), ops)
    }

    /// This entry answered for by `ops`, as a Filesystem wrapping another
    /// wraps each entry that one's lookup finds.
    pub fn with_ops(&self, ops: Arc<dyn InodeOperations>) -> Arc<Self> {
        Self::of_kind(self.kind, self.identity, ops)
    }

    fn of_kind(
        kind: FileType,
        identity: Option<Identity>,
        ops: Arc<dyn InodeOperations>,
    ) -> Arc<Self> {
        Arc::new(Self {
            kind,
            identity,
            ops,
            private: OnceLock::new(),
            rwsem: Arc::default(),
            dead: AtomicBool::new(false),
            dentry: Mutex::default(),
        })
    }

    /// The dentry other than `dentry` this directory already stands at, as
    /// `__d_find_dir_alias` finds it, or none with this entry recorded on
    /// `dentry`, as `__d_add` records it. Both run under one lock, as
    /// `d_splice_alias` holds `i_lock`, so two lookups of one directory never
    /// each find none, and none finds `dentry` before a walk can.
    pub(super) fn find_alias_or_add(self: &Arc<Self>, dentry: &Dentry) -> Option<Arc<Dentry>> {
        let mut held = self.dentry();
        if let Some(found) = held.upgrade() {
            if !std::ptr::eq(found.as_ref(), dentry) {
                return Some(found);
            }
        }
        dentry.__instantiate(self.clone(), &mut held);
        dentry.rehash();
        None
    }

    /// Record this entry on `dentry` and let a walk find it by its name, as
    /// `d_add` does under `i_lock`.
    pub(super) fn add_hashed(self: &Arc<Self>, dentry: &Dentry) {
        let mut held = self.dentry();
        dentry.__instantiate(self.clone(), &mut held);
        dentry.rehash();
    }

    pub(super) fn dentry(&self) -> MutexGuard<'_, Weak<Dentry>> {
        self.dentry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold it shared where nothing holds it exclusive, without waiting, as
    /// Linux's `inode_trylock_shared`.
    pub(super) fn trylock_shared(&self) -> Option<RwSemaphoreGuard> {
        self.rwsem.down_read_trylock()
    }

    /// Hold it shared, as Linux's `inode_lock_shared` holds the directory it
    /// looks a name up in, so no change through the VFS lands between the
    /// name's dentry and what the Filesystem answers for it.
    pub(super) fn lock_shared(&self) -> Result<RwSemaphoreGuard, Errno> {
        Ok(self.rwsem.down_read()?)
    }

    /// Hold it exclusive, as Linux's `inode_lock` holds a directory while a
    /// name in it is made, removed, or moved.
    pub(super) fn lock(&self) -> Result<RwSemaphoreGuard, Errno> {
        Ok(self.rwsem.down_write()?)
    }

    /// Whether `held` holds its lock, as Linux's `inode_is_locked` asks.
    pub(super) fn is_locked_by(&self, held: &RwSemaphoreGuard) -> bool {
        held.holds(&self.rwsem)
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
