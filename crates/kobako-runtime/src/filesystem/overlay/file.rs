//! The files a lower Layer shows that are open to read, so a copy up
//! moves each of them to the copy, as overlayfs's open file reaches the
//! upper inode its copy up made. overlayfs keeps that inode through its
//! own inode, even once removed; a Filesystem here hands out no inode of a
//! file, so the copy is opened as the copy up makes it and held instead.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::{File, FilesystemError, Metadata, OpenOptions};
use super::path::OverlayPath;
use crate::hashtable::Hashtable;

type Copy = Arc<Mutex<Box<dyn File>>>;

/// The copy a file open to read shows once a copy up made one, or `None`
/// while the lower Layer still shows it.
#[derive(Default)]
struct Upper(Mutex<Option<Copy>>);

impl Upper {
    fn copy(&self) -> Option<Copy> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Every path a lower Layer's file is open to read at, kept only while
/// one is open.
#[derive(Default)]
pub(super) struct Readers(Mutex<Hashtable<String, Upper>>);

impl Readers {
    fn lock(&self) -> MutexGuard<'_, Hashtable<String, Upper>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `file`, opened at `path` in a lower Layer, as one that reads
    /// the copy once a copy up makes one, as overlayfs's `ovl_open` hands
    /// out a file over the real one.
    pub(super) fn open(&self, path: &str, file: Box<dyn File>) -> Box<dyn File> {
        let upper = self
            .lock()
            .get_or_insert_with(path.to_string(), Arc::default);
        Box::new(OverlayFile { file, upper })
    }

    /// `path` was copied up as `name` into `dir`, the upper's directory, as overlayfs's `ovl_inode_update` records the upper
    /// inode; every file open to read there reads the copy from now on,
    /// wherever it moves.
    pub(super) fn inode_update(
        &self,
        path: &str,
        dir: &OverlayPath,
        name: &str,
    ) -> Result<(), FilesystemError> {
        let Some(upper) = self.lock().get(&path.to_string()) else {
            return Ok(());
        };
        let copy = dir.open(
            name,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        *upper.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(Mutex::new(copy)));
        Ok(())
    }
}

/// A lower Layer's file open to read, which reads the copy once a copy
/// up makes one, as overlayfs's `struct ovl_file` does.
struct OverlayFile {
    file: Box<dyn File>,
    upper: Arc<Upper>,
}

impl File for OverlayFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        match self.upper.copy() {
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
        match self.upper.copy() {
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
