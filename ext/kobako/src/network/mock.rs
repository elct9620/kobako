//! `Kobako::Network::Mock`, standing for the runtime's `Mock`: the table a
//! Host App's test writes in Ruby, answered in Rust. Nothing in it calls
//! back into Ruby, so its lookups and dials answer on any thread.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{
    function, method, prelude::*, Error as MagnusError, RArray, RClass, RHash, RModule, Ruby, Value,
};

use kobako_runtime::net::{Backends, Mock};

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
    mock.define_method("answer", method!(answer, -1))?;
    mock.define_method("route", method!(route, -1))?;
    mock.define_method("refuse", method!(refuse, 3))?;
    mock.define_method("hold", method!(hold, 1))?;
    mock.define_method("lookups", method!(lookups, 0))?;
    mock.define_method("dials", method!(dials, 0))?;
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
        Some(None) => Mock::default().without_local_addresses(),
        Some(Some(addresses)) => Mock::default().with_local_addresses(to_ips(addresses)?),
    };
    Ok(MockNet(Arc::new(mock)))
}

/// `answer(name, *addresses)`.
fn answer(rb_self: &MockNet, args: &[Value]) -> Result<(), MagnusError> {
    let args = scan_args::<(String,), (), RArray, (), (), ()>(args)?;
    let (name,) = args.required;
    rb_self.0.answer(&name, to_ips(args.splat)?);
    Ok(())
}

/// `route(address, port, to: [address, port])`.
fn route(rb_self: &MockNet, args: &[Value]) -> Result<(), MagnusError> {
    let args = scan_args::<(String, u16), (), (), (), RHash, ()>(args)?;
    let (address, port) = args.required;
    let kwargs = get_kwargs::<_, ((String, u16),), (), ()>(args.keywords, &["to"], &[])?;
    let (to_address, to_port) = kwargs.required.0;
    rb_self.0.route(
        SocketAddr::new(to_ip(&address)?, port),
        SocketAddr::new(to_ip(&to_address)?, to_port),
    );
    Ok(())
}

/// `refuse(address, port, errno)`, the errno an Integer or a
/// `SystemCallError` class, read as the host's own number.
fn refuse(rb_self: &MockNet, address: String, port: u16, errno: Value) -> Result<(), MagnusError> {
    let errno = match RClass::from_value(errno) {
        Some(class) => class.const_get::<_, i32>("Errno")?,
        None => i32::try_convert(errno)?,
    };
    rb_self
        .0
        .refuse(SocketAddr::new(to_ip(&address)?, port), errno);
    Ok(())
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
    let _hold = rb_self.0.hold(&name);
    ruby.yield_values(())
}

/// Each name the guest's lookups asked, in order.
fn lookups(rb_self: &MockNet) -> Vec<String> {
    rb_self.0.lookups()
}

/// Each address dialed, in order, as `[address, port]`.
fn dials(rb_self: &MockNet) -> Vec<(String, u16)> {
    rb_self
        .0
        .dials()
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
