//! A Protocol Family and a Resolver for a Host App's tests: names answer
//! from a table, a dial reaches only a route it was given, and each question
//! is recorded so a test reads back what the guest asked. It stands where
//! `HostInet` stands, beneath every rule, so the rules judge it alike. It
//! lives in the runtime so the Rust SDK and the Ruby gem hand the guest one
//! Mock, as neither frontend depends on the other.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use super::family::{HostInet, ProtocolFamily};
use super::lookup::{LookupError, Resolver};
use super::socket::CarriedErrno;
use crate::network::{Host, Protocol};
use crate::Errno;

/// A table of names, routes, and refusals, answering the guest's network in
/// place of the host's.
pub struct Mock {
    state: Mutex<State>,
    released: Condvar,
    local_addresses: Option<Vec<IpAddr>>,
}

#[derive(Default)]
struct State {
    answers: HashMap<String, Vec<IpAddr>>,
    routes: HashMap<(Transport, SocketAddr), Dial>,
    /// How many holds each held name has.
    held: HashMap<String, usize>,
    lookups: Vec<String>,
    dials: Vec<SocketAddr>,
    /// The address a UDP socket connecting again was routed for, which a
    /// fresh socket of another family dials next without a second record.
    rerouted: Option<SocketAddr>,
}

/// What a dial to one address does.
#[derive(Clone)]
enum Dial {
    To(SocketAddr),
    Refused(Errno),
}

/// The protocol a route keys on: HTTP rides a stream as TCP does.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Transport {
    Stream,
    Datagram,
}

impl From<Protocol> for Transport {
    fn from(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Udp => Self::Datagram,
            _ => Self::Stream,
        }
    }
}

impl Default for Mock {
    /// A table answering no name and dialing nowhere, on a host with no
    /// addresses of its own.
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            released: Condvar::new(),
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

    /// Dial `to` whenever `from` is dialed over `protocol`; HTTP dials as
    /// TCP does.
    pub fn route(&self, protocol: Protocol, from: SocketAddr, to: SocketAddr) {
        self.state()
            .routes
            .insert((protocol.into(), from), Dial::To(to));
    }

    /// Fail a dial to `addr` over `protocol` with `err`, which the socket
    /// layer answers as a host socket failing with it, alike on every host. A
    /// datagram socket fails as it connects.
    pub fn refuse(&self, protocol: Protocol, addr: SocketAddr, err: Errno) {
        self.state()
            .routes
            .insert((protocol.into(), addr), Dial::Refused(err));
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

    /// Where a dial to `addr` over `transport` goes, recording it.
    fn dial(&self, transport: Transport, addr: SocketAddr) -> io::Result<SocketAddr> {
        let mut state = self.state();
        state.rerouted = None;
        state.dials.push(addr);
        drop(state);
        self.target_of(transport, addr)
    }

    /// Where a dial to `addr` over `transport` goes.
    fn target_of(&self, transport: Transport, addr: SocketAddr) -> io::Result<SocketAddr> {
        let dial = self.state().routes.get(&(transport, addr)).cloned();
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
        drop(state);
        self.mock.released.notify_all();
    }
}

impl ProtocolFamily for Mock {
    /// The route's target, through the host's own sockets; an address with
    /// no route is refused, so nothing unrouted reaches the host network.
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        HostInet.connect(self.dial(Transport::Stream, addr)?)
    }

    /// The route's target over UDP, refused as it connects when there is
    /// none, so every host answers alike where one would wait for a peer's
    /// ICMP reply.
    fn connect_datagram(&self, addr: SocketAddr) -> io::Result<UdpSocket> {
        let rerouted = self.state().rerouted.take() == Some(addr);
        let target = match rerouted {
            true => self.target_of(Transport::Datagram, addr),
            false => self.dial(Transport::Datagram, addr),
        };
        HostInet.connect_datagram(target?)
    }

    /// The route's target, as a fresh socket's; one in another family than
    /// the socket's own has the socket's protocol dial it anew, recorded once.
    fn route_datagram(&self, addr: SocketAddr) -> io::Result<SocketAddr> {
        let routed = self.dial(Transport::Datagram, addr);
        self.state().rerouted = Some(addr);
        routed
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
    fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        let mut state = self.state();
        state.lookups.push(name.to_string());
        let Ok(key) = to_key(name) else {
            return Ok(Vec::new());
        };
        while state.held.contains_key(&key) {
            state = self
                .released
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        Ok(state.answers.get(&key).cloned().unwrap_or_default())
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
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_name_held_twice_waits_until_both_holds_drop() {
        let mock = Arc::new(Mock::default());
        mock.resolve_to_addrs("held.test", vec![IpAddr::from([93, 184, 216, 34])])
            .unwrap();
        let outer = mock.hold("held.test").unwrap();
        let inner = mock.hold("held.test").unwrap();
        let (done, answered) = mpsc::channel();
        let resolving = mock.clone();
        thread::spawn(move || {
            let _ = done.send(resolving.resolve("held.test").map(|found| found.len()).ok());
        });
        drop(inner);
        let waited = answered.recv_timeout(Duration::from_millis(100)).is_err();
        drop(outer);
        assert_eq!(
            (waited, answered.recv_timeout(Duration::from_secs(10)).ok()),
            (true, Some(Some(1))),
            "a lookup of a name held twice must wait while either hold lives, and answer once both \
             have dropped"
        );
    }
}
