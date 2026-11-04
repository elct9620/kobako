//! The names of one Filesystem a walk has reached, each kept by the
//! directory above it and its name there while something holds it, as
//! Linux's dentries are: a directory moved through the VFS takes every one
//! beneath it along, and one removed or replaced through it is dead. A name
//! found standing stays for the next walk, as Linux's dcache keeps it, up to a
//! bound; the walk asks `revalidate` before trusting it, so what changed behind
//! the runtime's back is found again.

use std::any::Any;
use std::num::NonZeroUsize;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::thread::ThreadId;

use lru::LruCache;

use super::file::FileOperations;
use super::inode::{Inode, InodeOperations, Lookup};
use super::{FileType, FilesystemError, Kstat, OpenHow, SuperBlock};
use crate::hashtable::Hashtable;
use crate::locking::seqlock::SeqLock;
use crate::locking::RwSemaphoreGuard;
use crate::sched;

/// What a Filesystem answers about a name rather than an entry, as Linux's
/// `dentry_operations`: a name may stand for nothing, so no inode can answer
/// for it.
pub trait DentryOperations: Send + Sync {
    /// Whether `dentry`, a name the runtime still holds in `dir`, names what
    /// it recorded, as `d_revalidate` answers; `false` has the runtime drop
    /// it and take the name afresh.
    fn revalidate(
        &self,
        dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError>;

    /// Whether `dentry` goes once nothing holds it, rather than stay for the
    /// next walk, as `d_delete` answers. A name whose `revalidate` cannot tell
    /// a change must answer `true`, since it would be trusted when reached again.
    fn delete(&self, _dentry: &Dentry) -> bool {
        false
    }
}

/// The most names a Filesystem keeps that nothing holds.
const MAX_CACHED_DENTRIES: NonZeroUsize = NonZeroUsize::new(1024).expect("the bound is not zero");

/// Every move of a dentry, as Linux's `rename_lock`, so a walk up its
/// parents answers where the dentry stood at one instant, and a walk learns of a
/// move landing meanwhile.
pub(super) static RENAME_LOCK: SeqLock = SeqLock::new();

/// One name of a Filesystem, and where it stands now. Only the VFS makes
/// one, so a Filesystem reads the name a call is about from it.
pub struct Dentry {
    state: Mutex<State>,
    /// This dentry, for whoever is handed it to keep, as Linux's `dget`
    /// takes a reference to any dentry it is handed.
    this: Weak<Dentry>,
    /// The tables of its Filesystem instance a walk finds it in, as `d_sb`
    /// reaches the instance a dentry belongs to.
    hashtables: Arc<Hashtables>,
    /// What the Filesystem keeps on the name, set once, as Linux's
    /// `d_fsdata` holds a filesystem's own state for a dentry.
    fsdata: OnceLock<Arc<dyn Any + Send + Sync>>,
    /// What answers about this name, as `d_alloc` copies `s_d_op` from the
    /// super block.
    ops: Option<Arc<dyn DentryOperations>>,
    /// Wakes the walks waiting while another looks this name up, as Linux's
    /// `d_wait_lookup` waits for `DCACHE_PAR_LOOKUP` to clear.
    wait: Condvar,
}

/// Where a walk asking for a lookup stands, as `d_alloc_parallel` answers.
enum InLookup {
    /// A lookup already answered for the name.
    Answered,
    /// This walk asks, and ends the lookup once answered.
    Begun,
    /// This walk's own lookup of the name is under way, and asks again within it.
    Within,
}

struct State {
    /// The directory above, `None` where the dentry is a root, which stands as
    /// its own parent, as Linux's `d_parent == self`, held without a cycle.
    parent: Option<Arc<Dentry>>,
    name: String,
    /// What the name stands for once a lookup found it, `None` while the
    /// dentry is negative.
    inode: Option<Arc<Inode>>,
    /// Where the lookup of the name stands, so a negative dentry says nothing
    /// stands there rather than that nobody asked.
    asked: LookupState,
    /// Whether a walk finds it by its name, as Linux's `d_hash` links it into
    /// `dentry_hashtable`; one still in lookup, or let go of, is not.
    hashed: bool,
}

/// Where the lookup of a name stands, as `DCACHE_PAR_LOOKUP` marks a dentry a
/// walk is asking its Filesystem about and `d_add` ends that.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LookupState {
    /// No lookup answered for the name: none asked yet, or the one that did
    /// ended without an answer here, as `d_lookup_done` leaves a dentry.
    Unasked,
    /// In lookup for the walk that made the dentry, which has yet to ask, as
    /// `d_alloc_parallel` marks the dentry it makes.
    Allocated(ThreadId),
    /// The walk asking the Filesystem what stands here, while it asks.
    Asking(ThreadId),
    /// A lookup answered for the name, or a call made what stands there.
    Answered,
}

impl Dentry {
    fn new(
        parent: Option<Arc<Dentry>>,
        name: &str,
        ops: Option<Arc<dyn DentryOperations>>,
        hashtables: &Arc<Hashtables>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                parent,
                name: name.to_string(),
                inode: None,
                asked: LookupState::Unasked,
                hashed: false,
            }),
            this: this.clone(),
            hashtables: hashtables.clone(),
            fsdata: OnceLock::new(),
            ops,
            wait: Condvar::new(),
        })
    }

    /// What answers about this name, the same for every dentry of one
    /// Filesystem.
    pub fn ops(&self) -> Option<&Arc<dyn DentryOperations>> {
        self.ops.as_ref()
    }

    /// The directory above, this dentry itself at a root, as Linux's
    /// `dget_parent` takes it.
    pub(crate) fn parent(&self) -> Arc<Dentry> {
        self.parent_and_name().0
    }

    /// Whether this dentry is a root, standing as its own parent, as Linux's
    /// `IS_ROOT` answers.
    pub(crate) fn is_root(&self) -> bool {
        self.state().parent.is_none()
    }

    /// The directory above and the name there, read together, so a move
    /// landing meanwhile cannot pair one's parent with the other's name, as 9p
    /// holds `rename_sem` to read both.
    pub(crate) fn parent_and_name(&self) -> (Arc<Dentry>, String) {
        let state = self.state();
        let parent = state.parent.clone().unwrap_or_else(|| self.get());
        (parent, state.name.clone())
    }

    /// This dentry, kept for as long as the caller holds it, as Linux's
    /// `dget` does.
    pub(crate) fn get(&self) -> Arc<Dentry> {
        self.this
            .upgrade()
            .expect("a dentry handed out is held by whoever handed it")
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The name in the directory above, empty for the root.
    pub fn name(&self) -> String {
        self.state().name.clone()
    }

    /// Where this name stands now beneath the Filesystem's root, empty for
    /// the root, as Linux's `dentry_path_raw` answers.
    pub fn path_raw(&self) -> String {
        self.names().join("/")
    }

    /// Each name from the Filesystem's root down to this directory, as Linux's
    /// `__dentry_path` walks up until `IS_ROOT`.
    pub(crate) fn names(&self) -> Vec<String> {
        RENAME_LOCK.read(|| {
            let mut names = Vec::new();
            let mut at = self.get();
            while !at.is_root() {
                let (parent, name) = at.parent_and_name();
                names.push(name);
                at = parent;
            }
            names.reverse();
            names
        })
    }

    /// Hold the directory this names shared, as
    /// `inode_lock_shared(d_inode(dentry))`.
    pub(crate) fn inode_lock_shared(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.inode().ok_or(FilesystemError::NotFound)?.lock_shared()
    }

    /// Hold the directory this names exclusive, as
    /// `inode_lock(d_inode(dentry))`.
    pub(crate) fn inode_lock(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.inode().ok_or(FilesystemError::NotFound)?.lock()
    }

    /// Whether the directory this names was removed or replaced, as
    /// `IS_DEADDIR(d_inode(dentry))`.
    pub(crate) fn is_deaddir(&self) -> bool {
        self.inode().is_some_and(|inode| inode.is_deaddir())
    }

    /// Mark the directory this names removed or replaced.
    pub(super) fn mark_dead(&self) {
        if let Some(inode) = self.inode() {
            inode.mark_dead();
        }
    }

    /// What the name stands for, `None` while the dentry is negative, as
    /// Linux's `d_inode` reads it.
    pub fn inode(&self) -> Option<Arc<Inode>> {
        self.state().inode.clone()
    }

    /// What answers for the entry this names, as `d_inode(dentry)->i_op`.
    pub(crate) fn inode_ops(&self) -> Result<Arc<dyn InodeOperations>, FilesystemError> {
        match self.inode() {
            Some(inode) => Ok(inode.ops().clone()),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Record `inode` as what the name stands for, `None` where nothing
    /// does, as a Filesystem's lookup hands Linux's `d_add` what it found.
    /// A dentry already naming something keeps it, as Linux instantiates a
    /// dentry once, so a later lookup never hides a change `d_revalidate`
    /// must see.
    pub fn add(&self, inode: Option<Arc<Inode>>) {
        match inode {
            Some(inode) => inode.add_hashed(self),
            None => self.rehash(),
        }
    }

    /// End the lookup of this name with an answer, and let a walk find it by
    /// its name from now on, waking every walk waiting on it, as Linux's
    /// `__d_add` takes it out of `in_lookup_hashtable` and `__d_rehash`es it.
    pub(super) fn rehash(&self) {
        {
            let mut table = self.hashtables.dentry_hashtable();
            let mut in_lookup = self.hashtables.in_lookup_hashtable();
            let (parent, name) = self.parent_and_name();
            let at = key(&parent, &name);
            let this = self.get();
            if in_lookup
                .get(&at)
                .is_some_and(|held| Arc::ptr_eq(&held, &this))
            {
                in_lookup.remove(&at);
            }
            let mut state = self.state();
            state.asked = LookupState::Answered;
            if !state.hashed {
                table.insert(at, &this);
                state.hashed = true;
            }
        }
        self.wait.notify_all();
    }

    /// Whether a walk finds this dentry by its name, as Linux's `!d_unhashed`
    /// answers.
    pub(super) fn hashed(&self) -> bool {
        self.state().hashed
    }

    /// Record `inode` here unless the dentry already names something, a
    /// directory's `held` dentry being this one from then on, as
    /// `__d_instantiate` hangs the dentry on `i_dentry`. The caller holds
    /// `held`, the inode's lock on the dentry it keeps.
    pub(super) fn instantiate(&self, inode: Arc<Inode>, held: &mut Weak<Dentry>) {
        let mut state = self.state();
        if state.inode.is_none() {
            if inode.kind() == FileType::Dir {
                *held = self.this.clone();
            }
            state.inode = Some(inode);
        }
    }

    /// What the Filesystem keeps on the name, set once, as Linux's
    /// `d_fsdata` holds a filesystem's own state for a dentry.
    pub fn fsdata(&self) -> &OnceLock<Arc<dyn Any + Send + Sync>> {
        &self.fsdata
    }

    /// Whether this goes once nothing holds it, as `d_delete` answers; a name
    /// no one answers for stays.
    pub(crate) fn goes_when_unused(&self) -> bool {
        self.ops.as_ref().is_some_and(|ops| ops.delete(self))
    }

    /// Whether both names stand for one entry, as hard links do, which Linux's
    /// `vfs_rename` answers as having nothing to move, comparing `source ==
    /// target`. A Filesystem naming no identity cannot tell.
    pub(crate) fn names_the_entry_of(&self, other: &Dentry) -> bool {
        let identity = |dentry: &Dentry| dentry.inode().and_then(|inode| inode.identity());
        identity(self).is_some() && identity(self) == identity(other)
    }

    /// Whether a walk is still looking the name up, as Linux's `d_in_lookup`
    /// tells a Filesystem's `atomic_open` the dentry it must look up first.
    pub fn in_lookup(&self) -> bool {
        matches!(
            self.state().asked,
            LookupState::Allocated(_) | LookupState::Asking(_)
        )
    }

    /// Ask the directory `dir` of `sb` what stands here where no lookup answered
    /// yet, and record it, as Linux's `lookup_fast` keeps a dentry, a negative
    /// one too, and `__lookup_slow` asks `->lookup` for one just made. The
    /// dentry to use in place of this one is handed back as `d_splice_alias`
    /// hands it, none where this one records the answer.
    pub(crate) fn lookup_inode(
        &self,
        sb: &SuperBlock,
        dir: &Dentry,
    ) -> Result<Option<Arc<Dentry>>, FilesystemError> {
        let asked = self.ask_in_lookup(|| {
            let found = dir.inode_ops()?.lookup(self)?;
            self.splice_alias(sb, found)
        })?;
        Ok(asked.flatten())
    }

    /// What `ask` answers for this name where no lookup answered yet, none where
    /// one did. The walk `d_alloc_parallel` made the dentry for asks, as it
    /// holds the dentry in lookup, and `d_lookup_done` ends that.
    pub(crate) fn ask_in_lookup<T>(
        &self,
        ask: impl FnOnce() -> Result<T, FilesystemError>,
    ) -> Result<Option<T>, FilesystemError> {
        let ends = match self.begin_lookup()? {
            InLookup::Answered => return Ok(None),
            InLookup::Begun => true,
            InLookup::Within => false,
        };
        let asked = ask();
        if ends {
            self.lookup_done();
        }
        asked.map(Some)
    }

    /// Record what a lookup found at this name of `sb`, as Linux's
    /// `d_splice_alias` does, handing back the dentry to use in place of this
    /// one, none where this one records it. A directory already standing at
    /// another dentry keeps that one, moved here, as a directory has one
    /// dentry: a move that would put it beneath itself answers ELOOP, and one
    /// whose locks another call holds answers ESTALE. An entry stored under
    /// another spelling is recorded on that spelling's dentry, as `d_add_ci`
    /// does.
    pub(crate) fn splice_alias(
        &self,
        sb: &SuperBlock,
        found: Lookup,
    ) -> Result<Option<Arc<Dentry>>, FilesystemError> {
        let inode = match found {
            Lookup::Found(inode) => inode,
            Lookup::Absent => {
                self.add(None);
                return Ok(None);
            }
            Lookup::Spelled { name, inode } => return self.add_ci(sb, &name, inode).map(Some),
        };
        match inode.find_alias_or_add(self) {
            None => Ok(None),
            Some(alias) => {
                self.get().unalias(sb, &alias)?;
                Ok(Some(alias))
            }
        }
    }

    /// The dentry of `name`, the spelling the directory above stores the
    /// entry under, with `inode` spliced on it, as Linux's `d_add_ci` finds
    /// that spelling's dentry, or makes one, rather than asking the
    /// Filesystem again. One already held there is taken as it stands, as
    /// `d_hash_and_lookup` asks no `d_revalidate`.
    pub(crate) fn add_ci(
        &self,
        sb: &SuperBlock,
        name: &str,
        inode: Arc<Inode>,
    ) -> Result<Arc<Dentry>, FilesystemError> {
        let found = sb.dentries().alloc_parallel(&self.parent(), name)?;
        let spliced = found.ask_in_lookup(|| found.splice_alias(sb, Lookup::Found(inode)))?;
        Ok(spliced.flatten().unwrap_or(found))
    }

    /// `alias`, where a directory of `sb` stands, moved to this dentry's name,
    /// taking its place, as Linux's `__d_unalias` moves it under `rename_lock`.
    /// One that would stand beneath itself answers ELOOP, as `d_ancestor`
    /// refuses it; so does a root, which only a Filesystem handing one inode
    /// to two instances reaches. A move out of another directory takes that
    /// directory's lock and the instance's `rename_mutex` without waiting,
    /// since the caller holds the lock of the one it looks the name up in,
    /// and answers ESTALE where another call holds either.
    fn unalias(
        self: &Arc<Self>,
        sb: &SuperBlock,
        alias: &Arc<Dentry>,
    ) -> Result<(), FilesystemError> {
        let (parent, name) = self.parent_and_name();
        let mut table = self.hashtables.dentry_hashtable();
        RENAME_LOCK.write(|| {
            if d_ancestor(alias, self) || alias.is_root() {
                return Err(FilesystemError::FilesystemLoop);
            }
            let (old_parent, old_name) = alias.parent_and_name();
            let _held = match Arc::ptr_eq(&old_parent, &parent) {
                true => None,
                false => {
                    let stale = || FilesystemError::StaleNetworkFileHandle;
                    let rename = sb.rename_mutex().down_write_trylock().ok_or_else(stale)?;
                    let dir = old_parent
                        .inode()
                        .expect("a directory holding a dentry names its inode")
                        .trylock_shared();
                    Some((rename, dir.ok_or_else(stale)?))
                }
            };
            let mut state = alias.state();
            if state.hashed {
                table.remove(&key(&old_parent, &old_name));
            }
            table.insert(key(&parent, &name), alias);
            state.hashed = true;
            state.parent = Some(parent.clone());
            state.name = name.clone();
            Ok(())
        })
    }

    /// Begin the lookup the calling walk holds the dentry in lookup for, or one
    /// nobody asked yet, once no other walk holds it. A Filesystem asking the
    /// runtime about the name its own lookup is answering asks it again within
    /// that lookup, as it would otherwise wait on itself.
    fn begin_lookup(&self) -> Result<InLookup, FilesystemError> {
        self.wait_lookup()?;
        let mut state = self.state();
        Ok(match state.asked {
            LookupState::Asking(_) => InLookup::Within,
            LookupState::Answered => InLookup::Answered,
            LookupState::Allocated(_) | LookupState::Unasked => {
                state.asked = LookupState::Asking(sched::current());
                InLookup::Begun
            }
        })
    }

    /// Wait while another walk holds this name in lookup, until the walk's run
    /// is to stop, as Linux's `d_wait_lookup` does, answering whether the
    /// calling walk holds it in lookup itself.
    fn wait_lookup(&self) -> Result<bool, FilesystemError> {
        let current = sched::current();
        loop {
            let state = self.state();
            match state.asked {
                LookupState::Allocated(walk) | LookupState::Asking(walk) if walk == current => {
                    return Ok(true)
                }
                LookupState::Allocated(_) | LookupState::Asking(_)
                    if sched::fatal_signal_pending() =>
                {
                    return Err(FilesystemError::Interrupted)
                }
                LookupState::Allocated(_) | LookupState::Asking(_) => {
                    drop(state);
                    sched::schedule(&mut || {
                        let state = self.state();
                        if matches!(
                            state.asked,
                            LookupState::Allocated(_) | LookupState::Asking(_)
                        ) {
                            drop(
                                self.wait
                                    .wait_timeout(state, sched::sleep_slice())
                                    .unwrap_or_else(PoisonError::into_inner),
                            );
                        }
                    });
                }
                LookupState::Unasked | LookupState::Answered => return Ok(false),
            }
        }
    }

    /// End this walk's lookup where no answer did, taking the dentry out of
    /// `in_lookup_hashtable` and waking every walk waiting on it, as
    /// `d_lookup_done` does; each of them looks the name up again.
    fn lookup_done(&self) {
        {
            let mut in_lookup = self.hashtables.in_lookup_hashtable();
            let (parent, name) = self.parent_and_name();
            let at = key(&parent, &name);
            let mut state = self.state();
            if matches!(
                state.asked,
                LookupState::Allocated(_) | LookupState::Asking(_)
            ) {
                if in_lookup
                    .get(&at)
                    .is_some_and(|held| std::ptr::eq(held.as_ref(), self))
                {
                    in_lookup.remove(&at);
                }
                state.asked = LookupState::Unasked;
            }
        }
        self.wait.notify_all();
    }

    /// What kind of entry this records, `NotFound` where it records none, as
    /// Linux's `d_is_*` read a dentry without asking its filesystem.
    pub(crate) fn kind(&self) -> Result<FileType, FilesystemError> {
        self.inode()
            .map(|inode| inode.kind())
            .ok_or(FilesystemError::NotFound)
    }

    /// What the entry this records reports of itself, `NotFound` where it
    /// records none, as `vfs_getattr` asks the inode a dentry holds.
    pub(crate) fn getattr(&self) -> Result<Kstat, FilesystemError> {
        match self.inode() {
            Some(inode) => inode.ops().getattr(self),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Open the file this records as `how` asks, as `vfs_open` finds
    /// `f_op->open` on the inode a dentry holds.
    pub(crate) fn open(&self, how: &OpenHow) -> Result<Box<dyn FileOperations>, FilesystemError> {
        match self.inode() {
            Some(inode) => inode.ops().open(self, how),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Where the link this records points, as `vfs_readlink` asks the inode a
    /// dentry holds; anything else refuses as `readlinkat` does.
    pub(crate) fn get_link(&self) -> Result<String, FilesystemError> {
        match self.inode() {
            Some(inode) => inode.ops().get_link(self),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Whether this dentry is `ancestor` or stands beneath it, as Linux's
    /// `is_subdir` answers.
    pub(crate) fn is_subdir(self: &Arc<Self>, ancestor: &Arc<Dentry>) -> bool {
        Arc::ptr_eq(self, ancestor) || RENAME_LOCK.read(|| d_ancestor(ancestor, self))
    }
}

/// Whether `ancestor` stands above `dentry`, as Linux's `d_ancestor` answers.
/// The caller holds `RENAME_LOCK`, so no move lands during the walk up.
fn d_ancestor(ancestor: &Arc<Dentry>, dentry: &Arc<Dentry>) -> bool {
    let mut at = dentry.clone();
    while !at.is_root() {
        at = at.parent();
        if Arc::ptr_eq(&at, ancestor) {
            return true;
        }
    }
    false
}

/// A dentry holds the one above it, and moves can stack them as deep as a
/// Filesystem holds directories, so the last reference lets go of each in a
/// loop, as Linux's `dput` walks up in one.
impl Drop for Dentry {
    fn drop(&mut self) {
        let parent = |dentry: &mut Dentry| {
            dentry
                .state
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .parent
                .take()
        };
        let mut above = parent(self);
        while let Some(mut dentry) = above.and_then(Arc::into_inner) {
            above = parent(&mut dentry);
        }
    }
}

/// The directory above a name, by its address, and the name there.
type Key = (usize, String);

fn key(parent: &Arc<Dentry>, name: &str) -> Key {
    (Arc::as_ptr(parent) as usize, name.to_string())
}

/// The dentries of one Filesystem a walk finds by the directory above and the
/// name there.
struct Hashtables {
    /// Those a lookup answered, as Linux's `dentry_hashtable`.
    dentry_hashtable: Mutex<Hashtable<Key, Dentry>>,
    /// Those a walk is still looking up, as Linux's `in_lookup_hashtable`, so
    /// any other walk reaching the name waits for that answer.
    in_lookup_hashtable: Mutex<Hashtable<Key, Dentry>>,
}

impl Hashtables {
    fn dentry_hashtable(&self) -> MutexGuard<'_, Hashtable<Key, Dentry>> {
        self.dentry_hashtable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Taken after `dentry_hashtable` where a call holds both.
    fn in_lookup_hashtable(&self) -> MutexGuard<'_, Hashtable<Key, Dentry>> {
        self.in_lookup_hashtable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Every name of one Filesystem something still holds, by the directory
/// above it and its name there.
pub(crate) struct Dentries {
    root: Arc<Dentry>,
    ops: Option<Arc<dyn DentryOperations>>,
    hashtables: Arc<Hashtables>,
    /// The names kept for the next walk, most recently found last, as Linux's
    /// `d_lru`, each by its address.
    lru: Mutex<LruCache<usize, Arc<Dentry>>>,
}

impl Dentries {
    pub(crate) fn new(ops: Option<Arc<dyn DentryOperations>>) -> Self {
        let hashtables = Arc::new(Hashtables {
            dentry_hashtable: Mutex::default(),
            in_lookup_hashtable: Mutex::default(),
        });
        Self {
            root: Dentry::new(None, "", ops.clone(), &hashtables),
            ops,
            hashtables,
            lru: Mutex::new(LruCache::new(MAX_CACHED_DENTRIES)),
        }
    }

    /// What every dentry of this table answers through.
    pub(super) fn ops(&self) -> Option<&Arc<dyn DentryOperations>> {
        self.ops.as_ref()
    }

    fn lru(&self) -> MutexGuard<'_, LruCache<usize, Arc<Dentry>>> {
        self.lru.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keep `dentry`, which a lookup answered for, standing or not, for the next
    /// walk, unless no walk finds it by its name any more or its Filesystem lets
    /// it go, as `retain_dentry` adds a hashed dentry `d_delete` allows to the
    /// LRU. What this pushes out goes once the table is let go of, as its
    /// release may reach another Filesystem.
    /// The check and the keeping run under the dentry's lock, which `unhash`
    /// takes too, as both hold `d_lock`.
    pub(crate) fn retain(&self, dentry: &Arc<Dentry>) {
        if dentry.goes_when_unused() {
            return;
        }
        let state = dentry.state();
        if !state.hashed {
            return;
        }
        let pushed_out = self
            .lru()
            .push(Arc::as_ptr(dentry) as usize, dentry.clone());
        drop(state);
        drop(pushed_out);
    }

    pub(crate) fn root(&self) -> Arc<Dentry> {
        self.root.clone()
    }

    /// A dentry no walk reaches, a root of its own under no name, as
    /// `d_alloc_pseudo` makes one for what is only ever open.
    pub(crate) fn alloc_pseudo(&self) -> Arc<Dentry> {
        Dentry::new(None, "", self.ops.clone(), &self.hashtables)
    }

    /// The dentry a lookup answered for `name` in `parent`, or a new one in
    /// lookup for the calling walk, as Linux's `d_alloc_parallel` finds or
    /// makes one. A walk finding the name in lookup by another waits for that
    /// walk's answer, until its run is to stop, and takes the dentry where the
    /// answer left it at the name, or looks again, as `d_alloc_parallel`
    /// retries on a mismatch.
    pub(crate) fn alloc_parallel(
        &self,
        parent: &Arc<Dentry>,
        name: &str,
    ) -> Result<Arc<Dentry>, FilesystemError> {
        let at = key(parent, name);
        loop {
            let waited = {
                let table = self.hashtables.dentry_hashtable();
                if let Some(found) = table.get(&at) {
                    return Ok(found);
                }
                let mut in_lookup = self.hashtables.in_lookup_hashtable();
                match in_lookup.get(&at) {
                    Some(waited) => waited,
                    None => {
                        let made = Dentry::new(
                            Some(parent.clone()),
                            name,
                            self.ops.clone(),
                            &self.hashtables,
                        );
                        made.state().asked = LookupState::Allocated(sched::current());
                        in_lookup.insert(at, &made);
                        return Ok(made);
                    }
                }
            };
            if waited.wait_lookup()? {
                return Ok(waited);
            }
            let (above, called) = waited.parent_and_name();
            if waited.hashed() && Arc::ptr_eq(&above, parent) && called == name {
                return Ok(waited);
            }
        }
    }

    /// The dentry a lookup answered for `name` in `parent`, as `d_lookup` finds
    /// one.
    pub(crate) fn lookup(&self, parent: &Arc<Dentry>, name: &str) -> Option<Arc<Dentry>> {
        self.hashtables.dentry_hashtable().get(&key(parent, name))
    }

    /// Whether `dentry` was let go of from its name, as Linux's `d_unlinked`
    /// answers; a root never is.
    pub(crate) fn unlinked(&self, dentry: &Arc<Dentry>) -> bool {
        !dentry.is_root() && !dentry.hashed()
    }

    /// Let go of `dentry` from its name, so the next lookup of the name makes
    /// another and whoever holds this one keeps it, as Linux's `d_drop` unhashes
    /// one for `d_invalidate` and for a `d_delete` of one still in use.
    pub(crate) fn unhash(&self, dentry: &Arc<Dentry>) {
        drop_hashed(&mut self.hashtables.dentry_hashtable(), dentry);
        let kept = self.lru().pop(&(Arc::as_ptr(dentry) as usize));
        drop(kept);
    }

    /// `dentry` moved to the name `target` stands at, taking every one beneath
    /// it along, and `target` let go of, as Linux's `d_move` follows a rename.
    /// A walk finding `dentry` by a name reaches it throughout, so it is never
    /// taken for unlinked.
    pub(crate) fn move_dentry(&self, dentry: &Arc<Dentry>, target: &Arc<Dentry>) {
        self.unhash(target);
        let mut table = self.hashtables.dentry_hashtable();
        let (parent, name) = target.parent_and_name();
        let (from, from_name) = dentry.parent_and_name();
        RENAME_LOCK.write(|| {
            let mut state = dentry.state();
            if state.hashed {
                table.remove(&key(&from, &from_name));
            }
            table.insert(key(&parent, &name), dentry);
            state.hashed = true;
            state.parent = Some(parent.clone());
            state.name = name.clone();
        });
    }

    /// `first` and `second` trade the names they stand at, as Linux's
    /// `d_exchange` follows an exchange, so whoever holds either reaches
    /// it where it went.
    pub(crate) fn exchange(&self, first: &Arc<Dentry>, second: &Arc<Dentry>) {
        let mut table = self.hashtables.dentry_hashtable();
        let (first_at, second_at) = (first.parent_and_name(), second.parent_and_name());
        RENAME_LOCK.write(|| {
            let (mut one, mut other) = (first.state(), second.state());
            for (state, (parent, name)) in [(&one, &first_at), (&other, &second_at)] {
                if state.hashed {
                    table.remove(&key(parent, name));
                }
            }
            table.insert(key(&second_at.0, &second_at.1), first);
            table.insert(key(&first_at.0, &first_at.1), second);
            std::mem::swap(&mut one.parent, &mut other.parent);
            std::mem::swap(&mut one.name, &mut other.name);
            (one.hashed, other.hashed) = (true, true);
        });
    }
}

/// Take `dentry` out of `table` where it is hashed, as Linux's `___d_drop`.
fn drop_hashed(table: &mut Hashtable<Key, Dentry>, dentry: &Arc<Dentry>) {
    let (parent, name) = dentry.parent_and_name();
    let mut state = dentry.state();
    if state.hashed {
        table.remove(&key(&parent, &name));
        state.hashed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;

    use super::{Dentries, Dentry};
    use crate::filesystem::{
        FileOperations, FileType, Filesystem, FilesystemError, InodeOperations, Kstat, Lookup,
        Tmpfs,
    };
    use crate::mount::{Access, Mount};
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{self, File, Finish, Identity, Inode, OpenHow, Path};

    /// The dentry of `name` in `parent`, a lookup having answered that nothing
    /// stands there, so a walk finds it by its name.
    fn negative(dentries: &Dentries, parent: &Arc<Dentry>, name: &str) -> Arc<Dentry> {
        let made = dentries
            .alloc_parallel(parent, name)
            .expect("no other walk holds the name");
        made.add(None);
        made
    }

    /// A Filesystem answering each call by the path its dentry stands at,
    /// as one written in Ruby does, so a removed directory it opened
    /// reaches whatever stands at that path later and keeps no death of
    /// its own.
    struct ByPath {
        root: KernPath,
    }

    /// A name it recorded nothing for is looked up again, as a Ruby Filesystem's
    /// `revalidate` answers, so one it made is found where the make left it.
    impl super::DentryOperations for ByPath {
        fn revalidate(
            &self,
            _dir: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            Ok(dentry.inode().is_some())
        }
    }

    impl crate::vfs::FileSystemType for ByPath {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            sb.set_default_d_op(self.clone());
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for ByPath {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            self.root.vfs_lstat(&dentry.path_raw())
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            match self.root.vfs_lstat(&dentry.path_raw()) {
                Ok(found) => Ok(Lookup::Found(Inode::new(
                    &found,
                    Arc::new(Self {
                        root: self.root.clone(),
                    }),
                ))),
                Err(FilesystemError::NotFound) => Ok(Lookup::Absent),
                Err(err) => Err(err),
            }
        }

        fn open(
            &self,
            dentry: &Dentry,
            how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            if dentry.kind() == Ok(FileType::Dir) {
                return Ok(Box::new(ByPathDir(self.root.clone())));
            }
            self.root.open(&dentry.path_raw(), how)
        }

        fn atomic_open(&self, dentry: &Dentry, how: &OpenHow) -> Result<Finish, FilesystemError> {
            if dentry.in_lookup() {
                let found = self.lookup(dentry)?;
                if !how.create || !matches!(found, Lookup::Absent) {
                    return Ok(Finish::Lookup(found));
                }
            }
            if !how.create {
                return Ok(Finish::NoOpen { created: false });
            }
            let file = self.root.create(&dentry.path_raw(), how)?;
            Ok(Finish::Open {
                file,
                created: true,
            })
        }

        fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.vfs_mkdir(&dentry.path_raw())
        }

        fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.vfs_rmdir(&dentry.path_raw())
        }

        fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.vfs_unlink(&dentry.path_raw())
        }

        fn rename(
            &self,
            from: &Dentry,
            to_dir: &dyn InodeOperations,
            to: &Dentry,
            _flag: Option<crate::vfs::RenameFlag>,
        ) -> Result<(), FilesystemError> {
            (to_dir as &dyn Any)
                .downcast_ref::<Self>()
                .ok_or(FilesystemError::CrossesDevices)?;
            self.root
                .rename(&from.path_raw(), &self.root, &to.path_raw())
        }
    }

    /// A directory of a `ByPath`, open.
    struct ByPathDir(KernPath);

    impl FileOperations for ByPathDir {
        fn iterate_shared(
            &mut self,
            dentry: &Dentry,
            ctx: &mut vfs::DirContext<'_>,
        ) -> Result<(), FilesystemError> {
            let entries = match dentry.path_raw().as_str() {
                "" => self.0.iterate_dir(),
                at => self.0.open_dir(at)?.iterate_dir(),
            }?;
            crate::filesystem::Rdir::default().iterate(dentry, ctx, || Ok(entries))
        }
    }

    fn by_path(tmpfs: &Arc<Tmpfs>) -> Arc<Filesystem> {
        Filesystem::new(Arc::new(ByPath {
            root: kern_path(tmpfs.clone()),
        }))
        .expect("the instance must stand")
    }

    fn root_of(filesystem: &Arc<Filesystem>) -> Path {
        let mount = Mount::new("/data", filesystem.clone(), Access::ReadWrite)
            .expect("/data is a guest path");
        Path::root(mount.mnt())
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            read: true,
            directory: true,
            ..OpenHow::default()
        };
        match vfs::kern_path::filp_open(&Arc::default(), start, path, &how) {
            Ok(dir) => dir.path().clone(),
            Err(_) => panic!("{path} must open as a directory"),
        }
    }

    // Linux's `path_init` reads `rename_lock` with `__read_seqcount_begin`,
    // which spins past a write under way, so a walk begun during a move is
    // never answered EAGAIN for that move.
    // @behavior RT-265
    #[test]
    fn a_walk_begun_during_a_move_begins_once_it_lands() {
        use crate::locking::seqlock::SeqLock;
        use crate::sched::testing::MAX_WAIT;
        use std::sync::mpsc;
        use std::thread;

        let lock = Arc::new(SeqLock::new());
        let (entered, entering) = mpsc::channel();
        let (release, releasing) = mpsc::channel::<()>();
        let writer = {
            let lock = lock.clone();
            thread::spawn(move || {
                lock.write(|| {
                    entered.send(()).ok();
                    releasing.recv_timeout(MAX_WAIT).ok();
                })
            })
        };
        entering
            .recv_timeout(MAX_WAIT)
            .expect("the write must begin");
        let reader = {
            let lock = lock.clone();
            thread::spawn(move || lock.read_seqbegin())
        };
        thread::sleep(std::time::Duration::from_millis(50));
        let waited = !reader.is_finished();
        release.send(()).ok();
        crate::sched::testing::join("the write", writer);
        let begun = crate::sched::testing::join("the read", reader);
        assert_eq!(
            (waited, begun, lock.read_seqretry(begun)),
            (true, 2, false),
            "SeqLock::read_seqbegin during a write must wait for it to end and answer where the \
             writes stand after it, which no later read finds changed"
        );
    }

    // `d_revalidate` compares what the dentry recorded first, so a later
    // lookup replacing it would hide the change it is there to see.
    #[test]
    fn a_dentry_keeps_what_a_lookup_first_found_there() {
        let ops: Arc<dyn InodeOperations> = Arc::new(Tmpfs::new());
        let (first, second) = (
            Kstat::new(FileType::Dir, 0).with_identity(1, 2),
            Kstat::new(FileType::Dir, 0).with_identity(1, 3),
        );
        let dentries = Dentries::new(None);
        let dentry = negative(&dentries, &dentries.root(), "dir");
        dentry.add(Some(Inode::new(&first, ops.clone())));
        dentry.add(Some(Inode::new(&second, ops)));
        assert_eq!(
            dentry.inode().and_then(|inode| inode.identity()),
            Identity::of(&first),
            "Dentry::add after a negative lookup and then a positive one must keep the first \
             inode recorded"
        );
    }

    // The Filesystem answers by path, so only the dentry the removal marked
    // keeps the make from landing in the directory made in its place.
    // @behavior RT-159
    #[test]
    fn a_directory_removed_through_the_vfs_makes_nothing_more_whatever_its_filesystem() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let root = root_of(&by_path(&tmpfs));
        let dir = open_dir(&root, "dir");
        vfs::filename_unlinkat(&Arc::default(), &root, "dir/keep").expect("the file must go");
        vfs::filename_rmdir(&Arc::default(), &root, "dir").expect("the directory must go");
        vfs::filename_mkdirat(&Arc::default(), &root, "dir")
            .expect("another directory must be made");
        vfs::filename_mkdirat(&Arc::default(), &root, "dir/other")
            .expect("an entry must be made in it");
        assert_eq!(
            (
                vfs::filename_mkdirat(&Arc::default(), &dir, "made"),
                File::dentry_open(
                    dir.clone(),
                    &OpenHow {
                        read: true,
                        ..OpenHow::default()
                    },
                    &Arc::default()
                )
                    .expect("the directory must open")
                    .iterate_dir(&mut vfs::DirContext::new(0, &mut |_| true)),
                kern_path(tmpfs.clone()).vfs_lstat("dir/made").err()
            ),
            (
                Err(FilesystemError::NotFound),
                Err(FilesystemError::NotFound),
                Some(FilesystemError::NotFound)
            ),
            "vfs::filename_mkdirat and File::iterate_dir through a directory vfs::filename_rmdir removed, over \
             a Filesystem answering by path, must each answer NotFound, as Linux asks \
             IS_DEADDIR, and make nothing in the directory made in its place"
        );
    }

    // @behavior RT-160
    #[test]
    fn a_directory_removed_beneath_one_mount_is_removed_beneath_another_of_its_instance() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let instance = by_path(&tmpfs);
        let (first, second) = (root_of(&instance), root_of(&instance));
        let dir = open_dir(&first, "dir");
        vfs::filename_unlinkat(&Arc::default(), &second, "dir/keep").expect("the file must go");
        vfs::filename_rmdir(&Arc::default(), &second, "dir").expect("the directory must go");
        vfs::filename_mkdirat(&Arc::default(), &second, "dir")
            .expect("another directory must be made");
        assert_eq!(
            vfs::filename_mkdirat(&Arc::default(), &dir, "made"),
            Err(FilesystemError::NotFound),
            "vfs::filename_mkdirat through a directory opened beneath one Mount, once vfs::filename_rmdir \
             removed it beneath another Mount of the same Filesystem, must answer NotFound"
        );
    }

    // Built directly, as a path cannot reach this deep. A depth a recursive
    // release cannot survive on this stack is what tells it from a loop.
    // @behavior RT-199
    #[test]
    fn a_dentry_goes_however_deep_it_stands() {
        let dentries = Dentries::new(None);
        let mut leaf = dentries.root();
        for _ in 0..200_000 {
            leaf = dentries
                .alloc_parallel(&leaf, "a")
                .expect("no other walk holds the name");
        }
        let released = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || drop(leaf))
            .expect("the thread must start")
            .join();
        assert!(
            released.is_ok(),
            "a dentry 200000 directories deep, dropped on a 64 KiB stack, must go without \
             overflowing it"
        );
    }

    // A walk let go of the dentry while another still held it, so only
    // refusing the unhashed one keeps that walk's retain from keeping a dentry
    // no walk can find, whose Filesystem state may be stale.
    #[test]
    fn a_dentry_let_go_of_is_not_kept_for_the_next_walk() {
        let dentries = Dentries::new(None);
        let dentry = negative(&dentries, &dentries.root(), "gone");
        dentries.unhash(&dentry);
        dentries.retain(&dentry);
        let held = Arc::downgrade(&dentry);
        drop(dentry);
        assert!(
            held.upgrade().is_none(),
            "Dentries::retain of a dentry Dentries::unhash let go of must not keep it, as \
             retain_dentry refuses an unhashed one"
        );
    }

    #[test]
    fn an_exchange_swaps_two_dentries() {
        let dentries = super::Dentries::new(None);
        let root = dentries.root();
        let a = negative(&dentries, &root, "a");
        let b = negative(&dentries, &negative(&dentries, &root, "d"), "b");
        dentries.exchange(&a, &b);
        assert_eq!(
            (
                a.path_raw(),
                b.path_raw(),
                dentries
                    .lookup(&root, "a")
                    .is_some_and(|found| Arc::ptr_eq(&found, &b))
            ),
            ("d/b".to_string(), "a".to_string(), true),
            "Dentries::exchange must hand each dentry the other's name, and find each by its new \
             name"
        );
    }

    /// Witnesses of a race, each run only by `rake crates:stress`: the race
    /// lands a few times in many tries, and only an optimized build makes
    /// that many in seconds.
    mod stress {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;

        use super::super::{Dentries, Dentry};
        use super::negative;

        /// The dentry a rename onto `name` in `dir` replaces.
        fn target(dentries: &Dentries, dir: &Arc<Dentry>, name: &str) -> Arc<Dentry> {
            dentries
                .alloc_parallel(dir, name)
                .expect("no other walk holds the name")
        }

        // `x` is named `z` only while `b` stands beneath `y`, so `z/b/leaf`
        // is a path `leaf` never stood at: only a read taking `b`'s parent
        // before the moves and `x`'s name after them answers it.
        // @behavior RT-172
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn a_path_asked_while_directories_above_it_move_names_one_that_stood() {
            let dentries = Arc::new(Dentries::new(None));
            let root = dentries.root();
            let (x, y) = (
                negative(&dentries, &root, "x"),
                negative(&dentries, &root, "y"),
            );
            let b = negative(&dentries, &x, "b");
            let leaf = negative(&dentries, &b, "leaf");
            let stop = Arc::new(AtomicBool::new(false));
            let moving = {
                let (dentries, stop) = (dentries.clone(), stop.clone());
                thread::spawn(move || {
                    let root = dentries.root();
                    while !stop.load(Ordering::SeqCst) {
                        dentries.move_dentry(&b, &target(&dentries, &y, "b"));
                        dentries.move_dentry(&x, &target(&dentries, &root, "z"));
                        dentries.move_dentry(&x, &target(&dentries, &root, "x"));
                        dentries.move_dentry(&b, &target(&dentries, &x, "b"));
                    }
                })
            };
            let torn = (0..5_000_000)
                .filter(|_| leaf.path_raw().starts_with("z/"))
                .count();
            stop.store(true, Ordering::SeqCst);
            moving.join().expect("the moves must not panic");
            assert_eq!(
                torn, 0,
                "Dentry::path_raw while Dentries::move_dentry moves the directories above it must \
                 answer only a path the dentry stood at"
            );
        }

        // Each move keeps the dentry in the table, so only a read of its parent
        // before the move and its name after it finds nothing at that key.
        // @behavior RT-228
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn a_directory_moving_through_the_vfs_is_never_taken_for_unlinked() {
            let dentries = Arc::new(Dentries::new(None));
            let root = dentries.root();
            let (x, y) = (
                negative(&dentries, &root, "x"),
                negative(&dentries, &root, "y"),
            );
            let moved = negative(&dentries, &x, "a");
            let stop = Arc::new(AtomicBool::new(false));
            let moving = {
                let (dentries, stop, moved) = (dentries.clone(), stop.clone(), moved.clone());
                thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        dentries.move_dentry(&moved, &target(&dentries, &y, "b"));
                        dentries.move_dentry(&moved, &target(&dentries, &x, "a"));
                    }
                })
            };
            let unlinked = (0..5_000_000).filter(|_| dentries.unlinked(&moved)).count();
            stop.store(true, Ordering::SeqCst);
            moving.join().expect("the moves must not panic");
            assert_eq!(
                unlinked, 0,
                "Dentries::unlinked while Dentries::move_dentry moves the dentry must answer that \
                 it still stands"
            );
        }
    }
}
