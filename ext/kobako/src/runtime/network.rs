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
    Addresses, Connection, Credential, CredentialSource, Endpoint, Entry, HttpRule, Name, Network,
    Placement, Policies, Protocol, Request, Secret,
};

use super::pinned_value::PinnedValue;
use super::{escape, gvl};

/// What one entry answered for this invocation: its endpoints, its own
/// `resolve?`, `connect?`, and `request?` for each it plays, and its
/// credentials.
struct Answered {
    endpoints: Vec<Endpoint>,
    resolve: Option<Arc<RubyPolicy>>,
    connect: Option<Arc<RubyPolicy>>,
    request: Option<Arc<RubyPolicy>>,
    credentials: Vec<Credential>,
}

impl Entry for Answered {
    fn endpoints(&self) -> Vec<Endpoint> {
        self.endpoints.clone()
    }

    fn credentials(&self) -> Vec<Credential> {
        self.credentials.clone()
    }

    fn policies(&self) -> Policies {
        let mut policies = Policies::default();
        if let Some(resolve) = &self.resolve {
            policies = policies.with_resolve(resolve.clone());
        }
        if let Some(connect) = &self.connect {
            policies = policies.with_connect(connect.clone());
        }
        if let Some(request) = &self.request {
            policies = policies.with_request(request.clone());
        }
        policies
    }
}

/// An entry asked one of its policy hooks: `resolve?` handed the name,
/// `connect?` a `Kobako::Network::Connection`, and `request?` a
/// `Kobako::Network::Request`. A falsy answer or a raised
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

impl Policy<Request<'_>> for RubyPolicy {
    fn allows(&self, request: &Request<'_>) -> bool {
        self.ask("request?", |ruby| to_request(ruby, request))
    }
}

/// A Ruby credential's `value(request)`, asked as a bound request leaves. A
/// String answers the value, `nil` sends nothing, and a raised StandardError
/// sends nothing too; anything else it raises is held for the host.
struct RubyCredential {
    credential: Opaque<Value>,
}

impl CredentialSource for RubyCredential {
    fn value(&self, request: &Request<'_>) -> Option<Secret> {
        gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable in a credential's value");
            let credential: Value = ruby.get_inner(self.credential);
            let answer = to_request(&ruby, request).and_then(|request| {
                credential.funcall::<_, _, Option<RString>>("value", (request,))
            });
            match answer {
                // SAFETY: the bytes are copied out before Ruby runs again.
                Ok(Some(value)) => Some(Secret::new(unsafe { value.as_slice() }.to_vec())),
                Ok(None) => None,
                Err(error) => {
                    let _ = escape::hold(&ruby, error);
                    None
                }
            }
        })
    }
}

/// The credentials a Ruby entry answers, each pinned while the guest may ask
/// it for a value.
fn to_credentials(
    ruby: &Ruby,
    entry: Value,
    pins: &mut Vec<PinnedValue>,
) -> Result<Vec<Credential>, MagnusError> {
    if !entry.respond_to("credentials", false)? {
        return Ok(Vec::new());
    }
    let refused = |message: &str| MagnusError::new(ruby.exception_arg_error(), message.to_string());
    let credentials: RArray = entry.funcall("credentials", ())?;
    (0..credentials.len() as isize)
        .map(|index| {
            let credential: Value = credentials.entry(index)?;
            let placement: Value = credential.funcall("placement", ())?;
            let placement = match symbol_name(placement)?.as_deref() {
                Some("bearer") => Placement::Bearer,
                Some("basic") => Placement::Basic,
                Some(_) => {
                    return Err(refused(
                        "a credential's placement must be :bearer or :basic",
                    ))
                }
                None => Placement::Header(
                    RString::try_convert(placement)?
                        .to_string()?
                        .to_ascii_lowercase(),
                ),
            };
            let placeholder: String = credential
                .funcall::<_, _, Value>("placeholder", ())?
                .funcall("to_s", ())?;
            pins.push(PinnedValue::new(ruby, credential));
            let source = Arc::new(RubyCredential {
                credential: Opaque::from(credential),
            });
            Ok(Credential::new(placement, placeholder, source))
        })
        .collect()
}

/// `request` as the `Kobako::Network::Request` a Ruby policy reads.
fn to_request(ruby: &Ruby, request: &Request<'_>) -> Result<Value, MagnusError> {
    let class: RClass = network_module(ruby)?.const_get("Request")?;
    let (host, port) = request.authority;
    let names = ruby.ary_from_iter(request.header_names.iter().map(|name| ruby.str_new(name)));
    class.funcall(
        "new",
        (
            ruby.ary_from_iter([
                ruby.str_new(host).as_value(),
                ruby.integer_from_i64(i64::from(port)).as_value(),
            ]),
            ruby.str_new(request.method),
            ruby.str_new(request.scheme),
            ruby.str_new(request.path),
            names,
        ),
    )
}

fn network_module(ruby: &Ruby) -> Result<RModule, MagnusError> {
    ruby.class_object()
        .const_get::<_, RModule>("Kobako")?
        .const_get::<_, RModule>("Network")
}

/// `request` as the `Kobako::Network::Connection` a Ruby policy reads.
fn to_connection(ruby: &Ruby, request: &Connection<'_>) -> Result<Value, MagnusError> {
    let class: RClass = network_module(ruby)?.const_get("Connection")?;
    let protocol = ruby.to_symbol(match request.protocol {
        Protocol::Http => "http",
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
        other => {
            return Err(MagnusError::new(
                ruby.exception_runtime_error(),
                format!("{other:?} has no Ruby name"),
            ))
        }
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
        let (resolve, connect, request) =
            (plays("resolve?")?, plays("connect?")?, plays("request?")?);
        if resolve.is_some() || connect.is_some() || request.is_some() {
            pins.push(PinnedValue::new(ruby, entry));
        }
        let credentials = to_credentials(ruby, entry, &mut pins)?;
        let answered = Answered {
            endpoints,
            resolve,
            connect,
            request,
            credentials,
        };
        let network = Network::new(name, Arc::new(answered))
            .map_err(|err| MagnusError::new(ruby.exception_arg_error(), err.to_string()))?;
        grants.push(Grant::Network(network));
    }
    Ok((grants, pins))
}

/// The endpoint a Ruby object answering `host` and `port` stands for, and
/// `protocol`, `addresses`, and `rules` where it answers them; one it leaves
/// out takes the default `Kobako::Network::Endpoint` gives it.
fn to_endpoint(ruby: &Ruby, endpoint: Value) -> Result<Endpoint, MagnusError> {
    let refused = |message: String| MagnusError::new(ruby.exception_arg_error(), message);
    let host: String = endpoint
        .funcall::<_, _, Value>("host", ())?
        .funcall("to_s", ())?;
    let port: u16 = endpoint.funcall("port", ())?;
    let mut answered = Endpoint::new(host, port).map_err(|err| refused(err.to_string()))?;
    if endpoint.respond_to("protocol", false)? {
        answered = answered.with_protocol(
            match symbol_name(endpoint.funcall("protocol", ())?)?.as_deref() {
                Some("http") => Protocol::Http,
                Some("tcp") => Protocol::Tcp,
                Some("udp") => Protocol::Udp,
                _ => return Err(refused("protocol must be :http, :tcp, or :udp".to_string())),
            },
        );
    }
    if endpoint.respond_to("addresses", false)? {
        let addresses: RArray = endpoint.funcall("addresses", ())?;
        answered = answered.with_addresses(
            (0..addresses.len() as isize)
                .map(|index| {
                    to_addresses(addresses.entry(index)?).ok_or_else(|| {
                        refused(
                            "addresses must be :public, :private, :local, a CIDR, or an IPAddr"
                                .to_string(),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if endpoint.respond_to("rules", false)? {
        let rules: RArray = endpoint.funcall("rules", ())?;
        answered = answered.with_rules(
            (0..rules.len() as isize)
                .map(|index| to_rule(rules.entry(index)?))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(answered)
}

/// The rule a Ruby object answering `allow`, `method`, and `path` stands for.
fn to_rule(rule: Value) -> Result<HttpRule, MagnusError> {
    let allow: bool = rule.funcall::<_, _, Value>("allow", ())?.to_bool();
    let method: Option<String> = rule.funcall("method", ())?;
    let path: String = rule
        .funcall::<_, _, Value>("path", ())?
        .funcall("to_s", ())?;
    Ok(match allow {
        true => HttpRule::allow(method.as_deref(), path),
        false => HttpRule::deny(method.as_deref(), path),
    })
}

/// An address set's symbol, a CIDR as `address/prefix`, or an `IPAddr`, which
/// carries its prefix apart from its address.
fn to_addresses(value: Value) -> Option<Addresses> {
    match symbol_name(value).ok()?.as_deref() {
        Some("public") => return Some(Addresses::Public),
        Some("private") => return Some(Addresses::Private),
        Some("local") => return Some(Addresses::Local),
        Some(_) => return None,
        None => {}
    }
    let cidr = match RString::try_convert(value) {
        Ok(text) => text.to_string().ok()?,
        Err(_) if value.respond_to("prefix", false).ok()? => {
            let address: String = value.funcall("to_s", ()).ok()?;
            let prefix: u8 = value.funcall("prefix", ()).ok()?;
            format!("{address}/{prefix}")
        }
        Err(_) => return None,
    };
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
