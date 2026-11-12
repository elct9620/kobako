//! A test's way to ask a Filesystem about a path, as kernel code reaches a
//! file by name through `kern_path`: the VFS resolves the path to a
//! dentry, as it does for every call, and the Filesystem is asked about
//! that dentry, a change going through the VFS's own `vfs_*` helpers as
//! kernel code's does, so a test sees what the Filesystem answers beneath
//! the VFS. It takes no directory's lock, so a test holding one can
//! still reach the Filesystem through it.

use std::sync::Arc;

use super::cred::Cred;
use super::dentry::Dentry;
use super::namei::{
    atomic_open, do_file_open, filename_lookup, filename_parentat, lookup_one_qstr_excl,
    may_create_dentry, vfs_mkdir, vfs_rename, vfs_rmdir, vfs_unlink, Nameidata, RenameData,
};
use super::path::{Path, Start};
use super::{
    build_open_flags, DirEntry, Errno, File, FileOperations, FileSystemType, FileType, Iattr,
    Kstat, OpenHow, SuperBlock,
};
use crate::mount::{Access, Mount};

/// Open `path` beneath `start` for `cred` as `how` asks, as kernel code opens a
/// file by name through `filp_open`, which builds the flags `do_file_open`
/// takes.
pub(crate) fn filp_open(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
    how: &OpenHow,
) -> Result<File, Errno> {
    do_file_open(cred, start, path, &build_open_flags(how)?)
}

/// A directory of a Filesystem reached through the VFS.
#[derive(Clone)]
pub(crate) struct KernPath(Path);

/// The root of `filesystem`, mounted read-write.
pub(crate) fn kern_path(filesystem: Arc<dyn FileSystemType>) -> KernPath {
    let filesystem = SuperBlock::new(filesystem).expect("the instance must stand");
    let mount = Mount::new("/data", filesystem, Access::ReadWrite).expect("/data is a guest path");
    KernPath(Path::root(mount.mnt()))
}

impl KernPath {
    /// `path` resolved beneath this directory, a link at its end left as
    /// it stands.
    fn resolve(&self, path: &str) -> Result<Nameidata, Errno> {
        filename_lookup(&Arc::default(), &self.0, path, false)
    }

    /// The directory holding the last name of `path`, and that name's dentry,
    /// whether or not anything stands there.
    fn parent(&self, path: &str) -> Result<(Nameidata, Arc<Dentry>), Errno> {
        let nd = filename_parentat(&Arc::default(), &self.0, path)?;
        let child = lookup_one_qstr_excl(nd.mnt.sb(), nd.dentry(), &nd.last)?;
        Ok((nd, child))
    }

    pub(crate) fn open(&self, path: &str, how: &OpenHow) -> Result<Box<dyn FileOperations>, Errno> {
        let child = self.resolve(path)?.path()?;
        // A link opens as itself nowhere, as `may_open` refuses one first.
        if child.kind() == Ok(FileType::Symlink) {
            return Err(Errno::FilesystemLoop);
        }
        let file = child.open(how)?;
        if how.truncate {
            child
                .inode()
                .ok_or(Errno::NotFound)?
                .ops()
                .setattr(&child, &Iattr::open_truncate())?;
        }
        Ok(file)
    }

    /// Open the file at `path`, made first where nothing stands.
    pub(crate) fn open_or_create(
        &self,
        path: &str,
        how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, Errno> {
        match self.vfs_lstat(path) {
            Err(Errno::NotFound) => self.create(path, how),
            _ => self.open(path, how),
        }
    }

    /// Make the file at `path` and open it, as `->atomic_open` does.
    pub(crate) fn create(
        &self,
        path: &str,
        how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, Errno> {
        let (nd, child) = self.parent(path)?;
        may_create_dentry(&nd.cred, nd.mnt.sb(), nd.dentry(), &child)?;
        let how = OpenHow {
            create: true,
            ..*how
        };
        let opened = atomic_open(nd.mnt.sb(), nd.dentry(), &child, &how)?;
        match opened.file {
            Some(file) => Ok(file),
            None => opened.spliced.unwrap_or(child).open(&how),
        }
    }

    /// Truncate or extend the file at `path`, as `->setattr` is asked.
    pub(crate) fn truncate(&self, path: &str, len: u64) -> Result<(), Errno> {
        self.setattr(path, &Iattr::size(len))
    }

    /// Hand `attr` to what `path` names, as `->setattr` is asked.
    pub(crate) fn setattr(&self, path: &str, attr: &Iattr) -> Result<(), Errno> {
        let child = self.resolve(path)?.path()?;
        child
            .inode()
            .ok_or(Errno::NotFound)?
            .ops()
            .setattr(&child, attr)
    }

    pub(crate) fn open_dir(&self, path: &str) -> Result<Self, Errno> {
        let dentry = self.resolve(path)?.path()?;
        match dentry.kind()? {
            FileType::Symlink => Err(Errno::FilesystemLoop),
            FileType::Dir => {
                dentry.getattr()?;
                Ok(Self(self.0.with_dentry(dentry)))
            }
            _ => Err(Errno::NotADirectory),
        }
    }

    /// What the directory reports of itself, as `vfs_getattr`.
    pub(crate) fn vfs_getattr(&self) -> Result<Kstat, Errno> {
        self.0.dentry().getattr()
    }

    /// What `path` names, a link reported as itself, as `vfs_lstat`; the
    /// directory itself for the empty path.
    pub(crate) fn vfs_lstat(&self, path: &str) -> Result<Kstat, Errno> {
        if path.is_empty() {
            return self.vfs_getattr();
        }
        self.resolve(path)?.getattr()
    }

    pub(crate) fn lookup(&self, path: &str) -> Result<String, Errno> {
        let nd = filename_parentat(&Arc::default(), &self.0, path)?;
        Ok(lookup_one_qstr_excl(nd.mnt.sb(), nd.dentry(), &nd.last)?.name())
    }

    #[cfg(unix)]
    pub(crate) fn vfs_readlink(&self, path: &str) -> Result<String, Errno> {
        self.resolve(path)?.path()?.get_link()
    }

    pub(crate) fn iterate_dir(&self) -> Result<Vec<DirEntry>, Errno> {
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        File::dentry_open(self.0.clone(), &read, &Arc::default())?.iterate_shared()
    }

    pub(crate) fn vfs_mkdir(&self, path: &str) -> Result<(), Errno> {
        let (nd, child) = self.parent(path)?;
        vfs_mkdir(&nd.cred, nd.mnt.sb(), nd.dentry(), &child)
    }

    pub(crate) fn vfs_rmdir(&self, path: &str) -> Result<(), Errno> {
        let (nd, child) = self.parent(path)?;
        vfs_rmdir(&nd.cred, nd.mnt.sb(), nd.dentry(), &child)
    }

    pub(crate) fn vfs_unlink(&self, path: &str) -> Result<(), Errno> {
        let (nd, child) = self.parent(path)?;
        vfs_unlink(&nd.cred, nd.mnt.sb(), nd.dentry(), &child)
    }

    /// Move `from` beneath this directory to `to` beneath `to_dir`,
    /// refused where the source stands above the target's directory, as
    /// `start_renaming` refuses it.
    pub(crate) fn rename(&self, from: &str, to_dir: &Self, to: &str) -> Result<(), Errno> {
        let ((source, old), (target, new)) = (self.parent(from)?, to_dir.parent(to)?);
        if target.dentry().is_subdir(&old) {
            return Err(Errno::InvalidInput);
        }
        if source.dentry().is_subdir(&new) {
            return Err(Errno::DirectoryNotEmpty);
        }
        vfs_rename(
            &source.cred,
            source.mnt.sb(),
            &RenameData {
                _held: Vec::new(),
                old_parent: source.dentry().clone(),
                old_dentry: old,
                new_parent: target.dentry().clone(),
                new_dentry: new,
                flag: None,
            },
        )
    }
}
