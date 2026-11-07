//! The network Grant: one named entry a Host App hands the guest, as Linux's
//! network namespace holds every socket its tasks open. Each entry is one
//! Grant, so a Context replaces it by its name, as one Mount by its path.

mod ruleset;

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

pub use ruleset::{address_set, AddressSet, Ruleset};

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
}

/// What a Host App's entry plays. Each role defaults to nothing, so an entry
/// answers only the roles it plays.
pub trait Entry: Send + Sync {
    /// What the guest may reach through this entry.
    fn endpoints(&self) -> Vec<Endpoint> {
        Vec::new()
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
