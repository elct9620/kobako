//! A Sandbox's network entries as contract Grants. Each entry's endpoints are
//! read from Ruby as an invocation starts, on the guest thread holding the
//! GVL, so the runtime judges them as data on any thread; its policies are
//! asked on the guest thread, re-entering Ruby as a mount's block does. A
//! `Kobako::Network::Builder` only collects what its block declares; the
//! runtime's `Builder` composes it with the object given beside it.

use std::net::IpAddr;
use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{
    prelude::*, Error as MagnusError, RArray, RClass, RHash, RModule, RString, Ruby, Symbol, Value,
};

use kobako_runtime::grant::{Grant, Policy};
use kobako_runtime::network::{
    Action, Addresses, Builder, Connection, Credential, CredentialSource, Endpoint, Entry,
    EntryError, HttpRule, Name, Network, Placement, Policies, Protocol, Request, Secret,
};

use super::pinned_value::PinnedValue;
use super::{escape, gvl};

/// A Ruby entry as one invocation reads it: its endpoints, its own
/// `resolve?`, `connect?`, and `request?` for each it plays, and its
/// credentials.
struct RubyEntry {
    endpoints: Vec<Endpoint>,
    resolve: Option<Arc<RubyPolicy>>,
    connect: Option<Arc<RubyPolicy>>,
    request: Option<Arc<RubyPolicy>>,
    credentials: Vec<Credential>,
}

impl Entry for RubyEntry {
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

/// A Ruby object judging one decision when sent `method`: an entry's
/// `resolve?`, `connect?`, or `request?`, or a policy block's `call`, handed
/// the name, a `Kobako::Network::Connection`, or a
/// `Kobako::Network::Request`. A falsy answer or a raised StandardError
/// refuses, so the guest's call fails and its script goes on; anything else
/// it raises is held for the host.
struct RubyPolicy {
    receiver: Opaque<Value>,
    method: &'static str,
}

impl RubyPolicy {
    fn new(receiver: Value, method: &'static str) -> Self {
        Self {
            receiver: Opaque::from(receiver),
            method,
        }
    }

    fn ask(&self, asked: impl FnOnce(&Ruby) -> Result<Value, MagnusError>) -> bool {
        gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable in an entry's policy");
            let receiver: Value = ruby.get_inner(self.receiver);
            asked(&ruby)
                .and_then(|asked| receiver.funcall::<_, _, Value>(self.method, (asked,)))
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
        self.ask(|ruby| Ok(ruby.str_new(request.name).as_value()))
    }
}

impl Policy<Connection<'_>> for RubyPolicy {
    fn allows(&self, request: &Connection<'_>) -> bool {
        self.ask(|ruby| to_connection(ruby, request))
    }
}

impl Policy<Request<'_>> for RubyPolicy {
    fn allows(&self, request: &Request<'_>) -> bool {
        self.ask(|ruby| to_request(ruby, request))
    }
}

/// A Ruby credential's `value(request)`, asked as a bound request leaves. A
/// String answers the value, `nil` sends nothing, and a raised StandardError
/// sends nothing too; anything else it raises is held for the host.
struct RubyCredential {
    credential: Opaque<Value>,
}

impl CredentialSource for RubyCredential {
    fn value_of(&self, request: &Request<'_>) -> Option<Secret> {
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

/// The credential a Ruby object answering `placement`, `placeholder`, and
/// `value` stands for, pinned while the guest may ask it for a value.
fn to_credential(
    ruby: &Ruby,
    credential: Value,
    pins: &mut Vec<PinnedValue>,
) -> Result<Credential, MagnusError> {
    let placement: Value = credential.funcall("placement", ())?;
    let placement = match symbol_name_of(placement)?.as_deref() {
        Some("bearer") => Placement::Bearer,
        Some("basic") => Placement::Basic,
        Some(_) => {
            return Err(MagnusError::new(
                ruby.exception_arg_error(),
                "a credential's placement must be :bearer or :basic",
            ))
        }
        None => Placement::Header(RString::try_convert(placement)?.to_string()?),
    };
    let placeholder: String = credential
        .funcall::<_, _, Value>("placeholder", ())?
        .funcall("to_s", ())?;
    pins.push(PinnedValue::new(ruby, credential));
    let source = Arc::new(RubyCredential {
        credential: Opaque::from(credential),
    });
    Ok(Credential::new(placement, placeholder, source))
}

/// Each element of the Ruby Array `values`, as `convert` reads it.
fn each_of<T>(
    values: RArray,
    mut convert: impl FnMut(Value) -> Result<T, MagnusError>,
) -> Result<Vec<T>, MagnusError> {
    (0..values.len() as isize)
        .map(|index| convert(values.entry(index)?))
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

/// `connection` as the `Kobako::Network::Connection` a Ruby policy reads.
fn to_connection(ruby: &Ruby, connection: &Connection<'_>) -> Result<Value, MagnusError> {
    let class: RClass = network_module(ruby)?.const_get("Connection")?;
    let protocol = ruby.to_symbol(match connection.protocol {
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
    class.funcall(
        "new",
        (ruby.str_new(connection.host), connection.port, protocol),
    )
}

/// One invocation's network Grants, from the Sandbox's entries as
/// `[name, entry]` pairs, beside the pins keeping each entry playing a policy
/// in place while the guest may ask it; the caller holds both until the
/// invocation returns.
pub(super) fn to_grants(
    ruby: &Ruby,
    entries: RArray,
) -> Result<(Vec<Grant>, Vec<PinnedValue>), MagnusError> {
    let to_arg_error =
        |err: EntryError| MagnusError::new(ruby.exception_arg_error(), err.to_string());
    let mut grants = Vec::with_capacity(entries.len());
    let mut pins = Vec::new();
    for index in 0..entries.len() as isize {
        let pair: RArray = entries.entry(index)?;
        let name = RString::try_convert(pair.entry(0)?)?.to_string()?;
        let entry = to_entry(ruby, pair.entry(1)?, &mut pins)?;
        grants.push(Grant::Network(
            Network::new(name, entry).map_err(to_arg_error)?,
        ));
    }
    Ok((grants, pins))
}

/// The entry a Ruby object stands for. A `Kobako::Network::Builder` is the
/// object given beside it, then its own endpoints, credentials, and policy
/// blocks, composed by the runtime's `Builder`; any other object plays the
/// roles it answers, and one it does not answer has no opinion, so an entry
/// without endpoints reaches nothing.
fn to_entry(
    ruby: &Ruby,
    entry: Value,
    pins: &mut Vec<PinnedValue>,
) -> Result<Arc<dyn Entry>, MagnusError> {
    let builder_class: RClass = network_module(ruby)?.const_get("Builder")?;
    if !entry.is_kind_of(builder_class) {
        return to_ruby_entry(ruby, entry, pins).map(|entry| Arc::new(entry) as Arc<dyn Entry>);
    }
    let held = |name: &str| entry.funcall::<_, _, Value>("instance_variable_get", (name,));
    let given = held("@entry")?;
    let mut builder = match given.is_nil() {
        true => Builder::default(),
        false => Builder::new(to_entry(ruby, given, pins)?),
    };
    for endpoint in each_of(RArray::try_convert(held("@endpoints")?)?, |endpoint| {
        to_endpoint(ruby, endpoint)
    })? {
        builder.endpoint(endpoint);
    }
    for credential in each_of(RArray::try_convert(held("@credentials")?)?, |credential| {
        to_credential(ruby, credential, pins)
    })? {
        builder.credential(credential);
    }
    let policies = RHash::try_convert(held("@policies")?)?;
    let blocks_of = |hook: &str| -> Result<RArray, MagnusError> {
        RArray::try_convert(policies.aref::<_, Value>(ruby.to_symbol(hook))?)
    };
    for (hook, blocks) in [
        ("resolve", blocks_of("resolve")?),
        ("connect", blocks_of("connect")?),
        ("request", blocks_of("request")?),
    ] {
        for block in each_of(blocks, Ok)? {
            pins.push(PinnedValue::new(ruby, block));
            let policy = RubyPolicy::new(block, "call");
            match hook {
                "resolve" => builder.resolve_policy(policy),
                "connect" => builder.connect_policy(policy),
                _ => builder.request_policy(policy),
            };
        }
    }
    Ok(Arc::new(builder))
}

/// The entry an object other than a Builder stands for, by each role it
/// answers.
fn to_ruby_entry(
    ruby: &Ruby,
    entry: Value,
    pins: &mut Vec<PinnedValue>,
) -> Result<RubyEntry, MagnusError> {
    let answered = |role: &str| -> Result<RArray, MagnusError> {
        match entry.respond_to(role, false)? {
            true => RArray::try_convert(entry.funcall::<_, _, Value>(role, ())?),
            false => Ok(ruby.ary_new()),
        }
    };
    let endpoints = each_of(answered("endpoints")?, |endpoint| {
        to_endpoint(ruby, endpoint)
    })?;
    let credentials = each_of(answered("credentials")?, |credential| {
        to_credential(ruby, credential, pins)
    })?;
    let policy_for = |hook: &'static str| -> Result<Option<Arc<RubyPolicy>>, MagnusError> {
        Ok(entry
            .respond_to(hook, false)?
            .then(|| Arc::new(RubyPolicy::new(entry, hook))))
    };
    let (resolve, connect, request) = (
        policy_for("resolve?")?,
        policy_for("connect?")?,
        policy_for("request?")?,
    );
    if resolve.is_some() || connect.is_some() || request.is_some() {
        pins.push(PinnedValue::new(ruby, entry));
    }
    Ok(RubyEntry {
        endpoints,
        resolve,
        connect,
        request,
        credentials,
    })
}

/// The endpoint a Ruby object answering `host` and `port` stands for, and
/// `protocol`, `addresses`, and `rules` where it answers them; one it leaves
/// out takes the default `Kobako::Network::Endpoint` gives it.
fn to_endpoint(ruby: &Ruby, object: Value) -> Result<Endpoint, MagnusError> {
    let to_arg_error = |message: String| MagnusError::new(ruby.exception_arg_error(), message);
    let host: String = object
        .funcall::<_, _, Value>("host", ())?
        .funcall("to_s", ())?;
    let port: u16 = object.funcall("port", ())?;
    let mut endpoint = Endpoint::new(host, port).map_err(|err| to_arg_error(err.to_string()))?;
    if object.respond_to("protocol", false)? {
        endpoint = endpoint.with_protocol(
            match symbol_name_of(object.funcall("protocol", ())?)?.as_deref() {
                Some("http") => Protocol::Http,
                Some("tcp") => Protocol::Tcp,
                Some("udp") => Protocol::Udp,
                _ => {
                    return Err(to_arg_error(
                        "protocol must be :http, :tcp, or :udp".to_string(),
                    ))
                }
            },
        );
    }
    if object.respond_to("addresses", false)? {
        let addresses = each_of(object.funcall("addresses", ())?, |address| {
            to_addresses(address).ok_or_else(|| {
                to_arg_error(
                    "addresses must be :public, :private, :local, a CIDR, or an IPAddr".to_string(),
                )
            })
        })?;
        endpoint = endpoint.with_addresses(addresses);
    }
    if object.respond_to("rules", false)? {
        endpoint = endpoint.with_rules(each_of(object.funcall("rules", ())?, to_rule)?);
    }
    Ok(endpoint)
}

/// The rule a Ruby object answering `allow?`, `method`, and `path` stands for.
fn to_rule(rule: Value) -> Result<HttpRule, MagnusError> {
    let action = match rule.funcall::<_, _, Value>("allow?", ())?.to_bool() {
        true => Action::Allow,
        false => Action::Deny,
    };
    let method: Option<String> = rule.funcall("method", ())?;
    let path: String = rule
        .funcall::<_, _, Value>("path", ())?
        .funcall("to_s", ())?;
    Ok(HttpRule::new(action, method.as_deref(), path))
}

/// An address set's symbol, a CIDR as `address/prefix`, or an `IPAddr`, which
/// carries its prefix apart from its address; the ruleset judges whether the
/// prefix fits its address.
fn to_addresses(value: Value) -> Option<Addresses> {
    match symbol_name_of(value).ok()?.as_deref() {
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
    Some(Addresses::Cidr(address, prefix))
}

fn symbol_name_of(value: Value) -> Result<Option<String>, MagnusError> {
    Symbol::from_value(value)
        .map(|symbol| symbol.name().map(|name| name.into_owned()))
        .transpose()
}
