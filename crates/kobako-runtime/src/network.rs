//! The network Grant: one named entry a Host App hands the guest, as Linux's
//! network namespace holds every socket its tasks open. Each entry is one
//! Grant, so a Context replaces it by its name, as one Mount by its path.

mod builder;
mod ruleset;

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

pub use builder::Builder;
pub use ruleset::{address_set, AddressSet, Ruleset};

/// The process's open connection limit, which only a Rust Host App moves.
pub use crate::net::{process_connections, OpenConnections};

use crate::grant::Policy;

/// One named entry of the network, handed over for an Invocation.
#[derive(Clone)]
pub struct Network {
    name: String,
    entry: Arc<dyn Entry>,
}

impl Network {
    /// The entry `entry` under `name`, refused for an empty name, which no
    /// Context could replace it by.
    pub fn new(name: impl Into<String>, entry: Arc<dyn Entry>) -> Result<Self, EntryError> {
        let name = name.into();
        if name.is_empty() {
            return Err(EntryError("an entry's name must not be empty".to_string()));
        }
        Ok(Self { name, entry })
    }

    /// The name a Context replaces this entry by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this entry lets the guest reach, as it answers for this
    /// Invocation.
    pub fn endpoints(&self) -> Vec<Endpoint> {
        self.entry.endpoints()
    }

    /// What this entry's policies judge, as it answers for this Invocation.
    pub fn policies(&self) -> Policies {
        self.entry.policies()
    }

    /// The credentials this entry swaps into requests bound to its own
    /// endpoints, as it answers for this Invocation.
    pub fn credentials(&self) -> Vec<Credential> {
        self.entry.credentials()
    }
}

/// What a Host App's entry plays. Each role defaults to nothing, so an entry
/// answers only the roles it plays.
pub trait Entry: Send + Sync {
    /// What the guest may reach through this entry.
    fn endpoints(&self) -> Vec<Endpoint> {
        Vec::new()
    }

    /// The hooks this entry's policies answer; one it leaves out has no
    /// opinion.
    fn policies(&self) -> Policies {
        Policies::default()
    }

    /// The credentials swapped into a request bound to this entry's own
    /// endpoints.
    fn credentials(&self) -> Vec<Credential> {
        Vec::new()
    }
}

/// A value the Host App hands no guest, its `Debug` a mask and its bytes zeroed
/// as it drops.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    /// The bytes, for the one header they are swapped into.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(**)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

/// Where a credential's value goes: the `authorization` header as a bearer
/// token or a basic pair, or one named header, as OpenShell's `auth_style`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Placement {
    Bearer,
    Basic,
    /// A header by its name, lowercase.
    Header(String),
}

impl Placement {
    /// The header the value is swapped into.
    pub fn header(&self) -> &str {
        match self {
            Placement::Bearer | Placement::Basic => "authorization",
            Placement::Header(name) => name,
        }
    }
}

/// What resolves a credential's value as a bound request leaves: the value, or
/// `None` to send nothing. Caching and refresh are its own.
pub trait CredentialSource: Send + Sync {
    fn value(&self, request: &Request<'_>) -> Option<Secret>;
}

impl<F: Fn(&Request<'_>) -> Option<Secret> + Send + Sync> CredentialSource for F {
    fn value(&self, request: &Request<'_>) -> Option<Secret> {
        self(request)
    }
}

/// A placement and a placeholder paired with the source of the value that
/// replaces the placeholder there.
#[derive(Clone)]
#[non_exhaustive]
pub struct Credential {
    pub placement: Placement,
    /// The string the Host App hands the guest in the value's place.
    pub placeholder: String,
    pub source: Arc<dyn CredentialSource>,
}

impl Credential {
    pub fn new(
        placement: Placement,
        placeholder: impl Into<String>,
        source: Arc<dyn CredentialSource>,
    ) -> Self {
        Self {
            placement,
            placeholder: placeholder.into(),
            source,
        }
    }
}

/// An entry's policies, one hook per decision point, as an LSM registers a
/// hook per kind of object. A policy only narrows what the endpoints allow.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct Policies {
    /// Asked at each lookup.
    pub resolve: Option<Arc<dyn for<'a> Policy<Name<'a>>>>,
    /// Asked at each connect.
    pub connect: Option<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    /// Asked at each HTTP request.
    pub request: Option<Arc<dyn for<'a> Policy<Request<'a>>>>,
}

/// One HTTP request the guest sends, as the `request?` hook judges it: the
/// authority, method, scheme, and path it named, and its header names, never
/// a header's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Request<'a> {
    /// The host, lowercase, and the port.
    pub authority: (&'a str, u16),
    /// Uppercase.
    pub method: &'a str,
    /// `http` or `https`.
    pub scheme: &'a str,
    /// The path, with no query.
    pub path: &'a str,
    /// Lowercase.
    pub header_names: &'a [&'a str],
}

impl<'a> Request<'a> {
    pub fn new(
        authority: (&'a str, u16),
        method: &'a str,
        scheme: &'a str,
        path: &'a str,
        header_names: &'a [&'a str],
    ) -> Self {
        Self {
            authority,
            method,
            scheme,
            path,
            header_names,
        }
    }
}

impl Policies {
    /// These policies with `policy` judging each lookup.
    pub fn with_resolve(self, policy: Arc<dyn for<'a> Policy<Name<'a>>>) -> Self {
        Self {
            resolve: Some(policy),
            ..self
        }
    }

    /// These policies with `policy` judging each connect.
    pub fn with_connect(self, policy: Arc<dyn for<'a> Policy<Connection<'a>>>) -> Self {
        Self {
            connect: Some(policy),
            ..self
        }
    }

    /// These policies with `policy` judging each HTTP request.
    pub fn with_request(self, policy: Arc<dyn for<'a> Policy<Request<'a>>>) -> Self {
        Self {
            request: Some(policy),
            ..self
        }
    }
}

/// One name the guest asks to look up, as the `resolve?` hook judges it:
/// lowercase, with no trailing dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Name<'a> {
    pub name: &'a str,
}

impl<'a> Name<'a> {
    pub fn new(name: &'a str) -> Self {
        Self { name }
    }
}

/// One connection the guest asks for, as the socket hooks judge it: the host
/// it named, its port, and its protocol. The host is what the guest named, a
/// name or a literal address, never a real address it resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Connection<'a> {
    pub host: &'a str,
    pub port: u16,
    pub protocol: Protocol,
}

impl<'a> Connection<'a> {
    pub fn new(host: &'a str, port: u16, protocol: Protocol) -> Self {
        Self {
            host,
            port,
            protocol,
        }
    }
}

/// What the guest may reach: a host, a port, a protocol, and where the host's
/// name may lead.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Endpoint {
    /// An exact name, a label-boundary wildcard, or an address.
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    /// Where the host's name may lead.
    pub addresses: Vec<Addresses>,
    /// For an `http` endpoint, the methods and paths it allows and denies;
    /// with none, it allows every request.
    pub rules: Vec<HttpRule>,
}

/// One rule an `http` endpoint judges a request by: a method, or any, and a
/// path pattern, where `*` stands for one segment and `**` for any number.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRule {
    pub allow: bool,
    /// Uppercase, or `None` for any method.
    pub method: Option<String>,
    pub path: String,
}

impl HttpRule {
    /// A rule allowing `method`, or any, on paths `path` matches.
    pub fn allow(method: Option<&str>, path: impl Into<String>) -> Self {
        Self {
            allow: true,
            method: method.map(str::to_ascii_uppercase),
            path: path.into(),
        }
    }

    /// A rule denying `method`, or any, on paths `path` matches.
    pub fn deny(method: Option<&str>, path: impl Into<String>) -> Self {
        Self {
            allow: false,
            ..Self::allow(method, path)
        }
    }

    /// Whether this rule speaks to `method` on `path`.
    pub(crate) fn matches(&self, method: &str, path: &str) -> bool {
        self.method
            .as_deref()
            .is_none_or(|own| own.eq_ignore_ascii_case(method))
            && glob(&self.path, path)
    }
}

/// Whether `path` fits `pattern`, segment by segment, as a path glob does.
fn glob(pattern: &str, path: &str) -> bool {
    fn fits(pattern: &[&str], path: &[&str]) -> bool {
        match (pattern.first(), path.first()) {
            (None, None) => true,
            (Some(&"**"), _) => {
                fits(&pattern[1..], path) || (!path.is_empty() && fits(pattern, &path[1..]))
            }
            (Some(&"*"), Some(_)) => fits(&pattern[1..], &path[1..]),
            (Some(own), Some(segment)) if own == segment => fits(&pattern[1..], &path[1..]),
            _ => false,
        }
    }
    let pattern = pattern.split('/').collect::<Vec<_>>();
    let path = path.split('/').collect::<Vec<_>>();
    fits(&pattern, &path)
}

impl Endpoint {
    /// An endpoint at `host` and `port`, speaking HTTP and leading only to
    /// public addresses, refused for an empty host or port zero, which no
    /// connection reaches.
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, EntryError> {
        let host = host.into();
        if host.is_empty() {
            return Err(EntryError(
                "an endpoint's host must not be empty".to_string(),
            ));
        }
        if port == 0 {
            return Err(EntryError(format!(
                "an endpoint's port must not be zero, got {host}:0"
            )));
        }
        Ok(Self {
            host,
            port,
            protocol: Protocol::Http,
            addresses: vec![Addresses::Public],
            rules: Vec::new(),
        })
    }

    /// This endpoint speaking `protocol`.
    pub fn with_protocol(self, protocol: Protocol) -> Self {
        Self { protocol, ..self }
    }

    /// This endpoint leading to `addresses`.
    pub fn with_addresses(self, addresses: Vec<Addresses>) -> Self {
        Self { addresses, ..self }
    }

    /// This endpoint judging each request by `rules`.
    pub fn with_rules(self, rules: Vec<HttpRule>) -> Self {
        Self { rules, ..self }
    }
}

/// What an endpoint speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protocol {
    Http,
    Tcp,
    Udp,
}

/// Where an endpoint's name may lead: an address set, or a CIDR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Addresses {
    /// Globally routable unicast.
    Public,
    /// RFC 1918, CGNAT, and ULA.
    Private,
    /// Loopback and the host's own addresses.
    Local,
    /// The addresses within a prefix.
    Cidr(IpAddr, u8),
}

/// An entry or an endpoint no connection could follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryError(String);

impl fmt::Display for EntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EntryError {}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-007
    #[test]
    fn an_endpoint_reaches_http_and_public_addresses_unless_written_otherwise() {
        let endpoint = Endpoint::new("api.example.com", 443).expect("a host and a port stand");
        assert_eq!(
            (endpoint.protocol, endpoint.addresses),
            (Protocol::Http, vec![Addresses::Public]),
            "Endpoint::new with only a host and a port must speak HTTP and lead to the public set"
        );
    }

    // @behavior NW-008
    #[test]
    fn an_endpoint_with_an_empty_host_or_port_zero_is_refused() {
        assert!(
            Endpoint::new("", 443).is_err() && Endpoint::new("api.example.com", 0).is_err(),
            "Endpoint::new with an empty host or port zero must be refused"
        );
    }
}
