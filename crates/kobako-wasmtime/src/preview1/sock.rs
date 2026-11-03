//! The 4 `sock_*` calls. The table holds no socket, so every open
//! descriptor answers `NOTSOCK`, as wasmtime-wasi's preview1 does, and one
//! standing only for its path `BADF`, as Linux's socket calls take theirs
//! through `fdget`.

use super::errno::{Answer, Errno};
use super::fd::open_file;
use crate::descriptors::Descriptors;

pub(crate) fn socket<T>(table: &Descriptors, fd: u32) -> Answer<T> {
    open_file(table, fd)?;
    Err(Errno::Notsock.into())
}
