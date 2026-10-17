//! The 21 `fd_*` calls over kobako's descriptor table.
//!
//! Each answers what wasmtime-wasi's preview1 answers for the same
//! descriptor; the differential probe in `preview1` holds that.

use bytes::Bytes;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::p2::{InputStream, OutputStream, StreamError};

use super::args;
use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors};

/// `fdstat` / `filestat` filetype for a pipe that is not a terminal.
const FILETYPE_UNKNOWN: u8 = 0;
const RIGHTS_FD_READ: u64 = 1 << 1;
const RIGHTS_FD_WRITE: u64 = 1 << 6;

/// The most bytes one write moves into a capture pipe at a time, the
/// preview2 `blocking-write-and-flush` limit wasmtime-wasi writes under.
const WRITE_CHUNK: usize = 4096;

fn descriptor(table: &Descriptors, fd: u32) -> Answer<&Descriptor> {
    table.get(fd).ok_or_else(|| Errno::Badf.into())
}

/// A call only a file or a directory answers. Stdio is neither.
fn file_only(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Badf.into()),
    }
}

/// A call that moves a stream position. Stdio has none to move.
fn seekable(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Spipe.into()),
    }
}

pub(crate) fn advise(table: &Descriptors, fd: u32, advice: u32) -> Answer {
    args::variant(advice, args::ADVICE_MAX)?;
    file_only(table, fd)
}

pub(crate) fn allocate(table: &Descriptors, fd: u32) -> Answer {
    file_only(table, fd)
}

pub(crate) fn close(table: &mut Descriptors, fd: u32) -> Answer {
    table.remove(fd).ok_or(Errno::Badf)?;
    Ok(())
}

pub(crate) fn datasync(table: &Descriptors, fd: u32) -> Answer {
    file_only(table, fd)
}

pub(crate) fn fdstat_get(table: &Descriptors, mem: &mut GuestMemory, fd: u32, out: u32) -> Answer {
    let rights = match descriptor(table, fd)? {
        Descriptor::Stdin(_) => RIGHTS_FD_READ,
        Descriptor::Channel(_) => RIGHTS_FD_WRITE,
    };
    GuestMemory::aligned(out, 8)?;
    mem.write_u8(out, FILETYPE_UNKNOWN)?;
    mem.write_u16(out + 2, 0)?;
    mem.write_u64(out + 8, rights)?;
    mem.write_u64(out + 16, rights)
}

pub(crate) fn fdstat_set_flags(table: &Descriptors, fd: u32, flags: u32) -> Answer {
    args::flags(flags, args::FDFLAGS)?;
    file_only(table, fd)
}

pub(crate) fn fdstat_set_rights(
    table: &Descriptors,
    fd: u32,
    base: u64,
    inheriting: u64,
) -> Answer {
    args::flags(base, args::RIGHTS)?;
    args::flags(inheriting, args::RIGHTS)?;
    file_only(table, fd)
}

pub(crate) fn filestat_get(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    out: u32,
) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => {
            GuestMemory::aligned(out, 8)?;
            mem.write(out, &[0; 64])?;
            mem.write_u8(out + 16, FILETYPE_UNKNOWN)
        }
    }
}

pub(crate) fn filestat_set_size(table: &Descriptors, fd: u32) -> Answer {
    file_only(table, fd)
}

pub(crate) fn filestat_set_times(table: &Descriptors, fd: u32, fst_flags: u32) -> Answer {
    args::fstflags(fst_flags)?;
    file_only(table, fd)
}

pub(crate) fn pread(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    iovs: u32,
    iovs_len: u32,
) -> Answer {
    mem.first_non_empty_buffer(iovs, iovs_len)?;
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) => Err(Errno::Spipe.into()),
        Descriptor::Channel(_) => Err(Errno::Badf.into()),
    }
}

pub(crate) fn prestat_get(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Badf.into()),
    }
}

pub(crate) fn prestat_dir_name(table: &Descriptors, fd: u32) -> Answer {
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Notdir.into()),
    }
}

pub(crate) fn pwrite(
    table: &Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    ciovs: u32,
    ciovs_len: u32,
) -> Answer {
    mem.first_non_empty_buffer(ciovs, ciovs_len)?;
    match descriptor(table, fd)? {
        Descriptor::Stdin(_) => Err(Errno::Badf.into()),
        Descriptor::Channel(_) => Err(Errno::Spipe.into()),
    }
}

pub(crate) fn read(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    iovs: u32,
    iovs_len: u32,
    nread: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(iovs, iovs_len)?;
    let bytes = match table.get_mut(fd).ok_or(Errno::Badf)? {
        Descriptor::Stdin(pipe) => read_stdin(pipe, len as usize)?,
        Descriptor::Channel(_) => return Err(Errno::Badf.into()),
    };
    mem.write(ptr, &bytes)?;
    mem.write_u32(nread, bytes.len() as u32)
}

pub(crate) fn readdir(table: &Descriptors, fd: u32) -> Answer {
    file_only(table, fd)
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

pub(crate) fn seek(table: &Descriptors, fd: u32, whence: u32) -> Answer {
    args::variant(whence, args::WHENCE_MAX)?;
    seekable(table, fd)
}

pub(crate) fn sync(table: &Descriptors, fd: u32) -> Answer {
    file_only(table, fd)
}

pub(crate) fn tell(table: &Descriptors, fd: u32) -> Answer {
    seekable(table, fd)
}

pub(crate) fn write(
    table: &mut Descriptors,
    mem: &mut GuestMemory,
    fd: u32,
    ciovs: u32,
    ciovs_len: u32,
    nwritten: u32,
) -> Answer {
    let (ptr, len) = mem.first_non_empty_buffer(ciovs, ciovs_len)?;
    let written = match table.get_mut(fd).ok_or(Errno::Badf)? {
        Descriptor::Channel(pipe) => write_channel(pipe, mem.bytes(ptr, len)?)?,
        Descriptor::Stdin(_) => return Err(Errno::Badf.into()),
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
