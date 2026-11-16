//! `Kobako::Network::Mock`, standing for the runtime's `Mock`: the table a
//! Host App's test writes in Ruby, answered in Rust. Nothing in it calls
//! back into Ruby, so its lookups and dials answer on any thread.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{
    function, method, prelude::*, Error as MagnusError, RArray, RClass, RHash, RModule, Ruby, Value,
};

use kobako_runtime::net::{Backends, IpProtocol, Mock};
use kobako_runtime::network::Protocol;

use super::{symbol_name_of, value_by_name, PROTOCOL_NAMES};
use crate::errors::errno_of;

#[magnus::wrap(class = "Kobako::Network::Mock", free_immediately, size)]
pub(crate) struct MockNet(Arc<Mock>);

impl MockNet {
    /// The backends a Sandbox built with this Mock dials and resolves
    /// through.
    pub(crate) fn backends(&self) -> Backends {
        Backends::new(self.0.clone(), self.0.clone())
    }
}

pub(super) fn register(network: RModule) -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let mock = network.define_class("Mock", ruby.class_object())?;
    mock.define_singleton_method("new", function!(mock_new, -1))?;
    mock.define_method("resolve", method!(resolve, -1))?;
    mock.define_method("route", method!(route, -1))?;
    mock.define_method("refuse", method!(refuse, -1))?;
    mock.define_method("hold", method!(hold, 1))?;
    mock.define_method("lookups", method!(lookups, 0))?;
    mock.define_method("dialed_addresses", method!(dialed_addresses, 0))?;
    Ok(())
}

/// `Mock.new(local_addresses: [])`: the host's own addresses, or `nil` for
/// a host that cannot say them.
fn mock_new(args: &[Value]) -> Result<MockNet, MagnusError> {
    let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
    let kwargs = get_kwargs::<_, (), (Option<Option<RArray>>,), ()>(
        args.keywords,
        &[],
        &["local_addresses"],
    )?;
    let mock = match kwargs.optional.0 {
        None => Mock::default(),
        Some(addresses) => Mock::default().with_local_addresses(addresses.map(to_ips).transpose()?),
    };
    Ok(MockNet(Arc::new(mock)))
}

/// `resolve(name, to: address)`, or `to: [address, ...]`.
fn resolve(rb_self: &MockNet, args: &[Value]) -> Result<(), MagnusError> {
    let args = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
    let (name,) = args.required;
    let kwargs = get_kwargs::<_, (Value,), (), ()>(args.keywords, &["to"], &[])?;
    let to = kwargs.required.0;
    let addresses = match RArray::from_value(to) {
        Some(addresses) => to_ips(addresses)?,
        None => vec![to_ip(&String::try_convert(to)?)?],
    };
    rb_self
        .0
        .resolve_to_addrs(&name, addresses)
        .map_err(|_| to_name_error(&name))
}

/// `route(address, port, to: [address, port], protocol: :tcp)`.
fn route(rb_self: &MockNet, args: &[Value]) -> Result<(), MagnusError> {
    let args = scan_args::<(String, u16), (), (), (), RHash, ()>(args)?;
    let (address, port) = args.required;
    let kwargs = get_kwargs::<_, ((String, u16),), (Option<Value>,), ()>(
        args.keywords,
        &["to"],
        &["protocol"],
    )?;
    let (to_address, to_port) = kwargs.required.0;
    rb_self.0.route(
        to_protocol(kwargs.optional.0)?,
        SocketAddr::new(to_ip(&address)?, port),
        SocketAddr::new(to_ip(&to_address)?, to_port),
    );
    Ok(())
}

/// `refuse(address, port, errno, protocol: :tcp)`, the errno an `Errno`
/// class read by its POSIX name, since the number behind one differs between
/// hosts.
fn refuse(rb_self: &MockNet, args: &[Value]) -> Result<(), MagnusError> {
    let args = scan_args::<(String, u16, RClass), (), (), (), RHash, ()>(args)?;
    let (address, port, errno) = args.required;
    let kwargs = get_kwargs::<_, (), (Option<Value>,), ()>(args.keywords, &[], &["protocol"])?;
    let err = errno_of(errno).ok_or_else(|| {
        let ruby = Ruby::get().expect("Ruby thread");
        MagnusError::new(
            ruby.exception_arg_error(),
            format!("kobako answers no errno {}", errno.inspect()),
        )
    })?;
    rb_self.0.refuse(
        to_protocol(kwargs.optional.0)?,
        SocketAddr::new(to_ip(&address)?, port),
        err,
    );
    Ok(())
}

/// The protocol a route or a refusal keys on, `:tcp` unless written, by the
/// name an endpoint's protocol has; HTTP is no protocol a dial keys on.
fn to_protocol(protocol: Option<Value>) -> Result<IpProtocol, MagnusError> {
    let Some(protocol) = protocol else {
        return Ok(IpProtocol::Tcp);
    };
    symbol_name_of(protocol)?
        .and_then(|name| value_by_name(&PROTOCOL_NAMES, &name))
        .and_then(|protocol| match protocol {
            Protocol::Tcp => Some(IpProtocol::Tcp),
            Protocol::Udp => Some(IpProtocol::Udp),
            _ => None,
        })
        .ok_or_else(|| {
            let ruby = Ruby::get().expect("Ruby thread");
            MagnusError::new(ruby.exception_arg_error(), "protocol must be :tcp or :udp")
        })
}

/// `hold(name) { ... }`: the name answers no lookup until the block
/// returns or raises.
fn hold(ruby: &Ruby, rb_self: &MockNet, name: String) -> Result<Value, MagnusError> {
    if !ruby.block_given() {
        return Err(MagnusError::new(
            ruby.exception_arg_error(),
            "hold needs a block, which the hold lasts for",
        ));
    }
    let _hold = rb_self.0.hold(&name).map_err(|_| to_name_error(&name))?;
    ruby.yield_values(())
}

/// The `ArgumentError` for `name`, which no guest's lookup asks.
fn to_name_error(name: &str) -> MagnusError {
    let ruby = Ruby::get().expect("Ruby thread");
    MagnusError::new(
        ruby.exception_arg_error(),
        format!("not a name a lookup asks: {name:?}"),
    )
}

/// Each name the guest's lookups asked, in order.
fn lookups(rb_self: &MockNet) -> Vec<String> {
    rb_self.0.lookups()
}

/// Each address dialed, in order, as `[address, port]`.
fn dialed_addresses(rb_self: &MockNet) -> Vec<(String, u16)> {
    rb_self
        .0
        .dialed_addresses()
        .into_iter()
        .map(|addr| (addr.ip().to_string(), addr.port()))
        .collect()
}

fn to_ips(addresses: RArray) -> Result<Vec<IpAddr>, MagnusError> {
    addresses
        .into_iter()
        .map(|address| to_ip(&String::try_convert(address)?))
        .collect()
}

/// An address the Host App wrote, or an `ArgumentError` naming it.
fn to_ip(address: &str) -> Result<IpAddr, MagnusError> {
    address.parse().map_err(|_| {
        let ruby = Ruby::get().expect("Ruby thread");
        MagnusError::new(
            ruby.exception_arg_error(),
            format!("not an IP address: {address:?}"),
        )
    })
}
