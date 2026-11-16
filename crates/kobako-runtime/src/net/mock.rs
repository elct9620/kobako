//! A Protocol Family and a Resolver for a Host App's tests: names answer
//! from a table, a dial reaches only a route it was given, and each question
//! is recorded so a test reads back what the guest asked. It stands where
//! `HostInet` stands, beneath every rule, so the rules judge it alike. It
//! lives in the runtime so the Rust SDK and the Ruby gem hand the guest one
//! Mock, as neither frontend depends on the other.

use std::collections::HashMap;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

use super::family::{is_same_family, HostInet, ProtocolFamily};
use super::lookup::{LookupError, Resolver, Resolving};
use super::socket::CarriedErrno;
use crate::network::Host;
use crate::Errno;

/// A table of names, routes, and refusals, answering the guest's network in
/// place of the host's.
pub struct Mock {
    state: Arc<Mutex<State>>,
    local_addresses: Option<Vec<IpAddr>>,
}

#[derive(Default)]
struct State {
    answers: HashMap<String, Vec<IpAddr>>,
    routes: HashMap<(IpProtocol, SocketAddr), Dial>,
    /// How many holds each held name has.
    held: HashMap<String, usize>,
    /// The lookups waiting for a hold to drop.
    waiting: Vec<Waker>,
    lookups: Vec<String>,
    dials: Vec<SocketAddr>,
}

/// What a dial to one address does.
#[derive(Clone)]
enum Dial {
    To(SocketAddr),
    Refused(Errno),
}

/// The protocol a dial runs over, as Linux's `IPPROTO_TCP` and `IPPROTO_UDP`
/// name it, which a route keys on. An HTTP request dials over TCP.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpProtocol {
    Tcp,
    Udp,
}

impl Default for Mock {
    /// A table answering no name and dialing nowhere, on a host with no
    /// addresses of its own.
    fn default() -> Self {
        Self {
            state: Arc::default(),
            local_addresses: Some(Vec::new()),
        }
    }
}

impl Mock {
    /// A table whose host holds `addresses` as its own, which the rules
    /// count as local; `None` for a host that cannot say them, so the rules
    /// refuse every connect to a name.
    pub fn with_local_addresses(self, addresses: Option<Vec<IpAddr>>) -> Self {
        Self {
            local_addresses: addresses,
            ..self
        }
    }

    /// Resolve `name` to `addresses` from now on; `InvalidName` for a host
    /// that is no name a guest's lookup asks.
    pub fn resolve_to_addrs(&self, name: &str, addresses: Vec<IpAddr>) -> Result<(), LookupError> {
        let name = to_key(name)?;
        self.state().answers.insert(name, addresses);
        Ok(())
    }

    /// Dial `to` whenever `from` is dialed over `protocol`.
    pub fn route(&self, protocol: IpProtocol, from: SocketAddr, to: SocketAddr) {
        self.state().routes.insert((protocol, from), Dial::To(to));
    }

    /// Fail a dial to `addr` over `protocol` with `err`, which the socket
    /// layer answers as a host socket failing with it, alike on every host. A
    /// datagram socket fails as it connects.
    pub fn refuse(&self, protocol: IpProtocol, addr: SocketAddr, err: Errno) {
        self.state()
            .routes
            .insert((protocol, addr), Dial::Refused(err));
    }

    /// Keep each lookup of `name` waiting until the hold drops, as a
    /// resolver that has not answered; `InvalidName` for a host that is no
    /// name a guest's lookup asks.
    pub fn hold(&self, name: &str) -> Result<Hold<'_>, LookupError> {
        let name = to_key(name)?;
        *self.state().held.entry(name.clone()).or_default() += 1;
        Ok(Hold { mock: self, name })
    }

    /// Each name the guest's lookups asked, in order.
    pub fn lookups(&self) -> Vec<String> {
        self.state().lookups.clone()
    }

    /// Each address dialed, in order, routed or not.
    pub fn dialed_addresses(&self) -> Vec<SocketAddr> {
        self.state().dials.clone()
    }

    /// Where a dial to `addr` over `protocol` goes, recording it.
    fn dial(&self, protocol: IpProtocol, addr: SocketAddr) -> io::Result<SocketAddr> {
        self.state().dials.push(addr);
        self.target_of(protocol, addr)
    }

    /// Where a dial to `addr` over `protocol` goes.
    fn target_of(&self, protocol: IpProtocol, addr: SocketAddr) -> io::Result<SocketAddr> {
        let dial = self.state().routes.get(&(protocol, addr)).cloned();
        match dial {
            Some(Dial::To(target)) => Ok(target),
            Some(Dial::Refused(err)) => Err(io::Error::other(CarriedErrno(err))),
            None => Err(io::ErrorKind::ConnectionRefused.into()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A name held unanswered; dropping it lets each waiting lookup answer.
pub struct Hold<'a> {
    mock: &'a Mock,
    name: String,
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        let mut state = self.mock.state();
        if let Some(count) = state.held.get_mut(&self.name) {
            *count -= 1;
            if *count == 0 {
                state.held.remove(&self.name);
            }
        }
        state.waiting.drain(..).for_each(Waker::wake);
    }
}

impl ProtocolFamily for Mock {
    /// The route's target, through the host's own sockets; an address with
    /// no route is refused, so nothing unrouted reaches the host network.
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        HostInet.connect(self.dial(IpProtocol::Tcp, addr)?)
    }

    /// The route's target over UDP, refused as it connects when there is
    /// none, so every host answers alike where one would wait for a peer's
    /// ICMP reply.
    fn connect_datagram(&self, addr: SocketAddr) -> io::Result<UdpSocket> {
        HostInet.connect_datagram(self.dial(IpProtocol::Udp, addr)?)
    }

    /// The route's target when it lies in the socket's family, recorded as a
    /// dial; one in another family is left for the fresh socket to dial, and
    /// recorded there.
    fn route_datagram(
        &self,
        local: SocketAddr,
        addr: SocketAddr,
    ) -> io::Result<Option<SocketAddr>> {
        match self.target_of(IpProtocol::Udp, addr) {
            Ok(target) if !is_same_family(local, target) => Ok(None),
            _ => self.dial(IpProtocol::Udp, addr).map(Some),
        }
    }

    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        self.local_addresses
            .clone()
            .ok_or_else(|| io::ErrorKind::Unsupported.into())
    }
}

impl Resolver for Mock {
    /// The table's answer for `name` once no hold keeps it; a name the table
    /// does not hold resolves to nothing.
    fn resolve(&self, name: &str) -> Resolving {
        self.state().lookups.push(name.to_string());
        let Ok(key) = to_key(name) else {
            return Box::pin(std::future::ready(Ok(Vec::new())));
        };
        let state = self.state.clone();
        Box::pin(poll_fn(move |cx| {
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.held.contains_key(&key) {
                state.waiting.push(cx.waker().clone());
                return Poll::Pending;
            }
            Poll::Ready(Ok(state.answers.get(&key).cloned().unwrap_or_default()))
        }))
    }
}

/// A name as the table keys it, read as a guest's lookup reads one: in any
/// case, with or without its root, and an IDN as its A-label.
fn to_key(name: &str) -> Result<String, LookupError> {
    match Host::parse(name) {
        Some(Host::Name(name)) => Ok(name),
        _ => Err(LookupError::InvalidName),
    }
}

#[cfg(test)]
mod tests {
    use std::task::Context;

    use super::*;

    #[test]
    fn a_name_held_twice_waits_until_both_holds_drop() {
        let mock = Mock::default();
        mock.resolve_to_addrs("held.test", vec![IpAddr::from([93, 184, 216, 34])])
            .unwrap();
        let outer = mock.hold("held.test").unwrap();
        let inner = mock.hold("held.test").unwrap();
        let mut resolving = mock.resolve("held.test");
        let mut cx = Context::from_waker(Waker::noop());
        drop(inner);
        let waited = resolving.as_mut().poll(&mut cx).is_pending();
        drop(outer);
        let answered = match resolving.as_mut().poll(&mut cx) {
            Poll::Ready(found) => found.ok().map(|found| found.len()),
            Poll::Pending => None,
        };
        assert_eq!(
            (waited, answered),
            (true, Some(1)),
            "a lookup of a name held twice must wait while either hold lives, and answer once both \
             have dropped"
        );
    }
}
