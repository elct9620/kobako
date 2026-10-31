//! The names of one Filesystem a walk has reached, each kept by the
//! directory above it and its name there while something holds it, as
//! Linux's dentries are: a directory moved through the VFS takes every one
//! beneath it along, and one removed or replaced through it is dead. A name
//! found standing stays for the next walk, as Linux's dcache keeps it, up to a
//! bound; the walk asks `revalidate` before trusting it, so what changed behind
//! the runtime's back is found again.

use std::any::Any;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use lru::LruCache;

use super::file::FileOperations;
use super::inode::{Inode, InodeOperations, Lookup};
use super::seqlock::SeqLock;
use super::{FileType, FilesystemError, Metadata, OpenOptions};
use crate::hashtable::Hashtable;
use crate::locking::RwSemaphoreGuard;

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
/// parents answers where the dentry stood at one instant.
static RENAME_LOCK: SeqLock = SeqLock::new();

/// One name of a Filesystem, and where it stands now. Only the VFS makes
/// one, so a Filesystem reads the name a call is about from it.
pub struct Dentry {
    state: Mutex<State>,
    /// This dentry, for whoever is handed it to keep, as Linux's `dget`
    /// takes a reference to any dentry it is handed.
    this: Weak<Dentry>,
    /// What the Filesystem keeps on the name, set once, as Linux's
    /// `d_fsdata` holds a filesystem's own state for a dentry.
    fsdata: OnceLock<Arc<dyn Any + Send + Sync>>,
    /// What answers about this name, as `d_alloc` copies `s_d_op` from the
    /// super block.
    ops: Option<Arc<dyn DentryOperations>>,
}

struct State {
    /// The directory above, `None` for the root.
    parent: Option<Arc<Dentry>>,
    name: String,
    /// What the name stands for once a lookup found it, `None` while the
    /// dentry is negative.
    inode: Option<Arc<Inode>>,
}

impl Dentry {
    fn new(
        parent: Option<Arc<Dentry>>,
        name: &str,
        ops: Option<Arc<dyn DentryOperations>>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            state: Mutex::new(State {
                parent,
                name: name.to_string(),
                inode: None,
            }),
            this: this.clone(),
            fsdata: OnceLock::new(),
            ops,
        })
    }

    /// What answers about this name, the same for every dentry of one
    /// Filesystem.
    pub fn ops(&self) -> Option<&Arc<dyn DentryOperations>> {
        self.ops.as_ref()
    }

    /// The directory above, `None` for the root, as Linux's `dget_parent`
    /// takes it.
    pub(crate) fn parent(&self) -> Option<Arc<Dentry>> {
        self.state().parent.clone()
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

    /// Each name from the Filesystem's root down to this directory.
    pub(crate) fn names(&self) -> Vec<String> {
        RENAME_LOCK.read(|| {
            let mut names = Vec::new();
            let mut at = {
                let state = self.state();
                names.push(state.name.clone());
                state.parent.clone()
            };
            while let Some(dentry) = at {
                let state = dentry.state();
                names.push(state.name.clone());
                at = state.parent.clone();
            }
            names.reverse();
            names.retain(|name| !name.is_empty());
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
    pub fn add(&self, inode: Option<Inode>) {
        let mut state = self.state();
        if state.inode.is_none() {
            state.inode = inode.map(Arc::new);
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

    /// Ask the directory `dir` what stands here where nothing is recorded yet,
    /// and record it, as Linux's `lookup_fast` keeps a dentry that names
    /// something and `lookup_slow` asks `->lookup` for one that does not. A
    /// directory holding the entry under another spelling answers that
    /// spelling, for the caller to take that dentry instead.
    pub(crate) fn lookup_inode(&self, dir: &Dentry) -> Result<Option<String>, FilesystemError> {
        if self.inode().is_some() {
            return Ok(None);
        }
        match dir.inode_ops()?.lookup(self)? {
            Lookup::Found(inode) => self.add(Some(inode)),
            Lookup::Absent => self.add(None),
            Lookup::Spelled(stored) => return Ok(Some(stored)),
        }
        Ok(None)
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
    pub(crate) fn getattr(&self) -> Result<Metadata, FilesystemError> {
        match self.inode() {
            Some(inode) => inode.ops().getattr(self),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Open the file this records as `options` asks, as `vfs_open` finds
    /// `f_op->open` on the inode a dentry holds.
    pub(crate) fn open(
        &self,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        match self.inode() {
            Some(inode) if inode.kind() == FileType::Directory => {
                Err(FilesystemError::IsADirectory)
            }
            Some(inode) => inode.ops().open(self, options),
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
        RENAME_LOCK.read(|| {
            let mut at = Some(self.clone());
            while let Some(dentry) = at {
                if Arc::ptr_eq(&dentry, ancestor) {
                    return true;
                }
                at = dentry.state().parent.clone();
            }
            false
        })
    }
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

/// Every name of one Filesystem something still holds, by the directory
/// above it and its name there.
pub(crate) struct Dentries {
    root: Arc<Dentry>,
    ops: Option<Arc<dyn DentryOperations>>,
    /// The directories held, by the directory above and the name there, as
    /// Linux's `dentry_hashtable` keeps them.
    hashtable: Mutex<Hashtable<(usize, String), Dentry>>,
    /// The names kept for the next walk, most recently found last, as Linux's
    /// `d_lru`, each by its address.
    lru: Mutex<LruCache<usize, Arc<Dentry>>>,
}

fn key(parent: &Arc<Dentry>, name: &str) -> (usize, String) {
    (Arc::as_ptr(parent) as usize, name.to_string())
}

impl Dentries {
    pub(crate) fn new(ops: Option<Arc<dyn DentryOperations>>) -> Self {
        Self {
            root: Dentry::new(None, "", ops.clone()),
            ops,
            hashtable: Mutex::default(),
            lru: Mutex::new(LruCache::new(MAX_CACHED_DENTRIES)),
        }
    }

    fn lru(&self) -> MutexGuard<'_, LruCache<usize, Arc<Dentry>>> {
        self.lru.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keep `dentry`, which a lookup found standing, for the next walk, unless
    /// its Filesystem lets it go, as `dput` adds a dentry `d_delete` allows to
    /// the LRU. What this pushes out goes once the table is let go of, as its
    /// release may reach another Filesystem.
    pub(crate) fn retain(&self, dentry: &Arc<Dentry>) {
        if dentry.inode().is_none() || dentry.goes_when_unused() {
            return;
        }
        let pushed_out = self
            .lru()
            .push(Arc::as_ptr(dentry) as usize, dentry.clone());
        drop(pushed_out);
    }

    fn hashtable(&self) -> MutexGuard<'_, Hashtable<(usize, String), Dentry>> {
        self.hashtable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn root(&self) -> Arc<Dentry> {
        self.root.clone()
    }

    /// The directory `name` in `parent`, as `d_lookup` finds it, or a new
    /// dentry for it, as `d_alloc` makes one.
    pub(crate) fn child(&self, parent: &Arc<Dentry>, name: &str) -> Arc<Dentry> {
        self.hashtable().get_or_insert_with(key(parent, name), || {
            Dentry::new(Some(parent.clone()), name, self.ops.clone())
        })
    }

    /// The dentry held for `name` in `parent`, as `d_lookup` finds one.
    pub(crate) fn lookup(&self, parent: &Arc<Dentry>, name: &str) -> Option<Arc<Dentry>> {
        self.hashtable().get(&key(parent, name))
    }

    /// Whether `dentry` was taken out of the table, as Linux's
    /// `d_unlinked` answers for one no longer reached by its name.
    pub(crate) fn unlinked(&self, dentry: &Arc<Dentry>) -> bool {
        let Some(parent) = dentry.parent() else {
            return false;
        };
        !self
            .hashtable()
            .get(&key(&parent, &dentry.name()))
            .is_some_and(|held| Arc::ptr_eq(&held, dentry))
    }

    /// Take `dentry` out of the table, so the next lookup of its name makes
    /// another and whoever holds this one keeps it, as Linux's `d_drop` unhashes
    /// one for `d_invalidate` and for a `d_delete` of one still in use.
    pub(crate) fn unhash(&self, dentry: &Arc<Dentry>) {
        let kept = self.lru().pop(&(Arc::as_ptr(dentry) as usize));
        drop(kept);
        let mut table = self.hashtable();
        let Some(parent) = dentry.state().parent.clone() else {
            return;
        };
        let at = key(&parent, &dentry.name());
        if table
            .get(&at)
            .is_some_and(|held| Arc::ptr_eq(&held, dentry))
        {
            table.remove(&at);
        }
    }

    /// The entry `from_name` in `from` moved to `to_name` in `to`, taking
    /// the directory held there, and every one beneath it, along, as
    /// Linux's does.
    pub(crate) fn move_dentry(
        &self,
        from: &Arc<Dentry>,
        from_name: &str,
        to: &Arc<Dentry>,
        to_name: &str,
    ) {
        let mut table = self.hashtable();
        let Some(moved) = table.remove(&key(from, from_name)) else {
            return;
        };
        table.insert(key(to, to_name), &moved);
        RENAME_LOCK.write(|| {
            let mut state = moved.state();
            state.parent = Some(to.clone());
            state.name = to_name.to_string();
        });
    }

    /// `first` and `second` trade the names they stand at, as Linux's
    /// `d_exchange` follows an exchange, so whoever holds either reaches
    /// it where it went.
    pub(crate) fn exchange(&self, first: &Arc<Dentry>, second: &Arc<Dentry>) {
        let mut table = self.hashtable();
        RENAME_LOCK.write(|| {
            let (mut one, mut other) = (first.state(), second.state());
            std::mem::swap(&mut one.parent, &mut other.parent);
            std::mem::swap(&mut one.name, &mut other.name);
        });
        for dentry in [first, second] {
            let state = dentry.state();
            if let Some(parent) = &state.parent {
                table.insert(key(parent, &state.name), dentry);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;

    use super::{Dentries, Dentry};
    use crate::filesystem::{
        DirEntry, FileOperations, FileType, Filesystem, FilesystemError, InodeOperations, Lookup,
        Metadata, OpenOptions, Tmpfs,
    };
    use crate::mount::{Access, Mount};
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{self, File, Identity, Inode, OpenHow, Path};

    /// A Filesystem answering each call by the path its dentry stands at,
    /// as one written in Ruby does, so a removed directory it opened
    /// reaches whatever stands at that path later and keeps no death of
    /// its own.
    struct ByPath {
        root: KernPath,
    }

    impl InodeOperations for ByPath {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&dentry.path_raw())
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            match self.root.symlink_metadata(&dentry.path_raw()) {
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
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            if dentry.kind() == Ok(FileType::Directory) {
                return Ok(Box::new(ByPathDir(self.root.clone())));
            }
            self.root.open(&dentry.path_raw(), options)
        }

        fn atomic_open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.root.create(&dentry.path_raw(), options)
        }

        fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.create_dir(&dentry.path_raw())
        }

        fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_dir(&dentry.path_raw())
        }

        fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_file(&dentry.path_raw())
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
        fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
            match dentry.path_raw().as_str() {
                "" => self.0.entries(),
                at => self.0.open_dir(at)?.entries(),
            }
        }
    }

    fn by_path(tmpfs: &Arc<Tmpfs>) -> Arc<Filesystem> {
        Filesystem::new(Arc::new(ByPath {
            root: kern_path(tmpfs.clone()),
        }))
    }

    fn root_of(filesystem: &Arc<Filesystem>) -> Path {
        let mount = Mount::new("/data", filesystem.clone(), Access::ReadWrite)
            .expect("/data is a guest path");
        Path::root(mount.mnt())
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        match vfs::do_file_open(&Arc::default(), start, path, &how) {
            Ok(dir) => dir.path().clone(),
            Err(_) => panic!("{path} must open as a directory"),
        }
    }

    // `d_revalidate` compares what the dentry recorded first, so a later
    // lookup replacing it would hide the change it is there to see.
    #[test]
    fn a_dentry_keeps_what_a_lookup_first_found_there() {
        let ops: Arc<dyn InodeOperations> = Arc::new(Tmpfs::new());
        let (first, second) = (
            Metadata::new(FileType::Directory, 0).with_identity(1, 2),
            Metadata::new(FileType::Directory, 0).with_identity(1, 3),
        );
        let dentries = Dentries::new(None);
        let dentry = dentries.child(&dentries.root(), "dir");
        dentry.add(None);
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
                File::directory(dir.clone(), Arc::default())
                    .expect("the directory must open")
                    .iterate_dir(&mut vfs::DirContext::new(0, &mut |_| true)),
                kern_path(tmpfs.clone()).symlink_metadata("dir/made").err()
            ),
            (
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
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
            Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
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
            leaf = dentries.child(&leaf, "a");
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

    #[test]
    fn an_exchange_swaps_two_dentries() {
        let dentries = super::Dentries::new(None);
        let root = dentries.root();
        let a = dentries.child(&root, "a");
        let b = dentries.child(&dentries.child(&root, "d"), "b");
        dentries.exchange(&a, &b);
        assert_eq!(
            (
                a.path_raw(),
                b.path_raw(),
                Arc::ptr_eq(&dentries.child(&root, "a"), &b)
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

        use super::super::Dentries;

        // `x` is named `z` only while `b` stands beneath `y`, so `z/b/leaf`
        // is a path `leaf` never stood at: only a read taking `b`'s parent
        // before the moves and `x`'s name after them answers it.
        // @behavior RT-172
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn a_path_asked_while_directories_above_it_move_names_one_that_stood() {
            let dentries = Arc::new(Dentries::new(None));
            let root = dentries.root();
            let (x, y) = (dentries.child(&root, "x"), dentries.child(&root, "y"));
            let leaf = dentries.child(&dentries.child(&x, "b"), "leaf");
            let stop = Arc::new(AtomicBool::new(false));
            let moving = {
                let (dentries, stop) = (dentries.clone(), stop.clone());
                thread::spawn(move || {
                    let root = dentries.root();
                    while !stop.load(Ordering::SeqCst) {
                        dentries.move_dentry(&x, "b", &y, "b");
                        dentries.move_dentry(&root, "x", &root, "z");
                        dentries.move_dentry(&root, "z", &root, "x");
                        dentries.move_dentry(&y, "b", &x, "b");
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
    }
}
