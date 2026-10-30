//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! The runtime's open file decides each answer, so this keeps only what
//! preview1 itself defines: which file a number stands for, its rights, a
//! preopen's name, and what wasi-testsuite asks of a directory's position.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{FileType, Iattr, Metadata, SetTime};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::{self, DirContext, DirEmit, File, Whence};
use wiggle::{GuestMemory as WitxMemory, GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
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
fn seekable(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match file(table, fd)? {
        file if file.is_directory() => Err(Errno::Badf.into()),
        file => Ok(file),
    }
}

/// A call any file or directory answers, which preview1 refuses for
/// stdio.
fn file_or_directory(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)?.file.is_stream() {
        true => Err(Errno::Badf.into()),
        false => Ok(()),
    }
}

fn to_filetype(file_type: FileType) -> Filetype {
    match file_type {
        FileType::Directory => Filetype::Directory,
        FileType::Symlink => Filetype::SymbolicLink,
        FileType::File => Filetype::RegularFile,
        _ => Filetype::Unknown,
    }
}

/// A `filestat` for what a Filesystem reports a path names, as the guest
/// holding `table` sees it: named once where it keeps no count of names, and
/// at time zero where it keeps no time.
pub(crate) fn filestat(table: &Descriptors, metadata: Metadata) -> types::Filestat {
    let metadata = table.profile().guest_metadata(metadata);
    types::Filestat {
        dev: metadata.dev,
        ino: metadata.ino,
        filetype: to_filetype(metadata.file_type),
        nlink: metadata.nlink.unwrap_or(1),
        size: metadata.len,
        atim: timestamp(metadata.accessed),
        mtim: timestamp(metadata.modified),
        ctim: timestamp(metadata.changed),
    }
}

/// `time` as a preview1 `timestamp`, nanoseconds since the Unix epoch;
/// zero for none, and the largest timestamp for a time past it.
fn timestamp(time: Option<SystemTime>) -> u64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
        })
}

/// The times `flags` ask to set, `None` where they ask none, as `utimensat`
/// returns before it looks at anything. Setting a timestamp and setting it to
/// now are exclusive: asking both is `INVAL`.
pub(crate) fn times_asked(
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

pub(crate) fn sync(table: &mut Descriptors, fd: u32) -> Answer {
    Ok(file(table, fd)?.fsync()?)
}

pub(crate) fn fdstat_get(table: &Descriptors, fd: u32) -> Answer<types::Fdstat> {
    let descriptor = descriptor(table, fd)?;
    let file = &descriptor.file;
    let mut fs_flags = Fdflags::empty();
    if !file.is_stream() {
        fs_flags.set(Fdflags::APPEND, file.append());
        fs_flags.set(Fdflags::NONBLOCK, file.nonblock());
        fs_flags.set(Fdflags::SYNC, file.sync());
    }
    let (fs_filetype, fs_rights_base, fs_rights_inheriting) = if file.is_stream() {
        let rights = match file.readable() {
            true => Rights::FD_READ,
            false => Rights::FD_WRITE,
        };
        (Filetype::Unknown, rights, rights)
    } else if descriptor.preopen.is_some() {
        (Filetype::Directory, preopen_rights(), preopen_inheriting())
    } else if file.is_directory() {
        let rights = Rights::all()
            - Rights::FD_WRITE
            - Rights::FD_SEEK
            - Rights::FD_FILESTAT_SET_SIZE
            - Rights::PATH_FILESTAT_SET_SIZE;
        (Filetype::Directory, rights, rights)
    } else {
        let mut rights = Rights::all();
        if !file.readable() {
            rights -= Rights::FD_READ | Rights::FD_READDIR;
        }
        if !file.writable() {
            rights -= Rights::FD_WRITE;
        }
        (Filetype::RegularFile, rights, rights)
    };
    Ok(types::Fdstat {
        fs_filetype,
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
    file_or_directory(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn filestat_get(table: &Descriptors, fd: u32) -> Answer<types::Filestat> {
    Ok(filestat(table, descriptor(table, fd)?.file.getattr()?))
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
    let times = times_asked(atim, mtim, flags)?;
    let file = &descriptor(table, fd)?.file;
    match times {
        Some(times) => Ok(vfs::vfs_utimes(table.cred(), file.path(), &times)?),
        None => Ok(()),
    }
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
fn dirent_header(emit: &DirEmit<'_>, d_ino: u64) -> Answer<Vec<u8>> {
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
    let header = dirent_header(emit, profile.guest_ino(emit.identity))?;
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
    let mut used: u32 = 0;
    let mut failed = None;
    let mut actor =
        |emit: &DirEmit<'_>| match write_dirent(mem, (start, capacity), &mut used, profile, emit) {
            Ok(room) => room,
            Err(error) => {
                failed = Some(error);
                false
            }
        };
    dir.iterate_dir(&mut DirContext::new(cookie, &mut actor))?;
    match failed {
        Some(error) => Err(error),
        None => Ok(used),
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
    Ok(seekable(table, fd)?.llseek(offset, whence)?)
}

pub(crate) fn tell(table: &mut Descriptors, fd: u32) -> Answer<u64> {
    Ok(seekable(table, fd)?.llseek(0, Whence::Cur)?)
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

    use super::{
        fdstat_get, fdstat_set_flags, filestat_set_size, filestat_set_times, renumber, seek, sync,
        tell, Answer, Errno, Fdflags, Fstflags,
    };
    use crate::descriptors::{Descriptor, Descriptors};
    use crate::preview1::memory::GuestMemory;
    use crate::preview1::path::create_directory;
    use crate::preview1::types::Whence;
    use kobako_runtime::filesystem::{
        FileOperations, FileType, FilesystemError, InodeOperations, Lookup, Metadata, OpenOptions,
    };
    use kobako_runtime::mount::{Access, Mount};
    use kobako_runtime::profile::Profile;
    use kobako_runtime::vfs::{
        self, create_pipe_files, Dentry, Filesystem, Inode, OpenHow, Path, PipeInodeInfo,
    };
    use wiggle::GuestPtr;

    /// A table holding only the three standard streams.
    fn standard_streams() -> Descriptors {
        Descriptors::new(
            create_pipe_files(&PipeInodeInfo::filled(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            Vec::new(),
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
                errno(sync(&mut table, 1)),
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

    /// A directory holding that one file at every name.
    struct HoldsRefusesClose;

    impl InodeOperations for HoldsRefusesClose {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>> {
            let found = Metadata::new(FileType::File, 0);
            Some(Ok(Lookup::Found(Inode::new(&found, Arc::new(Self)))))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
            Some(Ok(Box::new(RefusesClose)))
        }
    }

    fn open_file() -> Descriptor {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HoldsRefusesClose)),
            Access::Read,
        )
        .expect("/data is a guest path");
        let mut how = OpenHow::default();
        how.read = true;
        let start = Path::root(mount.mnt());
        Descriptor::of(
            vfs::do_file_open(&Arc::default(), &start, "file", &how)
                .unwrap_or_else(|_| panic!("the file must open")),
        )
    }

    // @behavior RT-176
    #[test]
    fn a_file_without_fsync_refuses_a_sync() {
        let mut table = standard_streams();
        let fd = table.push(open_file()).expect("a descriptor to sync");
        assert_eq!(
            errno(sync(&mut table, fd)),
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

    // wasi-testsuite's dir_fd_op_failures names these answers and writes that
    // they part from POSIX, where a directory seeks.
    // @behavior RT-184
    #[test]
    fn a_directory_refuses_a_call_about_its_position() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HoldsRefusesClose)),
            Access::Read,
        )
        .expect("/data is a guest path");
        let mut table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::filled(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
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

    // @behavior RT-186 RT-187
    #[test]
    fn setting_a_descriptors_times_answers_as_a_paths_do() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(HoldsRefusesClose)),
            Access::Read,
        )
        .expect("/data is a guest path");
        let table = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::filled(Vec::new())).0,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            create_pipe_files(&PipeInodeInfo::new(1)).1,
            vec![mount],
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
