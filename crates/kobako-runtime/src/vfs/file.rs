//! An open file, as Linux's `struct file`: where it was opened, what answers
//! for it, and what each descriptor of it shares, so any ABI a Driver encodes
//! holds the same open file.

use std::sync::Arc;

use super::cred::Cred;
use super::mount::VfsMount;
use super::namei::d_getattr;
use super::openat2::OpenHow;
use super::path::Path;
use super::Error;
use crate::filesystem::{
    DirEntry, FileOperations, FilesystemError, Iattr, Kiocb, Metadata, OpenOptions,
};

/// What an open file was opened for and what it can do, as Linux's `fmode_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FMode(u8);

impl FMode {
    /// Opened to read, as `FMODE_READ`.
    pub const READ: Self = Self(1);
    /// Opened to write, as `FMODE_WRITE`.
    pub const WRITE: Self = Self(1 << 1);
    /// Takes a position to seek, as `FMODE_LSEEK`.
    pub const LSEEK: Self = Self(1 << 2);
    /// Reads from a named offset, as `FMODE_PREAD`.
    pub const PREAD: Self = Self(1 << 3);
    /// Writes at a named offset, as `FMODE_PWRITE`.
    pub const PWRITE: Self = Self(1 << 4);
    /// Reads and writes what comes next, with no position, as `FMODE_STREAM`.
    pub const STREAM: Self = Self(1 << 5);

    fn of(how: &OpenHow) -> Self {
        let mut mode = Self::LSEEK.0 | Self::PREAD.0 | Self::PWRITE.0;
        if how.read {
            mode |= Self::READ.0;
        }
        if how.write {
            mode |= Self::WRITE.0;
        }
        Self(mode)
    }

    /// Whether this holds all of `mode`.
    pub fn contains(self, mode: Self) -> bool {
        self.0 & mode.0 == mode.0
    }
}

/// One open file or directory, as a descriptor refers to it.
pub struct File {
    /// Where it was opened, as `f_path`; it follows the name wherever it
    /// moves.
    path: Path,
    /// Who opened it, as `f_cred`.
    cred: Arc<Cred>,
    /// What answers for it, as `f_op`.
    ops: Box<dyn FileOperations>,
    directory: bool,
    /// What it was opened for, as `f_mode`.
    mode: FMode,
    /// Whether each write lands at the end, as `O_APPEND` in `f_flags`.
    append: bool,
    /// Where the next read or write lands, as `f_pos`.
    pos: u64,
    /// The entries a listing began with, held until it reaches its end, so
    /// each later step reads on without listing the directory again.
    listing: Option<Vec<DirEntry>>,
}

impl File {
    /// The file `ops` answers for, opened at `path` for `cred` as `how` asks.
    pub(super) fn new(
        path: Path,
        cred: Arc<Cred>,
        ops: Box<dyn FileOperations>,
        how: &OpenHow,
    ) -> Self {
        Self {
            path,
            cred,
            ops,
            directory: false,
            mode: FMode::of(how),
            append: how.append,
            pos: 0,
            listing: None,
        }
    }

    /// The directory at `path`, opened to be listed, as `dentry_open` calls
    /// `f_op->open`. A directory without `open` opens as Linux's opens with
    /// no `f_op->open`, and refuses a listing as one with no
    /// `iterate_shared`.
    pub(super) fn directory(path: Path, cred: Arc<Cred>) -> Result<Self, FilesystemError> {
        struct Unlistable;
        impl FileOperations for Unlistable {}

        let options = OpenOptions {
            read: true,
            directory: true,
            ..OpenOptions::default()
        };
        let ops = match path.dir().open(path.dentry(), &options) {
            Some(opened) => opened?,
            None => Box::new(Unlistable),
        };
        Ok(Self {
            directory: true,
            ..Self::new(path, cred, ops, &OpenHow::default())
        })
    }

    /// This file with no position, as `stream_open` makes a pipe's ends.
    pub(super) fn stream_open(&mut self) {
        let access = self.mode.0 & (FMode::READ.0 | FMode::WRITE.0);
        self.mode = FMode(access | FMode::STREAM.0);
    }

    /// The root of `mnt` opened as a directory for `cred`.
    pub fn root(mnt: &Arc<VfsMount>, cred: Arc<Cred>) -> Result<Self, FilesystemError> {
        Self::directory(Path::root(mnt), cred)
    }

    /// Where it was opened; a call relative to it starts here.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    /// What it was opened for.
    pub fn mode(&self) -> FMode {
        self.mode
    }

    /// Whether it was opened to read.
    pub fn readable(&self) -> bool {
        self.mode.contains(FMode::READ)
    }

    /// Whether it was opened to write.
    pub fn writable(&self) -> bool {
        self.mode.contains(FMode::WRITE)
    }

    /// Whether each write lands at the end.
    pub fn append(&self) -> bool {
        self.append
    }

    pub fn set_append(&mut self, append: bool) {
        self.append = append;
    }

    /// Where the next read or write lands.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    pub fn set_pos(&mut self, pos: u64) {
        self.pos = pos;
    }

    /// Whether it reads and writes what comes next, with no position.
    pub fn is_stream(&self) -> bool {
        self.mode.contains(FMode::STREAM)
    }

    /// A call landing at `pos`, at the end where it was opened to append, as
    /// `init_sync_kiocb` takes `IOCB_APPEND` from `f_flags`.
    fn kiocb(&self, pos: u64) -> Kiocb {
        match self.append {
            true => Kiocb::at(pos).appending(),
            false => Kiocb::at(pos),
        }
    }

    /// Read from its position, moving it past what was read, as `vfs_read`
    /// does.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut iocb = self.kiocb(self.pos);
        let read = self.ops.read_iter(&mut iocb, buf)?;
        self.pos = iocb.pos;
        Ok(read)
    }

    /// Read from `pos`, leaving its position, as `pread` does.
    pub fn pread(&mut self, buf: &mut [u8], pos: u64) -> Result<usize, FilesystemError> {
        self.ops.read_iter(&mut self.kiocb(pos), buf)
    }

    /// Write at its position, moving it past what was written, as `vfs_write`
    /// does.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut iocb = self.kiocb(self.pos);
        let written = self.ops.write_iter(&mut iocb, buf)?;
        self.pos = iocb.pos;
        Ok(written)
    }

    /// Write at `pos`, leaving its position, as `pwrite` does. Opened to
    /// append, it lands at the end whatever `pos` says.
    pub fn pwrite(&mut self, buf: &[u8], pos: u64) -> Result<usize, FilesystemError> {
        self.ops.write_iter(&mut self.kiocb(pos), buf)
    }

    /// Make what was written durable, as `vfs_fsync` does.
    pub fn fsync(&mut self) -> Result<(), FilesystemError> {
        self.ops.fsync()
    }

    /// What it reports of itself, as `vfs_getattr` answers.
    pub fn getattr(&self) -> Result<Metadata, Error> {
        match self.directory {
            true => {
                self.cred.security_path_walk(&self.path)?;
                self.path.metadata()
            }
            false => Ok(d_getattr(self.path.dentry())?),
        }
    }

    /// Truncate or extend it through what it was opened on, as `do_ftruncate`
    /// refuses anything but a file open to write, and reaches `->setattr`.
    pub fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {
        if self.directory || !self.writable() {
            return Err(FilesystemError::InvalidInput);
        }
        let dentry = self.path.dentry();
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        inode
            .ops()
            .setattr(dentry, &Iattr::size(len))
            .unwrap_or(Err(FilesystemError::InvalidInput))
    }

    /// The entries of the directory, without `.` or `..`; none once it was
    /// removed, as Linux lists a dead directory.
    pub fn iterate_dir(&mut self) -> Result<Vec<DirEntry>, Error> {
        self.cred.security_path_walk(&self.path)?;
        let dentry = self.path.dentry().clone();
        // A listing holds the directory shared, as Linux's `iterate_dir`
        // does, so a removal through the VFS lands before or after it.
        let _held = dentry.inode_lock_shared()?;
        if dentry.is_deaddir() {
            return Ok(Vec::new());
        }
        Ok(self.ops.iterate_shared(&dentry)?)
    }

    /// The entries a listing began with, once one is under way.
    pub fn take_listing(&mut self) -> Option<Vec<DirEntry>> {
        self.listing.take()
    }

    /// Hold the entries a listing began with for its next step.
    pub fn keep_listing(&mut self, listing: Vec<DirEntry>) {
        self.listing = Some(listing);
    }

    /// Close it once the last descriptor goes, answering what its flush
    /// answers, as `filp_close` does; dropping it is what `->release` does.
    pub fn close(mut self) -> Result<(), FilesystemError> {
        self.ops.flush()
    }
}
