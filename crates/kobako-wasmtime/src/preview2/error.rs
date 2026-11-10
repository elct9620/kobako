//! The host side of each WIT error a preview2 import answers, as wasmtime's
//! `trappable_error_type` gives one: a host method answers the guest's error or
//! traps through one `?`, and the generated glue turns it back into the WIT
//! error.

use kobako_runtime::vfs::FilesystemError;

use super::bindings::wasi_http_types_0_2_12 as http;
use super::bindings::wasi_sockets_network_0_2_12 as network;
use super::sockets::to_error_code;

/// The error `E` the guest reads, or a trap ending the run, as wasmtime-wasi's
/// `TrappableError`.
#[derive(Debug)]
pub(crate) enum TrappableError<E> {
    Error(E),
    Trap(wasmtime::Error),
}

impl<E> TrappableError<E> {
    /// The error the guest reads, or the trap.
    pub(crate) fn downcast(self) -> wasmtime::Result<E> {
        match self {
            Self::Error(err) => Ok(err),
            Self::Trap(trap) => Err(trap),
        }
    }
}

impl<E> From<wasmtime::Error> for TrappableError<E> {
    fn from(trap: wasmtime::Error) -> Self {
        Self::Trap(trap)
    }
}

pub(crate) type SocketError = TrappableError<network::ErrorCode>;
pub(crate) type HttpError = TrappableError<http::ErrorCode>;
pub(crate) type HeaderError = TrappableError<http::HeaderError>;

impl From<network::ErrorCode> for SocketError {
    fn from(code: network::ErrorCode) -> Self {
        Self::Error(code)
    }
}

/// A socket's failure, by the code its POSIX name maps to, as wasmtime-wasi's
/// `SocketError` takes an `io::Error`.
impl From<FilesystemError> for SocketError {
    fn from(err: FilesystemError) -> Self {
        Self::Error(to_error_code(&err))
    }
}

impl From<http::ErrorCode> for HttpError {
    fn from(code: http::ErrorCode) -> Self {
        Self::Error(code)
    }
}

impl From<http::HeaderError> for HeaderError {
    fn from(err: http::HeaderError) -> Self {
        Self::Error(err)
    }
}

/// A stream's failure, as wasmtime-wasi-io's `StreamError`: the end, an error
/// the guest may read, or a trap.
#[derive(Debug)]
pub(crate) enum StreamError {
    Closed,
    LastOperationFailed(FilesystemError),
    Trap(wasmtime::Error),
}

impl From<wasmtime::Error> for StreamError {
    fn from(trap: wasmtime::Error) -> Self {
        Self::Trap(trap)
    }
}

/// A broken pipe is the stream's end; anything else is an error the guest may
/// read.
impl From<FilesystemError> for StreamError {
    fn from(err: FilesystemError) -> Self {
        match err {
            FilesystemError::BrokenPipe => Self::Closed,
            err => Self::LastOperationFailed(err),
        }
    }
}

/// A host method's answer split as the generated glue splits it: the trap
/// apart from the result the guest reads.
#[cfg(test)]
pub(crate) trait IntoAnswer<T, E> {
    fn into_answer(self) -> wasmtime::Result<Result<T, E>>;
}

#[cfg(test)]
impl<T, E> IntoAnswer<T, E> for Result<T, TrappableError<E>> {
    fn into_answer(self) -> wasmtime::Result<Result<T, E>> {
        match self {
            Ok(answer) => Ok(Ok(answer)),
            Err(TrappableError::Error(err)) => Ok(Err(err)),
            Err(TrappableError::Trap(trap)) => Err(trap),
        }
    }
}

#[cfg(test)]
impl<T> IntoAnswer<T, StreamError> for Result<T, StreamError> {
    fn into_answer(self) -> wasmtime::Result<Result<T, StreamError>> {
        match self {
            Err(StreamError::Trap(trap)) => Err(trap),
            answer => Ok(answer),
        }
    }
}
