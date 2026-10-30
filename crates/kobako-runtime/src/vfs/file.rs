//! An open file, as Linux's `struct file`: where it was opened, what answers
//! for it, and what each descriptor of it shares, so any ABI a Driver encodes
//! holds the same open file.

use std::ops::BitOr;
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

/// The largest offset a file holds, where Linux's signed `loff_t` ends.
const MAX_OFFSET: u64 = i64::MAX as u64;

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

/// What a file would find ready, as Linux's `__poll_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollMask(u8);

impl PollMask {
    /// A read would not wait, as `EPOLLIN`.
    pub const IN: Self = Self(1);
    /// A write would not wait, as `EPOLLOUT`.
    pub const OUT: Self = Self(1 << 1);

    /// Whether this holds all of `mask`.
    pub fn contains(self, mask: Self) -> bool {
        self.0 & mask.0 == mask.0
    }
}

impl BitOr for PollMask {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Where a seek counts from, as Linux's `SEEK_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whence {
    Set,
    Cur,
    End,
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
    /// Whether it asked not to wait, as `O_NONBLOCK` in `f_flags`. Nothing
    /// here waits, so it only reports back.
    nonblock: bool,
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
            nonblock: how.nonblock,
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
    #[cfg(test)]
    pub(crate) fn mode(&self) -> FMode {
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

    /// Whether it asked not to wait.
    pub fn nonblock(&self) -> bool {
        self.nonblock
    }

    /// Change the flags an open file may change, as `fcntl`'s `setfl` takes
    /// only `SETFL_MASK`.
    pub fn setfl(&mut self, append: bool, nonblock: bool) {
        self.append = append;
        self.nonblock = nonblock;
    }

    /// Move its position, answering where it now stands, as `vfs_llseek` and
    /// `generic_file_llseek_size` do.
    pub fn llseek(&mut self, offset: i64, whence: Whence) -> Result<u64, Error> {
        if !self.mode.contains(FMode::LSEEK) {
            return Err(FilesystemError::NotSeekable.into());
        }
        let base = match whence {
            Whence::Set => 0,
            Whence::Cur => self.pos,
            Whence::End => self.getattr()?.len,
        };
        let pos = base
            .checked_add_signed(offset)
            .filter(|pos| *pos <= MAX_OFFSET)
            .ok_or(FilesystemError::InvalidInput)?;
        self.pos = pos;
        Ok(pos)
    }

    /// Take a hint about `len` bytes, as `generic_fadvise` refuses a pipe and
    /// a negative length and ignores the rest. Every Filesystem is still
    /// correct without one.
    pub fn fadvise(&self, _offset: u64, len: u64) -> Result<(), FilesystemError> {
        if self.is_stream() {
            return Err(FilesystemError::NotSeekable);
        }
        match len > MAX_OFFSET {
            true => Err(FilesystemError::InvalidInput),
            false => Ok(()),
        }
    }

    /// Reserve `len` bytes from `offset`, as `vfs_fallocate` refuses in this
    /// order before it asks `f_op->fallocate`.
    pub fn fallocate(&mut self, offset: u64, len: u64) -> Result<(), FilesystemError> {
        if offset > MAX_OFFSET || len == 0 || len > MAX_OFFSET {
            return Err(FilesystemError::InvalidInput);
        }
        self.may_write()?;
        if self.is_stream() {
            return Err(FilesystemError::NotSeekable);
        }
        if offset + len > MAX_OFFSET {
            return Err(FilesystemError::FileTooLarge);
        }
        self.ops.fallocate(offset, len)
    }

    /// What a call would find ready, as `vfs_poll` asks `f_op->poll`.
    pub fn poll(&self) -> PollMask {
        self.ops.poll(self.mode)
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

    /// Whether a read may reach it, as `vfs_read` asks of `f_mode` before it
    /// touches the caller's buffer.
    pub fn may_read(&self) -> Result<(), FilesystemError> {
        match self.readable() {
            true => Ok(()),
            false => Err(FilesystemError::BadDescriptor),
        }
    }

    /// Whether a write may reach it, as `vfs_write` asks of `f_mode` before
    /// it touches the caller's buffer.
    pub fn may_write(&self) -> Result<(), FilesystemError> {
        match self.writable() {
            true => Ok(()),
            false => Err(FilesystemError::BadDescriptor),
        }
    }

    /// Whether a read from `pos` may reach it, as `pread` refuses a negative
    /// offset, then a file without one.
    pub fn may_pread(&self, pos: u64) -> Result<(), FilesystemError> {
        self.may_at(pos, FMode::PREAD)?;
        self.may_read()
    }

    /// Whether a write at `pos` may reach it, as `pwrite` refuses a negative
    /// offset, then a file without one.
    pub fn may_pwrite(&self, pos: u64) -> Result<(), FilesystemError> {
        self.may_at(pos, FMode::PWRITE)?;
        self.may_write()
    }

    fn may_at(&self, pos: u64, mode: FMode) -> Result<(), FilesystemError> {
        if pos > MAX_OFFSET {
            return Err(FilesystemError::InvalidInput);
        }
        match self.mode.contains(mode) {
            true => Ok(()),
            false => Err(FilesystemError::NotSeekable),
        }
    }

    /// Refuse a call of `count` bytes from where it lands if it would end past
    /// the largest offset, as `rw_verify_area` does; a stream lands nowhere.
    fn rw_verify_area(&self, pos: u64, count: usize) -> Result<(), FilesystemError> {
        if self.is_stream() {
            return Ok(());
        }
        match pos.checked_add(count as u64) {
            Some(end) if end <= MAX_OFFSET => Ok(()),
            _ => Err(FilesystemError::InvalidInput),
        }
    }

    /// Read from its position, moving it past what was read, as `vfs_read`
    /// does.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.may_read()?;
        self.rw_verify_area(self.pos, buf.len())?;
        let mut iocb = self.kiocb(self.pos);
        let read = self.ops.read_iter(&mut iocb, buf)?;
        self.pos = iocb.pos;
        Ok(read)
    }

    /// Read from `pos`, leaving its position, as `pread` does.
    pub fn pread(&mut self, buf: &mut [u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pread(pos)?;
        self.rw_verify_area(pos, buf.len())?;
        self.ops.read_iter(&mut self.kiocb(pos), buf)
    }

    /// Write at its position, moving it past what was written, as `vfs_write`
    /// does.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.may_write()?;
        self.rw_verify_area(self.pos, buf.len())?;
        let mut iocb = self.kiocb(self.pos);
        let written = self.ops.write_iter(&mut iocb, buf)?;
        self.pos = iocb.pos;
        Ok(written)
    }

    /// Write at `pos`, leaving its position, as `pwrite` does. Opened to
    /// append, it lands at the end whatever `pos` says.
    pub fn pwrite(&mut self, buf: &[u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pwrite(pos)?;
        self.rw_verify_area(pos, buf.len())?;
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
        if self.directory || !self.writable() || len > MAX_OFFSET {
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

#[cfg(test)]
mod tests {
    use super::super::{create_pipe_files, do_file_open, PipeInodeInfo};
    use super::*;
    use crate::filesystem::Tmpfs;
    use crate::mount::{Access, Mount};
    use crate::vfs::Filesystem;

    /// `file.txt`, seven bytes long, opened for `access`.
    fn open(read: bool, write: bool) -> File {
        let tmpfs = Tmpfs::try_from_iter([("file.txt", "granted")]).expect("the seed must fit");
        let mount = Mount::new("/data", Filesystem::new(Arc::new(tmpfs)), Access::ReadWrite)
            .expect("/data is a guest path");
        let how = OpenHow {
            read,
            write,
            ..OpenHow::default()
        };
        do_file_open(
            &Arc::default(),
            &Path::root(mount.mnt()),
            "file.txt",
            &how,
            true,
        )
        .unwrap_or_else(|_| panic!("file.txt must open"))
    }

    // No Driver stands in front of these calls, so the file itself is what
    // refuses each one before its Filesystem is asked.
    // @behavior RT-180
    #[test]
    fn an_open_file_answers_for_what_it_was_opened_for() {
        let mut buf = [0; 2];
        assert_eq!(
            (
                open(true, false).write(b"xy"),
                open(true, false).pwrite(b"xy", 0),
                open(false, true).read(&mut buf),
                open(false, true).pread(&mut buf, 0),
            ),
            (
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::BadDescriptor),
            ),
            "File::write and pwrite on a file opened to read, and File::read and pread on one \
             opened to write, must answer EBADF, as vfs_read and vfs_write do"
        );
    }

    // @behavior RT-181
    #[test]
    fn an_allocation_refuses_in_linuxs_order() {
        let (_, mut pipe) = create_pipe_files(&PipeInodeInfo::new(1));
        assert_eq!(
            (
                open(true, false).fallocate(0, 0),
                open(true, false).fallocate(0, 1),
                open(false, true).fallocate(0, 1),
                pipe.fallocate(0, 1),
            ),
            (
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::Unsupported),
                Err(FilesystemError::NotSeekable),
            ),
            "File::fallocate must answer an empty range EINVAL, a file not open to write EBADF, \
             a file without fallocate ENOTSUP, and a pipe ESPIPE, as vfs_fallocate does"
        );
    }

    // @behavior RT-180
    #[test]
    fn an_open_file_refuses_a_call_ending_past_the_largest_offset() {
        let mut buf = [0; 2];
        let mut file = open(true, true);
        file.llseek(MAX_OFFSET as i64 - 1, Whence::Set)
            .unwrap_or_else(|_| panic!("the largest offset must be one a file can stand at"));
        assert_eq!(
            (
                file.read(&mut buf),
                file.write(b"xy"),
                file.pread(&mut buf, MAX_OFFSET - 1),
                file.pwrite(b"xy", MAX_OFFSET - 1),
                file.pread(&mut buf, MAX_OFFSET + 1),
                file.pwrite(b"xy", MAX_OFFSET + 1),
                file.truncate(MAX_OFFSET + 1),
                file.pwrite(b"", MAX_OFFSET),
            ),
            (
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::InvalidInput),
                Ok(0),
            ),
            "File calls ending past the largest signed offset must answer EINVAL, as \
             rw_verify_area does, and an empty one ending at it must pass"
        );
    }
}
