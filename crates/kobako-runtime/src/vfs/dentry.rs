//! The directories of one SuperBlock a guest has reached, each kept by the
//! directory above it and its name there, as Linux's dcache keeps its
//! dentries: a directory moved through the VFS takes every one beneath it
//! along, and one removed or replaced through it is dead.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

/// One directory of a SuperBlock, and where it stands now.
pub(crate) struct Dentry {
    state: Mutex<State>,
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
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Each name from the SuperBlock's root down to this directory.
    pub(crate) fn names(&self) -> Vec<String> {
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
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.state().dead
    }
}

/// Every directory of one SuperBlock a guest still holds, by the directory
/// above it and its name there.
pub(crate) struct Dentries {
    root: Arc<Dentry>,
    hashed: Mutex<Hashed>,
}

/// The hashed directories, and how many entries the table may reach before
/// those no longer held are swept out: twice what the last sweep left, so
/// each lookup pays a constant share of the sweeps.
#[derive(Default)]
struct Hashed {
    entries: HashMap<(usize, String), Weak<Dentry>>,
    sweep_at: usize,
}

fn key(parent: &Arc<Dentry>, name: &str) -> (usize, String) {
    (Arc::as_ptr(parent) as usize, name.to_string())
}

impl Default for Dentries {
    fn default() -> Self {
        Self {
            root: Dentry::new(None, ""),
            hashed: Mutex::default(),
        }
    }
}

impl Dentries {
    fn hashed(&self) -> MutexGuard<'_, Hashed> {
        self.hashed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn root(&self) -> Arc<Dentry> {
        self.root.clone()
    }

    /// The directory `name` in `parent`, as `d_lookup` finds it, or a new
    /// dentry for it, as `d_alloc` makes one.
    pub(crate) fn child(&self, parent: &Arc<Dentry>, name: &str) -> Arc<Dentry> {
        let mut hashed = self.hashed();
        if hashed.entries.len() >= hashed.sweep_at {
            hashed.entries.retain(|_, dentry| dentry.strong_count() > 0);
            hashed.sweep_at = (hashed.entries.len() * 2).max(16);
        }
        let key = key(parent, name);
        if let Some(child) = hashed.entries.get(&key).and_then(Weak::upgrade) {
            return child;
        }
        let child = Dentry::new(Some(parent.clone()), name);
        hashed.entries.insert(key, Arc::downgrade(&child));
        child
    }

    /// The directory `name` in `parent` was removed, or replaced by a
    /// move, as `d_delete` marks it.
    pub(crate) fn delete(&self, parent: &Arc<Dentry>, name: &str) {
        let removed = self
            .hashed()
            .entries
            .remove(&key(parent, name))
            .and_then(|dentry| dentry.upgrade());
        if let Some(removed) = removed {
            removed.state().dead = true;
        }
    }

    /// The entry `from_name` in `from` moved to `to_name` in `to`, taking
    /// the directory held there, and every one beneath it, along, as
    /// `d_move` does.
    pub(crate) fn rename(
        &self,
        from: &Arc<Dentry>,
        from_name: &str,
        to: &Arc<Dentry>,
        to_name: &str,
    ) {
        let mut hashed = self.hashed();
        let Some(moved) = hashed
            .entries
            .remove(&key(from, from_name))
            .and_then(|dentry| dentry.upgrade())
        else {
            return;
        };
        hashed
            .entries
            .insert(key(to, to_name), Arc::downgrade(&moved));
        let mut state = moved.state();
        state.parent = Some(to.clone());
        state.name = to_name.to_string();
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;

    use crate::filesystem::{
        Access, DirEntry, File, FileType, Filesystem, FilesystemError, Memory, Metadata,
        OpenOptions, Rename,
    };
    use crate::mount::Mount;
    use crate::profile::Profile;
    use crate::vfs::{self, OpenHow, Opened, Path, SuperBlock};

    /// A Filesystem answering each call by the path from its root, as one
    /// written in Ruby does, so a directory it opened reaches whatever
    /// stands at that path later and keeps no death of its own.
    struct ByPath {
        root: Arc<Memory>,
        at: String,
    }

    impl ByPath {
        fn path(&self, name: &str) -> String {
            [self.at.as_str(), name]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        }
    }

    impl Filesystem for ByPath {
        fn open(
            &self,
            path: &str,
            options: &OpenOptions,
        ) -> Result<Box<dyn File>, FilesystemError> {
            self.root.open(&self.path(path), options)
        }

        fn open_dir_nofollow(&self, path: &str) -> Result<Arc<dyn Filesystem>, FilesystemError> {
            let at = self.path(path);
            if self.root.symlink_metadata(&at)?.file_type != FileType::Directory {
                return Err(FilesystemError::NotADirectory);
            }
            Ok(Arc::new(Self {
                root: self.root.clone(),
                at,
            }))
        }

        fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&self.path(path))
        }

        fn stored_name(&self, name: &str) -> Result<String, FilesystemError> {
            Ok(name.to_string())
        }

        fn read_link(&self, _name: &str) -> Result<String, FilesystemError> {
            Err(FilesystemError::InvalidInput)
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            self.root.open_dir_nofollow(&self.at)?.entries()
        }

        fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.root.create_dir(&self.path(path))
        }

        fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
            self.root.remove_dir(&self.path(path))
        }

        fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
            self.root.remove_file(&self.path(path))
        }

        fn rename(
            &self,
            from: &str,
            to_dir: &dyn Filesystem,
            to: &str,
            rename: Rename,
        ) -> Result<(), FilesystemError> {
            let target = (to_dir as &dyn Any)
                .downcast_ref::<Self>()
                .ok_or(FilesystemError::CrossesDevices)?;
            self.root.rename(
                &self.path(from),
                self.root.as_ref(),
                &target.path(to),
                rename,
            )
        }
    }

    fn by_path(memory: &Arc<Memory>) -> Arc<SuperBlock> {
        SuperBlock::new(Arc::new(ByPath {
            root: memory.clone(),
            at: String::new(),
        }))
    }

    fn root_of(super_block: &Arc<SuperBlock>) -> Path {
        let mount = Mount::new("/data", super_block.clone(), Access::ReadWrite)
            .expect("/data is a guest path");
        Path::root(mount, Profile::Permissive)
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        match vfs::open(start, path, &how, true) {
            Ok(Opened::Directory(dir)) => dir,
            _ => panic!("{path} must open as a directory"),
        }
    }

    // The Filesystem answers by path, so only the dentry the removal marked
    // keeps the make from landing in the directory made in its place.
    // @behavior RT-159
    #[test]
    fn a_directory_removed_through_the_vfs_makes_nothing_more_whatever_its_filesystem() {
        let memory: Arc<Memory> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let root = root_of(&by_path(&memory));
        let dir = open_dir(&root, "dir");
        vfs::remove_file(&root, "dir/keep").expect("the file must go");
        vfs::remove_dir(&root, "dir").expect("the directory must go");
        vfs::create_dir(&root, "dir").expect("another directory must be made");
        vfs::create_dir(&root, "dir/other").expect("an entry must be made in it");
        assert_eq!(
            (
                vfs::create_dir(&dir, "made"),
                dir.entries(),
                memory.symlink_metadata("dir/made").err()
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
        let memory: Arc<Memory> = Arc::new([("dir/keep", "x")].into_iter().collect());
        let instance = by_path(&memory);
        let (first, second) = (root_of(&instance), root_of(&instance));
        let dir = open_dir(&first, "dir");
        vfs::remove_file(&second, "dir/keep").expect("the file must go");
        vfs::remove_dir(&second, "dir").expect("the directory must go");
        vfs::create_dir(&second, "dir").expect("another directory must be made");
        assert_eq!(
            vfs::create_dir(&dir, "made"),
            Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
            "vfs::create_dir through a directory opened beneath one Mount, once vfs::remove_dir \
             removed it beneath another Mount of the same SuperBlock, must answer NotFound"
        );
    }
}
