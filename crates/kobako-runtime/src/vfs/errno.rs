//! Why a call on a Filesystem failed, under the POSIX name each failure
//! stands for, as Linux's calls answer an errno.

use crate::locking::Deadlock;

/// Why a Filesystem call failed, for the ABI above to encode as its error code.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilesystemError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    PermissionDenied,
    ReadOnlyFilesystem,
    InvalidInput,
    Unsupported,
    /// A change past what the Filesystem can hold.
    StorageFull,
    /// A file grown past the largest the Filesystem keeps.
    FileTooLarge,
    /// A move that cannot happen in one step, as between two devices.
    CrossesDevices,
    /// A call that would wait on a lock its own thread holds.
    Deadlock,
    /// A symbolic link where the call asked to follow none.
    FilesystemLoop,
    /// A call only a privileged caller may make, as apart from one the
    /// file's permissions deny.
    NotPermitted,
    BadDescriptor,
    ResourceBusy,
    /// Bytes that are no valid character where the call needs one.
    IllegalSequence,
    Interrupted,
    TooManyOpenFiles,
    /// A name too long for the Filesystem.
    InvalidFilename,
    OutOfMemory,
    NotASocket,
    /// A value too large for the type the call answers in.
    ValueTooLarge,
    NotSeekable,
    /// Any other failure, with what the Filesystem knows of it.
    Other(String),
}

/// Each failure under the POSIX error name it stands for. `ENOTSUP` comes
/// before `EOPNOTSUPP`, a name some hosts give the same error, so
/// `Unsupported` answers to the first.
const POSIX_NAMES: [(&str, FilesystemError); 26] = [
    ("ENOENT", FilesystemError::NotFound),
    ("EEXIST", FilesystemError::AlreadyExists),
    ("ENOTDIR", FilesystemError::NotADirectory),
    ("EISDIR", FilesystemError::IsADirectory),
    ("ENOTEMPTY", FilesystemError::DirectoryNotEmpty),
    ("EACCES", FilesystemError::PermissionDenied),
    ("EROFS", FilesystemError::ReadOnlyFilesystem),
    ("EINVAL", FilesystemError::InvalidInput),
    ("ENOTSUP", FilesystemError::Unsupported),
    ("EOPNOTSUPP", FilesystemError::Unsupported),
    ("ENOSPC", FilesystemError::StorageFull),
    ("EFBIG", FilesystemError::FileTooLarge),
    ("EXDEV", FilesystemError::CrossesDevices),
    ("EDEADLK", FilesystemError::Deadlock),
    ("ELOOP", FilesystemError::FilesystemLoop),
    ("EPERM", FilesystemError::NotPermitted),
    ("EBADF", FilesystemError::BadDescriptor),
    ("EBUSY", FilesystemError::ResourceBusy),
    ("EILSEQ", FilesystemError::IllegalSequence),
    ("EINTR", FilesystemError::Interrupted),
    ("EMFILE", FilesystemError::TooManyOpenFiles),
    ("ENAMETOOLONG", FilesystemError::InvalidFilename),
    ("ENOMEM", FilesystemError::OutOfMemory),
    ("ENOTSOCK", FilesystemError::NotASocket),
    ("EOVERFLOW", FilesystemError::ValueTooLarge),
    ("ESPIPE", FilesystemError::NotSeekable),
];

impl From<Deadlock> for FilesystemError {
    fn from(_: Deadlock) -> Self {
        Self::Deadlock
    }
}

impl FilesystemError {
    /// The POSIX error name this failure stands for, as `ENOENT`. `Other`
    /// has none.
    pub fn posix_name(&self) -> Option<&'static str> {
        POSIX_NAMES
            .iter()
            .find(|(_, error)| error == self)
            .map(|(name, _)| *name)
    }

    /// The failure a POSIX error name stands for, if it is one of these.
    pub fn from_posix_name(name: &str) -> Option<Self> {
        POSIX_NAMES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, error)| error.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_posix_name_answers_back_to_its_failure() {
        for (name, error) in &POSIX_NAMES {
            assert_eq!(
                FilesystemError::from_posix_name(name)
                    .as_ref()
                    .and_then(FilesystemError::posix_name)
                    .and_then(FilesystemError::from_posix_name),
                Some(error.clone()),
                "{name} through FilesystemError::from_posix_name and posix_name must name {error:?} again"
            );
        }
        assert_eq!(
            (
                FilesystemError::Unsupported.posix_name(),
                FilesystemError::Other("x".into()).posix_name()
            ),
            (Some("ENOTSUP"), None),
            "Unsupported through FilesystemError::posix_name must answer ENOTSUP, and Other none"
        );
    }
}
