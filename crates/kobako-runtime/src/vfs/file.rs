//! An open file, as Linux's `struct file`: where it was opened, what answers
//! for it, and what each descriptor of it shares, so any ABI above holds the
//! same open file.

use std::ops::BitOr;
use std::sync::Arc;

use super::attr::notify_change;
use super::cred::{Cred, FileSecurity};
use super::dentry::Dentry;
use super::inode::Identity;
use super::mount::VfsMount;
use super::openat2::OpenHow;
use super::path::Path;
use super::{FileType, FilesystemError, Iattr, Metadata, PATH_MAX};

/// The largest offset a file holds, where Linux's signed `loff_t` ends.
const MAX_OFFSET: u64 = i64::MAX as u64;

/// The most one read or write takes, as Linux's `MAX_RW_COUNT`, the largest
/// signed 32-bit count rounded down to a page.
const MAX_RW_COUNT: usize = i32::MAX as usize & !4095;

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
    /// Whether what a write needs read back is durable before it answers, as
    /// `IOCB_DSYNC` asks.
    pub dsync: bool,
    /// Whether everything a write changed is durable before it answers, as
    /// `IOCB_SYNC` asks along with `IOCB_DSYNC`.
    pub sync: bool,
}

impl Kiocb {
    /// A call landing at `pos`.
    pub fn at(pos: u64) -> Self {
        Self {
            pos,
            append: false,
            dsync: false,
            sync: false,
        }
    }

    /// This call, landing at the file's end as it stands when the write
    /// lands.
    pub fn with_append(self) -> Self {
        Self {
            append: true,
            ..self
        }
    }
}

/// Make what `written` bytes changed durable through `file` where `iocb` asks, as
/// Linux's `generic_write_sync` is called by a Filesystem's `write_iter` once a
/// write landed: data alone unless `IOCB_SYNC` asks for everything.
pub fn generic_write_sync<F: FileOperations + ?Sized>(
    file: &mut F,
    iocb: &Kiocb,
    written: usize,
) -> Result<usize, FilesystemError> {
    if iocb.dsync && written > 0 {
        file.fsync(!iocb.sync)?;
    }
    Ok(written)
}

/// One open file. The runtime keeps its position and hands each call where it
/// lands.
pub trait FileOperations: Send {
    /// Where a seek by `offset` from `whence` leaves this file, which `dentry`
    /// names and whose position stands at `pos`, as `f_op->llseek` answers. A file
    /// without it takes no seek, as Linux clears `FMODE_LSEEK` for one; one that
    /// seeks as any file does answers through `generic_file_llseek`.
    fn llseek(
        &mut self,
        _dentry: &Dentry,
        _pos: u64,
        _offset: i64,
        _whence: Whence,
    ) -> Result<u64, FilesystemError> {
        Err(FilesystemError::NotSeekable)
    }

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

    /// Hand `ctx` the entries of this directory, which `dentry` names, from
    /// `ctx`'s position until its actor stops, as `iterate_shared` does through
    /// `dir_emit_dots` and `dir_emit`. A file without it answers `NotADirectory`,
    /// as Linux's `iterate_dir` does.
    fn iterate_shared(
        &mut self,
        _dentry: &Dentry,
        _ctx: &mut DirContext<'_>,
    ) -> Result<(), FilesystemError> {
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

    /// Make what was written durable, only what reading it back needs where
    /// `datasync` asks, as `f_op->fsync` takes it. A file with nothing to flush
    /// answers success itself, as tmpfs names `noop_fsync`; one without it refuses
    /// as Linux's `vfs_fsync_range` refuses one without `f_op->fsync`.
    fn fsync(&mut self, _datasync: bool) -> Result<(), FilesystemError> {
        Err(FilesystemError::InvalidInput)
    }

    /// Answer, as the file is closed, whether what it held was kept, as
    /// `f_op->flush` answers `close(2)`. The default has nothing to flush.
    /// What the file holds goes when it is dropped, as `->release` runs and
    /// no caller reads its answer, so a file never closed still lets go of it.
    fn flush(&mut self) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether this file reads and writes what comes next, with no position,
    /// as an `->open` calling `stream_open` marks one. The default takes a
    /// position.
    fn is_stream(&self) -> bool {
        false
    }
}

/// Where a seek by `offset` from `whence` leaves a file `dentry` names, whose
/// position stands at `pos`, refused past the largest offset, as Linux's
/// `generic_file_llseek` counts from the position, or the end its inode reports.
pub fn generic_file_llseek(
    dentry: &Dentry,
    pos: u64,
    offset: i64,
    whence: Whence,
) -> Result<u64, FilesystemError> {
    let base = match whence {
        Whence::Set => 0,
        Whence::Cur => pos,
        Whence::End => dentry.getattr()?.len,
    };
    base.checked_add_signed(offset)
        .filter(|pos| *pos <= MAX_OFFSET)
        .ok_or(FilesystemError::InvalidInput)
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
    /// Made by the open that opened it, as `FMODE_CREATED`.
    pub const CREATED: Self = Self(1 << 6);

    /// What an open of `how` is for, as `do_dentry_open` sets `f_mode`.
    pub(crate) fn of(how: &OpenHow) -> Self {
        let mut mode = Self::LSEEK.0 | Self::PREAD.0 | Self::PWRITE.0;
        if how.read {
            mode |= Self::READ.0;
        }
        if how.write {
            mode |= Self::WRITE.0;
        }
        Self(mode)
    }

    /// This mode with no position, as `stream_open` clears `FMODE_LSEEK`,
    /// `FMODE_PREAD`, and `FMODE_PWRITE` and sets `FMODE_STREAM`.
    fn stream_open(self) -> Self {
        Self(self.0 & (Self::READ.0 | Self::WRITE.0) | Self::STREAM.0)
    }

    /// This mode with `mode` added.
    pub(crate) fn with(self, mode: Self) -> Self {
        Self(self.0 | mode.0)
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
    /// Where the entry stands, as `dir_emit` hands `ctx->pos`, so a listing
    /// from there reaches it first.
    pub offset: u64,
}

impl DirEmit<'_> {
    /// This entry as a list keeps one.
    pub fn to_dir_entry(&self) -> DirEntry {
        let entry = DirEntry::new(self.name, self.file_type);
        match self.identity {
            Some(identity) => {
                let named = identity.onto(Metadata::new(self.file_type, 0));
                entry.with_identity(named.dev, named.ino)
            }
            None => entry,
        }
    }
}

/// A listing under way, as Linux's `struct dir_context`: where it stands, who
/// takes each entry, answering whether to go on, and who the listing is for.
pub struct DirContext<'a> {
    pos: u64,
    actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool,
    cred: Arc<Cred>,
}

impl<'a> DirContext<'a> {
    /// A listing from `pos`, each entry going to `actor`. A listing of an open
    /// directory starts where that directory stands instead.
    pub fn new(pos: u64, actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool) -> Self {
        Self {
            pos,
            actor,
            cred: Arc::default(),
        }
    }

    /// Where the listing stands, as `ctx->pos`.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// Who the listing is for, as `file->f_cred`.
    pub fn cred(&self) -> &Arc<Cred> {
        &self.cred
    }

    /// Move the listing to `pos`, as a Filesystem sets `ctx->pos` to where its
    /// next entry stands, or past its last.
    pub fn set_pos(&mut self, pos: u64) {
        self.pos = pos;
    }

    /// Hand the actor `entry`, standing where the listing stands, answering
    /// whether to go on, as `dir_emit` does. The Filesystem moves the listing
    /// past what the actor took. The actor judges the name, as `filldir64` asks
    /// `verify_dirent_name`.
    pub fn dir_emit(&mut self, entry: &DirEntry) -> bool {
        let identity = Identity::from_numbers(entry.dev, entry.ino);
        self.emit(&entry.name, entry.file_type, identity)
    }

    fn emit(&mut self, name: &str, file_type: FileType, identity: Option<Identity>) -> bool {
        (self.actor)(&DirEmit {
            name,
            file_type,
            identity,
            offset: self.pos,
        })
    }
}

/// What `dentry` reports it stands for, `None` where a Filesystem cannot answer,
/// rather than failing the listing.
fn identity_of(dentry: &Dentry) -> Option<Identity> {
    dentry
        .getattr()
        .ok()
        .and_then(|metadata| Identity::of(&metadata))
}

/// Hand `ctx` the `.` and `..` it has yet to reach, as Linux's `dir_emit_dots`
/// does, `..` naming what stands above `dentry`, or itself at the root. It
/// answers whether to go on.
pub fn dir_emit_dots(dentry: &Dentry, ctx: &mut DirContext<'_>) -> bool {
    if ctx.pos == 0 {
        if !ctx.emit(".", FileType::Directory, identity_of(dentry)) {
            return false;
        }
        ctx.pos = 1;
    }
    if ctx.pos == 1 {
        let above = match dentry.parent() {
            Some(parent) => identity_of(&parent),
            None => identity_of(dentry),
        };
        if !ctx.emit("..", FileType::Directory, above) {
            return false;
        }
        ctx.pos = 2;
    }
    true
}

/// Hand `ctx` the `.` and `..` it has yet to reach, then `entries` from where it
/// stands, each at its index past those two, answering whether it took them all.
pub fn dir_emit_entries(dentry: &Dentry, ctx: &mut DirContext<'_>, entries: &[DirEntry]) -> bool {
    if !dir_emit_dots(dentry, ctx) {
        return false;
    }
    let skip = usize::try_from(ctx.pos - 2).unwrap_or(usize::MAX);
    entries.iter().skip(skip).all(|entry| {
        let took = ctx.dir_emit(entry);
        if took {
            ctx.pos += 1;
        }
        took
    })
}

/// What a directory's read answers, as Linux's `generic_read_dir` is every
/// directory's `f_op->read`.
pub fn generic_read_dir() -> Result<usize, FilesystemError> {
    Err(FilesystemError::IsADirectory)
}

/// Refuse a name no directory can hold, as Linux's `verify_dirent_name` refuses
/// what a Filesystem hands it, so whoever walks a listing is never handed a
/// path where it was promised one name. A Filesystem that asks about each name
/// before it lists them asks this first.
pub fn verify_dirent_name(name: &str) -> Result<(), FilesystemError> {
    if name.is_empty() || name.len() >= PATH_MAX || name.contains('/') {
        return Err(FilesystemError::Other(
            "a Filesystem listed a name no directory can hold".to_string(),
        ));
    }
    Ok(())
}

/// One open file or directory, as a descriptor refers to it.
pub struct File {
    /// Where it was opened, as `f_path`; it follows the name wherever it
    /// moves.
    path: Path,
    /// What answers for it, as `f_op`.
    ops: Box<dyn FileOperations>,
    /// What it was opened for, as `f_mode`.
    mode: FMode,
    /// Whether each write lands at the end, as `O_APPEND` in `f_flags`.
    append: bool,
    /// Whether it asked not to wait, as `O_NONBLOCK` in `f_flags`. Nothing
    /// here waits, so it only reports back.
    nonblock: bool,
    /// Whether what each write needs read back is durable before it answers, as
    /// `O_DSYNC` in `f_flags`.
    dsync: bool,
    /// Whether everything each write changed is durable before it answers, as
    /// `O_SYNC` in `f_flags`.
    sync: bool,
    /// Where the next read or write lands, as `f_pos`.
    pos: u64,
    /// Who opened it, as `f_cred`.
    cred: Arc<Cred>,
    /// What each of `cred`'s security modules kept on it as it opened, as
    /// `f_security`.
    security: Vec<FileSecurity>,
}

impl File {
    /// The file `ops` answers for, opened at `path` as `how` asks, with no
    /// position where `ops` is a stream.
    pub(crate) fn new(
        path: Path,
        ops: Box<dyn FileOperations>,
        how: &OpenHow,
        cred: &Arc<Cred>,
    ) -> Self {
        let mode = match ops.is_stream() {
            true => FMode::of(how).stream_open(),
            false => FMode::of(how),
        };
        Self {
            path,
            ops,
            mode,
            append: how.append,
            nonblock: how.nonblock,
            dsync: how.dsync,
            sync: how.sync,
            pos: 0,
            cred: cred.clone(),
            security: Vec::new(),
        }
    }

    /// This file holding what its security modules kept on it as it opened.
    pub(crate) fn with_security(self, security: Vec<FileSecurity>) -> Self {
        Self { security, ..self }
    }

    /// The directory at `path`, opened to be listed for `cred`, as `dentry_open`
    /// calls `f_op->open`.
    pub(super) fn directory(path: Path, cred: &Arc<Cred>) -> Result<Self, FilesystemError> {
        let how = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let ops = path.dentry().inode_ops()?.open(path.dentry(), &how)?;
        Ok(Self::new(path, ops, &how, cred))
    }

    /// The root of `mnt` opened as a directory for `cred`.
    pub fn root(mnt: &Arc<VfsMount>, cred: &Arc<Cred>) -> Result<Self, FilesystemError> {
        Self::directory(Path::root(mnt), cred)
    }

    /// Where it was opened; a call relative to it starts here.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where its next read, write, or listing starts, as `f_pos`.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// Whether it is a directory, as `S_ISDIR(file_inode(file))`.
    pub fn is_directory(&self) -> bool {
        self.kind() == Some(FileType::Directory)
    }

    /// What kind of entry it is, as `file_inode(file)->i_mode` says.
    pub fn kind(&self) -> Option<FileType> {
        self.path.dentry().inode().map(|inode| inode.kind())
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

    /// Whether what each write needs read back is durable before it answers.
    pub fn dsync(&self) -> bool {
        self.dsync
    }

    /// Whether everything each write changed is durable before it answers.
    pub fn sync(&self) -> bool {
        self.sync
    }

    /// Change the flags an open file may change, as `fcntl`'s `setfl` takes
    /// only `SETFL_MASK`.
    pub fn setfl(&mut self, append: bool, nonblock: bool) {
        self.append = append;
        self.nonblock = nonblock;
    }

    /// Move its position, answering where it now stands, as `vfs_llseek` refuses
    /// a file with no position and asks `f_op->llseek` of the rest.
    pub fn llseek(&mut self, offset: i64, whence: Whence) -> Result<u64, FilesystemError> {
        if !self.mode.contains(FMode::LSEEK) {
            return Err(FilesystemError::NotSeekable);
        }
        let dentry = self.path.dentry().clone();
        self.pos = self.ops.llseek(&dentry, self.pos, offset, whence)?;
        Ok(self.pos)
    }

    /// Take a hint about `len` bytes, as `generic_fadvise` refuses a FIFO and
    /// a negative length and ignores the rest, an offset past the largest one
    /// included, as Linux answers. Every Filesystem is still correct without one.
    pub fn fadvise(&self, _offset: u64, len: u64) -> Result<(), FilesystemError> {
        if self.kind() == Some(FileType::Fifo) {
            return Err(FilesystemError::NotSeekable);
        }
        match len > MAX_OFFSET {
            true => Err(FilesystemError::InvalidInput),
            false => Ok(()),
        }
    }

    /// Reserve `len` bytes from `offset`, as `vfs_fallocate` refuses in this
    /// order before it asks `f_op->fallocate`: what the call asks, the open's
    /// mode, a FIFO, a directory, and then anything but a regular file or a
    /// block device.
    pub fn fallocate(&mut self, offset: u64, len: u64) -> Result<(), FilesystemError> {
        if offset > MAX_OFFSET || len == 0 || len > MAX_OFFSET {
            return Err(FilesystemError::InvalidInput);
        }
        self.may_write()?;
        match self.kind() {
            Some(FileType::Fifo) => return Err(FilesystemError::NotSeekable),
            Some(FileType::Directory) => return Err(FilesystemError::IsADirectory),
            Some(FileType::File | FileType::BlockDevice) | None => {}
            Some(_) => return Err(FilesystemError::NoSuchDevice),
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

    /// A call landing at `pos`, at the end where it was opened to append, and
    /// asking the sync it was opened for, as `init_sync_kiocb` takes
    /// `IOCB_APPEND`, `IOCB_DSYNC`, and `IOCB_SYNC` from `f_flags`.
    fn kiocb(&self, pos: u64) -> Kiocb {
        let iocb = Kiocb {
            dsync: self.dsync || self.sync,
            sync: self.sync,
            ..Kiocb::at(pos)
        };
        match self.append {
            true => iocb.with_append(),
            false => iocb,
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
        let count = buf.len().min(MAX_RW_COUNT);
        let read = self.ops.read_iter(&mut iocb, &mut buf[..count])?;
        self.pos = iocb.pos;
        Ok(read)
    }

    /// Read from `pos`, leaving its position, as `pread` does.
    pub fn pread(&mut self, buf: &mut [u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pread(pos)?;
        self.rw_verify_area(pos, buf.len())?;
        let count = buf.len().min(MAX_RW_COUNT);
        self.ops.read_iter(&mut self.kiocb(pos), &mut buf[..count])
    }

    /// Write at its position, moving it past what was written, as `vfs_write`
    /// does.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.may_write()?;
        self.rw_verify_area(self.pos, buf.len())?;
        let mut iocb = self.kiocb(self.pos);
        let written = self
            .ops
            .write_iter(&mut iocb, &buf[..buf.len().min(MAX_RW_COUNT)])?;
        if written > 0 {
            self.pos = iocb.pos;
        }
        Ok(written)
    }

    /// Write at `pos`, leaving its position, as `pwrite` does. Opened to
    /// append, it lands at the end whatever `pos` says.
    pub fn pwrite(&mut self, buf: &[u8], pos: u64) -> Result<usize, FilesystemError> {
        self.may_pwrite(pos)?;
        self.rw_verify_area(pos, buf.len())?;
        self.ops
            .write_iter(&mut self.kiocb(pos), &buf[..buf.len().min(MAX_RW_COUNT)])
    }

    /// Make what was written durable, as `vfs_fsync` does, and `vfs_fsync_range`
    /// passes `datasync` through.
    pub fn fsync(&mut self, datasync: bool) -> Result<(), FilesystemError> {
        self.ops.fsync(datasync)
    }

    /// What it reports of itself, as `vfs_getattr` answers.
    pub fn getattr(&self) -> Result<Metadata, FilesystemError> {
        self.path.dentry().getattr()
    }

    /// Truncate or extend it through what it was opened on, as `do_ftruncate`
    /// refuses anything but a file open to write, and reaches `->setattr`.
    pub fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {
        let regular = self.path.dentry().kind() == Ok(FileType::File);
        if !regular || !self.writable() || len > MAX_OFFSET {
            return Err(FilesystemError::InvalidInput);
        }
        self.cred.security_file_truncate(&self.security)?;
        notify_change(self.path.dentry(), &Iattr::size(len))
    }

    /// Hand `ctx` the entries of the directory from where it stands until the
    /// actor stops, leaving it past each taken, as `iterate_dir` moves `f_pos`. It
    /// holds the directory's lock shared, so a removal through the VFS lands
    /// before or after, and a removed directory answers not found, as it asks
    /// `IS_DEADDIR`.
    pub fn iterate_dir(&mut self, ctx: &mut DirContext<'_>) -> Result<(), FilesystemError> {
        let dentry = self.path.dentry().clone();
        let _held = dentry.inode_lock_shared()?;
        if dentry.is_deaddir() {
            return Err(FilesystemError::NotFound);
        }
        ctx.pos = self.pos;
        ctx.cred = self.cred.clone();
        let listed = self.ops.iterate_shared(&dentry, ctx);
        self.pos = ctx.pos;
        listed
    }

    /// What its Filesystem lists past `.` and `..`, for a test of what the
    /// Filesystem answers beneath the VFS.
    #[cfg(test)]
    pub(crate) fn iterate_shared(&mut self) -> Result<Vec<DirEntry>, FilesystemError> {
        let dentry = self.path.dentry().clone();
        let mut entries = Vec::new();
        let mut actor = |emit: &DirEmit<'_>| {
            entries.push(emit.to_dir_entry());
            true
        };
        self.ops
            .iterate_shared(&dentry, &mut DirContext::new(2, &mut actor))?;
        Ok(entries)
    }

    /// Close it once the last descriptor goes, answering what its flush
    /// answers, as `filp_close` does; dropping it is what `->release` does.
    pub fn close(mut self) -> Result<(), FilesystemError> {
        self.ops.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::super::kern_path::filp_open;
    use super::super::vfs_fstatat;
    use super::*;
    use crate::filesystem::{create_pipe_files, PipeInodeInfo};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::filesystem::{Filesystem, InodeOperations, Lookup, Overlay, Tmpfs};
    use crate::mount::{Access, Mount};
    use crate::vfs::Inode;

    /// A file whose seek lands at a position of its own.
    struct SeeksToNine;

    impl FileOperations for SeeksToNine {
        fn llseek(
            &mut self,
            _dentry: &Dentry,
            _pos: u64,
            _offset: i64,
            _whence: Whence,
        ) -> Result<u64, FilesystemError> {
            Ok(9)
        }
    }

    /// A directory holding at every name a file whose `open` answers what
    /// its function makes.
    struct Opens(fn() -> Box<dyn FileOperations>);

    impl crate::vfs::FileSystemType for Opens {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for Opens {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::File, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let found = Metadata::new(FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&found, Arc::new(Self(self.0)))))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok((self.0)())
        }
    }

    /// The file at any name beneath a Mount of a Filesystem whose `open`
    /// answers what `opens` makes, opened to read.
    fn open_one(opens: fn() -> Box<dyn FileOperations>) -> File {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Opens(opens))).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        filp_open(&Arc::default(), &Path::root(mount.mnt()), "file", &read).expect("the file opens")
    }

    /// A file its Filesystem opens with no position.
    struct Stream;

    impl FileOperations for Stream {
        fn is_stream(&self) -> bool {
            true
        }
    }

    // A file a Filesystem opens as a stream would otherwise seek as any file
    // does, so only the open's mark keeps a position from it.
    // @behavior RT-273
    #[test]
    fn a_file_its_filesystem_opens_as_a_stream_takes_no_position() {
        let mut file = open_one(|| Box::new(Stream));
        assert_eq!(
            (
                file.llseek(0, Whence::Set),
                file.pread(&mut [0], 0),
                file.is_stream()
            ),
            (
                Err(FilesystemError::NotSeekable),
                Err(FilesystemError::NotSeekable),
                true
            ),
            "File::llseek and File::pread on a file its Filesystem's open marks a stream must \
             answer ESPIPE, as stream_open clears FMODE_LSEEK and FMODE_PREAD"
        );
    }

    /// A file its Filesystem gives no operation at all.
    struct NoSeek;

    impl FileOperations for NoSeek {}

    // @behavior RT-280
    #[test]
    fn a_file_without_llseek_takes_no_seek() {
        let mut file = open_one(|| Box::new(NoSeek));
        assert_eq!(
            (file.llseek(0, Whence::Set), file.pos()),
            (Err(FilesystemError::NotSeekable), 0),
            "File::llseek of a file whose Filesystem answers no llseek must answer ESPIPE and \
             leave its position, as Linux clears FMODE_LSEEK for one"
        );
    }

    // @behavior RT-248
    #[test]
    fn a_file_answers_where_its_own_seek_lands() {
        let mut file = open_one(|| Box::new(SeeksToNine));
        assert_eq!(
            (file.llseek(0, Whence::Set), file.llseek(0, Whence::Cur)),
            (Ok(9), Ok(9)),
            "File::llseek on a file answering llseek itself must answer and keep where that \
             seek lands, as vfs_llseek asks f_op->llseek"
        );
    }

    /// A directory holding a file at every name, neither with `open`.
    struct OpensNothing;

    impl crate::vfs::FileSystemType for OpensNothing {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for OpensNothing {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Metadata::new(FileType::File, 0),
                None => Metadata::new(FileType::Directory, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let found = Metadata::new(FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&found, Arc::new(Self))))
        }
    }

    // Only `O_PATH` opens a Linux directory for neither, and a read of that
    // answers EBADF before anything asks what it is.
    // @behavior RT-235
    #[test]
    fn a_directory_opened_for_neither_reading_nor_writing_refuses_a_read() {
        let tmpfs: Tmpfs = [("d/f", "x")].into_iter().collect();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(mut dir) = filp_open(&Arc::default(), &Path::root(mount.mnt()), "d", &how) else {
            panic!("the directory must open");
        };
        assert_eq!(
            dir.read(&mut [0; 1]),
            Err(FilesystemError::BadDescriptor),
            "File::read of a directory opened for neither reading nor writing must answer EBADF"
        );
    }

    // Linux's `generic_write_checks_count` returns before it reads the end for
    // a write of nothing, and `new_sync_write` moves `f_pos` only past a positive
    // answer, so a Filesystem moving `Kiocb::pos` regardless cannot move a file.
    // @behavior RT-222
    #[test]
    fn a_write_of_nothing_leaves_an_appending_file_where_it_stands() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            create: true,
            append: true,
            ..OpenHow::default()
        };
        let Ok(mut file) = filp_open(&Arc::default(), &Path::root(mount.mnt()), "log", &how) else {
            panic!("the file must be made");
        };
        file.write(b"hello").expect("the write must land");
        file.llseek(0, Whence::Set).expect("the seek must land");
        file.write(b"").expect("a write of nothing must answer");
        assert_eq!(
            file.llseek(0, Whence::Cur),
            Ok(0),
            "File::write of nothing to a file opened to append, positioned at 0, must leave it at 0"
        );
    }

    // A built-in Filesystem always has `open`, so only one without it shows
    // what the runtime answers.
    // @behavior RT-218
    #[test]
    fn an_entry_without_open_opens_with_no_operations() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(OpensNothing)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let cred = Arc::default();
        let read = filp_open(&cred, &Path::root(mount.mnt()), "file", &how)
            .map_err(|_| "the open was refused")
            .map(|mut file| file.read(&mut [0; 1]));
        assert_eq!(
            read,
            Ok(Err(FilesystemError::InvalidInput)),
            "vfs::do_file_open of a file whose Filesystem has no open must succeed, and File::read \
             of it must answer EINVAL, as Linux's opens with no f_op->open"
        );
    }

    /// `file.txt`, seven bytes long, opened for `access`.
    fn open(read: bool, write: bool) -> File {
        let tmpfs = Tmpfs::try_from_iter([("file.txt", "granted")]).expect("the seed must fit");
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            read,
            write,
            ..OpenHow::default()
        };
        filp_open(&Arc::default(), &Path::root(mount.mnt()), "file.txt", &how)
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
    /// entries, as the name and the position of each, and where the directory
    /// stands once the actor refused the next.
    fn list(file: &mut File, pos: u64, room: usize) -> (Vec<(String, u64)>, u64) {
        let mut seen = Vec::new();
        let mut actor = |emit: &DirEmit<'_>| {
            let took = seen.len() < room;
            if took {
                seen.push((emit.name.to_string(), emit.offset));
            }
            took
        };
        file.llseek(pos as i64, Whence::Set)
            .unwrap_or_else(|_| panic!("the directory must seek"));
        file.iterate_dir(&mut DirContext::new(0, &mut actor))
            .unwrap_or_else(|_| panic!("the directory must list"));
        (seen, file.pos())
    }

    /// A directory reading its two entries at once, as a Bind does.
    struct ReadsWhole;

    struct ReadsWholeDir(crate::filesystem::Rdir);

    impl FileOperations for ReadsWholeDir {
        fn llseek(
            &mut self,
            dentry: &Dentry,
            pos: u64,
            offset: i64,
            whence: Whence,
        ) -> Result<u64, FilesystemError> {
            generic_file_llseek(dentry, pos, offset, whence)
        }

        fn iterate_shared(
            &mut self,
            dentry: &Dentry,
            ctx: &mut DirContext<'_>,
        ) -> Result<(), FilesystemError> {
            self.0.iterate(dentry, ctx, || {
                Ok(vec![
                    DirEntry::new("a", FileType::File),
                    DirEntry::new("b", FileType::File),
                ])
            })
        }
    }

    impl crate::vfs::FileSystemType for ReadsWhole {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for ReadsWhole {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok(Box::new(ReadsWholeDir(crate::filesystem::Rdir::default())))
        }
    }

    /// A listing of `file` from the start, its actor stopping after `room`
    /// entries.
    fn begin_listing(file: &mut File, room: usize) -> Result<(), FilesystemError> {
        let mut seen = 0;
        file.iterate_dir(&mut DirContext::new(0, &mut |_| {
            seen += 1;
            seen < room
        }))
    }

    // Every descriptor holds a listing of its own, so only counting them together
    // keeps a guest from multiplying a directory by its open files.
    // @behavior RT-237
    #[test]
    fn listings_held_between_calls_count_against_their_limit() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(ReadsWhole)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let one_listing = 2 * size_of::<DirEntry>() + 2;
        let cred = crate::security::prepare_creds_within([], Some(one_listing));
        let open =
            || File::root(mount.mnt(), &cred).unwrap_or_else(|_| panic!("the root must open"));
        let (mut first, mut second, mut whole) = (open(), open(), open());
        let held = begin_listing(&mut first, 1);
        let refused = begin_listing(&mut second, 1);
        let read_whole = begin_listing(&mut whole, usize::MAX);
        drop(first);
        let after_close = begin_listing(&mut second, 1);
        assert_eq!(
            (held, refused, read_whole, after_close),
            (Ok(()), Err(FilesystemError::OutOfMemory), Ok(()), Ok(())),
            "File::iterate_dir of a second listing stopped part way, past its caller's memory limit \
             allows, must answer ENOMEM, one read whole must hold nothing, and closing the \
             first must give its bytes back"
        );
    }

    /// Each entry of the root of `filesystem` with what a listing names it
    /// and what a stat of it names it.
    fn identities<F: crate::vfs::FileSystemType>(
        filesystem: F,
    ) -> Vec<(String, Option<Identity>, Option<Identity>)> {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(filesystem)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let start = Path::root(mount.mnt());
        let mut root = File::root(mount.mnt(), &Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let mut listed = Vec::new();
        root.llseek(2, Whence::Set)
            .unwrap_or_else(|_| panic!("the root must seek past its dots"));
        root.iterate_dir(&mut DirContext::new(0, &mut |emit| {
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
            vec![Filesystem::new(Arc::new(lower)).expect("the instance must stand")],
            Some(Filesystem::new(Arc::new(upper)).expect("the instance must stand")),
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
        let mount = Mount::new(
            "/data",
            Filesystem::new(tmpfs).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let mut root = File::root(mount.mnt(), &Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let entry = |name: &str, at| (name.to_string(), at);
        assert_eq!(
            (list(&mut root, 0, 3), list(&mut root, 2, 9)),
            (
                (vec![entry(".", 0), entry("..", 1), entry("a", 2)], 3),
                (vec![entry("a", 2), entry("b", 3)], 4),
            ),
            "File::iterate_dir must hand `.` and `..` ahead of the entries, each at its own \
             position as dir_emit hands ctx->pos, stand past the last one taken, and go on \
             from any of those positions"
        );
    }

    // @behavior RT-182
    #[test]
    fn a_listing_read_whole_goes_on_past_the_last_entry_taken() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(ReadsWhole)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let mut root = File::root(mount.mnt(), &Arc::default())
            .unwrap_or_else(|_| panic!("the root must open"));
        let entry = |name: &str, at| (name.to_string(), at);
        assert_eq!(
            (list(&mut root, 0, 3), list(&mut root, 3, 9)),
            (
                (vec![entry(".", 0), entry("..", 1), entry("a", 2)], 3),
                (vec![entry("b", 3)], 4),
            ),
            "File::iterate_dir over a Filesystem listing through Rdir must hand each entry its \
             own position and stand past the last one taken, so a listing going on from there \
             repeats nothing"
        );
    }

    /// A file counting its syncs, at every name of a directory.
    struct CountsSyncs(Arc<AtomicUsize>);

    impl FileOperations for CountsSyncs {
        fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
            generic_write_sync(self, iocb, buf.len())
        }

        fn fsync(&mut self, _datasync: bool) -> Result<(), FilesystemError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A file taking whatever it is handed, recording how much and how it syncs.
    #[derive(Clone, Default)]
    struct RecordsCalls {
        taken: Arc<AtomicUsize>,
        syncs: Arc<std::sync::Mutex<Vec<bool>>>,
    }

    impl FileOperations for RecordsCalls {
        fn read_iter(
            &mut self,
            _iocb: &mut Kiocb,
            buf: &mut [u8],
        ) -> Result<usize, FilesystemError> {
            self.taken.store(buf.len(), Ordering::SeqCst);
            Ok(0)
        }

        fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
            self.taken.store(buf.len(), Ordering::SeqCst);
            generic_write_sync(self, iocb, buf.len())
        }

        fn fsync(&mut self, datasync: bool) -> Result<(), FilesystemError> {
            self.syncs.lock().expect("never poisoned").push(datasync);
            Ok(())
        }
    }

    impl crate::vfs::FileSystemType for RecordsCalls {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for RecordsCalls {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Metadata::new(FileType::File, 0),
                None => Metadata::new(FileType::Directory, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let found = Metadata::new(FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&found, Arc::new(self.clone()))))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok(Box::new(self.clone()))
        }
    }

    /// `calls` open at `file`, as `how` asks.
    fn open_recording(calls: &RecordsCalls, how: OpenHow) -> File {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(calls.clone())).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        filp_open(&Arc::default(), &Path::root(mount.mnt()), "file", &how)
            .unwrap_or_else(|_| panic!("the file must open"))
    }

    // @behavior RT-251
    #[test]
    fn each_write_syncs_only_as_far_as_the_file_was_opened_to() {
        let calls = RecordsCalls::default();
        let open = |dsync, sync| {
            open_recording(
                &calls,
                OpenHow {
                    write: true,
                    dsync,
                    sync,
                    ..OpenHow::default()
                },
            )
        };
        open(false, false).write(b"x").expect("the write lands");
        open(true, false).write(b"x").expect("the write lands");
        open(false, true).write(b"x").expect("the write lands");
        let mut file = open(false, false);
        file.fsync(true).expect("the sync lands");
        file.fsync(false).expect("the sync lands");
        assert_eq!(
            *calls.syncs.lock().expect("never poisoned"),
            [true, false, true, false],
            "File::write on a file opened O_DSYNC must sync its data alone, on one opened O_SYNC \
             all of it, and File::fsync must pass datasync through, as vfs_fsync_range does"
        );
    }

    // @behavior RT-252
    #[test]
    fn a_single_call_takes_at_most_the_most_one_read_or_write_takes() {
        let calls = RecordsCalls::default();
        let mut file = open_recording(
            &calls,
            OpenHow {
                read: true,
                write: true,
                ..OpenHow::default()
            },
        );
        // The allocator hands zeroed pages it has not touched, so this holds little.
        let mut past = vec![0u8; MAX_RW_COUNT + 1];
        let mut taken = |call: &mut dyn FnMut(&mut File, &mut [u8])| {
            calls.taken.store(0, Ordering::SeqCst);
            call(&mut file, &mut past);
            calls.taken.load(Ordering::SeqCst)
        };
        let answers = [
            taken(&mut |file, buf| drop(file.read(buf))),
            taken(&mut |file, buf| drop(file.pread(buf, 0))),
            taken(&mut |file, buf| drop(file.write(buf))),
            taken(&mut |file, buf| drop(file.pwrite(buf, 0))),
        ];
        assert_eq!(
            answers, [0x7fff_f000; 4],
            "File::read, pread, write, and pwrite past MAX_RW_COUNT must hand the file only that \
             many bytes, as vfs_read and vfs_write cut count"
        );
    }

    struct HoldsCountsSyncs(Arc<AtomicUsize>);

    impl crate::vfs::FileSystemType for HoldsCountsSyncs {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            crate::vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for HoldsCountsSyncs {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let found = Metadata::new(FileType::File, 0);
            let ops = Arc::new(Self(self.0.clone()));
            Ok(Lookup::Found(Inode::new(&found, ops)))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok(Box::new(CountsSyncs(self.0.clone())))
        }
    }

    // @behavior RT-091
    #[test]
    fn a_file_opened_to_sync_makes_each_write_durable() {
        let syncs = Arc::new(AtomicUsize::new(0));
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HoldsCountsSyncs(syncs.clone())))
                .expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let open = |sync| {
            let how = OpenHow {
                write: true,
                sync,
                ..OpenHow::default()
            };
            filp_open(&Arc::default(), &Path::root(mount.mnt()), "file", &how)
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
        let mount = Mount::new(
            "/data",
            Filesystem::new(tmpfs).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            read: true,
            nonblock: true,
            sync: true,
            ..OpenHow::default()
        };
        let dir = filp_open(&Arc::default(), &Path::root(mount.mnt()), "dir", &how)
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
        let mount = Mount::new(
            "/data",
            Filesystem::new(tmpfs).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let mut dir = File::root(mount.mnt(), &Arc::default())
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
                dir.fsync(false),
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
        struct NoOperations;
        impl FileOperations for NoOperations {}
        let (_, mut pipe) = create_pipe_files(&PipeInodeInfo::new(1));
        let how = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        let path = open(false, true).path().clone();
        assert_eq!(
            (
                open(true, false).fallocate(0, 0),
                open(true, false).fallocate(0, 1),
                File::new(path, Box::new(NoOperations), &how, &Arc::default()).fallocate(0, 1),
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
