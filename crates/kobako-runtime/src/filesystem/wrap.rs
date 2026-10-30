//! A test's Filesystem over another: every call forwards to the entry it
//! wraps unless its `Hook` steps in, and every entry a lookup finds is
//! wrapped too, as a Filesystem wrapping another must.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use super::{
    DentryOperations, FileOperations, FilesystemError, InodeOperations, Lookup, Metadata,
    OpenOptions,
};
use crate::vfs::Dentry;

/// What a test changes of the Filesystem beneath it.
pub(crate) trait Hook: Send + Sync + 'static {
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
    ) -> Option<Result<Lookup, FilesystemError>> {
        inner.lookup(dentry)
    }

    fn open(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        inner.open(dentry, options)
    }

    fn create(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        inner.create(dentry, options)
    }

    fn mkdir(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        inner.mkdir(dentry)
    }

    fn rmdir(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        inner.rmdir(dentry)
    }

    fn unlink(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        inner.unlink(dentry)
    }

    fn rename(
        &self,
        inner: &dyn InodeOperations,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        inner.rename(from, to_dir, to)
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

    fn lookup(&self, dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
        let found = self.hook.lookup(self.inner.as_ref(), dentry)?;
        Some(found.map(|lookup| match lookup {
            Lookup::Found(inode) => {
                let ops = inode.ops().clone();
                Lookup::Found(inode.with_ops(Arc::new(Self::new(ops, self.hook.clone()))))
            }
            other => other,
        }))
    }

    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        let ops = self.inner.dentry_ops()?;
        Some(Arc::new(WrapDentry::<H> {
            ops,
            hook: PhantomData,
        }))
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        self.hook.open(self.inner.as_ref(), dentry, options)
    }

    fn create(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        self.hook.create(self.inner.as_ref(), dentry, options)
    }

    fn mkdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        self.hook.mkdir(self.inner.as_ref(), dentry)
    }

    fn rmdir(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        self.hook.rmdir(self.inner.as_ref(), dentry)
    }

    fn unlink(&self, dentry: &Dentry) -> Option<Result<(), FilesystemError>> {
        self.hook.unlink(self.inner.as_ref(), dentry)
    }

    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
    ) -> Option<Result<(), FilesystemError>> {
        let target = (to_dir as &dyn Any).downcast_ref::<Self>()?;
        self.hook
            .rename(self.inner.as_ref(), from, target.inner.as_ref(), to)
    }
}
