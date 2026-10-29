//! A lower Layer's file open to read, which reads the copy once a copy up
//! makes one, as overlayfs's open file reaches the upper inode its copy
//! up made. overlayfs keeps that inode through its own inode, even once
//! removed; a Filesystem here hands out no inode of a file, so the copy is
//! opened as the copy up makes it and held on the file's entry instead.

use std::sync::{Arc, PoisonError};

use super::super::{File, FilesystemError, Metadata};
use super::inode::ovl_inode;
use super::inode::OverlayInode;
use crate::vfs::Dentry;

/// `file`, opened in a lower Layer for the entry `dentry` names, as one
/// that reads the copy once a copy up makes one, as overlayfs's `ovl_open`
/// hands out a file over the real one.
pub(super) fn reader(
    file: Box<dyn File>,
    dentry: &Dentry,
) -> Result<Box<dyn File>, FilesystemError> {
    let inode = ovl_inode(dentry, || Ok(Vec::new()))?;
    inode.opened_to_read();
    Ok(Box::new(OverlayFile {
        file,
        _dentry: dentry.dget(),
        inode,
    }))
}

/// A lower Layer's file open to read, as overlayfs's `struct ovl_file`.
struct OverlayFile {
    file: Box<dyn File>,
    /// The entry's dentry, held while the file is open, as a `struct
    /// file` holds its `f_path`, so a copy up of the entry finds the state
    /// this file reads.
    _dentry: Arc<Dentry>,
    inode: Arc<OverlayInode>,
}

impl File for OverlayFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        match self.inode.copy() {
            Some(copy) => copy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read_at(buf, offset),
            None => self.file.read_at(buf, offset),
        }
    }

    /// A file open to read takes no write, so the lower Layer's file
    /// answers it.
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.file.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        match self.inode.copy() {
            Some(copy) => copy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .metadata(),
            None => self.file.metadata(),
        }
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.set_len(len)
    }

    fn close(self: Box<Self>) -> Result<(), FilesystemError> {
        self.file.close()
    }
}
