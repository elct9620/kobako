//! The 10 `path_*` calls. Each resolves a path beneath a directory
//! descriptor; until the table holds one, every call answers as
//! wasmtime-wasi does for a descriptor that is not a directory.

use std::convert::Infallible;

use super::args;
use super::errno::{Answer, Errno};
use super::memory::GuestMemory;
use crate::descriptors::{Descriptor, Descriptors};

/// The flags `path_open` takes besides the path itself.
pub(crate) struct OpenFlags {
    pub(crate) dirflags: u32,
    pub(crate) oflags: u32,
    pub(crate) rights_base: u64,
    pub(crate) rights_inheriting: u64,
    pub(crate) fdflags: u32,
}

/// The directory `fd` stands for. The table holds no directory yet.
fn directory(table: &Descriptors, fd: u32) -> Answer<Infallible> {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Badf.into()),
    }
}

pub(crate) fn create_directory(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}

pub(crate) fn filestat_get(table: &Descriptors, fd: u32, flags: u32) -> Answer {
    args::flags(flags, args::LOOKUPFLAGS)?;
    match directory(table, fd)? {}
}

pub(crate) fn filestat_set_times(
    table: &Descriptors,
    fd: u32,
    flags: u32,
    fst_flags: u32,
) -> Answer {
    args::flags(flags, args::LOOKUPFLAGS)?;
    args::fstflags(fst_flags)?;
    match directory(table, fd)? {}
}

/// The source directory is resolved first, so it decides the answer
/// while no directory exists.
pub(crate) fn link(table: &Descriptors, old_fd: u32, old_flags: u32) -> Answer {
    args::flags(old_flags, args::LOOKUPFLAGS)?;
    match directory(table, old_fd)? {}
}

pub(crate) fn open(
    table: &Descriptors,
    mem: &GuestMemory,
    fd: u32,
    path: u32,
    path_len: u32,
    flags: &OpenFlags,
) -> Answer {
    args::flags(flags.dirflags, args::LOOKUPFLAGS)?;
    args::flags(flags.oflags, args::OFLAGS)?;
    args::flags(flags.rights_base, args::RIGHTS)?;
    args::flags(flags.rights_inheriting, args::RIGHTS)?;
    args::flags(flags.fdflags, args::FDFLAGS)?;
    mem.string(path, path_len)?;
    match directory(table, fd)? {}
}

pub(crate) fn readlink(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}

pub(crate) fn remove_directory(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}

/// The source directory is resolved first, as for `link`.
pub(crate) fn rename(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}

pub(crate) fn symlink(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}

pub(crate) fn unlink_file(table: &Descriptors, fd: u32) -> Answer {
    match directory(table, fd)? {}
}
