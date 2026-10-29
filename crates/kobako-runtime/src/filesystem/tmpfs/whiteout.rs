//! The whiteout a Tmpfs holds for an Overlay, and the moves that place
//! one, as tmpfs's `shmem_whiteout` makes a character device 0:0 and
//! `shmem_rename2` takes `RENAME_NOREPLACE`, `RENAME_EXCHANGE`, and
//! `RENAME_WHITEOUT`.

use std::sync::Arc;

use super::super::FilesystemError;
use super::{Node, Space, Tmpfs};
use crate::vfs::RenameFlag;

/// A whiteout, counting one inode for as long as it stands, as the
/// character device tmpfs makes for one does.
pub(super) struct Whiteout {
    space: Arc<Space>,
}

impl Drop for Whiteout {
    fn drop(&mut self) {
        self.space.inodes.give(1);
    }
}

impl Tmpfs {
    fn new_whiteout(&self) -> Result<Node, FilesystemError> {
        self.tree.space.inodes.take(1)?;
        Ok(Node::Whiteout {
            _inode: Arc::new(Whiteout {
                space: self.tree.space.clone(),
            }),
        })
    }

    /// Whether a whiteout stands at `name`, as overlayfs's
    /// `ovl_is_whiteout` reads a character device 0:0.
    pub(crate) fn is_whiteout(&self, name: &str) -> bool {
        matches!(self.dir.get(name), Ok(Node::Whiteout { .. }))
    }

    /// Make a whiteout at `name`, as `shmem_mknod` makes the character
    /// device 0:0 `vfs_whiteout` asks for.
    pub(crate) fn whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let whiteout = self.new_whiteout()?;
        self.dir.state().insert(name, whiteout);
        Ok(())
    }

    /// Move `from` to `to` in `target` as `shmem_rename2` does with
    /// `flag`, the entry taking all it holds, so no name beneath it
    /// changes. Without one, what stands at `to` goes as rename(2)
    /// replaces it, once the VFS found the move fits what stands on each
    /// side.
    pub(crate) fn rename2(
        &self,
        from: &str,
        target: &Tmpfs,
        to: &str,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        if !Arc::ptr_eq(&target.tree, &self.tree) {
            return Err(FilesystemError::CrossesDevices);
        }
        let (from_dir, to_dir) = (&self.dir, &target.dir);
        let moving = from_dir.get(from)?;
        let replaced = to_dir.get(to).ok();
        if flag == Some(RenameFlag::Exchange) {
            let replaced = replaced.ok_or(FilesystemError::NotFound)?;
            from_dir.state().insert(from, replaced);
            to_dir.state().insert(to, moving);
            return Ok(());
        }
        if let Some(Node::Directory(dir)) = &replaced {
            if !dir.state().entries.is_empty() {
                return Err(FilesystemError::DirectoryNotEmpty);
            }
        }
        let left = match flag {
            Some(RenameFlag::Whiteout) => Some(self.new_whiteout()?),
            _ => None,
        };
        match left {
            Some(left) => from_dir.state().insert(from, left),
            None => from_dir.state().remove(from),
        }
        to_dir.state().insert(to, moving);
        if let Some(Node::Directory(_)) = replaced {
            self.tree.space.inodes.give(1);
        }
        Ok(())
    }
}
