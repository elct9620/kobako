//! The files a read-only Layer shows that are open to read, so a copy up
//! moves each of them to the copy, as overlayfs moves an open file to the
//! upper inode its copy up made.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::super::{File, Filesystem, FilesystemError, Metadata, OpenOptions};

type Copy = Arc<Mutex<Box<dyn File>>>;

/// The copy a file open to read shows once a copy up made one, or `None`
/// while the read-only Layer still shows it.
#[derive(Default)]
struct Shown(Mutex<Option<Copy>>);

impl Shown {
    fn copy(&self) -> Option<Copy> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Every path a read-only Layer's file is open to read at, kept only while
/// one is open.
#[derive(Default)]
pub(super) struct Readers(Mutex<HashMap<String, Weak<Shown>>>);

impl Readers {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Weak<Shown>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `file`, opened at `path` in a read-only Layer, as one that reads
    /// the copy once a copy up makes one.
    pub(super) fn reading(&self, path: &str, file: Box<dyn File>) -> Box<dyn File> {
        let mut paths = self.lock();
        paths.retain(|_, shown| shown.strong_count() > 0);
        let shown = match paths.get(path).and_then(Weak::upgrade) {
            Some(shown) => shown,
            None => {
                let shown = Arc::new(Shown::default());
                paths.insert(path.to_string(), Arc::downgrade(&shown));
                shown
            }
        };
        Box::new(Reading { file, shown })
    }

    /// `path` was copied up into `layer`; every file open to read there
    /// reads the copy from now on, wherever it moves.
    pub(super) fn copied(&self, path: &str, layer: &dyn Filesystem) -> Result<(), FilesystemError> {
        let Some(shown) = self.lock().get(path).and_then(Weak::upgrade) else {
            return Ok(());
        };
        let copy = layer.open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        *shown.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(Mutex::new(copy)));
        Ok(())
    }
}

/// A read-only Layer's file open to read, which reads the copy once a copy
/// up makes one.
struct Reading {
    file: Box<dyn File>,
    shown: Arc<Shown>,
}

impl File for Reading {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        match self.shown.copy() {
            Some(shared) => shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read_at(buf, offset),
            None => self.file.read_at(buf, offset),
        }
    }

    /// A file open to read takes no write, so the read-only Layer's file
    /// answers it.
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.file.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        match self.shown.copy() {
            Some(shared) => shared
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
