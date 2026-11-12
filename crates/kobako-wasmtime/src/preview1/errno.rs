//! What a preview1 call answers when it does not succeed: an error code
//! the guest reads, or a trap that ends the invocation.

use std::collections::HashMap;
use std::sync::LazyLock;

use wiggle::GuestError;

use super::types;
pub(crate) use super::types::Errno;

pub(crate) type Answer<T = ()> = Result<T, types::Error>;

/// The code a POSIX error name a Filesystem's failure carries stands for:
/// the witx case, `E` prefixed and upper-cased, so a name the runtime adds
/// reaches its own code without a second table to keep.
fn from_posix_name(name: &str) -> Option<Errno> {
    static ERRNO_BY_POSIX_NAME: LazyLock<HashMap<String, Errno>> = LazyLock::new(|| {
        (0..=u16::from(Errno::Notcapable))
            .filter_map(|code| Errno::try_from(code).ok())
            .map(|errno| (format!("E{errno:?}").to_ascii_uppercase(), errno))
            .collect()
    });
    ERRNO_BY_POSIX_NAME.get(name).copied()
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
impl From<kobako_runtime::Errno> for types::Error {
    fn from(error: kobako_runtime::Errno) -> Self {
        match error {
            kobako_runtime::Errno::Escape => Errno::Notcapable,
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
            (kobako_runtime::Errno::NotFound, 44),
            (kobako_runtime::Errno::AlreadyExists, 20),
            (kobako_runtime::Errno::NotADirectory, 54),
            (kobako_runtime::Errno::IsADirectory, 31),
            (kobako_runtime::Errno::DirectoryNotEmpty, 55),
            (kobako_runtime::Errno::PermissionDenied, 2),
            (kobako_runtime::Errno::ReadOnlyFilesystem, 69),
            (kobako_runtime::Errno::InvalidInput, 28),
            (kobako_runtime::Errno::Unsupported, 58),
            (kobako_runtime::Errno::StorageFull, 51),
            (kobako_runtime::Errno::FileTooLarge, 22),
            (kobako_runtime::Errno::CrossesDevices, 75),
            (kobako_runtime::Errno::Escape, 76),
            (kobako_runtime::Errno::Deadlock, 16),
            (kobako_runtime::Errno::FilesystemLoop, 32),
            (kobako_runtime::Errno::NotPermitted, 63),
            (kobako_runtime::Errno::BadDescriptor, 8),
            (kobako_runtime::Errno::ResourceBusy, 10),
            (kobako_runtime::Errno::IllegalSequence, 25),
            (kobako_runtime::Errno::Interrupted, 27),
            (kobako_runtime::Errno::WouldBlock, 6),
            (kobako_runtime::Errno::TooManyOpenFiles, 33),
            (kobako_runtime::Errno::InvalidFilename, 37),
            (kobako_runtime::Errno::OutOfMemory, 48),
            (kobako_runtime::Errno::NotASocket, 57),
            (kobako_runtime::Errno::ValueTooLarge, 61),
            (kobako_runtime::Errno::NotSeekable, 70),
            (kobako_runtime::Errno::NoSuchDevice, 43),
            (kobako_runtime::Errno::StaleNetworkFileHandle, 72),
            (kobako_runtime::Errno::MessageTooLong, 35),
            (kobako_runtime::Errno::AlreadyInProgress, 7),
            (kobako_runtime::Errno::ConnectionRefused, 14),
            (kobako_runtime::Errno::NoBufferSpace, 42),
            (kobako_runtime::Errno::ProtocolNotAvailable, 50),
            (kobako_runtime::Errno::AddressFamilyNotSupported, 5),
            (kobako_runtime::Errno::Other("x".into()), 29),
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
