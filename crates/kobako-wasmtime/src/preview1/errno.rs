//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

/// One preview1 `errno`, numbered as the witx `$errno` enum numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Errno {
    Badf = 8,
    Ilseq = 25,
    Intr = 27,
    Inval = 28,
    Io = 29,
    Notdir = 54,
    Notsock = 57,
    Spipe = 70,
}

/// A call that did not succeed. preview1 traps rather than answering
/// for a pointer outside guest memory, as wasmtime-wasi does.
pub(crate) enum Failure {
    Errno(Errno),
    Trap(wasmtime::Error),
}

impl From<Errno> for Failure {
    fn from(errno: Errno) -> Self {
        Self::Errno(errno)
    }
}

pub(crate) type Answer<T = ()> = Result<T, Failure>;
