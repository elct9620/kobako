//! A test's Filesystem over another: every call forwards to the entry it
//! wraps unless its `Hook` steps in, and every entry a lookup finds is
//! wrapped too, as a Filesystem wrapping another must.

use std::any::Any;
use std::sync::Arc;

use kobako_runtime::filesystem::{
    DirEntry, FileOperations, FilesystemError, InodeOperations, Lookup, Metadata, OpenOptions,
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
    ) -> Option<Result<Lookup, FilesystemError>> {
        inner.lookup(dentry)
    }

    fn d_revalidate(
        &self,
        inner: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Option<Result<bool, FilesystemError>> {
        inner.d_revalidate(dentry)
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
pub(super) struct Wrap<H> {
    inner: Arc<dyn InodeOperations>,
    hook: Arc<H>,
}

impl<H: Hook> Wrap<H> {
    pub(super) fn new(inner: Arc<dyn InodeOperations>, hook: Arc<H>) -> Arc<Self> {
        Arc::new(Self { inner, hook })
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
                let wrapped = Self::new(inode.ops().clone(), self.hook.clone());
                Lookup::Found(inode.with_ops(wrapped))
            }
            other => other,
        }))
    }

    fn d_revalidate(&self, dentry: &Dentry) -> Option<Result<bool, FilesystemError>> {
        self.hook.d_revalidate(self.inner.as_ref(), dentry)
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

    fn entries(&self, dentry: &Dentry) -> Option<Result<Vec<DirEntry>, FilesystemError>> {
        self.inner.entries(dentry)
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
