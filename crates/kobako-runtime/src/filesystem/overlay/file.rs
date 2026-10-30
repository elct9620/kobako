//! A lower Layer's file open to read, which reads the copy once a copy up
//! makes one, as overlayfs's open file reaches the upper inode its copy
//! up made. overlayfs keeps that inode through its own inode, even once
//! removed; a Layer's file is reached by name here, so the copy is opened as
//! the copy up makes it and held on the file's own inode instead.

use std::sync::{Arc, PoisonError};

use super::super::{FileOperations, FilesystemError, Metadata};
use super::inode::{ovl_inode, ReaderGuard};
use crate::vfs::Dentry;

/// `file`, opened in a lower Layer for the entry `dentry` names, as one
/// that reads the copy once a copy up makes one, as overlayfs's `ovl_open`
/// hands out a file over the real one.
pub(super) fn reader(
    file: Box<dyn FileOperations>,
    dentry: &Dentry,
) -> Result<Box<dyn FileOperations>, FilesystemError> {
    let inode = ovl_inode(dentry, || Ok(Vec::new()))?;
    Ok(Box::new(OverlayFile {
        file,
        _dentry: dentry.get(),
        inode: inode.hold_reader(),
    }))
}

/// A lower Layer's file open to read, as overlayfs's `struct ovl_file`.
struct OverlayFile {
    file: Box<dyn FileOperations>,
    /// The entry's dentry, held while the file is open, as a `struct
    /// file` holds its `f_path`, so a copy up of the entry finds the state
    /// this file reads.
    _dentry: Arc<Dentry>,
    /// The entry this file reads, counting it as a reader while it is open.
    inode: ReaderGuard,
}

impl FileOperations for OverlayFile {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        match self.inode.copy() {
            Some(copy) => copy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read(buf, offset),
            None => self.file.read(buf, offset),
        }
    }

    /// A file open to read takes no write, so the lower Layer's file
    /// answers it.
    fn write(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.write(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.file.append(buf)
    }

    fn getattr(&self) -> Result<Metadata, FilesystemError> {
        match self.inode.copy() {
            Some(copy) => copy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .getattr(),
            None => self.file.getattr(),
        }
    }

    fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.truncate(len)
    }

    /// A file open to read flushes what it reads, as `ovl_fsync` syncs the
    /// real file.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        match self.inode.copy() {
            Some(copy) => copy.lock().unwrap_or_else(PoisonError::into_inner).fsync(),
            None => self.file.fsync(),
        }
    }

    fn flush(&mut self) -> Result<(), FilesystemError> {
        self.file.flush()
    }
}
