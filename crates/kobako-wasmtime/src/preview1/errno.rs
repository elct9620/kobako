//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

use std::collections::HashMap;
use std::sync::LazyLock;

use kobako_runtime::filesystem::FilesystemError;
use wiggle::GuestError;

use super::types;
pub(crate) use super::types::Errno;

pub(crate) type Answer<T = ()> = Result<T, types::Error>;

/// The code a POSIX error name a Filesystem's failure carries stands for:
/// the witx case, `E` prefixed and upper-cased, so a name the runtime adds
/// reaches its own code without a second table to keep.
fn from_posix_name(name: &str) -> Option<Errno> {
    static BY_POSIX_NAME: LazyLock<HashMap<String, Errno>> = LazyLock::new(|| {
        (0..=u16::from(Errno::Notcapable))
            .filter_map(|code| Errno::try_from(code).ok())
            .map(|errno| (format!("E{errno:?}").to_ascii_uppercase(), errno))
            .collect()
    });
    BY_POSIX_NAME.get(name).copied()
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

/// The code a call's failure reaches the guest as: the one its POSIX name
/// stands for, or `IO` for a failure without one. A path escaping its
/// descriptor is `NOTCAPABLE`, as preview1 names a capability the guest does
/// not hold, where Linux answers EXDEV.
impl From<FilesystemError> for types::Error {
    fn from(error: FilesystemError) -> Self {
        match error {
            FilesystemError::Escape => Errno::Notcapable,
            error => error
                .posix_name()
                .and_then(from_posix_name)
                .unwrap_or(Errno::Io),
        }
        .into()
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
            (FilesystemError::Escape, 76),
            (FilesystemError::Deadlock, 16),
            (FilesystemError::FilesystemLoop, 32),
            (FilesystemError::NotPermitted, 63),
            (FilesystemError::BadDescriptor, 8),
            (FilesystemError::ResourceBusy, 10),
            (FilesystemError::IllegalSequence, 25),
            (FilesystemError::Interrupted, 27),
            (FilesystemError::WouldBlock, 6),
            (FilesystemError::TooManyOpenFiles, 33),
            (FilesystemError::InvalidFilename, 37),
            (FilesystemError::OutOfMemory, 48),
            (FilesystemError::NotASocket, 57),
            (FilesystemError::ValueTooLarge, 61),
            (FilesystemError::NotSeekable, 70),
            (FilesystemError::NoSuchDevice, 43),
            (FilesystemError::StaleNetworkFileHandle, 72),
            (FilesystemError::MessageTooLong, 35),
            (FilesystemError::AlreadyInProgress, 7),
            (FilesystemError::ConnectionRefused, 14),
            (FilesystemError::NoBufferSpace, 42),
            (FilesystemError::ProtocolNotAvailable, 50),
            (FilesystemError::AddressFamilyNotSupported, 5),
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
