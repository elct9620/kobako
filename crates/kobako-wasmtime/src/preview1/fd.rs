//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! For stdio each answers what wasmtime-wasi's preview1 answers for the
//! same descriptor; the differential probe in `preview1` holds that. For a
//! directory or a file beneath a Mount, the runtime's open file holds the
//! position and the Filesystem answers; the rights stay here.

use std::time::{SystemTime, UNIX_EPOCH};

use kobako_runtime::filesystem::{FileType, Metadata};
use kobako_runtime::vfs::{FMode, File};
use wiggle::{GuestMemory as WitxMemory, GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use super::types::{self, Fdflags, Filetype, Fstflags, Rights};
use crate::descriptors::{Descriptor, Descriptors, OpenDirectory};

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

fn descriptor_mut(table: &mut Descriptors, fd: u32) -> Answer<&mut Descriptor> {
    table.get_mut(fd).ok_or_else(|| Errno::Badf.into())
}

/// The open file `fd` stands for; any other descriptor is `BADF`.
fn file(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match descriptor_mut(table, fd)? {
        Descriptor::File(file) => Ok(file),
        Descriptor::Directory(_) => Err(Errno::Badf.into()),
    }
}

/// A call about a position in a file: a file has one, a pipe has none, and
/// a directory is no stream at all.
fn seekable(table: &mut Descriptors, fd: u32) -> Answer<&mut File> {
    match file(table, fd)? {
        file if file.is_stream() => Err(Errno::Spipe.into()),
        file => Ok(file),
    }
}

/// An offset or size `pread`, `pwrite`, or `ftruncate` names, which Linux
/// reads as a signed `off_t` and refuses once negative.
fn refuse_negative(offset: u64) -> Answer<u64> {
    if offset > i64::MAX as u64 {
        return Err(Errno::Inval.into());
    }
    Ok(offset)
}

/// A read of `len` bytes from `offset`, refused as Linux refuses one whose
/// end would pass the largest signed offset.
fn refuse_reading_past_largest_offset(offset: u64, len: u32) -> Answer<u64> {
    if offset.saturating_add(u64::from(len)) > i64::MAX as u64 {
        return Err(Errno::Inval.into());
    }
    Ok(offset)
}

/// The part of `buf` a write at `offset` lands, as Linux cuts a write
/// short at the largest signed offset and refuses one starting there as
/// too large.
fn trim_to_largest_offset(buf: &[u8], offset: u64) -> Answer<&[u8]> {
    let room = (i64::MAX as u64).saturating_sub(offset);
    if room == 0 && !buf.is_empty() {
        return Err(Errno::Fbig.into());
    }
    Ok(&buf[..buf.len().min(usize::try_from(room).unwrap_or(usize::MAX))])
}

/// A call any file or directory answers, which preview1 refuses for
/// stdio.
fn file_or_directory(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::File(file) if file.is_stream() => Err(Errno::Badf.into()),
        Descriptor::Directory(_) | Descriptor::File(_) => Ok(()),
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
/// holding `table` sees it: named once where it keeps no count of names, at
/// time zero where it keeps no time, and with no size for a link, so
/// nothing of where it points reaches the guest.
pub(crate) fn filestat(table: &Descriptors, metadata: Metadata) -> types::Filestat {
    let metadata = table.profile().mask_times(metadata);
    types::Filestat {
        dev: 0,
        ino: 0,
        filetype: to_filetype(metadata.file_type),
        nlink: metadata.nlink.unwrap_or(1),
        size: match metadata.file_type {
            FileType::Symlink => 0,
            _ => metadata.len,
        },
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

/// Setting a timestamp and setting it to now are exclusive: asking both is
/// `INVAL`.
pub(crate) fn check_fstflags(flags: Fstflags) -> Answer {
    if flags.contains(Fstflags::ATIM | Fstflags::ATIM_NOW)
        || flags.contains(Fstflags::MTIM | Fstflags::MTIM_NOW)
    {
        return Err(Errno::Inval.into());
    }
    Ok(())
}

/// The hints preview1 offers about a file. A Filesystem that ignores them
/// is still correct, so they succeed over a range Linux takes.
pub(crate) fn advise(table: &mut Descriptors, fd: u32, offset: u64, len: u64) -> Answer {
    seekable(table, fd)?;
    refuse_negative(offset)?;
    refuse_negative(len).map(drop)
}

/// A Filesystem has no room to reserve ahead of a write, so allocating it
/// is unsupported rather than answered as done.
pub(crate) fn allocate(table: &mut Descriptors, fd: u32) -> Answer {
    seekable(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn close(table: &mut Descriptors, fd: u32) -> Answer {
    match table.remove(fd).ok_or(Errno::Badf)? {
        Descriptor::File(file) => Ok(file.close()?),
        Descriptor::Directory(dir) => Ok(dir.file.close()?),
    }
}

pub(crate) fn sync(table: &mut Descriptors, fd: u32) -> Answer {
    Ok(file(table, fd)?.fsync()?)
}

pub(crate) fn fdstat_get(table: &Descriptors, fd: u32) -> Answer<types::Fdstat> {
    let (fs_filetype, fs_flags, fs_rights_base, fs_rights_inheriting) = match descriptor(table, fd)?
    {
        Descriptor::File(file) if file.is_stream() => {
            let rights = match file.readable() {
                true => Rights::FD_READ,
                false => Rights::FD_WRITE,
            };
            (Filetype::Unknown, Fdflags::empty(), rights, rights)
        }
        Descriptor::Directory(dir) if dir.preopen.is_some() => (
            Filetype::Directory,
            Fdflags::empty(),
            preopen_rights(),
            preopen_inheriting(),
        ),
        Descriptor::Directory(_) => {
            let rights = Rights::all()
                - Rights::FD_WRITE
                - Rights::FD_SEEK
                - Rights::FD_FILESTAT_SET_SIZE
                - Rights::PATH_FILESTAT_SET_SIZE;
            (Filetype::Directory, Fdflags::empty(), rights, rights)
        }
        Descriptor::File(file) => {
            let mut rights = Rights::all();
            if !file.readable() {
                rights -= Rights::FD_READ | Rights::FD_READDIR;
            }
            if !file.writable() {
                rights -= Rights::FD_WRITE;
            }
            let flags = match file.append() {
                true => Fdflags::APPEND,
                false => Fdflags::empty(),
            };
            (Filetype::RegularFile, flags, rights, rights)
        }
    };
    Ok(types::Fdstat {
        fs_filetype,
        fs_flags,
        fs_rights_base,
        fs_rights_inheriting,
    })
}

/// Only appending can change once a file is open; the sync flags are
/// `INVAL`, as in wasmtime-wasi, and a file never stops blocking, so
/// asking it to is unsupported.
pub(crate) fn fdstat_set_flags(table: &mut Descriptors, fd: u32, flags: Fdflags) -> Answer {
    let file = file(table, fd)?;
    if flags.intersects(Fdflags::DSYNC | Fdflags::RSYNC | Fdflags::SYNC) {
        return Err(Errno::Inval.into());
    }
    if flags.contains(Fdflags::NONBLOCK) {
        return Err(Errno::Notsup.into());
    }
    file.set_append(flags.contains(Fdflags::APPEND));
    Ok(())
}

/// Rights only narrow at open, so a later change is unsupported.
pub(crate) fn fdstat_set_rights(table: &Descriptors, fd: u32) -> Answer {
    file_or_directory(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn filestat_get(table: &Descriptors, fd: u32) -> Answer<types::Filestat> {
    let metadata = match descriptor(table, fd)? {
        Descriptor::Directory(dir) => dir.file.getattr()?,
        Descriptor::File(file) => file.getattr()?,
    };
    Ok(filestat(table, metadata))
}

/// A Mount that refuses the write answers first, as wasmtime-wasi answers
/// by its preopen; a file opened only to read then answers as Linux's
/// `ftruncate` does.
pub(crate) fn filestat_set_size(table: &mut Descriptors, fd: u32, size: u64) -> Answer {
    Ok(file(table, fd)?.truncate(refuse_negative(size)?)?)
}

/// A Filesystem keeps no timestamps of the guest's choosing, so setting
/// them is unsupported rather than silently ignored.
pub(crate) fn filestat_set_times(table: &Descriptors, fd: u32, flags: Fstflags) -> Answer {
    check_fstflags(flags)?;
    file_or_directory(table, fd)?;
    Err(Errno::Notsup.into())
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
    if !file.mode().contains(FMode::PREAD) {
        return Err(Errno::Spipe.into());
    }
    if !file.readable() {
        return Err(Errno::Badf.into());
    }
    let offset = refuse_reading_past_largest_offset(refuse_negative(offset)?, buf.len())?;
    Ok(file.pread(mem.bytes_mut(buf)?, offset)? as u32)
}

pub(crate) fn prestat_get(table: &Descriptors, fd: u32) -> Answer<types::Prestat> {
    match descriptor(table, fd)? {
        Descriptor::Directory(OpenDirectory {
            preopen: Some(at), ..
        }) => Ok(types::Prestat::Dir(types::PrestatDir {
            pr_name_len: at.len() as u32,
        })),
        _ => Err(Errno::Badf.into()),
    }
}

pub(crate) fn prestat_dir_name(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    path: GuestPtr<u8>,
    path_len: u32,
) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Directory(OpenDirectory {
            preopen: Some(at), ..
        }) => {
            let name = at.as_bytes();
            if name.len() > path_len as usize {
                return Err(Errno::Nametoolong.into());
            }
            mem.write_bytes(path, name)
        }
        _ => Err(Errno::Notdir.into()),
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
    if !file.mode().contains(FMode::PWRITE) {
        return Err(Errno::Spipe.into());
    }
    // The buffer is bounded before the access is checked, as wasmtime-wasi
    // bounds it before the host refuses the write.
    let buf = mem.bytes(buf)?;
    if !file.writable() {
        return Err(Errno::Badf.into());
    }
    let offset = refuse_negative(offset)?;
    // Linux's `pwrite` ignores the offset of a file opened to append.
    let buf = match file.append() {
        true => buf,
        false => trim_to_largest_offset(buf, offset)?,
    };
    Ok(file.pwrite(buf, offset)? as u32)
}

pub(crate) fn read(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    iovs: types::IovecArray,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_iovec(iovs)?;
    let file = file(table, fd)?;
    if file.is_stream() && !file.readable() {
        return Err(Errno::Badf.into());
    }
    // Bounded before the access is checked, as wasmtime-wasi bounds it before
    // the host refuses the read of a file, and before a pipe is drained, so a
    // trap takes no bytes.
    let len = buf.len();
    let buf = mem.bytes_mut(buf)?;
    if !file.readable() {
        return Err(Errno::Badf.into());
    }
    refuse_reading_past_largest_offset(file.pos(), len)?;
    Ok(file.read(buf)? as u32)
}

/// The `dirent` header ahead of one entry's name.
fn dirent_header(d_next: u64, name: &str, filetype: Filetype) -> Answer<Vec<u8>> {
    let mut header = vec![0; types::Dirent::guest_size() as usize];
    WitxMemory::Unshared(&mut header).write(
        GuestPtr::new(0),
        types::Dirent {
            d_next,
            d_ino: 0,
            d_namlen: name.len() as u32,
            d_type: filetype,
        },
    )?;
    Ok(header)
}

/// A directory's entries as preview1 `dirent`s, `.` and `..` first, from
/// the `cookie`th on, cut off where the buffer ends. A listing from the
/// start reads the directory; one going on reads the entries that listing
/// began with, so a guest reading in small steps costs one listing, not one
/// per step.
pub(crate) fn readdir(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    start: GuestPtr<u8>,
    capacity: u32,
    cookie: u64,
) -> Answer<types::Size> {
    let dir = match descriptor_mut(table, fd)? {
        Descriptor::Directory(dir) => dir,
        _ => return Err(Errno::Badf.into()),
    };
    let skip = usize::try_from(cookie).map_err(|_| Errno::Overflow)?;
    let listing = match dir.file.take_listing() {
        Some(listing) if cookie != 0 => listing,
        _ => dir.file.iterate_dir()?,
    };
    let entries = [(".", Filetype::Directory), ("..", Filetype::Directory)]
        .into_iter()
        .chain(
            listing
                .iter()
                .map(|entry| (entry.name.as_str(), to_filetype(entry.file_type))),
        );
    let mut used: u32 = 0;
    let mut full = false;
    'entries: for (index, (name, filetype)) in entries.enumerate().skip(skip) {
        let header = dirent_header(index as u64 + 1, name, filetype)?;
        for part in [&header[..], name.as_bytes()] {
            let room = (capacity - used) as usize;
            let take = part.len().min(room);
            if take > 0 {
                mem.write_bytes(start.add(used)?, &part[..take])?;
            }
            used += take as u32;
            if used == capacity {
                full = true;
                break 'entries;
            }
        }
    }
    if full {
        dir.file.keep_listing(listing);
    }
    Ok(used)
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
    let file = seekable(table, fd)?;
    let base = match whence {
        types::Whence::Set => 0,
        types::Whence::Cur => file.pos(),
        types::Whence::End => file.getattr()?.len,
    };
    // Linux holds an offset in a signed 64-bit `off_t`, past which a seek
    // is `OVERFLOW` rather than a position a write could wrap around.
    let position = base.checked_add_signed(offset).ok_or(Errno::Inval)?;
    if position > i64::MAX as u64 {
        return Err(Errno::Overflow.into());
    }
    file.set_pos(position);
    Ok(position)
}

pub(crate) fn tell(table: &mut Descriptors, fd: u32) -> Answer<u64> {
    Ok(seekable(table, fd)?.pos())
}

pub(crate) fn write(
    table: &mut Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    ciovs: types::CiovecArray,
) -> Answer<types::Size> {
    let buf = mem.first_non_empty_ciovec(ciovs)?;
    let file = file(table, fd)?;
    if file.is_stream() && !file.writable() {
        return Err(Errno::Badf.into());
    }
    // Bounded before the access is checked, as in `pwrite`.
    let buf = mem.bytes(buf)?;
    if !file.writable() {
        return Err(Errno::Badf.into());
    }
    let buf = match file.append() {
        true => buf,
        false => trim_to_largest_offset(buf, file.pos())?,
    };
    Ok(file.write(buf)? as u32)
}

#[cfg(test)]
mod tests {
    //! Witness that renumbering onto an open file closes it as `fd_close`
    //! does, so its Filesystem learns of the close and may refuse it.
    use std::sync::Arc;

    use super::{filestat_set_size, renumber, seekable, sync, Answer, Errno};
    use crate::descriptors::{Descriptor, Descriptors};
    use kobako_runtime::filesystem::{
        FileOperations, FileType, FilesystemError, InodeOperations, Lookup, Metadata, OpenOptions,
    };
    use kobako_runtime::mount::{Access, Mount};
    use kobako_runtime::profile::Profile;
    use kobako_runtime::vfs::{
        self, create_pipe_files, Dentry, Filesystem, Inode, OpenHow, Path, PipeInodeInfo,
    };

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
                errno(seekable(&mut table, 0).map(drop)),
                errno(seekable(&mut table, 1).map(drop)),
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
        Descriptor::File(
            vfs::do_file_open(&Arc::default(), &start, "file", &how, true)
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
