//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! For stdio each answers what wasmtime-wasi's preview1 answers for the
//! same descriptor; the differential probe in `preview1` holds that. For a
//! directory or a file beneath a Mount, the Filesystem answers
//! and the descriptor's position and rights stay here.

use bytes::Bytes;
use kobako_runtime::filesystem::{Kind, Metadata};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::p2::{InputStream, OutputStream, StreamError};

use super::args::{self, FDFLAGS_APPEND, FDFLAGS_SYNCHRONIZED, RIGHTS_FD_READ, RIGHTS_FD_WRITE};
use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors, OpenFile};

/// `fdstat` / `filestat` filetypes.
const FILETYPE_UNKNOWN: u8 = 0;
const FILETYPE_DIRECTORY: u8 = 3;
const FILETYPE_REGULAR_FILE: u8 = 4;

const RIGHTS_FD_DATASYNC: u64 = 1 << 0;
const RIGHTS_FD_SEEK: u64 = 1 << 2;
const RIGHTS_FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
const RIGHTS_FD_SYNC: u64 = 1 << 4;
const RIGHTS_FD_TELL: u64 = 1 << 5;
const RIGHTS_FD_ADVISE: u64 = 1 << 7;
const RIGHTS_FD_ALLOCATE: u64 = 1 << 8;
const RIGHTS_FD_READDIR: u64 = 1 << 14;
const RIGHTS_PATH_FILESTAT_SET_SIZE: u64 = 1 << 19;
const RIGHTS_FD_FILESTAT_GET: u64 = 1 << 21;
const RIGHTS_FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
const RIGHTS_FD_FILESTAT_SET_TIMES: u64 = 1 << 23;
const RIGHTS_POLL_FD_READWRITE: u64 = 1 << 27;

/// What a preopen allows itself: every path call, as wasmtime-wasi hands
/// most userlands.
const PREOPEN_RIGHTS: u64 = (1 << 9)
    | (1 << 10)
    | (1 << 11)
    | (1 << 12)
    | (1 << 13)
    | RIGHTS_FD_READDIR
    | (1 << 15)
    | (1 << 16)
    | (1 << 17)
    | (1 << 18)
    | (1 << 20)
    | RIGHTS_FD_FILESTAT_GET
    | RIGHTS_FD_FILESTAT_SET_TIMES
    | (1 << 24)
    | (1 << 25)
    | (1 << 26);

/// What a file opened beneath a preopen may hold. Reading and writing are
/// both inherited whatever the Mount's access, since wasi-libc intersects
/// them before asking: a read-only Mount answers a write itself, as `ROFS`.
const PREOPEN_INHERITING: u64 = PREOPEN_RIGHTS
    | RIGHTS_FD_DATASYNC
    | RIGHTS_FD_READ
    | RIGHTS_FD_SEEK
    | RIGHTS_FD_FDSTAT_SET_FLAGS
    | RIGHTS_FD_SYNC
    | RIGHTS_FD_TELL
    | RIGHTS_FD_WRITE
    | RIGHTS_FD_ADVISE
    | RIGHTS_FD_ALLOCATE
    | RIGHTS_FD_FILESTAT_SET_SIZE
    | RIGHTS_POLL_FD_READWRITE;

/// The most bytes one write moves into a capture pipe at a time, the
/// preview2 `blocking-write-and-flush` limit wasmtime-wasi writes under.
const WRITE_CHUNK: usize = 4096;

/// The size of one `dirent` header ahead of its name.
const DIRENT_SIZE: u32 = 24;

fn descriptor(table: &Descriptors, fd: u32) -> Answer<&Descriptor> {
    table.get(fd).ok_or_else(|| Errno::Badf.into())
}

fn descriptor_mut(table: &mut Descriptors, fd: u32) -> Answer<&mut Descriptor> {
    table.get_mut(fd).ok_or_else(|| Errno::Badf.into())
}

/// The open file `fd` stands for; any other descriptor is `BADF`.
fn file(table: &mut Descriptors, fd: u32) -> Answer<&mut OpenFile> {
    match descriptor_mut(table, fd)? {
        Descriptor::File(file) => Ok(file),
        Descriptor::Stdin(_) | Descriptor::Channel(_) | Descriptor::Directory(_) => {
            Err(Errno::Badf.into())
        }
    }
}

/// A call that moves a stream position: a file has one, stdio is a pipe
/// with none, and a directory is no stream at all.
fn seekable(table: &mut Descriptors, fd: u32) -> Answer<&mut OpenFile> {
    match descriptor_mut(table, fd)? {
        Descriptor::File(file) => Ok(file),
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Spipe.into()),
        Descriptor::Directory(_) => Err(Errno::Badf.into()),
    }
}

/// A call any file or directory answers, which preview1 refuses for
/// stdio.
fn file_or_directory(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Directory(_) | Descriptor::File(_) => Ok(()),
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Badf.into()),
    }
}

fn write_filestat(mem: &mut GuestMemory, out: u32, filetype: u8, size: u64) -> Answer {
    GuestMemory::aligned(out, 8)?;
    mem.write(out, &[0; 64])?;
    mem.write_u8(GuestMemory::field(out, 16)?, filetype)?;
    if filetype != FILETYPE_UNKNOWN {
        mem.write_u64(GuestMemory::field(out, 24)?, 1)?;
        mem.write_u64(GuestMemory::field(out, 32)?, size)?;
    }
    Ok(())
}

fn filetype(kind: Kind) -> u8 {
    match kind {
        Kind::Directory => FILETYPE_DIRECTORY,
        _ => FILETYPE_REGULAR_FILE,
    }
}

/// A `filestat` for what a Filesystem reports a path names.
pub(crate) fn write_metadata(mem: &mut GuestMemory, out: u32, metadata: Metadata) -> Answer {
    write_filestat(mem, out, filetype(metadata.kind), metadata.len)
}

/// The hints preview1 offers about a file. A Filesystem that ignores them
/// is still correct, so they succeed.
pub(crate) fn advise(table: &mut Descriptors, fd: u32, advice: u32) -> Answer {
    args::variant(advice, args::ADVICE_MAX)?;
    file(table, fd).map(drop)
}

pub(crate) fn allocate(table: &mut Descriptors, fd: u32) -> Answer {
    file(table, fd).map(drop)
}

pub(crate) fn close(table: &mut Descriptors, fd: u32) -> Answer {
    match table.remove(fd).ok_or(Errno::Badf)? {
        Descriptor::File(open) => Ok(open.file.close()?),
        _ => Ok(()),
    }
}

pub(crate) fn sync(table: &mut Descriptors, fd: u32) -> Answer {
    Ok(file(table, fd)?.file.sync()?)
}

pub(crate) fn fdstat_get(table: &Descriptors, mem: &mut GuestMemory, fd: u32, out: u32) -> Answer {
    let (filetype, flags, base, inheriting) = match descriptor(table, fd)? {
        Descriptor::Stdin(_) => (FILETYPE_UNKNOWN, 0, RIGHTS_FD_READ, RIGHTS_FD_READ),
        Descriptor::Channel(_) => (FILETYPE_UNKNOWN, 0, RIGHTS_FD_WRITE, RIGHTS_FD_WRITE),
        Descriptor::Directory(dir) if dir.preopen => {
            (FILETYPE_DIRECTORY, 0, PREOPEN_RIGHTS, PREOPEN_INHERITING)
        }
        Descriptor::Directory(_) => {
            let rights = args::RIGHTS
                & !RIGHTS_FD_SEEK
                & !RIGHTS_FD_FILESTAT_SET_SIZE
                & !RIGHTS_PATH_FILESTAT_SET_SIZE;
            (FILETYPE_DIRECTORY, 0, rights, rights)
        }
        Descriptor::File(file) => {
            let mut rights = args::RIGHTS;
            if !file.readable {
                rights &= !RIGHTS_FD_READ & !RIGHTS_FD_READDIR;
            }
            if !file.writable {
                rights &= !RIGHTS_FD_WRITE;
            }
            let flags = if file.append {
                FDFLAGS_APPEND as u16
            } else {
                0
            };
            (FILETYPE_REGULAR_FILE, flags, rights, rights)
        }
    };
    GuestMemory::aligned(out, 8)?;
    mem.write_u8(out, filetype)?;
    mem.write_u16(GuestMemory::field(out, 2)?, flags)?;
    mem.write_u64(GuestMemory::field(out, 8)?, base)?;
    mem.write_u64(GuestMemory::field(out, 16)?, inheriting)
}

/// Only appending can change once a file is open; the sync flags are
/// `INVAL`, as in wasmtime-wasi.
pub(crate) fn fdstat_set_flags(table: &mut Descriptors, fd: u32, flags: u32) -> Answer {
    args::flags(flags, args::FDFLAGS)?;
    let file = file(table, fd)?;
    if flags & FDFLAGS_SYNCHRONIZED != 0 {
        return Err(Errno::Inval.into());
    }
    file.append = flags & FDFLAGS_APPEND != 0;
    Ok(())
}

/// Rights only narrow at open, so a later change is unsupported.
pub(crate) fn fdstat_set_rights(
    table: &Descriptors,
    fd: u32,
    base: u64,
    inheriting: u64,
) -> Answer {
    args::flags(base, args::RIGHTS)?;
    args::flags(inheriting, args::RIGHTS)?;
    file_or_directory(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn filestat_get(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    out: u32,
) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => {
            write_filestat(mem, out, FILETYPE_UNKNOWN, 0)
        }
        Descriptor::Directory(dir) => {
            write_metadata(mem, out, dir.mount.filesystem().metadata(&dir.path)?)
        }
        Descriptor::File(file) => write_metadata(mem, out, file.file.metadata()?),
    }
}

pub(crate) fn filestat_set_size(table: &mut Descriptors, fd: u32, size: u64) -> Answer {
    let file = file(table, fd)?;
    if !file.writable {
        return Err(Errno::Badf.into());
    }
    Ok(file.file.set_len(size)?)
}

/// A Filesystem keeps no timestamps of the guest's choosing, so setting
/// them is unsupported rather than silently ignored.
pub(crate) fn filestat_set_times(table: &Descriptors, fd: u32, fst_flags: u32) -> Answer {
    args::fstflags(fst_flags)?;
    file_or_directory(table, fd)?;
    Err(Errno::Notsup.into())
}

pub(crate) fn pread(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    iovs: (u32, u32),
    offset: u64,
    nread: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(iovs.0, iovs.1)?;
    let file = match descriptor_mut(table, fd)? {
        Descriptor::Stdin(_) => return Err(Errno::Spipe.into()),
        Descriptor::Channel(_) | Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        Descriptor::File(file) => file,
    };
    if !file.readable {
        return Err(Errno::Badf.into());
    }
    let read = file.file.read_at(mem.bytes_mut(ptr, len)?, offset)?;
    mem.write_u32(nread, read as u32)
}

pub(crate) fn prestat_get(table: &Descriptors, mem: &mut GuestMemory, fd: u32, out: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Directory(dir) if dir.preopen => {
            GuestMemory::aligned(out, 4)?;
            mem.write_u8(out, 0)?;
            mem.write_u32(GuestMemory::field(out, 4)?, dir.mount.at().len() as u32)
        }
        _ => Err(Errno::Badf.into()),
    }
}

pub(crate) fn prestat_dir_name(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    path: u32,
    path_len: u32,
) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Directory(dir) if dir.preopen => {
            let name = dir.mount.at().as_bytes();
            if name.len() > path_len as usize {
                return Err(Errno::Nametoolong.into());
            }
            mem.write(path, name)
        }
        _ => Err(Errno::Notdir.into()),
    }
}

pub(crate) fn pwrite(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    ciovs: (u32, u32),
    offset: u64,
    nwritten: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(ciovs.0, ciovs.1)?;
    let file = match descriptor_mut(table, fd)? {
        Descriptor::Channel(_) => return Err(Errno::Spipe.into()),
        Descriptor::Stdin(_) | Descriptor::Directory(_) => return Err(Errno::Badf.into()),
        Descriptor::File(file) => file,
    };
    if !file.writable {
        return Err(Errno::Badf.into());
    }
    // Linux's `pwrite` ignores the offset of a file opened to append.
    let at = if file.append {
        file.file.metadata()?.len
    } else {
        offset
    };
    let written = file.file.write_at(mem.bytes(ptr, len)?, at)?;
    mem.write_u32(nwritten, written as u32)
}

pub(crate) fn read(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    iovs: (u32, u32),
    nread: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(iovs.0, iovs.1)?;
    let read = match descriptor_mut(table, fd)? {
        Descriptor::Stdin(pipe) => {
            let bytes = read_stdin(pipe, len as usize)?;
            mem.write(ptr, &bytes)?;
            bytes.len()
        }
        Descriptor::File(file) if file.readable => {
            let read = file.file.read_at(mem.bytes_mut(ptr, len)?, file.position)?;
            file.position += read as u64;
            read
        }
        Descriptor::Channel(_) | Descriptor::Directory(_) | Descriptor::File(_) => {
            return Err(Errno::Badf.into())
        }
    };
    mem.write_u32(nread, read as u32)
}

/// A directory's entries as preview1 `dirent`s, `.` and `..` first, from
/// the `cookie`th on, cut off where the buffer ends.
pub(crate) fn readdir(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    buf: (u32, u32),
    cookie: u64,
    bufused: u32,
) -> Answer {
    let dir = match descriptor(table, fd)? {
        Descriptor::Directory(dir) => dir,
        _ => return Err(Errno::Badf.into()),
    };
    let listed = dir.mount.filesystem().read_dir(&dir.path)?;
    let entries = [
        (".".to_string(), FILETYPE_DIRECTORY),
        ("..".to_string(), FILETYPE_DIRECTORY),
    ]
    .into_iter()
    .chain(
        listed
            .into_iter()
            .map(|entry| (entry.name, filetype(entry.kind))),
    );
    let skip = usize::try_from(cookie).map_err(|_| Errno::Overflow)?;
    let (start, capacity) = buf;
    let mut used: u32 = 0;
    for (index, (name, filetype)) in entries.enumerate().skip(skip) {
        let mut dirent = [0u8; DIRENT_SIZE as usize];
        dirent[0..8].copy_from_slice(&(index as u64 + 1).to_le_bytes());
        dirent[16..20].copy_from_slice(&(name.len() as u32).to_le_bytes());
        dirent[20] = filetype;
        for part in [&dirent[..], name.as_bytes()] {
            let room = (capacity - used) as usize;
            let take = part.len().min(room);
            mem.write(GuestMemory::field(start, used)?, &part[..take])?;
            used += take as u32;
            if used == capacity {
                return mem.write_u32(bufused, used);
            }
        }
    }
    mem.write_u32(bufused, used)
}

/// preview1 renumbering closes `to` and moves `from` onto its number;
/// both must already be open.
pub(crate) fn renumber(table: &mut Descriptors, from: u32, to: u32) -> Answer {
    if table.get(from).is_none() || table.get(to).is_none() {
        return Err(Errno::Badf.into());
    }
    if from == to {
        return Ok(());
    }
    table.remove(to);
    let moved = table.remove(from).ok_or(Errno::Badf)?;
    table.place(to, moved);
    Ok(())
}

pub(crate) fn seek(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    offset: i64,
    whence: u32,
    out: u32,
) -> Answer {
    args::variant(whence, args::WHENCE_MAX)?;
    let file = seekable(table, fd)?;
    let base = match whence {
        0 => 0,
        1 => file.position,
        _ => file.file.metadata()?.len,
    };
    let position = base.checked_add_signed(offset).ok_or(Errno::Inval)?;
    file.position = position;
    mem.write_u64(out, position)
}

pub(crate) fn tell(table: &mut Descriptors, mem: &mut GuestMemory, fd: u32, out: u32) -> Answer {
    let position = seekable(table, fd)?.position;
    mem.write_u64(out, position)
}

pub(crate) fn write(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    ciovs: (u32, u32),
    nwritten: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(ciovs.0, ciovs.1)?;
    let written = match descriptor_mut(table, fd)? {
        Descriptor::Channel(pipe) => write_channel(pipe, mem.bytes(ptr, len)?)?,
        Descriptor::File(file) if file.writable => {
            if file.append {
                file.position = file.file.metadata()?.len;
            }
            let written = file.file.write_at(mem.bytes(ptr, len)?, file.position)?;
            file.position += written as u64;
            written
        }
        Descriptor::Stdin(_) | Descriptor::Directory(_) | Descriptor::File(_) => {
            return Err(Errno::Badf.into())
        }
    };
    mem.write_u32(nwritten, written as u32)
}

/// A zero-length read of a stdin that still holds bytes is `INTR`, and one
/// of a drained stdin reads nothing, as wasmtime-wasi's blocking read
/// answers them.
fn read_stdin(pipe: &mut MemoryInputPipe, len: usize) -> Answer<Vec<u8>> {
    match pipe.read(len) {
        Ok(bytes) if bytes.is_empty() => Err(Errno::Intr.into()),
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(StreamError::Closed) => Ok(Vec::new()),
        Err(_) => Err(Errno::Io.into()),
    }
}

/// Fill the capture pipe as far as it holds. A pipe that fills before the
/// bytes run out answers `IO`, though what fit is kept: the cap one past
/// the configured limit is how a capture learns it was truncated.
fn write_channel(pipe: &mut MemoryOutputPipe, bytes: &[u8]) -> Answer<usize> {
    for chunk in bytes.chunks(WRITE_CHUNK) {
        let mut rest = chunk;
        while !rest.is_empty() {
            let permit = pipe.check_write().map_err(|_| Errno::Io)?;
            let (now, later) = rest.split_at(rest.len().min(permit));
            pipe.write(Bytes::copy_from_slice(now))
                .map_err(|_| Errno::Io)?;
            rest = later;
        }
    }
    Ok(bytes.len())
}
