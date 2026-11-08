//! The translations between `wasi:sockets` and the runtime: an address, and
//! the error code each POSIX name maps to, as the WIT's "typical errors"
//! lists pair them.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use kobako_runtime::net::LookupError;
use kobako_runtime::vfs::FilesystemError;

use super::bindings::wasi_sockets_network_0_2_12::{ErrorCode, IpSocketAddress, Ipv4SocketAddress};

/// The IPv4 address the guest named; an IPv6 one has no answer, since the
/// guest is IPv4-only.
pub(crate) fn to_socket_addr(addr: &IpSocketAddress) -> Option<SocketAddr> {
    match addr {
        IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port,
            address: (a, b, c, d),
        }) => Some(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(*a, *b, *c, *d),
            *port,
        ))),
        IpSocketAddress::Ipv6(_) => None,
    }
}

/// `addr` as the guest reads it; an IPv6 one reads as the unspecified IPv4
/// address, since the guest sees none.
pub(crate) fn from_socket_addr(addr: SocketAddr) -> IpSocketAddress {
    let (ip, port) = match addr {
        SocketAddr::V4(v4) => (*v4.ip(), v4.port()),
        SocketAddr::V6(v6) => (Ipv4Addr::UNSPECIFIED, v6.port()),
    };
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}

/// The error code a lookup's failure answers. A name no A-label spells is
/// refused as it is handed over, as an invalid argument; the same failure from
/// a lookup's answer is a name that resolves to nothing.
pub(crate) fn to_lookup_error_code(err: LookupError, handed_over: bool) -> ErrorCode {
    match err {
        LookupError::NameUnresolvable if handed_over => ErrorCode::InvalidArgument,
        LookupError::NameUnresolvable => ErrorCode::NameUnresolvable,
        LookupError::TemporaryFailure => ErrorCode::TemporaryResolverFailure,
        _ => ErrorCode::PermanentResolverFailure,
    }
}

/// The error code a failure's POSIX name maps to.
pub(crate) fn to_error_code(err: &FilesystemError) -> ErrorCode {
    match err {
        FilesystemError::PermissionDenied | FilesystemError::NotPermitted => {
            ErrorCode::AccessDenied
        }
        FilesystemError::Unsupported | FilesystemError::AddressFamilyNotSupported => {
            ErrorCode::NotSupported
        }
        FilesystemError::InvalidInput => ErrorCode::InvalidArgument,
        FilesystemError::OutOfMemory => ErrorCode::OutOfMemory,
        FilesystemError::TimedOut => ErrorCode::Timeout,
        FilesystemError::WouldBlock => ErrorCode::WouldBlock,
        FilesystemError::TooManyOpenFiles => ErrorCode::NewSocketLimit,
        FilesystemError::AddressNotAvailable => ErrorCode::AddressNotBindable,
        FilesystemError::AddressInUse => ErrorCode::AddressInUse,
        FilesystemError::HostUnreachable | FilesystemError::NetworkUnreachable => {
            ErrorCode::RemoteUnreachable
        }
        FilesystemError::ConnectionRefused => ErrorCode::ConnectionRefused,
        // WASI 0.2 names no broken pipe, so EPIPE answers `unknown`, as
        // wasmtime-wasi's preview2 does.
        FilesystemError::ConnectionReset => ErrorCode::ConnectionReset,
        FilesystemError::ConnectionAborted => ErrorCode::ConnectionAborted,
        FilesystemError::NotConnected | FilesystemError::AlreadyConnected => {
            ErrorCode::InvalidState
        }
        _ => ErrorCode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-074
    #[test]
    fn a_host_failure_answers_the_code_wasmtime_wasi_gives_it() {
        assert_eq!(
            [
                FilesystemError::BrokenPipe,
                FilesystemError::AddressInUse,
                FilesystemError::ConnectionReset,
            ]
            .map(|err| to_error_code(&err)),
            [
                ErrorCode::Unknown,
                ErrorCode::AddressInUse,
                ErrorCode::ConnectionReset
            ],
            "EPIPE, EADDRINUSE, and ECONNRESET must answer unknown, address-in-use, and \
             connection-reset, as wasmtime-wasi's preview2 maps them"
        );
    }
}
