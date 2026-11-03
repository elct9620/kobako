//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! The runtime's open file decides each answer, so this keeps only what
//! preview1 itself defines: which file a number stands for, its rights, a
//! preopen's name, and what wasi-testsuite asks of a directory's position.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{FileType, FilesystemError, Iattr, Metadata, SetTime};
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

/// What a file opened beneath a preopen may hold. Reading and writing are
/// both inherited whatever the Mount's access, since wasi-libc intersects
/// them before asking: a read-only Mount answers a write itself, as `ROFS`.
fn preopen_inheriting() -> Rights {
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

fn descriptor(table: &Descriptors, fd: u32) -> Answer<&Descriptor> {
    table.get(fd).ok_or_else(|| Errno::Badf.into())
}

/// The open file `fd` stands for.
fn file(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match table.get_mut(fd) {
        Some(descriptor) => Ok(&mut descriptor.file),
        None => Err(Errno::Badf.into()),
    }
}

/// The open file `fd` stands for, in a call about its position. wasi-testsuite
/// has a directory refuse one, where Linux would seek it.
fn seekable_file(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match file(table, fd)? {
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
        FileType::Directory => Filetype::Directory,
        FileType::Symlink => Filetype::SymbolicLink,
        FileType::File => Filetype::RegularFile,
        FileType::Socket => Filetype::SocketStream,
        FileType::BlockDevice => Filetype::BlockDevice,
        FileType::CharDevice => Filetype::CharacterDevice,
        _ => Filetype::Unknown,
    }
}

/// A `filestat` for what a Filesystem reports a path names, as the guest
/// holding `table` sees it, at time zero where it keeps no time.
pub(crate) fn to_filestat(table: &Descriptors, metadata: Metadata) -> types::Filestat {
    let metadata = table.profile().guest_metadata(metadata);
    types::Filestat {
        dev: metadata.dev,
        ino: metadata.ino,
        filetype: to_filetype(metadata.file_type),
        nlink: metadata.nlink,
        size: metadata.len,
        atim: to_timestamp(metadata.accessed),
        mtim: to_timestamp(metadata.modified),
        ctim: to_timestamp(metadata.changed),
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

/// The times `flags` ask to set, `None` where they ask none, as `utimensat`
/// returns before it looks at anything. Setting a timestamp and setting it to
/// now are exclusive: asking both is `INVAL`.
pub(crate) fn to_iattr(
    atim: types::Timestamp,
    mtim: types::Timestamp,
    flags: Fstflags,
) -> Answer<Option<Iattr>> {
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
    Ok(file(table, fd)?.fadvise(offset, len)?)
}

pub(crate) fn allocate(table: &mut Descriptors, fd: u32, offset: u64, len: u64) -> Answer {
    Ok(file(table, fd)?.fallocate(offset, len)?)
}

pub(crate) fn close(table: &mut Descriptors, fd: u32) -> Answer {
    Ok(table.remove(fd).ok_or(Errno::Badf)?.file.close()?)
}

pub(crate) fn sync(table: &mut Descriptors, fd: u32, datasync: bool) -> Answer {
    Ok(file(table, fd)?.fsync(datasync)?)
}

pub(crate) fn fdstat_get(table: &Descriptors, fd: u32) -> Answer<types::Fdstat> {
    let descriptor = descriptor(table, fd)?;
    let file = &descriptor.file;
    let mut fs_flags = Fdflags::empty();
    fs_flags.set(Fdflags::APPEND, file.append());
    fs_flags.set(Fdflags::NONBLOCK, file.nonblock());
    fs_flags.set(Fdflags::DSYNC, file.dsync() || file.sync());
    fs_flags.set(Fdflags::SYNC, file.sync());
    let (fs_rights_base, fs_rights_inheriting) = if file.is_stream() {
        let mut rights = Rights::empty();
        rights.set(Rights::FD_READ, file.readable());
        rights.set(Rights::FD_WRITE, file.writable());
        (rights, rights)
    } else if descriptor.preopen.is_some() {
        (preopen_rights(), preopen_inheriting())
    } else if file.is_directory() {
        let rights = Rights::all()
            - Rights::FD_WRITE
            - Rights::FD_SEEK
            - Rights::FD_FILESTAT_SET_SIZE
            - Rights::PATH_FILESTAT_SET_SIZE;
        (rights, rights)
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
    Ok(types::Fdstat {
        fs_filetype: file.kind().map_or(Filetype::Unknown, to_filetype),
        fs_flags,
        fs_rights_base,
        fs_rights_inheriting,
    })
}

pub(crate) fn fdstat_set_flags(table: &mut Descriptors, fd: u32, flags: Fdflags) -> Answer {
    file(table, fd)?.setfl(
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

pub(crate) fn filestat_set_size(table: &mut Descriptors, fd: u32, size: u64) -> Answer {
    Ok(file(table, fd)?.truncate(size)?)
}

pub(crate) fn filestat_set_times(
    table: &Descriptors,
    fd: u32,
    atim: types::Timestamp,
    mtim: types::Timestamp,
    flags: Fstflags,
) -> Answer {
    // Asking no time does nothing, so Linux's `utimensat` answers before it
    // takes the descriptor.
    let Some(times) = to_iattr(atim, mtim, flags)? else {
        return Ok(());
    };
    let file = &descriptor(table, fd)?.file;
    Ok(retry_eagain(|| {
        table.walk_guard().check(file.path())?;
        vfs::do_utimes_fd(table.cred(), file.path(), &times)
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
    let file = file(table, fd)?;
    file.may_pread(offset)?;
    Ok(file.pread(mem.bytes_mut(buf)?, offset)? as u32)
}

pub(crate) fn prestat_get(table: &Descriptors, fd: u32) -> Answer<types::Prestat> {
    match &descriptor(table, fd)?.preopen {
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
    match &descriptor(table, fd)?.preopen {
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
    let file = file(table, fd)?;
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
    let file = file(table, fd)?;
    file.may_read()?;
    // Bounded before the file is asked, as a trap takes no bytes from a pipe.
    Ok(file.read(mem.bytes_mut(buf)?)? as u32)
}

/// The `dirent` header ahead of one entry's name.
fn to_dirent_header(emit: &DirEmit<'_>, d_ino: u64) -> Answer<Vec<u8>> {
    let mut header = vec![0; types::Dirent::guest_size() as usize];
    WitxMemory::Unshared(&mut header).write(
        GuestPtr::new(0),
        types::Dirent {
            d_next: emit.next,
            d_ino,
            d_namlen: emit.name.len() as u32,
            d_type: to_filetype(emit.file_type),
        },
    )?;
    Ok(header)
}

/// One entry as a preview1 `dirent` at `used` bytes into the buffer, cut off
/// where it ends, answering whether the buffer has room left.
fn write_dirent(
    mem: &mut GuestMemory<'_, '_>,
    buf: (GuestPtr<u8>, u32),
    used: &mut u32,
    profile: Profile,
    emit: &DirEmit<'_>,
) -> Answer<bool> {
    let (start, capacity) = buf;
    let header = to_dirent_header(emit, profile.guest_ino(emit.identity))?;
    for part in [&header[..], emit.name.as_bytes()] {
        let room = (capacity - *used) as usize;
        let take = part.len().min(room);
        if take > 0 {
            mem.write_bytes(start.add(*used)?, &part[..take])?;
        }
        *used += take as u32;
        if *used == capacity {
            return Ok(false);
        }
    }
    Ok(true)
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
    let dir = file(table, fd)?;
    // preview1 names where a listing goes on by its cookie, as wasi-libc's
    // `seekdir` moves a directory's position before `getdents`; a listing going
    // on from where the last one stopped needs no seek.
    if cookie != dir.pos() {
        let cookie = i64::try_from(cookie).map_err(|_| FilesystemError::InvalidInput)?;
        dir.llseek(cookie, Whence::Set)?;
    }
    let mut used: u32 = 0;
    let mut failed = None;
    let mut actor = |emit: &DirEmit<'_>| {
        let written = verify_dirent_name(emit.name)
            .map_err(Into::into)
            .and_then(|()| write_dirent(mem, (start, capacity), &mut used, profile, emit));
        match written {
            Ok(room) => room,
            Err(error) => {
                failed = Some(error);
                false
            }
        }
    };
    dir.iterate_dir(&mut DirContext::new(0, &mut actor))?;
    // What was listed answers before a name the listing stopped at, which the
    // next call meets first, as Linux's `getdents64` answers `buf.error`.
    match failed {
        Some(error) if used == 0 => Err(error),
        _ => Ok(used),
    }
}

/// preview1 renumbering closes `to` as `fd_close` does, answering a close
/// its Filesystem refuses, and moves `from` onto its number; both must
/// already be open.
pub(crate) fn renumber(table: &mut Descriptors, from: u32, to: u32) -> Answer {
    if table.get(from).is_none() || table.get(to).is_none() {
        return Err(Errno::Badf.into());
    }
    if from == to {
        return Ok(());
    }
    close(table, to)?;
    let moved = table.remove(from).ok_or(Errno::Badf)?;
    table.place(to, moved);
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
    Ok(seekable_file(table, fd)?.llseek(0, Whence::Cur)?)
}

pub(crate) fn write(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    ciovs: types::CiovecArray,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_ciovec(ciovs)?;
    let file = file(table, fd)?;
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
                FileType::Socket,
                FileType::BlockDevice,
                FileType::CharDevice,
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
        seek, sync, tell, Answer, Errno, Fdflags, Fstflags,
    };
    use crate::descriptors::{Descriptor, Descriptors};
    use crate::preview1::memory::GuestMemory;
    use crate::preview1::path::{
        create_directory, filestat_set_times as path_set_times, open as path_open, readlink,
        rename, symlink, OpenFlags,
    };
    use crate::preview1::types::{self, Rights, Whence};
    use kobako_runtime::filesystem::{create_pipe_files, PipeInodeInfo};
    use kobako_runtime::filesystem::{
        FileOperations, FileType, Filesystem, FilesystemError, InodeOperations, Lookup, Metadata,
    };
    use kobako_runtime::mount::{Access, Mount};
    use kobako_runtime::profile::Profile;
    use kobako_runtime::vfs::{self, Dentry, Inode, OpenHow, Path, PATH_MAX};
    use wiggle::GuestPtr;

    /// What `do_file_open` takes for an open `how` asks.
    fn flags_of(how: &OpenHow) -> vfs::OpenFlags {
        vfs::build_open_flags(how).expect("the flags must mean an open")
    }

    /// A table holding only the three standard streams.
    fn standard_streams() -> Descriptors {
        Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            Vec::new(),
            None,
            None,
            Profile::Hermetic,
        )
        .expect("a table holding no Mount must make")
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
        fn flush(&mut self) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
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
    const HOLDS_REFUSES_CLOSE: Holds = Holds(FileType::File, || Box::new(RefusesClose));

    impl kobako_runtime::filesystem::FileSystemType for Holds {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            kobako_runtime::filesystem::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for Holds {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let found = Metadata::new(self.0, 0);
            Ok(Lookup::Found(Inode::new(
                &found,
                Arc::new(Self(self.0, self.1)),
            )))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
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
            vfs::do_file_open(&Arc::default(), &start, "file", &flags_of(how))
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
            .push(open_held(Holds(FileType::File, || Box::new(Stream)), &how))
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
        let mut how = OpenHow::default();
        how.read = true;
        let fd = table
            .push(open_held(
                Holds(FileType::CharDevice, || Box::new(RefusesClose)),
                &how,
            ))
            .expect("a descriptor to ask about");
        assert_eq!(
            fdstat_get(&table, fd)
                .unwrap_or_else(|_| panic!("the device must report"))
                .fs_filetype,
            types::Filetype::CharacterDevice,
            "fd_fdstat_get on a character device a Filesystem opens like any file must report \
             a character device, as fd_filestat_get does"
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
        let mut table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
            None,
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("the table must make"));
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
        let mut table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
            None,
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("the table must make"));
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

    impl kobako_runtime::mount::Policy for CountsAsks {
        fn allows(&self, _path: &str, _access: Access) -> bool {
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
        let mut table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
            None,
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("the table must make"));
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
                errno(filestat_set_times(&table, fd, 0, 0, Fstflags::ATIM_NOW)),
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
        let table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
            None,
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("the table must make"));
        assert_eq!(
            (
                errno(filestat_set_times(&table, 3, 0, 0, Fstflags::MTIM)),
                errno(filestat_set_times(&table, 3, 0, 0, Fstflags::empty())),
                errno(filestat_set_times(
                    &table,
                    3,
                    0,
                    0,
                    Fstflags::MTIM | Fstflags::MTIM_NOW
                )),
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
        let flags = OpenFlags {
            dirflags: types::Lookupflags::empty(),
            oflags: types::Oflags::CREAT | types::Oflags::DIRECTORY,
            rights_base: Rights::FD_READ,
            fdflags: Fdflags::empty(),
        };
        let path = |len| GuestPtr::new((0, len));
        assert_eq!(
            (
                errno(path_open(&mut table, &mem, fd, path(1), &flags)),
                errno(create_directory(&table, &mem, fd, path(0))),
                errno(create_directory(&table, &mem, fd, path(PATH_MAX as u32))),
                errno(path_set_times(
                    &table,
                    &mem,
                    fd,
                    types::Lookupflags::empty(),
                    path(1),
                    None
                )),
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
        let flags = |oflags| OpenFlags {
            dirflags: types::Lookupflags::empty(),
            oflags,
            rights_base: Rights::FD_READ,
            fdflags: Fdflags::empty(),
        };
        let path = |len| GuestPtr::new((0, len));
        let unopened = 99;
        let create_dir = flags(types::Oflags::CREAT | types::Oflags::DIRECTORY);
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
                errno(filestat_set_times(
                    &table,
                    unopened,
                    0,
                    0,
                    Fstflags::empty()
                )),
                errno(path_set_times(
                    &table,
                    &mem,
                    unopened,
                    types::Lookupflags::empty(),
                    GuestPtr::new((0, 1)),
                    None
                )),
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
        let mut table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            Vec::new(),
            Some(3),
            None,
            Profile::Hermetic,
        )
        .expect("a table holding no Mount must make");
        let mut bytes = *b"a";
        let mut memory = wiggle::GuestMemory::Unshared(&mut bytes);
        let mem = GuestMemory::new(&mut memory, 1);
        let flags = |oflags| OpenFlags {
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
                    &flags(types::Oflags::CREAT | types::Oflags::DIRECTORY)
                )),
                errno(path_open(
                    &mut table,
                    &mem,
                    unopened,
                    path,
                    &flags(types::Oflags::empty())
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
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            kobako_runtime::filesystem::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for ListsNames {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok(Box::new(Self(self.0)))
        }
    }

    impl FileOperations for ListsNames {
        fn iterate_shared(
            &mut self,
            dentry: &Dentry,
            ctx: &mut vfs::DirContext<'_>,
        ) -> Result<(), FilesystemError> {
            let entries = [
                vfs::DirEntry::new("ok", FileType::File),
                vfs::DirEntry::new(self.0, FileType::File),
            ];
            vfs::dir_emit_entries(dentry, ctx, &entries);
            Ok(())
        }
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
            let mut table = Descriptors::new(
                create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
                create_pipe_files(&PipeInodeInfo::new(1)).1,
                create_pipe_files(&PipeInodeInfo::new(1)).1,
                vec![mount],
                None,
                None,
                Profile::Hermetic,
            )
            .unwrap_or_else(|_| panic!("the table must make"));
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
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            kobako_runtime::filesystem::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for MakesLinks {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            Ok(Lookup::Absent)
        }

        fn symlink(&self, _dentry: &Dentry, _target: &str) -> Result<(), FilesystemError> {
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
        let table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
            None,
            None,
            Profile::Hermetic,
        )
        .unwrap_or_else(|_| panic!("the table must make"));
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

    // @behavior RT-114
    #[test]
    fn renumbering_onto_an_open_file_closes_it_first() {
        let mut table = standard_streams();
        let from = table.push(open_file()).expect("a descriptor to move");
        let to = table.push(open_file()).expect("a descriptor to move onto");
        assert_eq!(
            errno(renumber(&mut table, from, to)),
            Some(Errno::Acces),
            "fd_renumber onto a file whose Filesystem refuses the close must answer that refusal"
        );
        assert!(
            table.get(to).is_none() && table.get(from).is_some(),
            "a refused close through fd_renumber must leave the target closed and the source in place"
        );
    }
}
