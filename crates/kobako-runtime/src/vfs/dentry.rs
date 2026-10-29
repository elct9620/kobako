//! The names of one Filesystem a guest has reached, each kept by the
//! directory above it and its name there while something holds it, as
//! Linux's dentries are: a directory moved through the VFS takes every one
//! beneath it along, and one removed or replaced through it is dead. What
//! a Filesystem answers about a name is asked again on every call, so the
//! table caches no lookup.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::rwsem::RwSemaphore;
use super::seqlock::SeqLock;
use crate::hashtable::Hashtable;

/// Every move of a dentry, as Linux's `rename_lock`, so a walk up its
/// parents answers where the dentry stood at one instant.
static RENAME_LOCK: SeqLock = SeqLock::new();

/// One name of a Filesystem, and where it stands now. Only the VFS makes
/// one, so a Filesystem reads the name a call is about from it.
pub struct Dentry {
    state: Mutex<State>,
    /// The lock calls on the names in this directory run under. Linux
    /// keeps it on the directory's inode; the VFS owns no inode of its
    /// own, and a directory has one dentry, so it is kept here.
    pub(crate) rwsem: RwSemaphore,
}

struct State {
    /// The directory above, `None` for the root.
    parent: Option<Arc<Dentry>>,
    name: String,
    /// Whether the directory was removed or replaced through the VFS, as
    /// Linux marks such a directory dead.
    dead: bool,
}

impl Dentry {
    fn new(parent: Option<Arc<Dentry>>, name: &str) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                parent,
                name: name.to_string(),
                dead: false,
            }),
            rwsem: RwSemaphore::default(),
        })
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

    pub(crate) fn is_dead(&self) -> bool {
        self.state().dead
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

/// Every name of one Filesystem something still holds, by the directory
/// above it and its name there.
pub(crate) struct Dentries {
    root: Arc<Dentry>,
    /// The directories held, by the directory above and the name there, as
    /// Linux's `dentry_hashtable` keeps them.
    hashtable: Mutex<Hashtable<(usize, String), Dentry>>,
}

fn key(parent: &Arc<Dentry>, name: &str) -> (usize, String) {
    (Arc::as_ptr(parent) as usize, name.to_string())
}

impl Default for Dentries {
    fn default() -> Self {
        Self {
            root: Dentry::new(None, ""),
            hashtable: Mutex::default(),
        }
    }
}

impl Dentries {
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
            Dentry::new(Some(parent.clone()), name)
        })
    }

    /// The directory `name` in `parent` was removed, or replaced by a
    /// move, as `d_delete` marks it.
    pub(crate) fn delete(&self, parent: &Arc<Dentry>, name: &str) {
        let removed = self.hashtable().remove(&key(parent, name));
        if let Some(removed) = removed {
            removed.state().dead = true;
        }
    }

    /// The entry `from_name` in `from` moved to `to_name` in `to`, taking
    /// the directory held there, and every one beneath it, along, as
    /// Linux's does.
    pub(crate) fn d_move(
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
    pub(crate) fn d_exchange(&self, first: &Arc<Dentry>, second: &Arc<Dentry>) {
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

    use super::Dentry;
    use crate::filesystem::{
        Access, DirEntry, File, FileType, FilesystemError, Inode, Metadata, OpenOptions, Tmpfs,
    };
    use crate::mount::Mount;
    use crate::profile::Profile;
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{self, FileDescription, Filesystem, OpenHow, Path};

    /// A Filesystem answering each call by the path its dentry stands at,
    /// as one written in Ruby does, so a removed directory it opened
    /// reaches whatever stands at that path later and keeps no death of
    /// its own.
    struct ByPath {
        root: KernPath,
    }

    impl Inode for ByPath {
        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.root.open(&dentry.path_raw(), options)
        }

        fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
            if self.root.symlink_metadata(&dentry.path_raw())?.file_type != FileType::Directory {
                return Err(FilesystemError::NotADirectory);
            }
            Ok(Arc::new(Self {
                root: self.root.clone(),
            }))
        }

        fn metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&dentry.path_raw())
        }

        fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&dentry.path_raw())
        }

        fn lookup(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
            Ok(dentry.name())
        }

        fn read_link(&self, _dentry: &Dentry) -> Result<String, FilesystemError> {
            Err(FilesystemError::InvalidInput)
        }

        fn entries(&self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
            match dentry.path_raw().as_str() {
                "" => self.root.entries(),
                at => self.root.open_dir(at)?.entries(),
            }
        }

        fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.create_dir(&dentry.path_raw())
        }

        fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_dir(&dentry.path_raw())
        }

        fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_file(&dentry.path_raw())
        }

        fn rename(
            &self,
            from: &Dentry,
            to_dir: &dyn Inode,
            to: &Dentry,
        ) -> Result<(), FilesystemError> {
            (to_dir as &dyn Any)
                .downcast_ref::<Self>()
                .ok_or(FilesystemError::CrossesDevices)?;
            self.root
                .rename(&from.path_raw(), &self.root, &to.path_raw())
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
        Path::root(mount, Profile::Permissive)
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        match vfs::open(start, path, &how, true) {
            Ok(FileDescription::Directory(dir)) => dir,
            _ => panic!("{path} must open as a directory"),
        }
    }

    // The Filesystem answers by path, so only the dentry the removal marked
    // keeps the make from landing in the directory made in its place.
    // @behavior RT-159
    #[test]
    fn a_directory_removed_through_the_vfs_makes_nothing_more_whatever_its_filesystem() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let root = root_of(&by_path(&tmpfs));
        let dir = open_dir(&root, "dir");
        vfs::remove_file(&root, "dir/keep").expect("the file must go");
        vfs::remove_dir(&root, "dir").expect("the directory must go");
        vfs::create_dir(&root, "dir").expect("another directory must be made");
        vfs::create_dir(&root, "dir/other").expect("an entry must be made in it");
        assert_eq!(
            (
                vfs::create_dir(&dir, "made"),
                dir.entries(),
                kern_path(tmpfs.clone()).symlink_metadata("dir/made").err()
            ),
            (
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                Ok(Vec::new()),
                Some(FilesystemError::NotFound)
            ),
            "vfs::create_dir and Path::entries through a directory vfs::remove_dir removed, over \
             a Filesystem answering by path, must answer NotFound and list nothing, and make \
             nothing in the directory made in its place"
        );
    }

    // @behavior RT-160
    #[test]
    fn a_directory_removed_beneath_one_mount_is_removed_beneath_another_of_its_instance() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let instance = by_path(&tmpfs);
        let (first, second) = (root_of(&instance), root_of(&instance));
        let dir = open_dir(&first, "dir");
        vfs::remove_file(&second, "dir/keep").expect("the file must go");
        vfs::remove_dir(&second, "dir").expect("the directory must go");
        vfs::create_dir(&second, "dir").expect("another directory must be made");
        assert_eq!(
            vfs::create_dir(&dir, "made"),
            Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
            "vfs::create_dir through a directory opened beneath one Mount, once vfs::remove_dir \
             removed it beneath another Mount of the same Filesystem, must answer NotFound"
        );
    }

    #[test]
    fn an_exchange_swaps_two_dentries_and_kills_neither() {
        let dentries = super::Dentries::default();
        let root = dentries.root();
        let a = dentries.child(&root, "a");
        let b = dentries.child(&dentries.child(&root, "d"), "b");
        dentries.d_exchange(&a, &b);
        assert_eq!(
            (
                a.path_raw(),
                b.path_raw(),
                a.is_dead() || b.is_dead(),
                Arc::ptr_eq(&dentries.child(&root, "a"), &b)
            ),
            ("d/b".to_string(), "a".to_string(), false, true),
            "Dentries::d_exchange must hand each dentry the other's name, keep both alive, and \
             find each by its new name"
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
            let dentries = Arc::new(Dentries::default());
            let root = dentries.root();
            let (x, y) = (dentries.child(&root, "x"), dentries.child(&root, "y"));
            let leaf = dentries.child(&dentries.child(&x, "b"), "leaf");
            let stop = Arc::new(AtomicBool::new(false));
            let moving = {
                let (dentries, stop) = (dentries.clone(), stop.clone());
                thread::spawn(move || {
                    let root = dentries.root();
                    while !stop.load(Ordering::SeqCst) {
                        dentries.d_move(&x, "b", &y, "b");
                        dentries.d_move(&root, "x", &root, "z");
                        dentries.d_move(&root, "z", &root, "x");
                        dentries.d_move(&y, "b", &x, "b");
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
                "Dentry::path_raw while Dentries::d_move moves the directories above it must \
                 answer only a path the dentry stood at"
            );
        }
    }
}
