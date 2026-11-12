//! Why a call failed, under the POSIX name each failure stands for, as Linux's
//! calls answer an errno, whether a Filesystem, a socket, or a wait refused it.

use std::io;

/// Why a call failed, for the ABI above to encode as its error code.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Errno {
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
    /// A path that is absolute, climbs above the directory it is resolved
    /// beneath, or holds a link that does either, as `openat2`'s
    /// `RESOLVE_BENEATH` refuses one. Linux names it EXDEV as well, and an ABI
    /// may have a code of its own for it.
    Escape,
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
    /// A call to make again, as a walk a move landed in answers.
    WouldBlock,
    TooManyOpenFiles,
    /// A name too long for the Filesystem.
    InvalidFilename,
    OutOfMemory,
    NotASocket,
    /// A value too large for the type the call answers in.
    ValueTooLarge,
    NotSeekable,
    /// A call the kind of file it reached cannot take, as a device that
    /// allocates nothing.
    NoSuchDevice,
    /// A directory a lookup found standing elsewhere, which could not be moved
    /// to the name it was found under.
    StaleNetworkFileHandle,
    /// A peer that refused the connection.
    ConnectionRefused,
    /// A peer that reset the connection.
    ConnectionReset,
    /// A connection the host gave up on.
    ConnectionAborted,
    /// A connect or a read past its time.
    TimedOut,
    HostUnreachable,
    NetworkUnreachable,
    /// A network the host holds no working link to.
    NetworkDown,
    /// Buffer room the host ran out of for a socket.
    NoBufferSpace,
    /// A local address the host cannot hand out.
    AddressNotAvailable,
    /// A local address another socket holds.
    AddressInUse,
    /// A write to a connection whose writing side is shut down.
    BrokenPipe,
    /// A call that needs a connection on a socket that has none.
    NotConnected,
    /// A send on a datagram socket naming no peer.
    DestinationAddressRequired,
    /// A datagram past what one can carry.
    MessageTooLong,
    /// A connect on a socket already connected.
    AlreadyConnected,
    /// A connect still under way.
    InProgress,
    /// A connect asked again while one is under way.
    AlreadyInProgress,
    AddressFamilyNotSupported,
    /// A socket option its protocol does not have.
    ProtocolNotAvailable,
    /// Any other failure, with what the Filesystem knows of it.
    Other(String),
}

/// Each failure a Filesystem can name under the POSIX error name it stands for.
/// `ENOTSUP` comes before `EOPNOTSUPP`, a name some hosts give the same error,
/// so `Unsupported` answers to the first.
const POSIX_NAMES: [(&str, Errno); 48] = [
    ("EAGAIN", Errno::WouldBlock),
    ("ENOENT", Errno::NotFound),
    ("EEXIST", Errno::AlreadyExists),
    ("ENOTDIR", Errno::NotADirectory),
    ("EISDIR", Errno::IsADirectory),
    ("ENOTEMPTY", Errno::DirectoryNotEmpty),
    ("EACCES", Errno::PermissionDenied),
    ("EROFS", Errno::ReadOnlyFilesystem),
    ("EINVAL", Errno::InvalidInput),
    ("ENOTSUP", Errno::Unsupported),
    ("EOPNOTSUPP", Errno::Unsupported),
    ("ENOSPC", Errno::StorageFull),
    ("EFBIG", Errno::FileTooLarge),
    ("EXDEV", Errno::CrossesDevices),
    ("EDEADLK", Errno::Deadlock),
    ("ELOOP", Errno::FilesystemLoop),
    ("EPERM", Errno::NotPermitted),
    ("EBADF", Errno::BadDescriptor),
    ("EBUSY", Errno::ResourceBusy),
    ("EILSEQ", Errno::IllegalSequence),
    ("EINTR", Errno::Interrupted),
    ("EMFILE", Errno::TooManyOpenFiles),
    ("ENAMETOOLONG", Errno::InvalidFilename),
    ("ENOMEM", Errno::OutOfMemory),
    ("ENOTSOCK", Errno::NotASocket),
    ("EOVERFLOW", Errno::ValueTooLarge),
    ("ESPIPE", Errno::NotSeekable),
    ("ENODEV", Errno::NoSuchDevice),
    ("ESTALE", Errno::StaleNetworkFileHandle),
    ("ECONNREFUSED", Errno::ConnectionRefused),
    ("ECONNRESET", Errno::ConnectionReset),
    ("ECONNABORTED", Errno::ConnectionAborted),
    ("ETIMEDOUT", Errno::TimedOut),
    ("EHOSTUNREACH", Errno::HostUnreachable),
    ("ENETUNREACH", Errno::NetworkUnreachable),
    ("ENETDOWN", Errno::NetworkDown),
    ("ENOBUFS", Errno::NoBufferSpace),
    ("EADDRNOTAVAIL", Errno::AddressNotAvailable),
    ("EADDRINUSE", Errno::AddressInUse),
    ("EPIPE", Errno::BrokenPipe),
    ("ENOTCONN", Errno::NotConnected),
    ("EDESTADDRREQ", Errno::DestinationAddressRequired),
    ("EMSGSIZE", Errno::MessageTooLong),
    ("EISCONN", Errno::AlreadyConnected),
    ("EINPROGRESS", Errno::InProgress),
    ("EALREADY", Errno::AlreadyInProgress),
    ("EAFNOSUPPORT", Errno::AddressFamilyNotSupported),
    ("ENOPROTOOPT", Errno::ProtocolNotAvailable),
];

impl Errno {
    /// The failure a host call's `kind` stands for, as std names the errno it
    /// read; `None` for a kind std names no POSIX error for. A layer reading
    /// the host's own errno first, or knowing what its host library means by a
    /// kind, answers that before asking this.
    pub(crate) fn from_io_kind(kind: io::ErrorKind) -> Option<Self> {
        use io::ErrorKind as Kind;
        Some(match kind {
            Kind::NotFound => Self::NotFound,
            Kind::AlreadyExists => Self::AlreadyExists,
            Kind::NotADirectory => Self::NotADirectory,
            Kind::IsADirectory => Self::IsADirectory,
            Kind::DirectoryNotEmpty => Self::DirectoryNotEmpty,
            Kind::PermissionDenied => Self::PermissionDenied,
            Kind::ReadOnlyFilesystem => Self::ReadOnlyFilesystem,
            Kind::InvalidInput => Self::InvalidInput,
            Kind::Unsupported => Self::Unsupported,
            Kind::StorageFull => Self::StorageFull,
            Kind::FileTooLarge => Self::FileTooLarge,
            Kind::CrossesDevices => Self::CrossesDevices,
            Kind::Deadlock => Self::Deadlock,
            Kind::ResourceBusy => Self::ResourceBusy,
            Kind::InvalidFilename => Self::InvalidFilename,
            Kind::NotSeekable => Self::NotSeekable,
            Kind::Interrupted => Self::Interrupted,
            Kind::WouldBlock => Self::WouldBlock,
            Kind::OutOfMemory => Self::OutOfMemory,
            Kind::NetworkDown => Self::NetworkDown,
            Kind::ConnectionRefused => Self::ConnectionRefused,
            Kind::ConnectionReset => Self::ConnectionReset,
            Kind::ConnectionAborted => Self::ConnectionAborted,
            Kind::TimedOut => Self::TimedOut,
            Kind::HostUnreachable => Self::HostUnreachable,
            Kind::NetworkUnreachable => Self::NetworkUnreachable,
            Kind::AddrNotAvailable => Self::AddressNotAvailable,
            Kind::AddrInUse => Self::AddressInUse,
            Kind::BrokenPipe => Self::BrokenPipe,
            Kind::NotConnected => Self::NotConnected,
            _ => return None,
        })
    }

    /// The POSIX error name this failure stands for, as `ENOENT`. `Other`
    /// has none, and `Escape` shares `CrossesDevices`'s.
    pub fn posix_name(&self) -> Option<&'static str> {
        if *self == Self::Escape {
            return Some("EXDEV");
        }
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
                Errno::from_posix_name(name)
                    .as_ref()
                    .and_then(Errno::posix_name)
                    .and_then(Errno::from_posix_name),
                Some(error.clone()),
                "{name} through Errno::from_posix_name and posix_name must name {error:?} again"
            );
        }
        assert_eq!(
            (
                Errno::Unsupported.posix_name(),
                Errno::Other("x".into()).posix_name()
            ),
            (Some("ENOTSUP"), None),
            "Unsupported through Errno::posix_name must answer ENOTSUP, and Other none"
        );
    }
}
