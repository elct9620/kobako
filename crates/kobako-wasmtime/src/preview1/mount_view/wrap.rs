//! A test's Filesystem over another: every call forwards to the entry it
//! wraps unless its `Hook` steps in, and every entry a lookup finds is
//! wrapped too, as a Filesystem wrapping another must.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use kobako_runtime::filesystem::{
    DentryOperations, FileOperations, FilesystemError, InodeOperations, Lookup, Metadata,
    OpenOptions, RenameFlag,
};
use kobako_runtime::vfs::Dentry;

/// What a test changes of the Filesystem beneath it.
pub(super) trait Hook: Send + Sync + 'static {
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

    fn dentry_ops(&self, inner: &dyn InodeOperations) -> Option<Arc<dyn DentryOperations>> {
        inner.dentry_ops()
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
pub(super) struct Wrap<H> {
    inner: Arc<dyn InodeOperations>,
    hook: Arc<H>,
}

impl<H: Hook> Wrap<H> {
    pub(super) fn new(inner: Arc<dyn InodeOperations>, hook: Arc<H>) -> Arc<Self> {
        Arc::new(Self { inner, hook })
    }
}

/// `ops`, handed the directory a `Wrap<H>` wraps.
struct WrapDentry<H> {
    ops: Arc<dyn DentryOperations>,
    hook: PhantomData<H>,
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
        self.ops.revalidate(dir, dentry)
    }
}

impl<H: Hook> InodeOperations for Wrap<H> {
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        self.hook.getattr(self.inner.as_ref(), dentry)
    }

    fn setattr(
        &self,
        dentry: &Dentry,
        attr: &kobako_runtime::filesystem::Iattr,
    ) -> Result<(), FilesystemError> {
        self.inner.setattr(dentry, attr)
    }

    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        Ok(match self.hook.lookup(self.inner.as_ref(), dentry)? {
            Lookup::Found(inode) => {
                let wrapped = Self::new(inode.ops().clone(), self.hook.clone());
                Lookup::Found(inode.with_ops(wrapped))
            }
            other => other,
        })
    }

    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        let ops = self.hook.dentry_ops(self.inner.as_ref())?;
        Some(Arc::new(WrapDentry::<H> {
            ops,
            hook: PhantomData,
        }))
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
