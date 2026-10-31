//! An open file, as Linux's `struct file`: where it was opened, what answers
//! for it, and what each descriptor of it shares, so any ABI a Driver encodes
//! holds the same open file.

use std::ops::BitOr;
use std::sync::Arc;

use super::cred::Cred;
use super::dentry::Dentry;
use super::inode::Identity;
use super::mount::VfsMount;
use super::namei::d_getattr;
use super::openat2::{OpenHow, OpenOptions};
use super::path::Path;
use super::{Error, FileType, FilesystemError, Iattr, Metadata};

/// The largest offset a file holds, where Linux's signed `loff_t` ends.
const MAX_OFFSET: u64 = i64::MAX as u64;

/// Where one read or write lands, and where the file left off once it did,
/// as Linux's `struct kiocb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Kiocb {
    /// Where the call lands; the file moves it past what it took.
    pub pos: u64,
    /// Whether a write lands at the file's end as it stands then, as
    /// `IOCB_APPEND` asks.
    pub append: bool,
}

impl Kiocb {
    /// A call landing at `pos`.
    pub fn at(pos: u64) -> Self {
        Self { pos, append: false }
    }

    /// This call, landing at the file's end as it stands when the write
    /// lands.
    pub fn appending(self) -> Self {
        Self {
            append: true,
            ..self
        }
    }
}

/// One open file. The runtime keeps its position and hands each call where it
/// lands.
pub trait FileOperations: Send {
    /// Read into `buf` from where `iocb` lands, moving it past what was read
    /// unless the file has no positions. A file without it refuses as Linux's
    /// `vfs_read` refuses one without `f_op->read_iter`.
    fn read_iter(&mut self, _iocb: &mut Kiocb, _buf: &mut [u8]) -> Result<usize, FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// Write `buf` where `iocb` lands, moving it past what was written unless
    /// the file has no positions. A file without it refuses as Linux's
    /// `vfs_write` refuses one without `f_op->write_iter`.
    ///
    /// A call landing at the end must keep another writer's from landing over
    /// it, as `generic_file_write_iter` reads the end under the inode's lock,
    /// so a file that cannot read its end and write there in one step answers
    /// `Unsupported`.
    fn write_iter(&mut self, _iocb: &mut Kiocb, _buf: &[u8]) -> Result<usize, FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// The entries of this directory, which `dentry` names, without `.`
    /// or `..`, as `iterate_shared` lists the file it is handed. A file
    /// without it answers `NotADirectory`, as Linux's `iterate_dir` does.
    fn iterate_shared(&mut self, _dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        Err(FilesystemError::NotADirectory)
    }

    /// Reserve `len` bytes from `offset`, as `f_op->fallocate` does. A file
    /// without it refuses as Linux's `vfs_fallocate` refuses one without it.
    fn fallocate(&mut self, _offset: u64, _len: u64) -> Result<(), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// What a call would find ready on this end, opened for `mode`, as
    /// `f_op->poll` answers. A file without it is always ready, as Linux's
    /// `vfs_poll` answers `DEFAULT_POLLMASK`.
    fn poll(&self, _mode: FMode) -> PollMask {
        PollMask::IN | PollMask::OUT
    }

    /// Make what was written durable. A file with nothing to flush answers
    /// success itself, as tmpfs names `noop_fsync`; one without it refuses
    /// as Linux's `vfs_fsync_range` refuses one without `f_op->fsync`.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// Answer, as the guest closes the file, whether what it held was kept,
    /// as `f_op->flush` answers `close(2)`. The default has nothing to flush.
    /// What the file holds goes when it is dropped, as `->release` runs and
    /// no caller reads its answer, so a file the guest never closes still
    /// lets go of it.
    fn flush(&mut self) -> Result<(), FilesystemError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
    /// The device and inode numbers naming the entry, as `Metadata` reports
    /// them and `dir_emit` hands `d_ino`. An `ino` of zero reports none.
    pub dev: u64,
    pub ino: u64,
}

impl DirEntry {
    /// The entry `name` in its directory, naming a `file_type`.
    pub fn new(name: impl Into<String>, file_type: FileType) -> Self {
        Self {
            name: name.into(),
            file_type,
            dev: 0,
            ino: 0,
        }
    }

    /// This entry naming `dev` and `ino`, as a stat of it reports them.
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {
        Self { dev, ino, ..self }
    }
}

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

/// One entry a listing hands its actor, as `dir_emit` does.
#[non_exhaustive]
pub struct DirEmit<'a> {
    pub name: &'a str,
    pub file_type: FileType,
    /// What the entry is, `None` where its Filesystem names no inode number.
    pub identity: Option<Identity>,
    /// The position a listing going on from just after this entry starts at.
    pub next: u64,
}

/// A listing under way, as Linux's `struct dir_context`: where it stands and
/// who takes each entry, answering whether to go on.
pub struct DirContext<'a> {
    pos: u64,
    actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool,
}

impl<'a> DirContext<'a> {
    /// A listing from `pos`, each entry going to `actor`.
    pub fn new(pos: u64, actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool) -> Self {
        Self { pos, actor }
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
    /// What it was opened for, as `f_mode`.
    mode: FMode,
    /// Whether each write lands at the end, as `O_APPEND` in `f_flags`.
    append: bool,
    /// Whether it asked not to wait, as `O_NONBLOCK` in `f_flags`. Nothing
    /// here waits, so it only reports back.
    nonblock: bool,
    /// Whether each write is made durable before it answers, as `O_SYNC` in
    /// `f_flags`.
    sync: bool,
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
            mode: FMode::of(how),
            append: how.append,
            nonblock: how.nonblock,
            sync: how.sync,
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
        let how = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        Ok(Self::new(path, cred, ops, &how))
    }

    /// This directory keeping the flags it was opened with, as `f_flags` does
    /// whatever the file is.
    pub(super) fn with_flags(mut self, how: &OpenHow) -> Self {
        self.append = how.append;
        self.nonblock = how.nonblock;
        self.sync = how.sync;
        self
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

    /// Where a walk relative to it starts, as `path_init` refuses to start
    /// one anywhere but at a directory.
    pub fn path_init(&self) -> Result<&Path, FilesystemError> {
        match self.is_directory() {
            true => Ok(&self.path),
            false => Err(FilesystemError::NotADirectory),
        }
    }

    /// Whether it is a directory, as `S_ISDIR(file_inode(file))`.
    pub fn is_directory(&self) -> bool {
        self.path
            .dentry()
            .inode()
            .is_some_and(|inode| inode.kind() == FileType::Directory)
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

    /// Whether each write is made durable before it answers.
    pub fn sync(&self) -> bool {
        self.sync
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

    /// Refuse a directory's read, as every directory's `f_op->read` is
    /// `generic_read_dir`.
    fn generic_read_dir(&self) -> Result<(), FilesystemError> {
        match self.is_directory() {
            true => Err(FilesystemError::IsADirectory),
            false => Ok(()),
        }
    }

    /// Read from its position, moving it past what was read, as `vfs_read`
    /// does.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.may_read()?;
        self.rw_verify_area(self.pos, buf.len())?;
        self.generic_read_dir()?;
        let mut iocb = self.kiocb(self.pos);
        let read = self.ops.read_iter(&mut iocb, buf)?;
        self.pos = iocb.pos;
        Ok(read)
    }

    /// Read from `pos`, leaving its position, as `pread` does.
    pub fn pread(&mut self, buf: &mut [u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pread(pos)?;
        self.rw_verify_area(pos, buf.len())?;
        self.generic_read_dir()?;
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
        self.generic_write_sync(written)
    }

    /// Make `written` bytes durable where it was opened to, as
    /// `generic_write_sync` does once a write has landed.
    fn generic_write_sync(&mut self, written: usize) -> Result<usize, FilesystemError> {
        if self.sync && written > 0 {
            self.ops.fsync()?;
        }
        Ok(written)
    }

    /// Write at `pos`, leaving its position, as `pwrite` does. Opened to
    /// append, it lands at the end whatever `pos` says.
    pub fn pwrite(&mut self, buf: &[u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pwrite(pos)?;
        self.rw_verify_area(pos, buf.len())?;
        let written = self.ops.write_iter(&mut self.kiocb(pos), buf)?;
        self.generic_write_sync(written)
    }

    /// Make what was written durable, as `vfs_fsync` does.
    pub fn fsync(&mut self) -> Result<(), FilesystemError> {
        self.ops.fsync()
    }

    /// What it reports of itself, as `vfs_getattr` answers.
    pub fn getattr(&self) -> Result<Metadata, Error> {
        match self.is_directory() {
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
        if self.is_directory() || !self.writable() || len > MAX_OFFSET {
            return Err(FilesystemError::InvalidInput);
        }
        let dentry = self.path.dentry();
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        inode
            .ops()
            .setattr(dentry, &Iattr::size(len))
            .unwrap_or(Err(FilesystemError::InvalidInput))
    }

    /// Hand `ctx` the entries of the directory from its position, `.` and `..`
    /// first as `dir_emit_dots` emits them, until its actor stops, moving it
    /// past each. A listing from the start reads the directory; one going on
    /// reads the entries that listing began with, so a caller reading in small
    /// steps costs one listing, not one per step. A removed directory answers
    /// not found, as `iterate_dir` asks `IS_DEADDIR`.
    pub fn iterate_dir(&mut self, ctx: &mut DirContext<'_>) -> Result<(), Error> {
        let listing = match self.listing.take() {
            Some(listing) if ctx.pos != 0 && !self.path.dentry().is_deaddir() => listing,
            _ => self.read_entries()?,
        };
        let stopped = {
            let dots = match ctx.pos < 2 {
                true => self.dot_identities(),
                false => (None, None),
            };
            let entries = [
                (".", FileType::Directory, dots.0),
                ("..", FileType::Directory, dots.1),
            ]
            .into_iter()
            .chain(listing.iter().map(|entry| {
                (
                    entry.name.as_str(),
                    entry.file_type,
                    Identity::named(entry.dev, entry.ino),
                )
            }));
            let skip = usize::try_from(ctx.pos).unwrap_or(usize::MAX);
            let mut stopped = false;
            for (index, (name, file_type, identity)) in entries.enumerate().skip(skip) {
                let next = index as u64 + 1;
                let emit = DirEmit {
                    name,
                    file_type,
                    identity,
                    next,
                };
                if !(ctx.actor)(&emit) {
                    stopped = true;
                    break;
                }
                ctx.pos = next;
            }
            stopped
        };
        if stopped {
            self.listing = Some(listing);
        }
        Ok(())
    }

    /// What `.` and `..` name, as `dir_emit_dot` reads the directory's inode
    /// and `dir_emit_dotdot` its parent's, the root its own. Asked of a
    /// Filesystem where the root records none, so one that cannot answer names
    /// none rather than failing the listing.
    fn dot_identities(&self) -> (Option<Identity>, Option<Identity>) {
        let identity = |dentry: &Dentry| {
            d_getattr(dentry)
                .ok()
                .and_then(|metadata| Identity::of(&metadata))
        };
        let dentry = self.path.dentry();
        let itself = identity(dentry);
        let parent = match dentry.parent() {
            Some(parent) => identity(&parent),
            None => itself,
        };
        (itself, parent)
    }

    /// The entries of the directory, under its lock shared as `iterate_dir`
    /// holds it, so a removal through the VFS lands before or after them.
    fn read_entries(&mut self) -> Result<Vec<DirEntry>, Error> {
        self.cred.security_path_walk(&self.path)?;
        let dentry = self.path.dentry().clone();
        let _held = dentry.inode_lock_shared()?;
        if dentry.is_deaddir() {
            return Err(FilesystemError::NotFound.into());
        }
        Ok(self.ops.iterate_shared(&dentry)?)
    }

    /// What its Filesystem lists, with nothing the VFS adds, for a test of
    /// what the Filesystem answers beneath the VFS.
    #[cfg(test)]
    pub(crate) fn iterate_shared(&mut self) -> Result<Vec<DirEntry>, FilesystemError> {
        let dentry = self.path.dentry().clone();
        self.ops.iterate_shared(&dentry)
    }

    /// Close it once the last descriptor goes, answering what its flush
    /// answers, as `filp_close` does; dropping it is what `->release` does.
    pub fn close(mut self) -> Result<(), FilesystemError> {
        self.ops.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{create_pipe_files, do_file_open, vfs_fstatat, PipeInodeInfo};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::filesystem::{Filesystem, InodeOperations, Lookup, Overlay, Tmpfs};
    use crate::mount::{Access, Mount};
    use crate::vfs::Inode;

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
        do_file_open(&Arc::default(), &Path::root(mount.mnt()), "file.txt", &how)
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

    /// What a listing of the root from `pos` hands an actor taking `room`
    /// entries, as the name and the position after each.
    fn list(file: &mut File, pos: u64, room: usize) -> Vec<(String, u64)> {
        let mut seen = Vec::new();
        let mut actor = |emit: &DirEmit<'_>| {
            seen.push((emit.name.to_string(), emit.next));
            seen.len() < room
        };
        file.iterate_dir(&mut DirContext::new(pos, &mut actor))
            .unwrap_or_else(|_| panic!("the directory must list"));
        seen
    }

    /// Each entry of the root of `filesystem` with what a listing names it
    /// and what a stat of it names it.
    fn identities<F: InodeOperations>(
        filesystem: F,
    ) -> Vec<(String, Option<Identity>, Option<Identity>)> {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(filesystem)),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let start = Path::root(mount.mnt());
        let mut root = File::root(mount.mnt(), Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let mut listed = Vec::new();
        root.iterate_dir(&mut DirContext::new(2, &mut |emit| {
            listed.push((emit.name.to_string(), emit.identity));
            true
        }))
        .unwrap_or_else(|_| panic!("the root must list"));
        listed
            .into_iter()
            .map(|(name, identity)| {
                let stat = vfs_fstatat(&Arc::default(), &start, &name, false)
                    .unwrap_or_else(|_| panic!("{name} must stat"));
                (name, identity, Identity::of(&stat))
            })
            .collect()
    }

    fn assert_named_alike(label: &str, entries: Vec<(String, Option<Identity>, Option<Identity>)>) {
        assert!(!entries.is_empty(), "{label} must list something");
        for (name, listed, stat) in entries {
            assert!(
                listed.is_some() && listed == stat,
                "{name} of a {label} through File::iterate_dir must name what vfs_fstatat names, \
                 not {listed:?} beside {stat:?}"
            );
        }
    }

    // @behavior RT-183
    #[test]
    fn a_tmpfs_lists_each_entry_as_a_stat_names_it() {
        let tmpfs: Tmpfs = [("a", "x"), ("d/b", "y")].into_iter().collect();
        assert_named_alike("Tmpfs", identities(tmpfs));
    }

    // @behavior RT-183
    #[test]
    fn an_overlay_lists_each_entry_as_a_stat_names_it() {
        let lower: Tmpfs = [("a", "x"), ("d/b", "y")].into_iter().collect();
        let upper = Tmpfs::new();
        let overlay = Overlay::new(
            vec![Filesystem::new(Arc::new(lower))],
            Some(Filesystem::new(Arc::new(upper))),
        )
        .unwrap_or_else(|_| panic!("the Overlay must make"));
        assert_named_alike("Overlay", identities(overlay));
    }

    // A host names no identity through a Bind beyond unix.
    // @behavior RT-183
    #[cfg(unix)]
    #[test]
    fn a_bind_lists_each_entry_as_a_stat_names_it() {
        let dir = std::env::temp_dir().join(format!("kobako-listed-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("d")).expect("the host directory must make");
        std::fs::write(dir.join("a"), "x").expect("the host file must write");
        let bind =
            crate::filesystem::Bind::new(&dir).unwrap_or_else(|_| panic!("the Bind must make"));
        let entries = identities(bind);
        std::fs::remove_dir_all(&dir).expect("the host directory must go");
        assert_named_alike("Bind", entries);
    }

    // @behavior RT-182
    #[test]
    fn a_listing_hands_the_dots_first_and_goes_on_where_it_stopped() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("a", "x"), ("b", "y")].into_iter().collect());
        let mount = Mount::new("/data", Filesystem::new(tmpfs), Access::ReadWrite)
            .expect("/data is a guest path");
        let mut root = File::root(mount.mnt(), Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let entry = |name: &str, next| (name.to_string(), next);
        assert_eq!(
            (list(&mut root, 0, 3), list(&mut root, 2, 9)),
            (
                vec![entry(".", 1), entry("..", 2), entry("a", 3)],
                vec![entry("a", 3), entry("b", 4)],
            ),
            "File::iterate_dir must hand `.` and `..` ahead of the entries, each with the \
             position after it, and go on from any of those positions"
        );
    }

    /// A file counting its syncs, at every name of a directory.
    struct CountsSyncs(Arc<AtomicUsize>);

    impl FileOperations for CountsSyncs {
        fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
            Ok(buf.len())
        }

        fn fsync(&mut self) -> Result<(), FilesystemError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct HoldsCountsSyncs(Arc<AtomicUsize>);

    impl InodeOperations for HoldsCountsSyncs {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
            let found = Metadata::new(FileType::File, 0);
            let ops = Arc::new(Self(self.0.clone()));
            Some(Ok(Lookup::Found(Inode::new(&found, ops))))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            Some(Ok(Box::new(CountsSyncs(self.0.clone()))))
        }
    }

    // @behavior RT-091
    #[test]
    fn a_file_opened_to_sync_makes_each_write_durable() {
        let syncs = Arc::new(AtomicUsize::new(0));
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HoldsCountsSyncs(syncs.clone()))),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let open = |sync| {
            let how = OpenHow {
                write: true,
                sync,
                ..OpenHow::default()
            };
            do_file_open(&Arc::default(), &Path::root(mount.mnt()), "file", &how)
                .unwrap_or_else(|_| panic!("the file must open"))
        };
        let mut plain = open(false);
        let mut synced = open(true);
        plain.write(b"x").expect("the write must land");
        let after_plain = syncs.load(Ordering::SeqCst);
        synced.write(b"x").expect("the write must land");
        synced.pwrite(b"x", 0).expect("the write must land");
        synced.write(b"").expect("the empty write must land");
        assert_eq!(
            (after_plain, syncs.load(Ordering::SeqCst)),
            (0, 2),
            "File::write and pwrite must ask a file opened to sync to sync once per write landing \
             bytes, as generic_write_sync does, and a file opened otherwise never"
        );
    }

    // Linux's answer in a container: `F_GETFL` reports `O_NONBLOCK` and
    // `O_SYNC` on a directory opened with them.
    // @behavior RT-210
    #[test]
    fn a_directory_opened_with_flags_reports_them() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("dir/a", "x")].into_iter().collect());
        let mount = Mount::new("/data", Filesystem::new(tmpfs), Access::ReadWrite)
            .expect("/data is a guest path");
        let how = OpenHow {
            read: true,
            nonblock: true,
            sync: true,
            ..OpenHow::default()
        };
        let dir = do_file_open(&Arc::default(), &Path::root(mount.mnt()), "dir", &how)
            .unwrap_or_else(|_| panic!("the directory must open"));
        assert_eq!(
            (dir.nonblock(), dir.sync()),
            (true, true),
            "File::nonblock and File::sync on a directory do_file_open opened with both must \
             report them, as F_GETFL does"
        );
    }

    // @behavior RT-184
    #[test]
    fn a_directory_answers_as_a_directory_open_to_read_does() {
        let tmpfs: Arc<Tmpfs> = Arc::new([("a", "x")].into_iter().collect());
        let mount = Mount::new("/data", Filesystem::new(tmpfs), Access::ReadWrite)
            .expect("/data is a guest path");
        let mut dir = File::root(mount.mnt(), Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let mut buf = [0; 2];
        assert_eq!(
            (
                dir.read(&mut buf),
                dir.pread(&mut buf, 0),
                dir.write(b"xy"),
                dir.pwrite(b"xy", 0),
                dir.truncate(0),
                dir.fallocate(0, 1),
                dir.fsync(),
                dir.fadvise(0, 0),
            ),
            (
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::IsADirectory),
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::BadDescriptor),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::BadDescriptor),
                Ok(()),
                Ok(()),
            ),
            "File calls on a directory must answer as Linux answers one open to read: EISDIR to \
             read, EBADF to write or allocate, EINVAL to truncate, and success to sync or advise"
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
