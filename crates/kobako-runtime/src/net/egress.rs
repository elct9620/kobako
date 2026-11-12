//! The HTTP layer's own judgment, as an egress proxy makes it beside the
//! kernel's: the socket hooks and the ruleset on the authority, each entry's
//! request policies, and the credentials bound to its own endpoints. It reads
//! the namespace's state and decides at layer 7.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use super::credential::{carries, Swap};
use super::http::{
    join_authority, split_authority, to_field_name, to_field_value, to_origin_form, Exchange,
    HttpError, OutgoingRequest, Scheme,
};
use super::lookup::{allows_lookup, normalize};
use super::transport::Connector;
use super::Namespace;
use crate::grant::Policy;
use crate::network::{Connection, Credential, Protocol, Request, Ruleset};
use crate::vfs::Cred;
use crate::Errno;

/// Send an HTTP request in `net` as `cred`. The socket hooks judge the
/// authority the guest named first, a name passing the `resolve` policies
/// too, and an `http` endpoint must name it at its port; a literal address
/// passes the ruleset as itself, so a virtual one is refused, and an IPv6 one
/// reaches nothing. A header the guest may not set is refused too, so the
/// seam frames every request itself, and so is a target not in origin form,
/// which a proxy would forward past the judged authority. Any refusal answers
/// `Denied`, and nothing is sent.
pub fn send(
    net: &Namespace,
    cred: &Arc<Cred>,
    request: OutgoingRequest,
) -> Result<Arc<Exchange>, HttpError> {
    let in_origin_form = to_origin_form(&request.path_with_query).as_deref()
        == Some(request.path_with_query.as_str());
    if !in_origin_form
        || request.headers.iter().any(|(name, value)| {
            to_field_name(name).is_err() || to_field_value(value.clone()).is_err()
        })
    {
        return Err(HttpError::Denied);
    }
    let (host, port) =
        split_authority(&request.authority, request.scheme).ok_or(HttpError::Denied)?;
    let literal = host.parse::<IpAddr>().ok();
    let host = match literal {
        Some(_) => host,
        None => normalize(&host).map_err(|_| HttpError::Denied)?,
    };
    let connection = Connection::new(&host, port, Protocol::Http);
    if cred.security_socket_connect(&connection).is_err() {
        return Err(HttpError::Denied);
    }
    let reachable = match literal {
        Some(ip) => admit_literal(net, SocketAddr::new(ip, port)).is_ok(),
        None => allows_lookup(net, &host),
    };
    let path = request
        .path_with_query
        .split_once('?')
        .map_or(request.path_with_query.as_str(), |(path, _)| path);
    let method = request.method.to_ascii_uppercase();
    let names = request
        .headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let scheme = match request.scheme {
        Scheme::Http => "http",
        Scheme::Https => "https",
    };
    let policy_request = Request::new((&host, port), &method, scheme, path, &names);
    let proxy = net.egress();
    let admitted = reachable
        && net.ruleset().admits_request(&host, port, &method, path)
        && proxy.admits(&policy_request);
    if !admitted {
        return Err(HttpError::Denied);
    }
    let swaps = match request.scheme {
        Scheme::Https => proxy.swaps_of(&policy_request, &request.headers),
        Scheme::Http => Vec::new(),
    };
    let mut request = request;
    request.authority = join_authority(&host, port, request.scheme);
    request.swaps = swaps;
    let connector = Connector::new(net, cred, host, port);
    net.within_connection_budget(HttpError::ConnectionLimitReached, || {
        Exchange::start(
            net.outbound().clone(),
            request,
            connector,
            net.open_connections(),
            cred,
        )
    })
}

/// Whether a request may reach `addr`, a literal the socket hooks already
/// passed: the guest has no IPv6, so an IPv6 one answers `EAFNOSUPPORT`, as a
/// socket's `connect4` does; any other meets `net`'s ruleset as itself.
fn admit_literal(net: &Namespace, addr: SocketAddr) -> Result<(), Errno> {
    if addr.is_ipv6() {
        return Err(Errno::AddressFamilyNotSupported);
    }
    net.ruleset().admit_literal(addr, Protocol::Http)
}

/// The entries' request policies, which judge a request `send` found its
/// authority reachable for, and the credentials it swaps into one.
pub(crate) struct EgressProxy {
    pub(crate) request: Vec<Arc<dyn for<'a> Policy<Request<'a>>>>,
    /// Each entry's own endpoints and the credentials bound to them.
    pub(crate) bindings: Vec<(Ruleset, Vec<Credential>)>,
}

impl EgressProxy {
    /// Whether every request policy lets `request` leave.
    pub(crate) fn admits(&self, request: &Request<'_>) -> bool {
        self.request.iter().all(|policy| policy.allows(request))
    }

    /// The credentials bound to `request`, one over `https`: an entry's own
    /// endpoint names its exact host and allows it, and the placement's header
    /// carries the placeholder. Each source answers here, on the guest thread,
    /// so its time counts against the deadline.
    pub(crate) fn swaps_of(
        &self,
        request: &Request<'_>,
        headers: &[(String, Vec<u8>)],
    ) -> Vec<Swap> {
        let (host, port) = request.authority;
        let mut swaps = Vec::new();
        for (rules, credentials) in &self.bindings {
            if !rules.binds(host, port, request.method, request.path) {
                continue;
            }
            for credential in credentials {
                let header = credential.placement.header();
                let placeholder = credential.placeholder.as_bytes();
                let carried = headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case(header) && carries(value, placeholder)
                });
                if !carried {
                    continue;
                }
                if let Some(secret) = credential.source.value_of(request) {
                    swaps.push(Swap {
                        header: header.to_string(),
                        placeholder: placeholder.to_vec(),
                        secret,
                    });
                }
            }
        }
        swaps
    }
}
