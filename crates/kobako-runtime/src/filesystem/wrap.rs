//! A test's Filesystem over another: every call forwards to the entry it
//! wraps unless its `Hook` steps in, and every entry a lookup finds is
//! wrapped too, as a Filesystem wrapping another must. It makes a file
//! through `create` and leaves the open to the VFS, as overlayfs does, so an
//! open's lookup reaches the `Hook` as any lookup does. It hands the
//! Filesystem beneath the dentry it is handed, where overlayfs hands a real
//! one, so what that Filesystem makes is recorded unwrapped and later calls
//! on it reach that Filesystem past the `Hook`.

use std::any::Any;
use std::sync::Arc;

use super::{
    DentryOperations, FileOperations, FileSystemType, Filesystem, FilesystemError, Iattr,
    InodeOperations, Kstat, Lookup, Mask, OpenHow, RenameFlag,
};
use crate::vfs::{Dentry, Inode};

/// What a test changes of the Filesystem beneath it.
pub(crate) trait Hook: Send + Sync + 'static {
    fn permission(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
        mask: Mask,
    ) -> Result<(), FilesystemError> {
        inner.permission(dentry, mask)
    }

    fn revalidate(
        &self,
        ops: &dyn DentryOperations,
        dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        ops.revalidate(dir, dentry)
    }

    fn getattr(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<Kstat, FilesystemError> {
        inner.getattr(dentry)
    }

    fn lookup(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<Lookup, FilesystemError> {
        inner.lookup(dentry)
    }

    fn open(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
        how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        inner.open(dentry, how)
    }

    fn mkdir(&self, inner: &dyn InodeOperations, dentry: &Dentry) -> Result<(), FilesystemError> {
        inner.mkdir(dentry)
    }

    fn rmdir(&self, inner: &dyn InodeOperations, dentry: &Dentry) -> Result<(), FilesystemError> {
        inner.rmdir(dentry)
    }

    fn unlink(&self, inner: &dyn InodeOperations, dentry: &Dentry) -> Result<(), FilesystemError> {
        inner.unlink(dentry)
    }

    fn rename(
        &self,
        inner: &dyn InodeOperations,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        inner.rename(from, to_dir, to, flag)
    }
}

/// A Filesystem over the one `fs` makes, with `hook` stepping in.
pub(crate) struct Wrap<H> {
    fs: Arc<dyn FileSystemType>,
    hook: Arc<H>,
}

impl<H: Hook> Wrap<H> {
    pub(crate) fn new(fs: Arc<dyn FileSystemType>, hook: Arc<H>) -> Self {
        Self { fs, hook }
    }
}

impl<H: Hook> FileSystemType for Wrap<H> {
    fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
        self.fs.clone().fill_super(sb)?;
        if let Some(ops) = sb.d_op().cloned() {
            sb.set_default_d_op(Arc::new(WrapDentry {
                ops,
                hook: self.hook.clone(),
            }));
        }
        if let Some(root) = sb.root().and_then(|root| root.inode()) {
            let wrapped = WrapInode {
                inner: root.ops().clone(),
                hook: self.hook.clone(),
            };
            sb.d_make_root(root.with_ops(Arc::new(wrapped)));
        }
        Ok(())
    }
}

/// `inner`, an entry of the Filesystem beneath, with `hook` stepping in.
struct WrapInode<H> {
    inner: Arc<dyn InodeOperations>,
    hook: Arc<H>,
}

impl<H: Hook> WrapInode<H> {
    /// `found` with the entry it names wrapped too.
    fn wrap(&self, found: Lookup) -> Lookup {
        let wrapped = |inode: Arc<Inode>| {
            let ops = Self {
                inner: inode.ops().clone(),
                hook: self.hook.clone(),
            };
            inode.with_ops(Arc::new(ops))
        };
        match found {
            Lookup::Found(inode) => Lookup::Found(wrapped(inode)),
            Lookup::Spelled { name, inode } => Lookup::Spelled {
                name,
                inode: wrapped(inode),
            },
            Lookup::Absent => Lookup::Absent,
        }
    }
}

/// `ops`, handed the directory a `WrapInode<H>` wraps.
struct WrapDentry<H> {
    ops: Arc<dyn DentryOperations>,
    hook: Arc<H>,
}

impl<H: Hook> DentryOperations for WrapDentry<H> {
    fn revalidate(
        &self,
        dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        let dir = (dir as &dyn Any)
            .downcast_ref::<WrapInode<H>>()
            .map_or(dir, |wrap| wrap.inner.as_ref());
        self.hook.revalidate(self.ops.as_ref(), dir, dentry)
    }
}

impl<H: Hook> InodeOperations for WrapInode<H> {
    fn permission(&self, dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
        self.hook.permission(self.inner.as_ref(), dentry, mask)
    }

    fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
        self.hook.getattr(self.inner.as_ref(), dentry)
    }

    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        Ok(self.wrap(self.hook.lookup(self.inner.as_ref(), dentry)?))
    }

    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        self.inner.setattr(dentry, attr)
    }

    fn get_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.inner.get_link(dentry)
    }

    fn open(
        &self,
        dentry: &Dentry,
        how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        self.hook.open(self.inner.as_ref(), dentry, how)
    }

    fn create(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.inner.create(dentry)
    }

    fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.hook.mkdir(self.inner.as_ref(), dentry)
    }

    fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.hook.rmdir(self.inner.as_ref(), dentry)
    }

    fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.hook.unlink(self.inner.as_ref(), dentry)
    }

    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .ok_or(FilesystemError::CrossesDevices)?;
        self.hook
            .rename(self.inner.as_ref(), from, target.inner.as_ref(), to, flag)
    }
}
