//! A lower Layer's file open to read, which reads the upper's once a copy up
//! makes one, as overlayfs's `ovl_real_file` looks where the data is at each
//! call and opens the upper file the first time it finds it there.

use std::sync::Arc;

use super::super::{FileOperations, FilesystemError, Kiocb, OpenHow};
use super::inode::OverlayFileInode;
use crate::vfs::{generic_file_llseek, Dentry, Whence};

/// `file`, opened on `real` for the entry `inode` keeps, as `ovl_open` hands out
/// a file over the real one.
pub(super) fn ovl_file(
    file: Box<dyn FileOperations>,
    real: Arc<Dentry>,
    inode: Arc<OverlayFileInode>,
) -> Box<dyn FileOperations> {
    Box::new(OverlayFile {
        file,
        real,
        inode,
        upper: None,
    })
}

/// A lower Layer's file open to read, as overlayfs's `struct ovl_file`.
struct OverlayFile {
    file: Box<dyn FileOperations>,
    /// The Layer's dentry `file` was opened on.
    real: Arc<Dentry>,
    /// The entry this file reads, which says where its data is now.
    inode: Arc<OverlayFileInode>,
    /// The upper's file, once a copy up made one and a call found it.
    upper: Option<Box<dyn FileOperations>>,
}

impl OverlayFile {
    /// The file holding the data now, as `ovl_real_file_path` finds it.
    fn real_file(&mut self) -> Result<&mut dyn FileOperations, FilesystemError> {
        if self.upper.is_none() {
            let real = self.inode.real();
            if !Arc::ptr_eq(&real, &self.real) {
                let read = OpenHow {
                    read: true,
                    ..OpenHow::default()
                };
                self.upper = Some(real.open(&read)?);
            }
        }
        Ok(match &mut self.upper {
            Some(upper) => upper.as_mut(),
            None => self.file.as_mut(),
        })
    }
}

impl FileOperations for OverlayFile {
    /// The view's size answers an end, which is the real file's, as
    /// `ovl_llseek` seeks the real file.
    fn llseek(
        &mut self,
        dentry: &Dentry,
        pos: u64,
        offset: i64,
        whence: Whence,
    ) -> Result<u64, FilesystemError> {
        generic_file_llseek(dentry, pos, offset, whence)
    }

    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.real_file()?.read_iter(iocb, buf)
    }

    /// A file open to read flushes what it reads, as `ovl_fsync` syncs the
    /// real file.
    fn fsync(&mut self, datasync: bool) -> Result<(), FilesystemError> {
        self.real_file()?.fsync(datasync)
    }

    fn flush(&mut self) -> Result<(), FilesystemError> {
        self.file.flush()
    }

    /// A stream stays one through the view, as overlayfs hands a special file's
    /// open to its real inode.
    fn is_stream(&self) -> bool {
        self.file.is_stream()
    }
}
