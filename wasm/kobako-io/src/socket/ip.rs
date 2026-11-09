//! What `IPSocket` answers for an end, as CRuby's `ext/socket/ipsocket.c`
//! spells one as `["AF_INET", port, host, host]`.

use std::net::Ipv4Addr;

use beni::prelude::*;
use beni::{Error, IntoValue, Mrb, Qtrue, Symbol, Value};

use super::to_error;
use crate::io::argument_error;
use crate::wasi::wasi::sockets::network::{ErrorCode, IpSocketAddress, Ipv4SocketAddress};

/// What `addr` answers for `local`, a socket's own end: an unbound socket's
/// `invalid-state` stands for the unspecified address, as Linux's
/// `getsockname` answers an unbound one.
pub(super) fn local_addr_array(
    mrb: &Mrb,
    local: Result<IpSocketAddress, ErrorCode>,
    class: &str,
) -> Result<Value, Error> {
    match local {
        Ok(IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port,
            address: (a, b, c, d),
        })) => to_addr_array(mrb, (Ipv4Addr::new(a, b, c, d), port)),
        Ok(IpSocketAddress::Ipv6(_)) | Err(ErrorCode::InvalidState) => {
            to_addr_array(mrb, (Ipv4Addr::UNSPECIFIED, 0))
        }
        Err(code) => Err(to_error(mrb, code, class, "getsockname(2)")),
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

/// An address as CRuby's `["AF_INET", port, host, host]`.
pub(super) fn to_addr_array(mrb: &Mrb, (ip, port): (Ipv4Addr, u16)) -> Result<Value, Error> {
    let host = mrb.str_new(ip.to_string().as_bytes()).as_value();
    let addr = mrb.ary_new();
    addr.push(mrb, mrb.str_new(b"AF_INET").as_value())?;
    addr.push(mrb, i64::from(port).into_value(mrb))?;
    addr.push(mrb, host)?;
    addr.push(mrb, host)?;
    Ok(addr.as_value())
}
