//! A lower Layer's file open to read, which reads the upper's once a copy up
//! makes one, as overlayfs's `ovl_real_file` looks where the data is at each
//! call and opens the upper file the first time it finds it there.

use std::sync::Arc;

use super::super::{FileOperations, FilesystemError, Kiocb, OpenOptions};
use super::inode::OverlayInode;
use crate::vfs::Dentry;

/// `file`, opened on `real` for the entry `dentry` names and `inode` keeps, as
/// `ovl_open` hands out a file over the real one.
pub(super) fn ovl_file(
    file: Box<dyn FileOperations>,
    real: Arc<Dentry>,
    dentry: &Dentry,
    inode: Arc<OverlayInode>,
) -> Box<dyn FileOperations> {
    Box::new(OverlayFile {
        file,
        real,
        _dentry: dentry.get(),
        inode,
        upper: None,
    })
}

/// A lower Layer's file open to read, as overlayfs's `struct ovl_file`.
struct OverlayFile {
    file: Box<dyn FileOperations>,
    /// The Layer's dentry `file` was opened on.
    real: Arc<Dentry>,
    /// The entry's dentry, held while the file is open, as a `struct file`
    /// holds its `f_path`, so a copy up of the entry finds the state this file
    /// reads.
    _dentry: Arc<Dentry>,
    /// The entry this file reads, which says where its data is now.
    inode: Arc<OverlayInode>,
    /// The upper's file, once a copy up made one and a call found it.
    upper: Option<Box<dyn FileOperations>>,
}

impl OverlayFile {
    /// The file holding the data now, as `ovl_real_file_path` finds it.
    fn real_file(&mut self) -> Result<&mut dyn FileOperations, FilesystemError> {
        if self.upper.is_none() {
            match self.inode.real() {
                Some(real) if !Arc::ptr_eq(&real, &self.real) => {
                    let read = OpenOptions {
                        read: true,
                        ..OpenOptions::default()
                    };
                    self.upper = Some(real.open(&read)?);
                }
                _ => return Ok(self.file.as_mut()),
            }
        }
        match &mut self.upper {
            Some(upper) => Ok(upper.as_mut()),
            None => Ok(self.file.as_mut()),
        }
    }
}

impl FileOperations for OverlayFile {
    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.real_file()?.read_iter(iocb, buf)
    }

    /// A file open to read takes no write, so the lower Layer's file
    /// answers it.
    fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.file.write_iter(iocb, buf)
    }

    /// A file open to read flushes what it reads, as `ovl_fsync` syncs the
    /// real file.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        self.real_file()?.fsync()
    }

    fn flush(&mut self) -> Result<(), FilesystemError> {
        self.file.flush()
    }
}
