//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry. A name is found, and the directory held while one
//! is made, removed, or moved, through the VFS's `lookup_one_unlocked`,
//! `start_creating`, `start_removing`, and `__start_renaming`, and an entry
//! made, removed, or moved through its `vfs_mkdir`, `vfs_create`, `vfs_rmdir`,
//! `vfs_unlink`, and `vfs_rename`, as overlayfs's are, so the VFS checks what
//! stands there and the Layer's dentries follow each change. Each Layer is
//! asked its own permission as the Overlay's creator, as overlayfs asks
//! `inode_permission` of the real inode before it searches or opens one.

use std::any::Any;
use std::sync::Arc;

use super::super::tmpfs::TmpfsFile;
use super::super::{
    DirEntry, Errno, FileOperations, FileType, Filesystem, Iattr, InodeOperations, Kstat, Lookup,
    OpenHow, SetTime, Tmpfs,
};
use crate::locking::RwSemaphoreGuard;
use crate::vfs::{
    __start_renaming, inode_permission, lookup_one_unlocked, may_create_dentry, notify_change,
    start_creating, start_removing, vfs_create, vfs_mkdir, vfs_rename, vfs_rmdir, vfs_unlink, Cred,
    Dentry, DirContext, DirEmit, Mask, RenameFlag,
};

/// The change setting what `stat` reports of its times, as
/// `ovl_set_timestamps` sets `ATTR_ATIME_SET` and `ATTR_MTIME_SET`.
pub(super) fn timestamps(stat: &Kstat) -> Iattr<'static> {
    Iattr::times(stat.atime.map(SetTime::At), stat.mtime.map(SetTime::At))
}

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
            dentry: filesystem.root().expect(crate::vfs::MADE_ROOT).clone(),
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
    /// recorded on it.
    fn lookup_real(&self, name: &str) -> Result<Arc<Dentry>, Errno> {
        lookup_one_unlocked(&self.creator_cred, &self.filesystem, &self.dentry, name)
    }

    /// This directory held for making `name`, and its dentry.
    fn start_creating(&self, name: &str) -> Result<(RwSemaphoreGuard, Arc<Dentry>), Errno> {
        start_creating(&self.creator_cred, &self.filesystem, &self.dentry, name)
    }

    /// This directory held for removing what stands at `name`, and its dentry.
    fn start_removing(&self, name: &str) -> Result<(RwSemaphoreGuard, Arc<Dentry>), Errno> {
        start_removing(&self.creator_cred, &self.filesystem, &self.dentry, name)
    }

    fn inode_lock_shared(&self) -> Result<RwSemaphoreGuard, Errno> {
        self.dentry.inode_lock_shared()
    }

    fn inode_lock(&self) -> Result<RwSemaphoreGuard, Errno> {
        self.dentry.inode_lock()
    }

    /// This directory as a Tmpfs, the one kind holding an Overlay's marks.
    fn tmpfs(&self) -> Option<Arc<Tmpfs>> {
        let ops: Arc<dyn Any + Send + Sync> = self.dentry.inode_ops().ok()?;
        ops.downcast().ok()
    }

    /// This directory of the upper, which is always a Tmpfs.
    fn upper(&self) -> Result<Arc<Tmpfs>, Errno> {
        self.tmpfs().ok_or(Errno::ReadOnlyFilesystem)
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
        dentry.kind() == Ok(FileType::Chrdev) && self.tmpfs().is_some()
    }

    /// Whether this directory hides the Layers below it.
    pub(super) fn is_opaque(&self) -> bool {
        self.tmpfs()
            .and_then(|tmpfs| tmpfs.getxattr(OPAQUE_XATTR))
            .is_some_and(|value| value == OPAQUE)
    }

    pub(super) fn set_opaque(&self) -> Result<(), Errno> {
        let _held = self.inode_lock()?;
        self.upper()?.setxattr(OPAQUE_XATTR, OPAQUE)
    }

    pub(super) fn do_whiteout(&self, name: &str) -> Result<(), Errno> {
        let (_held, dentry) = self.start_creating(name)?;
        may_create_dentry(&self.creator_cred, &self.filesystem, &self.dentry, &dentry)?;
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
    ) -> Result<(), Errno> {
        let rd = __start_renaming(
            &self.filesystem,
            (&self.dentry, from),
            (&target.dentry, to),
            flag,
        )?;
        vfs_rename(&self.creator_cred, &self.filesystem, &rd)
    }

    pub(super) fn do_tmpfile(&self) -> Result<TmpfsFile, Errno> {
        self.upper()?.tmpfile()
    }

    /// Name `file` here, answering the dentry of what was linked, as
    /// `ovl_do_link` leaves it on the dentry it was handed, so nothing landing at
    /// the name afterwards is taken for it.
    pub(super) fn do_link(&self, file: &TmpfsFile, name: &str) -> Result<Arc<Dentry>, Errno> {
        let (_held, dentry) = self.start_creating(name)?;
        may_create_dentry(&self.creator_cred, &self.filesystem, &self.dentry, &dentry)?;
        let upper = self.upper()?;
        upper.link(file, name)?;
        if let Lookup::Found(inode) = upper.lookup(&dentry)? {
            dentry.add(Some(inode));
        }
        Ok(dentry)
    }

    /// Set this directory's times to those `stat` reports, as
    /// `ovl_set_timestamps` hands `notify_change` the upper dentry.
    pub(super) fn set_timestamps(&self, stat: &Kstat) -> Result<(), Errno> {
        let held = self.dentry.inode_lock()?;
        notify_change(&self.dentry, &held, &timestamps(stat))
    }

    /// `name` as this directory stores it and the dentry standing there, `None`
    /// where nothing does, as `ovl_lookup_single` asks a layer once for a name.
    pub(super) fn lookup_single(&self, name: &str) -> Result<Option<(String, Arc<Dentry>)>, Errno> {
        let dentry = self.lookup_real(name)?;
        Ok(dentry.inode().is_some().then(|| (dentry.name(), dentry)))
    }

    /// The directory `dentry` names beneath this one.
    pub(super) fn beneath(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            dentry,
            ..self.clone()
        }
    }

    /// Make the file `name`, holding the directory, as `ovl_do_create` does.
    pub(super) fn do_create(&self, name: &str) -> Result<(), Errno> {
        let (_held, child) = self.start_creating(name)?;
        vfs_create(&self.creator_cred, &self.filesystem, &self.dentry, &child)
    }

    /// The directory standing at `name`, a link there refused as a loop, as
    /// `ovl_lookup` follows no link in a Layer.
    pub(super) fn open_dir(&self, name: &str) -> Result<Self, Errno> {
        let dentry = self.lookup_real(name)?;
        let inode = dentry.inode().ok_or(Errno::NotFound)?;
        match inode.kind() {
            FileType::Dir => Ok(self.beneath(dentry)),
            FileType::Symlink => Err(Errno::FilesystemLoop),
            _ => Err(Errno::NotADirectory),
        }
    }

    pub(super) fn getattr(&self) -> Result<Kstat, Errno> {
        self.dentry.getattr()
    }

    pub(super) fn getattr_at(&self, name: &str) -> Result<Kstat, Errno> {
        self.lookup_real(name)?.getattr()
    }

    /// This directory opened, kept open for whoever holds it, as overlayfs's
    /// `ovl_dir_open` opens the real directory.
    pub(super) fn open_real(&self) -> Result<Box<dyn FileOperations>, Errno> {
        self.open_realfile(&OpenHow {
            read: true,
            ..OpenHow::default()
        })
    }

    /// What this dentry names opened as `how` asks, once its Layer lets the
    /// Overlay's creator open it so, as `ovl_open_realfile` asks
    /// `inode_permission` for `MAY_OPEN` and the access before it opens.
    pub(super) fn open_realfile(&self, how: &OpenHow) -> Result<Box<dyn FileOperations>, Errno> {
        let mut mask = Mask::OPEN;
        if how.read {
            mask = mask | Mask::READ;
        }
        if how.write {
            mask = mask | Mask::WRITE;
        }
        inode_permission(&self.creator_cred, &self.filesystem, &self.dentry, mask)?;
        self.dentry.open(how)
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, Errno> {
        let _held = self.inode_lock_shared()?;
        let mut entries = Vec::new();
        let mut actor = |emit: &DirEmit<'_>| {
            entries.push(emit.to_dir_entry());
            true
        };
        self.open_real()?
            .iterate_shared(&self.dentry, &mut DirContext::new(2, &mut actor))?;
        Ok(entries)
    }

    pub(super) fn do_mkdir(&self, name: &str) -> Result<(), Errno> {
        let (_held, child) = self.start_creating(name)?;
        vfs_mkdir(&self.creator_cred, &self.filesystem, &self.dentry, &child)
    }

    pub(super) fn do_rmdir(&self, name: &str) -> Result<(), Errno> {
        let (_held, victim) = self.start_removing(name)?;
        vfs_rmdir(&self.creator_cred, &self.filesystem, &self.dentry, &victim)
    }

    pub(super) fn do_unlink(&self, name: &str) -> Result<(), Errno> {
        let (_held, victim) = self.start_removing(name)?;
        vfs_unlink(&self.creator_cred, &self.filesystem, &self.dentry, &victim)
    }
}
