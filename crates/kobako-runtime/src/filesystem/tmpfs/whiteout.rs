//! The whiteout a Tmpfs holds for an Overlay, and the moves that place
//! one, as tmpfs's `shmem_whiteout` makes a character device 0:0 and
//! `shmem_rename2` takes `RENAME_NOREPLACE`, `RENAME_EXCHANGE`, and
//! `RENAME_WHITEOUT`.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::Errno;
use super::{Node, Space, Times, Tmpfs};
use crate::vfs::{FileType, Kstat, RenameFlag};

/// A whiteout, counting one inode for as long as it stands, as the
/// character device `shmem_mknod` makes for one does: an inode of its own,
/// with its own times.
pub(super) struct Whiteout {
    space: Arc<Space>,
    dev: u64,
    ino: u64,
    times: Mutex<Times>,
}

impl Whiteout {
    pub(super) fn getattr(&self) -> Kstat {
        self.times().report(
            Kstat::new(FileType::Chrdev, 0)
                .with_identity(self.dev, self.ino)
                .with_nlink(1),
        )
    }

    pub(super) fn ino(&self) -> u64 {
        self.ino
    }

    pub(super) fn times(&self) -> MutexGuard<'_, Times> {
        self.times.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Whiteout {
    fn drop(&mut self) {
        self.space.inodes.give(1);
    }
}

impl Tmpfs {
    fn new_whiteout(&self) -> Result<Node, Errno> {
        self.tree.space.inodes.take(1)?;
        Ok(Node::Whiteout(Arc::new(Whiteout {
            space: self.tree.space.clone(),
            dev: self.tree.dev,
            ino: self.tree.next_ino.fetch_add(1, Ordering::Relaxed),
            times: Mutex::new(Times::now()),
        })))
    }

    /// Whether a whiteout stands at `name`, as overlayfs's
    /// `ovl_is_whiteout` reads a character device 0:0.
    pub(crate) fn is_whiteout(&self, name: &str) -> bool {
        matches!(self.dir.get(name), Ok(Node::Whiteout(_)))
    }

    /// Make a whiteout at `name`, as `shmem_mknod` makes the character
    /// device 0:0 `vfs_whiteout` asks for.
    pub(crate) fn whiteout(&self, name: &str) -> Result<(), Errno> {
        let whiteout = self.new_whiteout()?;
        self.dir.state().insert(name, whiteout);
        Ok(())
    }

    /// Move `old_name` to `new_name` in `target` as `shmem_rename2` does with
    /// `flags`, the entry taking all it holds, so no name beneath it
    /// changes. Without one, what stands at `new_name` goes as rename(2)
    /// replaces it, once the VFS found the move fits what stands on each
    /// side.
    pub(super) fn rename_at(
        &self,
        old_name: &str,
        target: &Tmpfs,
        new_name: &str,
        flags: Option<RenameFlag>,
    ) -> Result<(), Errno> {
        let (old_dir, new_dir) = (&self.dir, &target.dir);
        let moving = old_dir.get(old_name)?;
        let replaced = new_dir.get(new_name).ok();
        if flags == Some(RenameFlag::Exchange) {
            let replaced = replaced.ok_or(Errno::NotFound)?;
            old_dir.state().exchange(old_name, replaced);
            new_dir.state().exchange(new_name, moving);
            return Ok(());
        }
        if let Some(Node::Directory(dir)) = &replaced {
            if !dir.state().entries.is_empty() {
                return Err(Errno::DirectoryNotEmpty);
            }
        }
        let left = match flags {
            Some(RenameFlag::Whiteout) => Some(self.new_whiteout()?),
            _ => None,
        };
        match left {
            Some(left) => old_dir.state().exchange(old_name, left),
            None => old_dir.state().take(old_name),
        }
        new_dir.state().insert(new_name, moving);
        Ok(())
    }
}
