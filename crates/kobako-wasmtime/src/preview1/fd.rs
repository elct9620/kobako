//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! The runtime's open file decides each answer, so this keeps only what
//! preview1 itself defines: which file a number stands for, its rights, a
//! preopen's name, and what wasi-testsuite asks of a directory's position.
//! It also keeps what a Linux syscall entry checks before the VFS: which
//! descriptor a call takes, the order of its errors, and the caller's buffer.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{FileType, Iattr, Kstat, SetTime};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::{self, verify_dirent_name, DirContext, DirEmit, File, Whence};
use wiggle::{GuestMemory as WitxMemory, GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use super::path::retry_eagain;
use super::types::{self, Fdflags, Filetype, Fstflags, Rights};
use crate::descriptors::{Descriptor, Descriptors};

/// What a preopen allows itself: every path call, as wasmtime-wasi hands
/// most userlands.
fn preopen_rights() -> Rights {
    Rights::PATH_CREATE_DIRECTORY
        | Rights::PATH_CREATE_FILE
        | Rights::PATH_LINK_SOURCE
        | Rights::PATH_LINK_TARGET
        | Rights::PATH_OPEN
        | Rights::FD_READDIR
        | Rights::PATH_READLINK
        | Rights::PATH_RENAME_SOURCE
        | Rights::PATH_RENAME_TARGET
        | Rights::PATH_FILESTAT_GET
        | Rights::PATH_FILESTAT_SET_TIMES
        | Rights::FD_FILESTAT_GET
        | Rights::FD_FILESTAT_SET_TIMES
        | Rights::PATH_SYMLINK
        | Rights::PATH_REMOVE_DIRECTORY
        | Rights::PATH_UNLINK_FILE
}

/// What a file opened beneath a directory may hold. Reading and writing are
/// both inherited whatever the Mount's access or the directory's own mode,
/// since wasi-libc intersects them before asking and Linux takes an open's
/// mode from its flags alone: a read-only Mount answers a write itself, as
/// `ROFS`.
fn directory_rights_inheriting() -> Rights {
    preopen_rights()
        | Rights::FD_DATASYNC
        | Rights::FD_READ
        | Rights::FD_SEEK
        | Rights::FD_FDSTAT_SET_FLAGS
        | Rights::FD_SYNC
        | Rights::FD_TELL
        | Rights::FD_WRITE
        | Rights::FD_ADVISE
        | Rights::FD_ALLOCATE
        | Rights::FD_FILESTAT_SET_SIZE
        | Rights::POLL_FD_READWRITE
}

/// What `fd` stands for, a file that stands only for its path included, as
/// `fdget_raw` takes one.
fn descriptor(table: &Descriptors, fd: u32) -> Answer<&Descriptor> {
    table.get(fd).ok_or_else(|| Errno::Badf.into())
}

/// The open file `fd` stands for, refusing one that stands only for its path,
/// as `fdget` does.
pub(crate) fn file(table: &Descriptors, fd: u32) -> Answer<&File> {
    match table.get(fd) {
        Some(descriptor) if !descriptor.file.is_path() => Ok(&descriptor.file),
        _ => Err(Errno::Badf.into()),
    }
}

/// The open file `fd` stands for, to change, refusing one that stands only for
/// its path, as `fdget` does.
fn file_mut(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match table.get_mut(fd) {
        Some(descriptor) if !descriptor.file.is_path() => Ok(&mut descriptor.file),
        _ => Err(Errno::Badf.into()),
    }
}

/// The open file `fd` stands for, in a call about its position. wasi-testsuite
/// has a directory refuse one, where Linux would seek it.
fn seekable_file(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match file_mut(table, fd)? {
        file if file.is_directory() => Err(Errno::Badf.into()),
        file => Ok(file),
    }
}

/// A call any file or directory answers, which preview1 refuses for
/// stdio.
fn refuse_stream(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)?.file.is_stream() {
        true => Err(Errno::Badf.into()),
        false => Ok(()),
    }
}

/// `file_type` as a preview1 `filetype`, which has no code for a FIFO and two
/// for the one kind `st_mode` names a socket.
fn to_filetype(file_type: FileType) -> Filetype {
    match file_type {
        FileType::Dir => Filetype::Directory,
        FileType::Symlink => Filetype::SymbolicLink,
        FileType::RegFile => Filetype::RegularFile,
        FileType::Sock => Filetype::SocketStream,
        FileType::Blkdev => Filetype::BlockDevice,
        FileType::Chrdev => Filetype::CharacterDevice,
        _ => Filetype::Unknown,
    }
}

/// A `filestat` for what a Filesystem reports a path names, as the guest
/// holding `table` sees it, at time zero where it keeps no time.
pub(crate) fn to_filestat(table: &Descriptors, stat: Kstat) -> types::Filestat {
    let stat = table.profile().guest_kstat(stat);
    types::Filestat {
        dev: stat.dev,
        ino: stat.ino,
        filetype: to_filetype(stat.file_type),
        nlink: stat.nlink,
        size: stat.size,
        atim: to_timestamp(stat.atime),
        mtim: to_timestamp(stat.mtime),
        ctim: to_timestamp(stat.ctime),
    }
}

/// `time` as a preview1 `timestamp`, nanoseconds since the Unix epoch;
/// zero for none, and the largest timestamp for a time past it.
fn to_timestamp(time: Option<SystemTime>) -> u64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
        })
}

/// Set the times `flags` ask through `call`, answering before it looks at the
/// name or the descriptor where they ask none, as Linux's `utimensat` does
/// for both. Setting a timestamp and setting it to now are exclusive: asking
/// both is `INVAL`.
pub(crate) fn utimensat(
    atim: types::Timestamp,
    mtim: types::Timestamp,
    flags: Fstflags,
    call: impl FnOnce(&Iattr) -> Answer,
) -> Answer {
    match to_iattr(atim, mtim, flags)? {
        Some(times) => call(&times),
        None => Ok(()),
    }
}

/// The times `flags` ask to set, `None` where they ask none.
fn to_iattr(
    atim: types::Timestamp,
    mtim: types::Timestamp,
    flags: Fstflags,
) -> Answer<Option<Iattr<'static>>> {
    let time = |at, set, now| match (flags.contains(set), flags.contains(now)) {
        (true, true) => Err(Errno::Inval),
        (true, false) => Ok(Some(SetTime::At(UNIX_EPOCH + Duration::from_nanos(at)))),
        (false, true) => Ok(Some(SetTime::Now)),
        (false, false) => Ok(None),
    };
    let atime = time(atim, Fstflags::ATIM, Fstflags::ATIM_NOW)?;
    let mtime = time(mtim, Fstflags::MTIM, Fstflags::MTIM_NOW)?;
    Ok((atime.is_some() || mtime.is_some()).then(|| Iattr::times(atime, mtime)))
}

pub(crate) fn advise(table: &mut Descriptors, fd: u32, offset: u64, len: u64) -> Answer {
    Ok(file_mut(table, fd)?.fadvise(offset, len)?)
}

pub(crate) fn allocate(table: &mut Descriptors, fd: u32, offset: u64, len: u64) -> Answer {
    Ok(file_mut(table, fd)?.fallocate(offset, len)?)
}

pub(crate) fn close(table: &mut Descriptors, fd: u32) -> Answer {
    Ok(table.remove(fd).ok_or(Errno::Badf)?.file.close()?)
}

pub(crate) fn sync(table: &mut Descriptors, fd: u32, datasync: bool) -> Answer {
    Ok(file_mut(table, fd)?.fsync(datasync)?)
}

pub(crate) fn fdstat_get(table: &Descriptors, fd: u32) -> Answer<types::Fdstat> {
    let descriptor = descriptor(table, fd)?;
    Ok(to_fdstat(
        &descriptor.file,
        descriptor.preopen_path.is_some(),
    ))
}

/// What `fd_fdstat_get` reports of `file`, a preopen's root when `preopen`.
fn to_fdstat(file: &File, preopen: bool) -> types::Fdstat {
    let mut fs_flags = Fdflags::empty();
    fs_flags.set(Fdflags::APPEND, file.append());
    fs_flags.set(Fdflags::NONBLOCK, file.nonblock());
    fs_flags.set(Fdflags::DSYNC, file.dsync());
    fs_flags.set(Fdflags::SYNC, file.sync());
    let (fs_rights_base, fs_rights_inheriting) = if file.is_stream() {
        let mut rights = Rights::empty();
        rights.set(Rights::FD_READ, file.readable());
        rights.set(Rights::FD_WRITE, file.writable());
        (rights, rights)
    } else if preopen {
        (preopen_rights(), directory_rights_inheriting())
    } else if file.is_directory() {
        let mut rights = Rights::all()
            - Rights::FD_WRITE
            - Rights::FD_SEEK
            - Rights::FD_FILESTAT_SET_SIZE
            - Rights::PATH_FILESTAT_SET_SIZE;
        if !file.readable() {
            rights -= Rights::FD_READ | Rights::FD_READDIR;
        }
        (rights, directory_rights_inheriting())
    } else {
        let mut rights = Rights::all();
        if !file.readable() {
            rights -= Rights::FD_READ | Rights::FD_READDIR;
        }
        if !file.writable() {
            rights -= Rights::FD_WRITE;
        }
        (rights, rights)
    };
    types::Fdstat {
        fs_filetype: to_filetype(file.kind()),
        fs_flags,
        fs_rights_base,
        fs_rights_inheriting,
    }
}

pub(crate) fn fdstat_set_flags(table: &mut Descriptors, fd: u32, flags: Fdflags) -> Answer {
    file_mut(table, fd)?.setfl(
        flags.contains(Fdflags::APPEND),
        flags.contains(Fdflags::NONBLOCK),
    );
    Ok(())
}

/// Rights only narrow at open, so a later change is unsupported.
pub(crate) fn fdstat_set_rights(table: &Descriptors, fd: u32) -> Answer {
    refuse_stream(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn filestat_get(table: &Descriptors, fd: u32) -> Answer<types::Filestat> {
    Ok(to_filestat(table, descriptor(table, fd)?.file.getattr()?))
}

/// A size Linux's signed `loff_t` reads as negative answers before the
/// descriptor is looked for, as `ksys_ftruncate` checks it before `fdget`.
pub(crate) fn filestat_set_size(table: &mut Descriptors, fd: u32, size: u64) -> Answer {
    if i64::try_from(size).is_err() {
        return Err(Errno::Inval.into());
    }
    Ok(file_mut(table, fd)?.truncate(size)?)
}

pub(crate) fn filestat_set_times(table: &Descriptors, fd: u32, times: &Iattr) -> Answer {
    let file = file(table, fd)?;
    Ok(retry_eagain(|| {
        table.walk_guard().check(file.path())?;
        vfs::do_utimes_fd(table.cred(), file.path(), times)
    })?)
}

pub(crate) fn pread(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    iovs: types::IovecArray,
    offset: u64,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_iovec(iovs)?;
    let file = file_mut(table, fd)?;
    file.may_pread(offset)?;
    Ok(file.pread(mem.bytes_mut(buf)?, offset)? as u32)
}

pub(crate) fn prestat_get(table: &Descriptors, fd: u32) -> Answer<types::Prestat> {
    match &descriptor(table, fd)?.preopen_path {
        Some(at) => Ok(types::Prestat::Dir(types::PrestatDir {
            pr_name_len: at.len() as u32,
        })),
        None => Err(Errno::Badf.into()),
    }
}

pub(crate) fn prestat_dir_name(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<u8>,
    path_len: u32,
) -> Answer {
    match &descriptor(table, fd)?.preopen_path {
        Some(at) => {
            let name = at.as_bytes();
            if name.len() > path_len as usize {
                return Err(Errno::Nametoolong.into());
            }
            mem.write_bytes(path, name)
        }
        None => Err(Errno::Notdir.into()),
    }
}

pub(crate) fn pwrite(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    ciovs: types::CiovecArray,
    offset: u64,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_ciovec(ciovs)?;
    let file = file_mut(table, fd)?;
    file.may_pwrite(offset)?;
    Ok(file.pwrite(mem.bytes(buf)?, offset)? as u32)
}

pub(crate) fn read(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    iovs: types::IovecArray,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_iovec(iovs)?;
    let file = file_mut(table, fd)?;
    file.may_read()?;
    // Bounded before the file is asked, as a trap takes no bytes from a pipe.
    Ok(file.read(mem.bytes_mut(buf)?)? as u32)
}

/// The `dirent` header ahead of one entry's name, its `d_next` left for the
/// listing to fill in once it knows it.
fn to_dirent_header(emit: &DirEmit<'_>, d_ino: u64) -> Answer<Vec<u8>> {
    let mut header = vec![0; types::Dirent::guest_size() as usize];
    WitxMemory::Unshared(&mut header).write(
        GuestPtr::new(0),
        types::Dirent {
            d_next: 0,
            d_ino,
            d_namlen: emit.name.len() as u32,
            d_type: to_filetype(emit.file_type),
        },
    )?;
    Ok(header)
}

/// One entry as a preview1 `dirent` at `used` bytes into the buffer, cut off
/// where it ends, answering where its header stands when it fit whole.
fn write_dirent(
    mem: &mut GuestMemory<'_, '_>,
    buf: (GuestPtr<u8>, u32),
    used: &mut u32,
    profile: Profile,
    emit: &DirEmit<'_>,
) -> Answer<Option<u32>> {
    let (start, capacity) = buf;
    let at = *used;
    let header = to_dirent_header(emit, profile.guest_ino(emit.identity))?;
    let mut whole = true;
    for part in [&header[..], emit.name.as_bytes()] {
        let room = (capacity - *used) as usize;
        let take = part.len().min(room);
        if take > 0 {
            mem.write_bytes(start.add(*used)?, &part[..take])?;
        }
        *used += take as u32;
        whole &= take == part.len();
    }
    Ok(whole.then_some(at))
}

/// Where a listing goes on from past the entry whose header stands at `at`,
/// as `filldir64` writes `prev->d_off` once the next entry or the listing's end
/// says, `d_next` leading each header.
fn write_d_next(
    mem: &mut GuestMemory<'_, '_>,
    start: GuestPtr<u8>,
    at: u32,
    d_next: u64,
) -> Answer {
    mem.write_bytes(start.add(at)?, &d_next.to_le_bytes())?;
    Ok(())
}

/// A directory's entries from the `cookie`th on, cut off where the buffer
/// ends.
pub(crate) fn readdir(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    start: GuestPtr<u8>,
    capacity: u32,
    cookie: u64,
) -> Answer<types::Size> {
    let profile = table.profile();
    let dir = file_mut(table, fd)?;
    // preview1 names where a listing goes on by its cookie, as wasi-libc's
    // `seekdir` moves a directory's position before `getdents`; a listing going
    // on from where the last one stopped needs no seek.
    if cookie != dir.pos() {
        let cookie = i64::try_from(cookie).map_err(|_| kobako_runtime::Errno::InvalidInput)?;
        dir.llseek(cookie, Whence::Set)?;
    }
    let mut used: u32 = 0;
    let mut failed = None;
    let mut last_whole = None;
    let mut actor = |emit: &DirEmit<'_>| {
        let written = verify_dirent_name(emit.name)
            .map_err(Into::into)
            .and_then(|()| match last_whole.take() {
                Some(at) => write_d_next(mem, start, at, emit.offset),
                None => Ok(()),
            })
            .and_then(|()| write_dirent(mem, (start, capacity), &mut used, profile, emit));
        match written {
            Ok(whole) => {
                last_whole = whole;
                whole.is_some()
            }
            Err(error) => {
                failed = Some(error);
                false
            }
        }
    };
    let listed = dir.iterate_dir(&mut DirContext::new(0, &mut actor));
    // The last entry written whole goes on from where the listing stopped,
    // as `getdents64` sets the last `d_off` from `f_pos`.
    if let Some(at) = last_whole {
        write_d_next(mem, start, at, dir.pos())?;
    }
    // What was listed answers before what stopped the listing, which the next
    // call meets first, as Linux's `getdents64` answers the bytes it wrote over
    // `iterate_dir`'s error and `buf.error`.
    match listed.err().map(Into::into).or(failed) {
        Some(error) if used == 0 => Err(error),
        _ => Ok(used),
    }
}

/// preview1 renumbering moves `from` onto `to`, both already open, then
/// closes what stood at `to`, ignoring how its close answers, as Linux's
/// `do_dup2` installs the file before `filp_close` of the one it replaced.
pub(crate) fn renumber(table: &mut Descriptors, from: u32, to: u32) -> Answer {
    if table.get(from).is_none() || table.get(to).is_none() {
        return Err(Errno::Badf.into());
    }
    if from == to {
        return Ok(());
    }
    let moved = table.remove(from).expect("found open above");
    let replaced = table.remove(to).expect("found open above");
    table.place(to, moved);
    let _ = replaced.file.close();
    Ok(())
}

pub(crate) fn seek(
    table: &mut Descriptors,
    fd: u32,
    offset: i64,
    whence: types::Whence,
) -> Answer<u64> {
    let whence = match whence {
        types::Whence::Set => Whence::Set,
        types::Whence::Cur => Whence::Cur,
        types::Whence::End => Whence::End,
    };
    Ok(seekable_file(table, fd)?.llseek(offset, whence)?)
}

pub(crate) fn tell(table: &mut Descriptors, fd: u32) -> Answer<u64> {
    seek(table, fd, 0, types::Whence::Cur)
}

pub(crate) fn write(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    ciovs: types::CiovecArray,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_ciovec(ciovs)?;
    let file = file_mut(table, fd)?;
    file.may_write()?;
    Ok(file.write(mem.bytes(buf)?)? as u32)
}

#[cfg(test)]
mod tests {
    //! Witness that renumbering onto an open file closes it as `fd_close`
    //! does, so its Filesystem learns of the close and may refuse it.
    use std::sync::Arc;

    use super::{to_filetype, Filetype};

    // @behavior RT-195
    #[test]
    fn a_special_files_kind_reads_as_preview1_names_it() {
        assert_eq!(
            [
                FileType::Sock,
                FileType::Blkdev,
                FileType::Chrdev,
                FileType::Fifo,
                FileType::Unknown,
            ]
            .map(to_filetype),
            [
                Filetype::SocketStream,
                Filetype::BlockDevice,
                Filetype::CharacterDevice,
                Filetype::Unknown,
                Filetype::Unknown,
            ],
            "to_filetype must name a socket as a stream, each device as itself, and a FIFO, which \
             preview1 has no code for, as unknown"
        );
    }

    use super::{
        fdstat_get, fdstat_set_flags, filestat_set_size, filestat_set_times, readdir, renumber,
        seek, sync, tell, utimensat, Answer, Errno, Fdflags, Fstflags,
    };

    /// `fd_filestat_set_times` of `fd` setting both times to zero as `flags`
    /// ask, as the dispatcher answers it.
    fn set_times(table: &Descriptors, fd: u32, flags: Fstflags) -> Answer {
        utimensat(0, 0, flags, |times| filestat_set_times(table, fd, times))
    }
    use crate::descriptors::{Descriptor, Descriptors};
    use crate::preview1::memory::GuestMemory;
    use crate::preview1::path::{
        create_directory, filestat_set_times as path_set_times, open as path_open, readlink,
        rename, symlink, PathOpen,
    };
    use crate::preview1::types::{self, Rights, Whence};
    use kobako_runtime::filesystem::{create_pipe_files, PipeInodeInfo};
    use kobako_runtime::filesystem::{
        FileOperations, FileType, Filesystem, InodeOperations, Kstat, Lookup,
    };
    use kobako_runtime::mount::{Access, Mount};
    use kobako_runtime::profile::Profile;
    use kobako_runtime::security::prepare_creds_within;
    use kobako_runtime::vfs::{self, Dentry, Inode, OpenHow, Path, PATH_MAX};
    use wiggle::{GuestPtr, GuestType};

    /// What `do_file_open` takes for an open `how` asks.
    fn flags_of(how: &OpenHow) -> vfs::OpenFlags {
        vfs::build_open_flags(how).expect("the flags must mean an open")
    }

    /// A table holding the three standard streams and `mounts`, at most `limit`
    /// descriptors open, as a Driver makes one.
    fn table_of(
        mounts: Vec<Mount>,
        limit: Option<usize>,
    ) -> Result<Descriptors, kobako_runtime::Errno> {
        let cred = prepare_creds_within(&[], None);
        let ends = |pipe| create_pipe_files(&pipe, &cred);
        Descriptors::new(
            ends(PipeInodeInfo::from_bytes(Vec::new())).0,
            ends(PipeInodeInfo::new(1)).1,
            ends(PipeInodeInfo::new(1)).1,
            &mounts
                .into_iter()
                .map(kobako_runtime::grant::Grant::Mount)
                .collect::<Vec<_>>(),
            kobako_runtime::vfs::OpenFiles::new(limit),
            cred.clone(),
            Profile::Hermetic,
        )
    }

    /// A table holding only the three standard streams.
    fn standard_streams() -> Descriptors {
        table_of(Vec::new(), None).expect("a table holding no Mount must make")
    }

    /// The code a call answered, if it answered one.
    fn errno<T>(answer: Answer<T>) -> Option<Errno> {
        answer.err().and_then(|err| err.downcast().ok())
    }

    // @behavior RT-177
    #[test]
    fn a_standard_stream_answers_as_a_pipe_does() {
        let mut table = standard_streams();
        assert_eq!(
            [
                errno(sync(&mut table, 1, false)),
                errno(filestat_set_size(&mut table, 1, 0)),
                errno(tell(&mut table, 0)),
                errno(tell(&mut table, 1)),
            ],
            [
                Some(Errno::Inval),
                Some(Errno::Inval),
                Some(Errno::Spipe),
                Some(Errno::Spipe)
            ],
            "fd_sync and fd_filestat_set_size on a standard stream through preview1 must answer \
             EINVAL, and a call about its position ESPIPE, as Linux answers for a pipe"
        );
    }

    /// A file whose Filesystem refuses to close it.
    struct RefusesClose;

    impl FileOperations for RefusesClose {
        fn flush(&mut self) -> Result<(), kobako_runtime::Errno> {
            Err(kobako_runtime::Errno::PermissionDenied)
        }
    }

    /// A file its Filesystem opens with no position.
    struct Stream;

    impl FileOperations for Stream {
        fn is_stream(&self) -> bool {
            true
        }
    }

    /// A directory holding at every name an entry of the kind it names, whose
    /// open answers what its function makes.
    struct Holds(FileType, fn() -> Box<dyn FileOperations>);

    /// A directory holding a file its Filesystem refuses to close.
    const HOLDS_REFUSES_CLOSE: Holds = Holds(FileType::RegFile, || Box::new(RefusesClose));

    impl kobako_runtime::filesystem::FileSystemType for Holds {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), kobako_runtime::Errno> {
            sb.d_make_root(kobako_runtime::vfs::Inode::new(
                &kobako_runtime::Kstat::new(kobako_runtime::FileType::Dir, 0),
                self,
            ));
            Ok(())
        }
    }

    impl InodeOperations for Holds {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, kobako_runtime::Errno> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, kobako_runtime::Errno> {
            let found = Kstat::new(self.0, 0);
            Ok(Lookup::Found(Inode::new(
                &found,
                Arc::new(Self(self.0, self.1)),
            )))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, kobako_runtime::Errno> {
            Ok((self.1)())
        }
    }

    /// The file `holds` holds, opened as `how` asks beneath a Mount that
    /// writes when `how` does.
    fn open_held(holds: Holds, how: &OpenHow) -> Descriptor {
        let access = match how.write {
            true => Access::ReadWrite,
            false => Access::Read,
        };
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(holds)).expect("the instance must stand"),
            access,
        )
        .expect("/data is a guest path");
        let start = Path::root(mount.mnt());
        Descriptor::for_file(
            vfs::do_file_open(
                &prepare_creds_within(&[], None),
                &start,
                "file",
                &flags_of(how),
            )
            .unwrap_or_else(|_| panic!("the file must open")),
        )
    }

    fn open_file() -> Descriptor {
        let mut how = OpenHow::default();
        how.read = true;
        open_held(HOLDS_REFUSES_CLOSE, &how)
    }

    // @behavior RT-176
    #[test]
    fn a_file_without_fsync_refuses_a_sync() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to sync");
        assert_eq!(
            errno(sync(&mut table, fd, false)),
            Some(Errno::Inval),
            "fd_sync on a file whose Filesystem has no fsync must answer EINVAL"
        );
    }

    // @behavior RT-129
    #[test]
    fn a_file_set_not_to_block_reports_it() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to set");
        let flags = |table: &Descriptors| {
            fdstat_get(table, fd)
                .unwrap_or_else(|_| panic!("the file must report its flags"))
                .fs_flags
        };
        assert!(
            fdstat_set_flags(&mut table, fd, Fdflags::NONBLOCK | Fdflags::SYNC).is_ok(),
            "fd_fdstat_set_flags asking not to block and to sync must succeed, as Linux's setfl"
        );
        assert_eq!(
            flags(&table),
            Fdflags::NONBLOCK,
            "fd_fdstat_get after fd_fdstat_set_flags must report not blocking and, as Linux's \
             setfl ignores it, no sync"
        );
    }

    // A pipe end is open one way only, so a stream open both ways comes from
    // a Filesystem's open.
    // @behavior RT-275
    #[test]
    fn a_stream_reports_what_it_was_opened_and_set_for() {
        let mut table = standard_streams();
        let mut how = OpenHow::default();
        how.read = true;
        how.write = true;
        let fd = table
            .push(open_held(
                Holds(FileType::RegFile, || Box::new(Stream)),
                &how,
            ))
            .expect("a descriptor to set");
        assert!(
            fdstat_set_flags(&mut table, fd, Fdflags::APPEND | Fdflags::NONBLOCK).is_ok(),
            "fd_fdstat_set_flags asking a stream to append and not to block must succeed"
        );
        let stat = fdstat_get(&table, fd).unwrap_or_else(|_| panic!("the stream must report"));
        assert_eq!(
            (
                stat.fs_flags,
                stat.fs_rights_base
                    .contains(Rights::FD_READ | Rights::FD_WRITE)
            ),
            (Fdflags::APPEND | Fdflags::NONBLOCK, true),
            "fd_fdstat_get on a stream open to read and write, set to append and not to block, \
             must report both flags and both rights, as Linux's F_GETFL reports a pipe's"
        );
    }

    // @behavior RT-276
    #[test]
    fn a_descriptor_reports_the_kind_its_file_is() {
        let mut table = standard_streams();
        let fd = table
            .push(open_held(
                Holds(FileType::Chrdev, || Box::new(RefusesClose)),
                &OpenHow::default(),
            ))
            .expect("a descriptor to ask about");
        assert_eq!(
            fdstat_get(&table, fd)
                .unwrap_or_else(|_| panic!("the device must report"))
                .fs_filetype,
            types::Filetype::CharacterDevice,
            "fd_fdstat_get on a character device opened for its path alone must report a \
             character device, as fd_filestat_get does"
        );
    }

    // wasi-testsuite's dir_fd_op_failures names these answers and writes that
    // they part from POSIX, where a directory seeks.
    // @behavior RT-184
    #[test]
    fn a_directory_refuses_a_call_about_its_position() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HOLDS_REFUSES_CLOSE)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let mut table =
            table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        assert_eq!(
            (
                errno(tell(&mut table, 3)),
                errno(seek(&mut table, 3, 0, Whence::Set)),
            ),
            (Some(Errno::Badf), Some(Errno::Badf)),
            "fd_tell and fd_seek on a directory through preview1 must answer EBADF, as \
             wasi-testsuite requires"
        );
    }

    // @behavior RT-251
    #[test]
    fn a_descriptor_reports_how_far_it_was_opened_to_sync() {
        let tmpfs: kobako_runtime::filesystem::Tmpfs = [("f", "x")].into_iter().collect();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let mut table =
            table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        let cred = table.cred().clone();
        let mut reported = |dsync, sync| {
            let mut how = OpenHow::default();
            how.write = true;
            how.dsync = dsync;
            how.sync = sync;
            let opened = vfs::do_file_open(&cred, &root, "f", &flags_of(&how))
                .unwrap_or_else(|_| panic!("the file must open"));
            let fd = table
                .push(Descriptor::for_file(opened))
                .unwrap_or_else(|| panic!("the table must have room"));
            let Ok(stat) = fdstat_get(&table, fd) else {
                panic!("the descriptor must report itself");
            };
            (
                stat.fs_flags.contains(Fdflags::DSYNC),
                stat.fs_flags.contains(Fdflags::SYNC),
            )
        };
        assert_eq!(
            (reported(true, false), reported(false, true)),
            ((true, false), (true, true)),
            "fd_fdstat_get must report DSYNC alone for a file opened O_DSYNC, and both for one \
             opened O_SYNC, whose bits hold O_DSYNC's as Linux's do"
        );
    }

    /// Allows every path, counting each it is asked about.
    #[derive(Default)]
    struct CountsAsks(std::sync::atomic::AtomicUsize);

    impl kobako_runtime::grant::Policy<kobako_runtime::mount::MountRequest<'_>> for CountsAsks {
        fn allows(&self, _request: &kobako_runtime::mount::MountRequest<'_>) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        }
    }

    // The guard sits in the Driver rather than the VFS, so only this call shows
    // that setting a descriptor's times asks it.
    // @behavior RT-201
    #[test]
    fn setting_a_descriptors_times_asks_whether_it_still_stands() {
        use std::sync::atomic::Ordering;
        let policy = Arc::new(CountsAsks::default());
        let tmpfs: kobako_runtime::filesystem::Tmpfs = [("f", "x")].into_iter().collect();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path")
        .with_policy(policy.clone());
        let root = Path::root(mount.mnt());
        let mut table =
            table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        let cred = table.cred().clone();
        let mut read = OpenHow::default();
        read.read = true;
        let opened = vfs::do_file_open(&cred, &root, "f", &flags_of(&read))
            .unwrap_or_else(|_| panic!("the file must open"));
        let fd = table
            .push(Descriptor::for_file(opened))
            .unwrap_or_else(|| panic!("the table must have room"));
        vfs::filename_unlinkat(&cred, &root, "f").unwrap_or_else(|_| panic!("the name must go"));
        policy.0.store(0, Ordering::SeqCst);
        assert_eq!(
            (
                errno(set_times(&table, fd, Fstflags::ATIM_NOW)),
                policy.0.load(Ordering::SeqCst)
            ),
            (Some(Errno::Noent), 0),
            "fd_filestat_set_times through a descriptor of a file whose name is gone, beneath a \
             Policy, must answer ENOENT without asking the Policy about the path it stood at"
        );
    }

    // @behavior RT-186 RT-187
    #[test]
    fn setting_a_descriptors_times_answers_as_a_paths_do() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HOLDS_REFUSES_CLOSE)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let table = table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        assert_eq!(
            (
                errno(set_times(&table, 3, Fstflags::MTIM)),
                errno(set_times(&table, 3, Fstflags::empty())),
                errno(set_times(&table, 3, Fstflags::MTIM | Fstflags::MTIM_NOW)),
            ),
            (Some(Errno::Rofs), None, Some(Errno::Inval)),
            "fd_filestat_set_times beneath a read-only Mount through preview1 must answer EROFS, \
             a call asking no time must succeed, and one asking a time and now EINVAL"
        );
    }

    // @behavior RT-185
    #[test]
    fn a_path_call_starting_from_a_file_is_not_a_directory() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to start from");
        let mut bytes = *b"a";
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 1);
        assert_eq!(
            errno(create_directory(&table, &mem, fd, GuestPtr::new((0, 1)))),
            Some(Errno::Notdir),
            "path_create_directory starting from a file through preview1 must answer ENOTDIR, as \
             Linux refuses a walk starting there"
        );
    }

    // Linux reads the flags and the path before it looks at where a walk starts,
    // so a Driver checking the start first would hide each of these.
    // @behavior RT-225 RT-230 RT-231 RT-232
    #[test]
    fn a_call_no_walk_can_take_answers_before_where_it_starts() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to start from");
        let mut bytes = vec![b'a'; PATH_MAX];
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 2 * PATH_MAX);
        let call = PathOpen {
            dirflags: types::Lookupflags::empty(),
            oflags: types::Oflags::CREAT | types::Oflags::DIRECTORY,
            rights_base: Rights::FD_READ,
            fdflags: Fdflags::empty(),
        };
        let path = |len| GuestPtr::new((0, len));
        assert_eq!(
            (
                errno(path_open(&mut table, &mem, fd, path(1), &call)),
                errno(create_directory(&table, &mem, fd, path(0))),
                errno(create_directory(&table, &mem, fd, path(PATH_MAX as u32))),
                errno(utimensat(0, 0, Fstflags::empty(), |times| path_set_times(
                    &table,
                    &mem,
                    fd,
                    types::Lookupflags::empty(),
                    path(1),
                    times
                ))),
            ),
            (
                Some(Errno::Inval),
                Some(Errno::Noent),
                Some(Errno::Nametoolong),
                None
            ),
            "path_open with O_CREAT and O_DIRECTORY, and path_create_directory of an empty and of \
             an overlong path, starting from a file through preview1 must answer EINVAL, ENOENT, \
             and ENAMETOOLONG, and path_filestat_set_times setting no time must succeed"
        );
    }

    // Linux reads the flags and the name before `path_init` takes the
    // descriptor, so a number naming nothing answers only once both are fine.
    // @behavior RT-257 RT-258
    #[test]
    fn a_call_no_walk_can_take_answers_before_the_descriptor_it_names() {
        let mut table = standard_streams();
        let mut bytes = vec![b'a'; PATH_MAX];
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 2 * PATH_MAX);
        let call = |oflags| PathOpen {
            dirflags: types::Lookupflags::empty(),
            oflags,
            rights_base: Rights::FD_READ,
            fdflags: Fdflags::empty(),
        };
        let path = |len| GuestPtr::new((0, len));
        let unopened = 99;
        let create_dir = call(types::Oflags::CREAT | types::Oflags::DIRECTORY);
        assert_eq!(
            (
                errno(path_open(&mut table, &mem, unopened, path(1), &create_dir)),
                errno(create_directory(&table, &mem, unopened, path(0))),
                errno(create_directory(
                    &table,
                    &mem,
                    unopened,
                    path(PATH_MAX as u32)
                )),
                errno(create_directory(&table, &mem, unopened, path(1))),
            ),
            (
                Some(Errno::Inval),
                Some(Errno::Noent),
                Some(Errno::Nametoolong),
                Some(Errno::Badf)
            ),
            "path_open with O_CREAT and O_DIRECTORY, and path_create_directory of an empty and of \
             an overlong path, from a descriptor number naming nothing through preview1 must \
             answer EINVAL, ENOENT, and ENAMETOOLONG, and a path a walk can take EBADF"
        );
    }

    // Standard output is a pipe's end, so a walk from it answers ENOTDIR; the
    // second path is wrong too, so only the order picks which answers.
    // @behavior RT-286
    #[test]
    fn a_call_naming_two_paths_answers_for_its_first_before_its_second() {
        let table = standard_streams();
        let mut bytes = vec![b'a'; PATH_MAX];
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 2 * PATH_MAX);
        let path = |len| GuestPtr::new((0, len));
        let (stdout, unopened) = (1, 99);
        assert_eq!(
            (
                errno(rename(
                    &table,
                    &mem,
                    (stdout, path(1)),
                    (stdout, path(PATH_MAX as u32))
                )),
                errno(rename(&table, &mem, (stdout, path(1)), (unopened, path(1)))),
            ),
            (Some(Errno::Notdir), Some(Errno::Notdir)),
            "path_rename from standard output to a path past PATH_MAX, or to a descriptor \
             number naming nothing, through preview1 must answer ENOTDIR for the first path"
        );
    }

    // @behavior RT-287
    #[test]
    fn an_absolute_path_answers_before_the_descriptor_it_names() {
        let table = standard_streams();
        let mut bytes = b"/a".to_vec();
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 16);
        assert_eq!(
            errno(create_directory(&table, &mem, 99, GuestPtr::new((0, 2)))),
            Some(Errno::Notcapable),
            "path_create_directory of an absolute path from a descriptor number naming nothing \
             through preview1 must answer ENOTCAPABLE, as path_init looks at no descriptor"
        );
    }

    // @behavior RT-277
    #[test]
    fn a_readlink_into_no_room_is_invalid() {
        let table = standard_streams();
        let mut bytes = vec![0u8; 8];
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mut mem = GuestMemory::new(&mut memory, 16);
        assert_eq!(
            errno(readlink(
                &table,
                &mut mem,
                99,
                GuestPtr::new((0, 0)),
                GuestPtr::new(0),
                0
            )),
            Some(Errno::Inval),
            "path_readlink into a buffer of no bytes, of an empty path from a descriptor number \
             naming nothing, through preview1 must answer EINVAL, as do_readlinkat refuses the \
             size first"
        );
    }

    // Linux's `do_readlinkat` takes its name with `LOOKUP_EMPTY`, so an empty
    // path names the descriptor itself: one naming nothing answers as
    // `path_init` does, and a file, which is no link, as not found. The file
    // also shows the start need not be a directory.
    // @behavior RT-289
    #[test]
    fn a_readlink_of_an_empty_path_answers_for_its_descriptor() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to start from");
        let mut bytes = vec![0u8; 8];
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mut mem = GuestMemory::new(&mut memory, 16);
        let mut empty_from = |start| {
            errno(readlink(
                &table,
                &mut mem,
                start,
                GuestPtr::new((0, 0)),
                GuestPtr::new(0),
                8,
            ))
        };
        assert_eq!(
            (empty_from(99), empty_from(fd)),
            (Some(Errno::Badf), Some(Errno::Noent)),
            "path_readlink of an empty path through preview1 must answer EBADF from a descriptor \
             number naming nothing and ENOENT from a file, as do_readlinkat reads the descriptor \
             itself"
        );
    }

    // Linux's `utimensat` answers a call asking no time before it takes the
    // name or the descriptor, so a number naming nothing never shows.
    // @behavior RT-271
    #[test]
    fn a_call_asking_no_time_succeeds_whatever_descriptor_it_names() {
        let table = standard_streams();
        let mut bytes = *b"a";
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 1);
        let unopened = 99;
        assert_eq!(
            (
                errno(set_times(&table, unopened, Fstflags::empty())),
                errno(utimensat(0, 0, Fstflags::empty(), |times| path_set_times(
                    &table,
                    &mem,
                    unopened,
                    types::Lookupflags::empty(),
                    GuestPtr::new((0, 1)),
                    times
                ))),
            ),
            (None, None),
            "fd_filestat_set_times and path_filestat_set_times asking no time through preview1 \
             must succeed from a descriptor number naming nothing"
        );
    }

    // Linux's `do_sys_openat2` reads the flags before `FD_ADD` reserves a
    // descriptor, and the name before the walk looks at where it starts.
    // @behavior RT-259
    #[test]
    fn a_full_table_answers_after_the_flags_and_before_the_start() {
        let mut table = table_of(Vec::new(), Some(3)).expect("a table holding no Mount must make");
        let mut bytes = *b"a";
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 1);
        let call = |oflags| PathOpen {
            dirflags: types::Lookupflags::empty(),
            oflags,
            rights_base: Rights::FD_READ,
            fdflags: Fdflags::empty(),
        };
        let unopened = 99;
        let path = GuestPtr::new((0, 1));
        assert_eq!(
            (
                errno(path_open(
                    &mut table,
                    &mem,
                    unopened,
                    path,
                    &call(types::Oflags::CREAT | types::Oflags::DIRECTORY)
                )),
                errno(path_open(
                    &mut table,
                    &mem,
                    unopened,
                    path,
                    &call(types::Oflags::empty())
                )),
            ),
            (Some(Errno::Inval), Some(Errno::Mfile)),
            "path_open with no descriptor left through preview1 must answer EINVAL for O_CREAT \
             with O_DIRECTORY, and EMFILE from a descriptor number naming nothing"
        );
    }

    /// A directory listing `ok` and then `bad`, whatever `bad` is.
    struct ListsNames(&'static str);

    impl kobako_runtime::filesystem::FileSystemType for ListsNames {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), kobako_runtime::Errno> {
            sb.d_make_root(kobako_runtime::vfs::Inode::new(
                &kobako_runtime::Kstat::new(kobako_runtime::FileType::Dir, 0),
                self,
            ));
            Ok(())
        }
    }

    impl InodeOperations for ListsNames {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, kobako_runtime::Errno> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, kobako_runtime::Errno> {
            Ok(Box::new(Self(self.0)))
        }
    }

    impl FileOperations for ListsNames {
        fn iterate_shared(
            &mut self,
            dentry: &Dentry,
            ctx: &mut vfs::DirContext<'_>,
        ) -> Result<(), kobako_runtime::Errno> {
            let entries = vec![
                vfs::DirEntry::new("ok", FileType::RegFile),
                vfs::DirEntry::new(self.0, FileType::RegFile),
            ];
            kobako_runtime::filesystem::Rdir::default().iterate(dentry, ctx, || Ok(entries))
        }
    }

    /// Each whole `dirent` in `bytes`, as its name and its `d_next`.
    fn dirents(bytes: &[u8]) -> Vec<(String, u64)> {
        let header = types::Dirent::guest_size() as usize;
        let mut seen = Vec::new();
        let mut at = 0;
        while at + header <= bytes.len() {
            let field = |from: usize, len: usize| &bytes[at + from..at + from + len];
            let d_next = u64::from_le_bytes(field(0, 8).try_into().expect("eight bytes"));
            let namlen = u32::from_le_bytes(field(16, 4).try_into().expect("four bytes")) as usize;
            if at + header + namlen > bytes.len() {
                break;
            }
            let name = String::from_utf8_lossy(field(header, namlen)).into_owned();
            seen.push((name, d_next));
            at += header + namlen;
        }
        seen
    }

    // The first buffer holds `.` and `..` exactly, so `..` learns where the
    // listing goes on only from the entry refused after it, as `filldir64`
    // writes `prev->d_off`, and the last entry of the second from where the
    // listing stopped, as `getdents64` sets it from `f_pos`.
    // @behavior RT-182
    #[test]
    fn each_listed_entry_names_where_the_listing_goes_on() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(ListsNames("bb"))).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let mut table =
            table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        let header = types::Dirent::guest_size();
        let dots = 2 * header + 3;
        let mut bytes = vec![0u8; 256];
        let used = {
            let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
            let mut mem = GuestMemory::new(&mut memory, 256);
            let first = readdir(&mut table, &mut mem, 3, GuestPtr::new(0), dots, 0);
            let rest = readdir(&mut table, &mut mem, 3, GuestPtr::new(128), 128, 2);
            (first.ok(), rest.ok())
        };
        let entry = |name: &str, d_next| (name.to_string(), d_next);
        assert_eq!(
            (
                used,
                dirents(&bytes[..dots as usize]),
                dirents(&bytes[128..128 + 2 * (header as usize + 2)])
            ),
            (
                (Some(dots), Some(2 * (header + 2))),
                vec![entry(".", 1), entry("..", 2)],
                vec![entry("ok", 3), entry("bb", 4)],
            ),
            "fd_readdir through preview1 must give each whole entry the cookie a listing goes on \
             from past it, the one filling the buffer included, and the last the position the \
             listing stopped at"
        );
    }

    // A built-in Filesystem cannot hold such a name, so only one that hands it
    // over shows the listing refuses it; Linux's `getdents64` answers what came
    // before it and leaves the name to the next call, whose cookie stands on it.
    // @behavior RT-212 RT-269
    #[test]
    fn a_listing_stops_at_a_name_no_directory_can_hold() {
        let listed = |bad: &'static str| {
            let mount = Mount::new(
                "/data",
                Filesystem::new(Arc::new(ListsNames(bad))).expect("the instance must stand"),
                Access::Read,
            )
            .expect("/data is a guest path");
            let mut table =
                table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
            let mut bytes = vec![0u8; 256];
            let (first, next) = {
                let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
                let mut mem = GuestMemory::new(&mut memory, 256);
                let buf = GuestPtr::new(0);
                let first = readdir(&mut table, &mut mem, 3, buf, 256, 0).ok();
                (first, errno(readdir(&mut table, &mut mem, 3, buf, 256, 3)))
            };
            let handed_over = bytes.windows(bad.len().max(1)).any(|w| w == bad.as_bytes());
            (
                first.is_some_and(|used| used > 0),
                next,
                handed_over && !bad.is_empty(),
            )
        };
        assert_eq!(
            (listed("../escape"), listed("")),
            ((true, Some(Errno::Io), false), (true, Some(Errno::Io), false)),
            "fd_readdir through preview1 of a Filesystem listing `ok` and then a name with a `/` or \
             an empty one must hand over what came before it, and answer EIO from where it stands, \
             handing over no such name"
        );
    }

    /// A directory that would make any link it is asked to.
    struct MakesLinks;

    impl kobako_runtime::filesystem::FileSystemType for MakesLinks {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), kobako_runtime::Errno> {
            sb.d_make_root(kobako_runtime::vfs::Inode::new(
                &kobako_runtime::Kstat::new(kobako_runtime::FileType::Dir, 0),
                self,
            ));
            Ok(())
        }
    }

    impl InodeOperations for MakesLinks {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, kobako_runtime::Errno> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, kobako_runtime::Errno> {
            Ok(Lookup::Absent)
        }

        fn symlink(&self, _dentry: &Dentry, _target: &str) -> Result<(), kobako_runtime::Errno> {
            Ok(())
        }
    }

    // No built-in Filesystem makes a link, so only one that would shows the
    // call stopped here.
    // @behavior RT-202
    #[test]
    fn a_link_to_an_absolute_path_is_not_made() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(MakesLinks)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let table = table_of(vec![mount], None).unwrap_or_else(|_| panic!("the table must make"));
        let mut bytes = *b"/trelative";
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 64);
        let (absolute, relative, name) = (
            GuestPtr::new((0, 2)),
            GuestPtr::new((1, 1)),
            GuestPtr::new((2, 8)),
        );
        assert_eq!(
            (
                errno(symlink(&table, &mem, absolute, 3, name)),
                errno(symlink(&table, &mem, relative, 3, name)),
            ),
            (Some(Errno::Perm), None),
            "path_symlink through preview1 to an absolute target must answer EPERM, as \
             wasi-testsuite requires, and to a relative one must reach the Filesystem"
        );
    }

    // The replaced file refuses its close, so only a renumber that moves the
    // source before closing, and ignores that answer, succeeds.
    // @behavior RT-114
    #[test]
    fn renumbering_onto_an_open_file_replaces_it_whatever_its_close_answers() {
        let mut table = standard_streams();
        let from = table.push(open_file()).expect("a descriptor to move");
        let to = table.push(open_file()).expect("a descriptor to move onto");
        assert_eq!(
            (
                errno(renumber(&mut table, from, to)),
                table.get(from).is_none(),
                table.get(to).is_some()
            ),
            (None, true, true),
            "fd_renumber through preview1 onto a file whose Filesystem refuses the close must \
             succeed, the source now standing at the target and its own number free, as Linux's \
             do_dup2 ignores filp_close of the file it replaced"
        );
    }

    // @behavior RT-309
    #[test]
    fn renumbering_onto_a_number_not_open_answers_ebadf_and_keeps_the_source() {
        let mut table = standard_streams();
        let from = table.push(open_file()).expect("a descriptor to move");
        let to = from + 1;
        assert_eq!(
            (
                errno(renumber(&mut table, from, to)),
                table.get(from).is_some(),
                table.get(to).is_none()
            ),
            (Some(Errno::Badf), true, true),
            "fd_renumber through preview1 onto a number nothing stands at must answer EBADF and \
             leave the source at its own number, as wasmtime-wasi replaces only an open one"
        );
    }
}
