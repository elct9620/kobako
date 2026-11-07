//! A Sandbox's network entries as contract Grants. Each entry's endpoints are
//! read from Ruby as an invocation starts, on the guest thread holding the
//! GVL, so the runtime judges them as data on any thread; its policies are
//! asked on the guest thread, re-entering Ruby as a mount's block does.

use std::net::IpAddr;
use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{
    prelude::*, Error as MagnusError, RArray, RClass, RModule, RString, Ruby, Symbol, Value,
};

use kobako_runtime::grant::{Grant, Policy};
use kobako_runtime::network::{
    Addresses, Connection, Endpoint, Entry, Name, Network, Policies, Protocol,
};

use super::pinned_value::PinnedValue;
use super::{escape, gvl};

/// What one entry answered for this invocation: its endpoints, and its own
/// `resolve?` and `connect?` for each it plays.
struct Answered {
    endpoints: Vec<Endpoint>,
    resolve: Option<Arc<RubyPolicy>>,
    connect: Option<Arc<RubyPolicy>>,
}

impl Entry for Answered {
    fn endpoints(&self) -> Vec<Endpoint> {
        self.endpoints.clone()
    }

    fn policies(&self) -> Policies {
        let mut policies = Policies::default();
        if let Some(resolve) = &self.resolve {
            policies = policies.with_resolve(resolve.clone());
        }
        if let Some(connect) = &self.connect {
            policies = policies.with_connect(connect.clone());
        }
        policies
    }
}

/// An entry asked one of its policy hooks: `resolve?` handed the name, and
/// `connect?` a `Kobako::Network::Connection`. A falsy answer or a raised
/// StandardError refuses, so the guest's call fails and its script goes on;
/// anything else it raises is held for the host.
struct RubyPolicy {
    entry: Opaque<Value>,
}

impl RubyPolicy {
    fn ask(&self, hook: &str, request: impl FnOnce(&Ruby) -> Result<Value, MagnusError>) -> bool {
        gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable in an entry's policy");
            let entry: Value = ruby.get_inner(self.entry);
            request(&ruby)
                .and_then(|request| entry.funcall::<_, _, Value>(hook, (request,)))
                .map(|answer| answer.to_bool())
                .unwrap_or_else(|error| {
                    let _ = escape::hold(&ruby, error);
                    false
                })
        })
    }
}

impl Policy<Name<'_>> for RubyPolicy {
    fn allows(&self, request: &Name<'_>) -> bool {
        self.ask("resolve?", |ruby| Ok(ruby.str_new(request.name).as_value()))
    }
}

impl Policy<Connection<'_>> for RubyPolicy {
    fn allows(&self, request: &Connection<'_>) -> bool {
        self.ask("connect?", |ruby| to_connection(ruby, request))
    }
}

/// `request` as the `Kobako::Network::Connection` a Ruby policy reads.
fn to_connection(ruby: &Ruby, request: &Connection<'_>) -> Result<Value, MagnusError> {
    let class: RClass = ruby
        .class_object()
        .const_get::<_, RModule>("Kobako")?
        .const_get::<_, RModule>("Network")?
        .const_get("Connection")?;
    let protocol = ruby.to_symbol(match request.protocol {
        Protocol::Http => "http",
        Protocol::Tcp => "tcp",
        _ => "udp",
    });
    class.funcall("new", (ruby.str_new(request.host), request.port, protocol))
}

/// One invocation's network Grants, from the Sandbox's entries as
/// `[name, entry]` pairs, beside the pins keeping each entry playing a policy
/// in place while the guest may ask it; the caller holds both until the
/// invocation returns.
pub(super) fn to_grants(
    ruby: &Ruby,
    entries: RArray,
) -> Result<(Vec<Grant>, Vec<PinnedValue>), MagnusError> {
    let mut grants = Vec::with_capacity(entries.len());
    let mut pins = Vec::new();
    for index in 0..entries.len() as isize {
        let pair: RArray = entries.entry(index)?;
        let name = RString::try_convert(pair.entry(0)?)?.to_string()?;
        let entry: Value = pair.entry(1)?;
        // A role an entry does not play has no opinion, so an entry
        // without endpoints reaches nothing.
        let endpoints: RArray = match entry.respond_to("endpoints", false)? {
            true => entry.funcall("endpoints", ())?,
            false => ruby.ary_new(),
        };
        let endpoints = (0..endpoints.len() as isize)
            .map(|index| to_endpoint(ruby, endpoints.entry(index)?))
            .collect::<Result<Vec<_>, _>>()?;
        let plays = |hook| -> Result<Option<Arc<RubyPolicy>>, MagnusError> {
            Ok(entry.respond_to(hook, false)?.then(|| {
                Arc::new(RubyPolicy {
                    entry: Opaque::from(entry),
                })
            }))
        };
        let (resolve, connect) = (plays("resolve?")?, plays("connect?")?);
        if resolve.is_some() || connect.is_some() {
            pins.push(PinnedValue::new(ruby, entry));
        }
        let answered = Answered {
            endpoints,
            resolve,
            connect,
        };
        let network = Network::new(name, Arc::new(answered))
            .map_err(|err| MagnusError::new(ruby.exception_arg_error(), err.to_string()))?;
        grants.push(Grant::Network(network));
    }
    Ok((grants, pins))
}

/// The endpoint a Ruby object answering `host`, `port`, `protocol`, and
/// `addresses` stands for.
fn to_endpoint(ruby: &Ruby, endpoint: Value) -> Result<Endpoint, MagnusError> {
    let refused = |message: String| MagnusError::new(ruby.exception_arg_error(), message);
    let host: String = endpoint
        .funcall::<_, _, Value>("host", ())?
        .funcall("to_s", ())?;
    let port: u16 = endpoint.funcall("port", ())?;
    let protocol = match symbol_name(endpoint.funcall("protocol", ())?)?.as_deref() {
        Some("http") => Protocol::Http,
        Some("tcp") => Protocol::Tcp,
        Some("udp") => Protocol::Udp,
        _ => return Err(refused("protocol must be :http, :tcp, or :udp".to_string())),
    };
    let addresses: RArray = endpoint.funcall("addresses", ())?;
    let addresses = (0..addresses.len() as isize)
        .map(|index| {
            to_addresses(addresses.entry(index)?).ok_or_else(|| {
                refused("addresses must be :public, :private, :local, or a CIDR".to_string())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Endpoint::new(host, port)
        .map(|endpoint| endpoint.with_protocol(protocol).with_addresses(addresses))
        .map_err(|err| refused(err.to_string()))
}

/// An address set's symbol, or a CIDR as `address/prefix`.
fn to_addresses(value: Value) -> Option<Addresses> {
    match symbol_name(value).ok()?.as_deref() {
        Some("public") => return Some(Addresses::Public),
        Some("private") => return Some(Addresses::Private),
        Some("local") => return Some(Addresses::Local),
        Some(_) => return None,
        None => {}
    }
    let cidr = RString::try_convert(value).ok()?.to_string().ok()?;
    let (address, prefix) = cidr.split_once('/')?;
    let address: IpAddr = address.parse().ok()?;
    let prefix: u8 = prefix.parse().ok()?;
    let longest = if address.is_ipv4() { 32 } else { 128 };
    (prefix <= longest).then_some(Addresses::Cidr(address, prefix))
}

fn symbol_name(value: Value) -> Result<Option<String>, MagnusError> {
    Symbol::from_value(value)
        .map(|symbol| symbol.name().map(|name| name.into_owned()))
        .transpose()
}
