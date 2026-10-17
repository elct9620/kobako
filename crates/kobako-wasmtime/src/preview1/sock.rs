//! The 4 `sock_*` calls. The table holds no socket, so every open
//! descriptor answers `NOTSOCK`, as wasmtime-wasi's preview1 does.

use super::args;
use super::errno::{Answer, Errno};
use crate::descriptors::{Descriptor, Descriptors};

fn socket(table: &Descriptors, fd: u32) -> Answer {
    match table.get(fd).ok_or(Errno::Badf)? {
        Descriptor::Stdin(_) | Descriptor::Channel(_) => Err(Errno::Notsock.into()),
    }
}

pub(crate) fn accept(table: &Descriptors, fd: u32, flags: u32) -> Answer {
    args::flags(flags, args::FDFLAGS)?;
    socket(table, fd)
}

pub(crate) fn recv(table: &Descriptors, fd: u32, ri_flags: u32) -> Answer {
    args::flags(ri_flags, args::RIFLAGS)?;
    socket(table, fd)
}

pub(crate) fn send(table: &Descriptors, fd: u32) -> Answer {
    socket(table, fd)
}

pub(crate) fn shutdown(table: &Descriptors, fd: u32, how: u32) -> Answer {
    args::flags(how, args::SDFLAGS)?;
    socket(table, fd)
}
