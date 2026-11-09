//! The translations between `wasi:sockets` and the runtime: an address, and
//! the error code each POSIX name maps to, as the WIT's "typical errors"
//! lists pair them.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use kobako_runtime::net::LookupError;
use kobako_runtime::vfs::FilesystemError;

use super::bindings::wasi_sockets_network_0_2_12::{ErrorCode, IpSocketAddress, Ipv4SocketAddress};

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// The longest keepalive time and the most probes, Linux's `MAX_TCP_KEEPIDLE`
/// and `MAX_TCP_KEEPCNT`, which wasmtime clamps to as the lowest across hosts.
const MAX_KEEPALIVE_SECS: u64 = 32767;
pub(crate) const MAX_KEEPALIVE_COUNT: u32 = 127;

/// A time in seconds as the nanoseconds WASI's `duration` counts.
pub(crate) fn to_nanos(secs: u32) -> u64 {
    u64::from(secs) * NANOS_PER_SEC
}

/// A keepalive time in whole seconds, clamped to 1 through Linux's ceiling
/// as wasmtime clamps it; zero has none, as WIT answers it `invalid-argument`.
pub(crate) fn to_keepalive_secs(nanos: u64) -> Option<u32> {
    let secs = (nanos / NANOS_PER_SEC).clamp(1, MAX_KEEPALIVE_SECS);
    (nanos != 0).then(|| u32::try_from(secs).expect("clamped within u32"))
}

/// The size a guest asked for, as a socket's queue holds twice it; wasmtime
/// halves Linux's answer the same way.
pub(crate) fn to_asked_size(held: usize) -> u64 {
    u64::try_from(held / 2).unwrap_or(u64::MAX)
}

/// A buffer size a set takes; zero has none, as WIT answers it
/// `invalid-argument`.
pub(crate) fn to_buffer_size(value: u64) -> Option<usize> {
    (value != 0).then(|| usize::try_from(value).unwrap_or(usize::MAX))
}

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

/// Whether `addr` may be bound to, as wasmtime's `is_valid_unicast_address`
/// asks: no multicast and no broadcast address.
pub(crate) fn is_valid_unicast_address(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(v4) => !v4.ip().is_multicast() && !v4.ip().is_broadcast(),
        SocketAddr::V6(v6) => !v6.ip().is_multicast(),
    }
}

/// Whether `addr` may be reached, as wasmtime's `is_valid_remote_address`
/// asks with `is_valid_unicast_address`: a unicast address, neither
/// unspecified nor at port 0.
pub(crate) fn is_valid_remote_address(addr: SocketAddr) -> bool {
    is_valid_unicast_address(addr) && !addr.ip().is_unspecified() && addr.port() != 0
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
        FilesystemError::ConnectionReset => ErrorCode::ConnectionReset,
        FilesystemError::ConnectionAborted => ErrorCode::ConnectionAborted,
        FilesystemError::NotConnected | FilesystemError::AlreadyConnected => {
            ErrorCode::InvalidState
        }
        FilesystemError::MessageTooLong => ErrorCode::DatagramTooLarge,
        // WASI 0.2 names no broken pipe, so EPIPE answers `unknown`, as
        // wasmtime-wasi's preview2 does.
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
