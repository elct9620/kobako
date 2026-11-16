//! What `IPSocket` answers for an end, as CRuby's `ext/socket/ipsocket.c`
//! spells one as `["AF_INET", port, host, host]`.

use std::net::{IpAddr, Ipv4Addr};

use beni::prelude::*;
use beni::{Error, IntoValue, Mrb, Qtrue, RArray, RClass, Symbol, Value};

use super::address::{addresses_of, ensure_named, host_text_of, ipv4_end_of, node_of};
use super::basic::{open_socket_of, receive_past_buffer, PastBuffer, Wait};
use super::to_error;
use crate::io::argument_error;
use crate::wasi::wasi::sockets::network::{ErrorCode, IpSocketAddress};

/// Define `IPSocket.getaddress`, `IPSocket#addr`, `IPSocket#peeraddr`, and
/// `IPSocket#recvfrom`.
pub(super) fn init(mrb: &Mrb, ip: RClass) -> Result<(), Error> {
    ip.define_singleton_method(mrb, c"getaddress", beni::method!(ip_socket_s_getaddress, 1))?;
    ip.define_method(mrb, c"addr", beni::method!(ip_socket_addr, -1))?;
    ip.define_method(mrb, c"peeraddr", beni::method!(ip_socket_peeraddr, -1))?;
    ip.define_method(mrb, c"recvfrom", beni::method!(ip_socket_recvfrom, -1))?;
    Ok(())
}

/// `recvfrom(maxlen, flags = 0, outbuf = nil)` answers what comes next, cut
/// to `maxlen`, with its sender as `[mesg, ["AF_INET", port, host, host]]`;
/// a stream names no sender, so a `TCPSocket` answers `[mesg, nil]`, as
/// CRuby's `ip_recvfrom` does.
fn ip_socket_recvfrom(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    receive_past_buffer(mrb, self_, args, PastBuffer::new(Wait::Blocking, true))
}

/// `addr(reverse_lookup = nil)` answers the socket's own end: the virtual
/// address and port the host hands the guest for it.
fn ip_socket_addr(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    ensure_reverse_lookup_args(mrb, args)?;
    Ok(to_addr_array(mrb, open_socket_of(mrb, self_)?.local_end(mrb)?)?.as_value())
}

/// `peeraddr(reverse_lookup = nil)` answers the peer, as the guest named it.
/// The guest names no host, so a reverse lookup asked for answers the
/// address.
fn ip_socket_peeraddr(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    ensure_reverse_lookup_args(mrb, args)?;
    Ok(to_addr_array(mrb, open_socket_of(mrb, self_)?.peer_end(mrb)?)?.as_value())
}

/// `addr` and `peeraddr` take at most a `reverse_lookup` flag.
fn ensure_reverse_lookup_args(mrb: &Mrb, args: &[Value]) -> Result<(), Error> {
    // CRuby's `ip_addr` reads the first argument alone and leaves the rest.
    match args.first() {
        None => Ok(()),
        Some(flag) => ensure_reverse_lookup_flag(mrb, *flag),
    }
}

/// A socket's own end as `getsockname` answers `local`: an unbound socket's
/// `invalid-state` stands for the unspecified address, as Linux's answers an
/// unbound one.
pub(super) fn local_end_of(
    mrb: &Mrb,
    local: Result<IpSocketAddress, ErrorCode>,
) -> Result<(Ipv4Addr, u16), Error> {
    match local {
        Ok(local) => Ok(ipv4_end_of(&local).unwrap_or((Ipv4Addr::UNSPECIFIED, 0))),
        Err(ErrorCode::InvalidState) => Ok((Ipv4Addr::UNSPECIFIED, 0)),
        Err(code) => Err(to_error(mrb, code, "getsockname(2)")),
    }
}

/// Refuses a `reverse_lookup` flag CRuby's refuses: one of `nil`, `true`,
/// `false`, `:hostname`, or `:numeric` passes.
pub(super) fn ensure_reverse_lookup_flag(mrb: &Mrb, flag: Value) -> Result<(), Error> {
    if !flag.to_bool() || Qtrue::from_value(flag).is_some() {
        return Ok(());
    }
    let Some(symbol) = Symbol::from_value(flag) else {
        let message = format!(
            "wrong argument type {} (expected Symbol)",
            flag.classname(mrb)
        );
        return Err(Error::new(mrb, mrb.exception_type_error()?, &message));
    };
    match symbol.name(mrb)?.as_str() {
        "hostname" | "numeric" => Ok(()),
        _ => {
            let message = format!("invalid reverse_lookup flag: {}", flag.inspect(mrb));
            Err(argument_error(mrb, &message))
        }
    }
}

/// An address as CRuby's `["AF_INET", port, host, host]`, or `"AF_INET6"`
/// for an IPv6 one.
pub(super) fn to_addr_array(
    mrb: &Mrb,
    (ip, port): (impl Into<IpAddr>, u16),
) -> Result<RArray, Error> {
    let ip = ip.into();
    let family: &[u8] = if ip.is_ipv4() {
        b"AF_INET"
    } else {
        b"AF_INET6"
    };
    let host = mrb.str_new(ip.to_string().as_bytes()).as_value();
    let addr = mrb.ary_new();
    addr.push(mrb, mrb.str_new(family).as_value())?;
    addr.push(mrb, i64::from(port).into_value(mrb))?;
    addr.push(mrb, host)?;
    addr.push(mrb, host)?;
    Ok(addr)
}

/// `IPSocket.getaddress(host)` answers the first address `host` leads to, as
/// CRuby's looks it up and spells the first answer; a name answers its
/// virtual address.
fn ip_socket_s_getaddress(mrb: &Mrb, _self: Value, host: Value) -> Result<Value, Error> {
    let host_text = host_text_of(mrb, host)?;
    ensure_named(mrb, &host_text, &None)?;
    let ip = addresses_of(mrb, &node_of(host_text, false))?[0];
    Ok(mrb.str_new(ip.to_string().as_bytes()).as_value())
}
