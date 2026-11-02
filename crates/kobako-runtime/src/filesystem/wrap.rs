//! A test's Filesystem over another: every call forwards to the entry it
//! wraps unless its `Hook` steps in, and every entry a lookup finds is
//! wrapped too, as a Filesystem wrapping another must.

use std::any::Any;
use std::sync::Arc;

use super::{
    DentryOperations, FileOperations, Filesystem, FilesystemError, Iattr, InodeOperations, Lookup,
    Mask, Metadata, OpenOptions, RenameFlag,
};
use crate::vfs::Dentry;

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
    ) -> Result<Metadata, FilesystemError> {
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
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        inner.open(dentry, options)
    }

    fn atomic_open(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        inner.atomic_open(dentry, options)
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

/// `inner`, with `hook` stepping in.
pub(crate) struct Wrap<H> {
    inner: Arc<dyn InodeOperations>,
    hook: Arc<H>,
}

impl<H: Hook> Wrap<H> {
    pub(crate) fn new(inner: Arc<dyn InodeOperations>, hook: Arc<H>) -> Self {
        Self { inner, hook }
    }
}

/// `ops`, handed the directory a `Wrap<H>` wraps.
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
            .downcast_ref::<Wrap<H>>()
            .map_or(dir, |wrap| wrap.inner.as_ref());
        self.hook.revalidate(self.ops.as_ref(), dir, dentry)
    }
}

impl<H: Hook> InodeOperations for Wrap<H> {
    fn permission(&self, dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
        self.hook.permission(self.inner.as_ref(), dentry, mask)
    }

    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        self.hook.getattr(self.inner.as_ref(), dentry)
    }

    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        Ok(match self.hook.lookup(self.inner.as_ref(), dentry)? {
            Lookup::Found(inode) => {
                let ops = inode.ops().clone();
                Lookup::Found(inode.with_ops(Arc::new(Self::new(ops, self.hook.clone()))))
            }
            other => other,
        })
    }

    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        self.inner.setattr(dentry, attr)
    }

    fn get_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.inner.get_link(dentry)
    }

    fn fill_super(&self, sb: &mut Filesystem) {
        self.inner.fill_super(sb);
        if let Some(ops) = sb.d_op().cloned() {
            sb.set_default_d_op(Arc::new(WrapDentry {
                ops,
                hook: self.hook.clone(),
            }));
        }
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        self.hook.open(self.inner.as_ref(), dentry, options)
    }

    fn atomic_open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        self.hook.atomic_open(self.inner.as_ref(), dentry, options)
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
