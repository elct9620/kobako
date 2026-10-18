//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

use kobako_runtime::filesystem::FilesystemError;
use kobako_runtime::mount::Denial;

/// One preview1 `errno`, numbered as the witx `$errno` enum numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Errno {
    Acces = 2,
    Badf = 8,
    Exist = 20,
    Ilseq = 25,
    Intr = 27,
    Inval = 28,
    Io = 29,
    Isdir = 31,
    Mfile = 33,
    Nametoolong = 37,
    Noent = 44,
    Nospc = 51,
    Notdir = 54,
    Notempty = 55,
    Notsock = 57,
    Notsup = 58,
    Overflow = 61,
    Rofs = 69,
    Spipe = 70,
    Xdev = 75,
    Notcapable = 76,
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

/// The code a Filesystem's failure reaches the guest as.
impl From<FilesystemError> for Failure {
    fn from(error: FilesystemError) -> Self {
        Self::Errno(match error {
            FilesystemError::NotFound => Errno::Noent,
            FilesystemError::AlreadyExists => Errno::Exist,
            FilesystemError::NotADirectory => Errno::Notdir,
            FilesystemError::IsADirectory => Errno::Isdir,
            FilesystemError::DirectoryNotEmpty => Errno::Notempty,
            FilesystemError::PermissionDenied => Errno::Acces,
            FilesystemError::ReadOnly => Errno::Rofs,
            FilesystemError::InvalidInput => Errno::Inval,
            FilesystemError::Unsupported => Errno::Notsup,
            FilesystemError::NoSpace => Errno::Nospc,
            _ => Errno::Io,
        })
    }
}

/// A change beneath a read-only Mount is `ROFS`; a Policy's refusal is
/// `ACCES`.
impl From<Denial> for Failure {
    fn from(denial: Denial) -> Self {
        Self::Errno(match denial {
            Denial::ReadOnly => Errno::Rofs,
            Denial::Refused => Errno::Acces,
        })
    }
}

pub(crate) type Answer<T = ()> = Result<T, Failure>;
