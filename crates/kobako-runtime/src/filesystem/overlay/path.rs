//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry. Each call holds that directory's lock as the
//! VFS helpers overlayfs calls do: shared while a name is looked up, as
//! `lookup_one_unlocked`, and exclusive while one is made, removed, or
//! moved, as `start_creating`, `start_removing`, and `start_renaming`. A
//! name is found through the VFS's `lookup_one_qstr_excl`, and an entry made,
//! removed, or moved through its `vfs_mkdir`, `vfs_create`, `vfs_rmdir`,
//! `vfs_unlink`, and `vfs_rename`, as overlayfs's are, so the VFS checks
//! what stands there and the Layer's dentries follow each change.

use std::any::Any;
use std::sync::Arc;

use super::super::tmpfs::TmpfsFile;
use super::super::{
    DirEntry, FileOperations, FileType, Filesystem, FilesystemError, InodeOperations, Lookup,
    Metadata, OpenOptions, Tmpfs,
};
use crate::locking::RwSemaphoreGuard;
use crate::vfs::{
    __start_renaming, lookup_one_qstr_excl, may_create_dentry, vfs_create, vfs_mkdir, vfs_rename,
    vfs_rmdir, vfs_unlink, Cred, Dentry, DirContext, DirEmit, RenameFlag,
};

/// The attribute marking a directory opaque, and its value, as
/// overlayfs's `OVL_XATTR_OPAQUE`.
const OPAQUE_XATTR: &str = "trusted.overlay.opaque";
const OPAQUE: &[u8] = b"y";

#[derive(Clone)]
pub(super) struct OverlayPath {
    filesystem: Arc<Filesystem>,
    dentry: Arc<Dentry>,
    /// Who every call on a Layer is made as, whoever asked the view, as overlayfs
    /// overrides the caller's creds with `ofs->creator_cred`.
    creator_cred: Arc<Cred>,
}

impl OverlayPath {
    /// The root of the Layer `filesystem` stands for.
    pub(super) fn root(filesystem: &Arc<Filesystem>, creator_cred: &Arc<Cred>) -> Self {
        Self {
            dentry: filesystem.dentries().root(),
            filesystem: filesystem.clone(),
            creator_cred: creator_cred.clone(),
        }
    }

    /// Where this directory stands in its Layer.
    pub(super) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// Whether what this Layer holds at `name`, if it holds a dentry there, is
    /// `real`, as `ovl_matches_upper` compares what stands in the upper.
    pub(super) fn matches_real(&self, name: &str, real: &Arc<Dentry>) -> bool {
        let held = self.filesystem.dentries().lookup(&self.dentry, name);
        held.is_none_or(|held| Arc::ptr_eq(&held, real))
    }

    /// The dentry of `name` as this directory stores it, what stands there
    /// recorded on it. The caller holds this directory's lock.
    fn lookup_real(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        Ok(lookup_one_qstr_excl(&self.filesystem, &self.dentry, name)?.1)
    }

    fn inode_lock_shared(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock_shared()
    }

    fn inode_lock(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock()
    }

    /// This directory as a Tmpfs, the one kind holding an Overlay's marks.
    fn tmpfs(&self) -> Option<Arc<Tmpfs>> {
        let ops: Arc<dyn Any + Send + Sync> = self.dentry.inode_ops().ok()?;
        ops.downcast().ok()
    }

    /// This directory of the upper, which is always a Tmpfs.
    fn upper(&self) -> Result<Arc<Tmpfs>, FilesystemError> {
        self.tmpfs().ok_or(FilesystemError::ReadOnlyFilesystem)
    }

    /// Whether a whiteout stands at `name`, as `ovl_is_whiteout` reads the
    /// entry; a Layer that is no Tmpfs holds none.
    pub(super) fn is_whiteout(&self, name: &str) -> bool {
        self.tmpfs().is_some_and(|tmpfs| tmpfs.is_whiteout(name))
    }

    /// Whether `dentry`, found in this directory, stands for a whiteout, as
    /// `ovl_is_whiteout` reads a dentry, a character device being one only in a
    /// Tmpfs, whose marks an Overlay reads.
    pub(super) fn is_whiteout_dentry(&self, dentry: &Dentry) -> bool {
        dentry.kind() == Ok(FileType::CharDevice) && self.tmpfs().is_some()
    }

    /// Whether this directory hides the Layers below it.
    pub(super) fn is_opaque(&self) -> bool {
        self.tmpfs()
            .and_then(|tmpfs| tmpfs.getxattr(OPAQUE_XATTR))
            .is_some_and(|value| value == OPAQUE)
    }

    pub(super) fn set_opaque(&self) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        self.upper()?.setxattr(OPAQUE_XATTR, OPAQUE)
    }

    pub(super) fn do_whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        may_create_dentry(
            &self.creator_cred,
            &self.filesystem,
            &self.dentry,
            &*self.lookup_real(name)?,
        )?;
        self.upper()?.whiteout(name)
    }

    /// Move `from` here to `to` in `target` as `renameat2` does with
    /// `flag`.
    pub(super) fn do_rename(
        &self,
        from: &str,
        target: &OverlayPath,
        to: &str,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        let rd = __start_renaming(
            &self.filesystem,
            (&self.dentry, from),
            (&target.dentry, to),
            flag,
        )?;
        vfs_rename(&self.creator_cred, &self.filesystem, &rd)
    }

    pub(super) fn do_tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        self.upper()?.tmpfile()
    }

    /// Name `file` here, answering the dentry of what was linked, as
    /// `ovl_do_link` leaves it on the dentry it was handed, so nothing landing at
    /// the name afterwards is taken for it.
    pub(super) fn do_link(
        &self,
        file: &TmpfsFile,
        name: &str,
    ) -> Result<Arc<Dentry>, FilesystemError> {
        let _held = self.inode_lock()?;
        let dentry = self.lookup_real(name)?;
        may_create_dentry(&self.creator_cred, &self.filesystem, &self.dentry, &dentry)?;
        let upper = self.upper()?;
        upper.link(file, name)?;
        if let Lookup::Found(inode) = upper.lookup(&dentry)? {
            dentry.add(Some(inode));
        }
        Ok(dentry)
    }

    pub(super) fn set_times(&self, metadata: &Metadata) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        self.upper()?
            .set_times(metadata.accessed, metadata.modified);
        Ok(())
    }

    /// `name` as this directory stores it and the dentry standing there, `None`
    /// where nothing does, as `ovl_lookup_single` asks a layer once for a name.
    pub(super) fn lookup_single(
        &self,
        name: &str,
    ) -> Result<Option<(String, Arc<Dentry>)>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let (stored, dentry) = lookup_one_qstr_excl(&self.filesystem, &self.dentry, name)?;
        Ok(dentry.inode().is_some().then_some((stored, dentry)))
    }

    /// The directory `dentry` names beneath this one.
    pub(super) fn beneath(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            dentry,
            ..self.clone()
        }
    }

    /// Make the file `name`, holding the directory, as `ovl_do_create` does.
    pub(super) fn do_create(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_create(
            &self.creator_cred,
            &self.filesystem,
            &self.dentry,
            &*self.lookup_real(name)?,
        )
    }

    /// The directory standing at `name`, a link there refused as a loop, as
    /// `ovl_lookup` follows no link in a Layer.
    pub(super) fn open_dir(&self, name: &str) -> Result<Self, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let dentry = self.lookup_real(name)?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        match inode.kind() {
            FileType::Directory => Ok(self.beneath(dentry)),
            FileType::Symlink => Err(FilesystemError::FilesystemLoop),
            _ => Err(FilesystemError::NotADirectory),
        }
    }

    pub(super) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.dentry.getattr()
    }

    pub(super) fn symlink_metadata(&self, name: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        self.lookup_real(name)?.getattr()
    }

    /// This directory opened, kept open for whoever holds it, as overlayfs's
    /// `ovl_dir_open` opens the real directory.
    pub(super) fn open_real(&self) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let options = OpenOptions {
            read: true,
            ..OpenOptions::default()
        };
        self.dentry.inode_ops()?.open(&self.dentry, &options)
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let options = OpenOptions {
            read: true,
            ..OpenOptions::default()
        };
        let mut entries = Vec::new();
        let mut actor = |emit: &DirEmit<'_>| {
            entries.push(emit.to_dir_entry());
            true
        };
        self.dentry
            .inode_ops()?
            .open(&self.dentry, &options)?
            .iterate_shared(&self.dentry, &mut DirContext::new(2, &mut actor))?;
        Ok(entries)
    }

    pub(super) fn do_mkdir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        let child = self.lookup_real(name)?;
        vfs_mkdir(&self.creator_cred, &self.filesystem, &self.dentry, &child)
    }

    pub(super) fn do_rmdir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_rmdir(
            &self.creator_cred,
            &self.filesystem,
            &self.dentry,
            &self.lookup_real(name)?,
        )
    }

    pub(super) fn do_unlink(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_unlink(
            &self.creator_cred,
            &self.filesystem,
            &self.dentry,
            &self.lookup_real(name)?,
        )
    }
}
