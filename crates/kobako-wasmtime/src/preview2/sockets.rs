//! The translations between `wasi:sockets` and the runtime: an address, and
//! the error code each POSIX name maps to, as the WIT's "typical errors"
//! lists pair them.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use kobako_runtime::net::LookupError;
use kobako_runtime::Errno;

use super::bindings::wasi_sockets_network_0_2_12::{
    ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// A time in seconds as the nanoseconds WASI's `duration` counts.
pub(crate) fn to_nanos(secs: u32) -> u64 {
    u64::from(secs) * NANOS_PER_SEC
}

/// A keepalive time in whole seconds, a part of one rounding up as rustix's
/// `duration_to_secs` does for wasmtime, clamped to 1 through `max`, the
/// runtime's ceiling; zero has none, as WIT answers it `invalid-argument`.
pub(crate) fn to_keepalive_secs(nanos: u64, max: u32) -> Option<u32> {
    let secs = nanos.div_ceil(NANOS_PER_SEC).clamp(1, u64::from(max));
    (nanos != 0).then(|| u32::try_from(secs).expect("clamped within u32"))
}

/// The size a guest asked for, as a socket's queue holds twice it; wasmtime
/// halves Linux's answer the same way.
pub(crate) fn to_asked_size(held: u32) -> u64 {
    u64::from(held / 2)
}

/// The hop limit an `IP_TTL` read answers, which the runtime keeps within a
/// byte.
pub(crate) fn to_hop_limit(ttl: u32) -> u8 {
    u8::try_from(ttl).expect("the runtime keeps a hop limit within a byte")
}

/// A buffer size a set takes; zero has none, as WIT answers it
/// `invalid-argument`.
pub(crate) fn to_buffer_size(value: u64) -> Option<u32> {
    (value != 0).then(|| u32::try_from(value).unwrap_or(u32::MAX))
}

/// The address the guest named for a socket of `family`, once it is of that
/// family, as wasmtime's `is_valid_address_family` asks; an address of the
/// other family has no answer. The runtime makes no IPv6 socket, so an IPv4
/// one is the only socket an address reaches.
pub(crate) fn to_socket_addr(
    addr: &IpSocketAddress,
    family: IpAddressFamily,
) -> Option<SocketAddr> {
    match (family, addr) {
        (
            IpAddressFamily::Ipv4,
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (a, b, c, d),
            }),
        ) => Some(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(*a, *b, *c, *d),
            *port,
        ))),
        _ => None,
    }
}

/// Whether a TCP socket may bind or connect to `addr`, as wasmtime's
/// `is_valid_unicast_address` asks only of TCP: no multicast and no broadcast
/// address.
pub(crate) fn is_valid_unicast_address(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(v4) => !v4.ip().is_multicast() && !v4.ip().is_broadcast(),
        SocketAddr::V6(v6) => !v6.ip().is_multicast(),
    }
}

/// The peer the guest named, if it may be reached, as wasmtime's
/// `is_valid_remote_address` asks: neither unspecified nor at port 0.
pub(crate) fn to_remote_addr(
    addr: &IpSocketAddress,
    family: IpAddressFamily,
) -> Option<SocketAddr> {
    to_socket_addr(addr, family).filter(|addr| !addr.ip().is_unspecified() && addr.port() != 0)
}

/// `ip` as the guest reads it.
pub(crate) fn to_ip_address(ip: IpAddr) -> IpAddress {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, d] = v4.octets();
            IpAddress::Ipv4((a, b, c, d))
        }
        IpAddr::V6(v6) => {
            let [a, b, c, d, e, f, g, h] = v6.segments();
            IpAddress::Ipv6((a, b, c, d, e, f, g, h))
        }
    }
}

/// `addr` as the guest reads it.
pub(crate) fn from_socket_addr(addr: SocketAddr) -> IpSocketAddress {
    match addr {
        SocketAddr::V4(v4) => {
            let [a, b, c, d] = v4.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: v4.port(),
                address: (a, b, c, d),
            })
        }
        SocketAddr::V6(v6) => {
            let [a, b, c, d, e, f, g, h] = v6.ip().segments();
            IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port: v6.port(),
                flow_info: v6.flowinfo(),
                address: (a, b, c, d, e, f, g, h),
                scope_id: v6.scope_id(),
            })
        }
    }
}

/// The error code a lookup's failure answers.
pub(crate) fn to_lookup_error_code(err: LookupError) -> ErrorCode {
    match err {
        LookupError::InvalidName => ErrorCode::InvalidArgument,
        LookupError::NameUnresolvable => ErrorCode::NameUnresolvable,
        LookupError::TemporaryFailure => ErrorCode::TemporaryResolverFailure,
        _ => ErrorCode::PermanentResolverFailure,
    }
}

/// The error code a failure's POSIX name maps to.
pub(crate) fn to_error_code(err: &Errno) -> ErrorCode {
    match err {
        Errno::PermissionDenied | Errno::NotPermitted => ErrorCode::AccessDenied,
        Errno::Unsupported | Errno::AddressFamilyNotSupported | Errno::ProtocolNotAvailable => {
            ErrorCode::NotSupported
        }
        Errno::InvalidInput => ErrorCode::InvalidArgument,
        Errno::OutOfMemory | Errno::NoBufferSpace => ErrorCode::OutOfMemory,
        Errno::TimedOut => ErrorCode::Timeout,
        Errno::WouldBlock => ErrorCode::WouldBlock,
        Errno::TooManyOpenFiles => ErrorCode::NewSocketLimit,
        Errno::AddressNotAvailable => ErrorCode::AddressNotBindable,
        Errno::AddressInUse => ErrorCode::AddressInUse,
        Errno::HostUnreachable | Errno::NetworkUnreachable | Errno::NetworkDown => {
            ErrorCode::RemoteUnreachable
        }
        Errno::ConnectionRefused => ErrorCode::ConnectionRefused,
        Errno::ConnectionReset => ErrorCode::ConnectionReset,
        Errno::ConnectionAborted => ErrorCode::ConnectionAborted,
        Errno::NotConnected | Errno::AlreadyConnected | Errno::DestinationAddressRequired => {
            ErrorCode::InvalidState
        }
        Errno::MessageTooLong => ErrorCode::DatagramTooLarge,
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
                Errno::BrokenPipe,
                Errno::AddressInUse,
                Errno::ConnectionReset,
                Errno::NetworkDown,
                Errno::NoBufferSpace,
                Errno::DestinationAddressRequired,
                Errno::ProtocolNotAvailable,
            ]
            .map(|err| to_error_code(&err)),
            [
                ErrorCode::Unknown,
                ErrorCode::AddressInUse,
                ErrorCode::ConnectionReset,
                ErrorCode::RemoteUnreachable,
                ErrorCode::OutOfMemory,
                ErrorCode::InvalidState,
                ErrorCode::NotSupported,
            ],
            "EPIPE, EADDRINUSE, ECONNRESET, ENETDOWN, ENOBUFS, EDESTADDRREQ, and ENOPROTOOPT \
             must answer unknown, address-in-use, connection-reset, remote-unreachable, \
             out-of-memory, invalid-state, and not-supported, as wasmtime-wasi maps them"
        );
    }
}
