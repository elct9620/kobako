//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

use kobako_runtime::filesystem::FilesystemError;
use kobako_runtime::mount::Denial;
use kobako_runtime::vfs;
use wiggle::GuestError;

use super::types;
pub(crate) use super::types::Errno;

pub(crate) type Answer<T = ()> = Result<T, types::Error>;

/// The code a POSIX error name a Filesystem's failure carries stands for:
/// the witx case, `E` prefixed and upper-cased.
fn from_posix_name(name: &str) -> Option<Errno> {
    Some(match name {
        "ENOENT" => Errno::Noent,
        "EEXIST" => Errno::Exist,
        "ENOTDIR" => Errno::Notdir,
        "EISDIR" => Errno::Isdir,
        "ENOTEMPTY" => Errno::Notempty,
        "EACCES" => Errno::Acces,
        "EROFS" => Errno::Rofs,
        "EINVAL" => Errno::Inval,
        "ENOTSUP" => Errno::Notsup,
        "ENOSPC" => Errno::Nospc,
        "EFBIG" => Errno::Fbig,
        "EXDEV" => Errno::Xdev,
        "EDEADLK" => Errno::Deadlk,
        "ELOOP" => Errno::Loop,
        "EPERM" => Errno::Perm,
        "EBADF" => Errno::Badf,
        "EBUSY" => Errno::Busy,
        "EILSEQ" => Errno::Ilseq,
        "EINTR" => Errno::Intr,
        "EMFILE" => Errno::Mfile,
        "ENAMETOOLONG" => Errno::Nametoolong,
        "ENOMEM" => Errno::Nomem,
        "ENOTSOCK" => Errno::Notsock,
        "EOVERFLOW" => Errno::Overflow,
        "ESPIPE" => Errno::Spipe,
        _ => return None,
    })
}

/// A pointer outside the memory or misaligned traps, as the witx
/// documentation requires; a value inside memory that no witx type holds is
/// `INVAL`, as in wasmtime-wasi.
impl From<GuestError> for types::Error {
    fn from(error: GuestError) -> Self {
        match error {
            GuestError::InvalidFlagValue { .. } | GuestError::InvalidEnumValue { .. } => {
                Errno::Inval.into()
            }
            GuestError::PtrOverflow
            | GuestError::PtrOutOfBounds { .. }
            | GuestError::PtrNotAligned { .. } => Self::trap(error.into()),
            GuestError::InvalidUtf8 { .. } => Errno::Ilseq.into(),
            GuestError::TryFromIntError { .. } => Errno::Overflow.into(),
            GuestError::SliceLengthsDiffer => Errno::Fault.into(),
            GuestError::InFunc { err, .. } => Self::from(*err),
        }
    }
}

/// The code a Filesystem's failure reaches the guest as: the one its
/// POSIX name stands for, or `IO` for a failure without one.
impl From<FilesystemError> for types::Error {
    fn from(error: FilesystemError) -> Self {
        error
            .posix_name()
            .and_then(from_posix_name)
            .unwrap_or(Errno::Io)
            .into()
    }
}

/// A change beneath a read-only Mount is `ROFS`; a Policy's refusal is
/// `ACCES`.
impl From<Denial> for types::Error {
    fn from(denial: Denial) -> Self {
        match denial {
            Denial::ReadOnly => Errno::Rofs,
            Denial::Refused => Errno::Acces,
        }
        .into()
    }
}

/// A path escaping its descriptor is `NOTCAPABLE`, as preview1 names a
/// capability the guest does not hold.
impl From<vfs::Error> for types::Error {
    fn from(error: vfs::Error) -> Self {
        match error {
            vfs::Error::Filesystem(error) => error.into(),
            vfs::Error::Denied(denial) => denial.into(),
            vfs::Error::Escape => Errno::Notcapable.into(),
        }
    }
}

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
            let Ok(errno) = types::Error::from(error).downcast() else {
                panic!("{label} must answer an error code, not a trap");
            };
            assert_eq!(
                u16::from(errno),
                code,
                "{label} through types::Error::from must reach the guest as preview1 errno {code}"
            );
        }
    }
}
