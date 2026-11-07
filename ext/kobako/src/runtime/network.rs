//! A Sandbox's network entries as contract Grants. Each entry's endpoints are
//! read from Ruby as an invocation starts, on the guest thread holding the
//! GVL, so the runtime judges them as data on any thread.

use std::net::IpAddr;
use std::sync::Arc;

use magnus::value::ReprValue;
use magnus::{prelude::*, Error as MagnusError, RArray, RString, Ruby, Symbol, Value};

use kobako_runtime::grant::Grant;
use kobako_runtime::network::{Addresses, Endpoint, Entry, Network, Protocol};

/// The endpoints one entry answered for this invocation.
struct Endpoints(Vec<Endpoint>);

impl Entry for Endpoints {
    fn endpoints(&self) -> Vec<Endpoint> {
        self.0.clone()
    }
}

/// One invocation's network Grants, from the Sandbox's entries as
/// `[name, entry]` pairs.
pub(super) fn to_grants(ruby: &Ruby, entries: RArray) -> Result<Vec<Grant>, MagnusError> {
    (0..entries.len() as isize)
        .map(|index| {
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
            Network::new(name, Arc::new(Endpoints(endpoints)))
                .map(Grant::Network)
                .map_err(|err| MagnusError::new(ruby.exception_arg_error(), err.to_string()))
        })
        .collect()
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
