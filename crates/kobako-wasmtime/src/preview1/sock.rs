//! The 4 `sock_*` calls. preview1 reaches no socket, since a socket lives in
//! preview2 and an older WASI adapts to a newer one, so every descriptor it
//! holds is a file: a file's answers `NOTSOCK`, as wasmtime-wasi's preview1
//! and Linux's `sockfd_lookup` answer, and one standing only for its path
//! `BADF`, as Linux's socket calls take theirs through `fdget`.

use super::errno::{Answer, Errno};
use super::fd::file;
use crate::descriptors::Descriptors;

/// What a socket call on `fd` answers: the socket it never finds.
pub(crate) fn sockfd_lookup<T>(table: &Descriptors, fd: u32) -> Answer<T> {
    file(table, fd)?;
    Err(Errno::Notsock.into())
}
