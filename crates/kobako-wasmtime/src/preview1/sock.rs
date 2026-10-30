//! The 4 `sock_*` calls. The table holds no socket, so every open
//! descriptor answers `NOTSOCK`, as wasmtime-wasi's preview1 does.

use super::errno::{Answer, Errno};
use crate::descriptors::Descriptors;

pub(crate) fn socket<T>(table: &Descriptors, fd: u32) -> Answer<T> {
    table.get(fd).ok_or(Errno::Badf)?;
    Err(Errno::Notsock.into())
}
