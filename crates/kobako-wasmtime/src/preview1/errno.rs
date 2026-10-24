//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

use kobako_runtime::filesystem::FilesystemError;
use kobako_runtime::mount::Denial;

/// One preview1 `errno`, numbered as the witx `$errno` enum numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Errno {
    TooBig = 1,
    Acces = 2,
    Addrinuse = 3,
    Addrnotavail = 4,
    Afnosupport = 5,
    Again = 6,
    Already = 7,
    Badf = 8,
    Badmsg = 9,
    Busy = 10,
    Canceled = 11,
    Child = 12,
    Connaborted = 13,
    Connrefused = 14,
    Connreset = 15,
    Deadlk = 16,
    Destaddrreq = 17,
    Dom = 18,
    Dquot = 19,
    Exist = 20,
    Fault = 21,
    Fbig = 22,
    Hostunreach = 23,
    Idrm = 24,
    Ilseq = 25,
    Inprogress = 26,
    Intr = 27,
    Inval = 28,
    Io = 29,
    Isconn = 30,
    Isdir = 31,
    Loop = 32,
    Mfile = 33,
    Mlink = 34,
    Msgsize = 35,
    Multihop = 36,
    Nametoolong = 37,
    Netdown = 38,
    Netreset = 39,
    Netunreach = 40,
    Nfile = 41,
    Nobufs = 42,
    Nodev = 43,
    Noent = 44,
    Noexec = 45,
    Nolck = 46,
    Nolink = 47,
    Nomem = 48,
    Nomsg = 49,
    Noprotoopt = 50,
    Nospc = 51,
    Nosys = 52,
    Notconn = 53,
    Notdir = 54,
    Notempty = 55,
    Notrecoverable = 56,
    Notsock = 57,
    Notsup = 58,
    Notty = 59,
    Nxio = 60,
    Overflow = 61,
    Ownerdead = 62,
    Perm = 63,
    Pipe = 64,
    Proto = 65,
    Protonosupport = 66,
    Prototype = 67,
    Range = 68,
    Rofs = 69,
    Spipe = 70,
    Srch = 71,
    Stale = 72,
    Timedout = 73,
    Txtbsy = 74,
    Xdev = 75,
    Notcapable = 76,
}

impl Errno {
    /// The code a POSIX error name stands for: the witx case, `E`
    /// prefixed and upper-cased. `ENOTCAPABLE` is WASI's own.
    fn from_posix_name(name: &str) -> Option<Self> {
        Some(match name {
            "E2BIG" => Self::TooBig,
            "EACCES" => Self::Acces,
            "EADDRINUSE" => Self::Addrinuse,
            "EADDRNOTAVAIL" => Self::Addrnotavail,
            "EAFNOSUPPORT" => Self::Afnosupport,
            "EAGAIN" => Self::Again,
            "EALREADY" => Self::Already,
            "EBADF" => Self::Badf,
            "EBADMSG" => Self::Badmsg,
            "EBUSY" => Self::Busy,
            "ECANCELED" => Self::Canceled,
            "ECHILD" => Self::Child,
            "ECONNABORTED" => Self::Connaborted,
            "ECONNREFUSED" => Self::Connrefused,
            "ECONNRESET" => Self::Connreset,
            "EDEADLK" => Self::Deadlk,
            "EDESTADDRREQ" => Self::Destaddrreq,
            "EDOM" => Self::Dom,
            "EDQUOT" => Self::Dquot,
            "EEXIST" => Self::Exist,
            "EFAULT" => Self::Fault,
            "EFBIG" => Self::Fbig,
            "EHOSTUNREACH" => Self::Hostunreach,
            "EIDRM" => Self::Idrm,
            "EILSEQ" => Self::Ilseq,
            "EINPROGRESS" => Self::Inprogress,
            "EINTR" => Self::Intr,
            "EINVAL" => Self::Inval,
            "EIO" => Self::Io,
            "EISCONN" => Self::Isconn,
            "EISDIR" => Self::Isdir,
            "ELOOP" => Self::Loop,
            "EMFILE" => Self::Mfile,
            "EMLINK" => Self::Mlink,
            "EMSGSIZE" => Self::Msgsize,
            "EMULTIHOP" => Self::Multihop,
            "ENAMETOOLONG" => Self::Nametoolong,
            "ENETDOWN" => Self::Netdown,
            "ENETRESET" => Self::Netreset,
            "ENETUNREACH" => Self::Netunreach,
            "ENFILE" => Self::Nfile,
            "ENOBUFS" => Self::Nobufs,
            "ENODEV" => Self::Nodev,
            "ENOENT" => Self::Noent,
            "ENOEXEC" => Self::Noexec,
            "ENOLCK" => Self::Nolck,
            "ENOLINK" => Self::Nolink,
            "ENOMEM" => Self::Nomem,
            "ENOMSG" => Self::Nomsg,
            "ENOPROTOOPT" => Self::Noprotoopt,
            "ENOSPC" => Self::Nospc,
            "ENOSYS" => Self::Nosys,
            "ENOTCONN" => Self::Notconn,
            "ENOTDIR" => Self::Notdir,
            "ENOTEMPTY" => Self::Notempty,
            "ENOTRECOVERABLE" => Self::Notrecoverable,
            "ENOTSOCK" => Self::Notsock,
            "ENOTSUP" => Self::Notsup,
            "ENOTTY" => Self::Notty,
            "ENXIO" => Self::Nxio,
            "EOVERFLOW" => Self::Overflow,
            "EOWNERDEAD" => Self::Ownerdead,
            "EPERM" => Self::Perm,
            "EPIPE" => Self::Pipe,
            "EPROTO" => Self::Proto,
            "EPROTONOSUPPORT" => Self::Protonosupport,
            "EPROTOTYPE" => Self::Prototype,
            "ERANGE" => Self::Range,
            "EROFS" => Self::Rofs,
            "ESPIPE" => Self::Spipe,
            "ESRCH" => Self::Srch,
            "ESTALE" => Self::Stale,
            "ETIMEDOUT" => Self::Timedout,
            "ETXTBSY" => Self::Txtbsy,
            "EXDEV" => Self::Xdev,
            "ENOTCAPABLE" => Self::Notcapable,
            _ => return None,
        })
    }
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

/// The code a Filesystem's failure reaches the guest as: the one its
/// POSIX name stands for, or `IO` for a failure without one.
impl From<FilesystemError> for Failure {
    fn from(error: FilesystemError) -> Self {
        Self::Errno(
            error
                .posix_name()
                .and_then(Errno::from_posix_name)
                .unwrap_or(Errno::Io),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_filesystem_failure_reaches_the_guest_as_its_own_code() {
        let cases = [
            (FilesystemError::NotFound, 44),
            (FilesystemError::AlreadyExists, 20),
            (FilesystemError::NotADirectory, 54),
            (FilesystemError::IsADirectory, 31),
            (FilesystemError::DirectoryNotEmpty, 55),
            (FilesystemError::PermissionDenied, 2),
            (FilesystemError::ReadOnlyFilesystem, 69),
            (FilesystemError::InvalidInput, 28),
            (FilesystemError::Unsupported, 58),
            (FilesystemError::StorageFull, 51),
            (FilesystemError::FileTooLarge, 22),
            (FilesystemError::CrossesDevices, 75),
            (FilesystemError::Deadlock, 16),
            (FilesystemError::FilesystemLoop, 32),
            (FilesystemError::NotPermitted, 63),
            (FilesystemError::BadDescriptor, 8),
            (FilesystemError::ResourceBusy, 10),
            (FilesystemError::IllegalSequence, 25),
            (FilesystemError::Interrupted, 27),
            (FilesystemError::TooManyOpenFiles, 33),
            (FilesystemError::InvalidFilename, 37),
            (FilesystemError::OutOfMemory, 48),
            (FilesystemError::NotASocket, 57),
            (FilesystemError::ValueTooLarge, 61),
            (FilesystemError::NotSeekable, 70),
            (FilesystemError::Other("x".into()), 29),
        ];
        for (error, code) in cases {
            let label = format!("{error:?}");
            let Failure::Errno(errno) = Failure::from(error) else {
                panic!("{label} must answer an error code, not a trap");
            };
            assert_eq!(
                errno as u16, code,
                "{label} through Failure::from must reach the guest as preview1 errno {code}"
            );
        }
    }
}
