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
        matches!(self.dir.find(name), Ok(Node::Whiteout { .. }))
    }

    /// Make a whiteout at `name`, as `vfs_whiteout` does; refused when
    /// anything stands there.
    pub(crate) fn whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let (dir, name) = self.dir.parent_of(name)?;
        if dir.state().entries.contains_key(name) {
            return Err(FilesystemError::AlreadyExists);
        }
        let whiteout = self.new_whiteout()?;
        dir.state().insert(name, whiteout);
        Ok(())
    }

    /// Move `from` to `to` in `target` as `renameat2` does with `flag`,
    /// the entry taking all it holds, so no name beneath it changes.
    /// Without one, what stands at `to` goes as rename(2) replaces it. A
    /// directory never moves into itself or beneath itself, which would
    /// cut it off from the root for good, as Linux refuses it once it
    /// finds the source above the target, or with an exchange the target
    /// above the source.
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
        let (from_dir, from_name) = self.dir.parent_of(from)?;
        let (to_dir, to_name) = target.dir.parent_of(to)?;
        let moving = from_dir
            .state()
            .entries
            .get(from_name)
            .cloned()
            .ok_or(FilesystemError::NotFound)?;
        let replaced = to_dir.state().entries.get(to_name).cloned();
        let beneath = |node: &Node, dir: &Arc<super::Dir>| matches!(node, Node::Directory(node) if dir.within(node));
        if beneath(&moving, &to_dir) {
            return Err(FilesystemError::InvalidInput);
        }
        if flag == Some(RenameFlag::Exchange) {
            let replaced = replaced.ok_or(FilesystemError::NotFound)?;
            if beneath(&replaced, &from_dir) {
                return Err(FilesystemError::InvalidInput);
            }
            Self::reparent(&replaced, &from_dir);
            Self::reparent(&moving, &to_dir);
            from_dir.state().insert(from_name, replaced);
            to_dir.state().insert(to_name, moving);
            return Ok(());
        }
        match (&moving, &replaced) {
            (_, None) => {}
            (_, Some(_)) if flag == Some(RenameFlag::NoReplace) => {
                return Err(FilesystemError::AlreadyExists)
            }
            (Node::Directory(_), Some(Node::Directory(dir))) if !dir.state().entries.is_empty() => {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            (Node::Directory(_), Some(Node::Directory(_))) => {}
            (Node::Directory(_), Some(_)) => return Err(FilesystemError::NotADirectory),
            (_, Some(Node::Directory(_))) => return Err(FilesystemError::IsADirectory),
            _ => {}
        }
        let left = match flag {
            Some(RenameFlag::Whiteout) => Some(self.new_whiteout()?),
            _ => None,
        };
        match left {
            Some(left) => from_dir.state().insert(from_name, left),
            None => from_dir.state().remove(from_name),
        }
        Self::reparent(&moving, &to_dir);
        to_dir.state().insert(to_name, moving);
        if let Some(Node::Directory(replaced)) = replaced {
            replaced.state().removed = true;
            self.tree.space.inodes.give(1);
        }
        Ok(())
    }

    fn reparent(node: &Node, dir: &Arc<super::Dir>) {
        if let Node::Directory(moved) = node {
            moved.state().parent = Arc::downgrade(dir);
        }
    }
}
