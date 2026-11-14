//! A Protocol Family and a Resolver for a Host App's tests: names answer
//! from a table, a dial reaches only a route it was given, and each question
//! is recorded so a test reads back what the guest asked. It stands where
//! `HostInet` stands, beneath every rule, so the rules judge it alike.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use super::family::{HostInet, ProtocolFamily};
use super::lookup::Resolver;

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
    routes: HashMap<SocketAddr, Dial>,
    held: HashSet<String>,
    lookups: Vec<String>,
    dials: Vec<SocketAddr>,
}

/// What a dial to one address does.
#[derive(Clone, Copy)]
enum Dial {
    To(SocketAddr),
    Refused(i32),
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
    /// count as local.
    pub fn with_local_addresses(self, addresses: Vec<IpAddr>) -> Self {
        Self {
            local_addresses: Some(addresses),
            ..self
        }
    }

    /// A table that cannot say the host's own addresses, so the rules refuse
    /// every connect to a name.
    pub fn without_local_addresses(self) -> Self {
        Self {
            local_addresses: None,
            ..self
        }
    }

    /// Answer `name` with `addresses` from now on.
    pub fn answer(&self, name: &str, addresses: Vec<IpAddr>) {
        self.state().answers.insert(to_key(name), addresses);
    }

    /// Dial `to` whenever `from` is dialed.
    pub fn route(&self, from: SocketAddr, to: SocketAddr) {
        self.state().routes.insert(from, Dial::To(to));
    }

    /// Fail a dial to `addr` as a host socket failing with `errno` does.
    pub fn refuse(&self, addr: SocketAddr, errno: i32) {
        self.state().routes.insert(addr, Dial::Refused(errno));
    }

    /// Keep each lookup of `name` waiting until the hold drops, as a
    /// resolver that has not answered.
    pub fn hold(&self, name: &str) -> Hold<'_> {
        let name = to_key(name);
        self.state().held.insert(name.clone());
        Hold { mock: self, name }
    }

    /// Each name the guest's lookups asked, in order.
    pub fn lookups(&self) -> Vec<String> {
        self.state().lookups.clone()
    }

    /// Each address dialed, in order, routed or not.
    pub fn dials(&self) -> Vec<SocketAddr> {
        self.state().dials.clone()
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
        self.mock.state().held.remove(&self.name);
        self.mock.released.notify_all();
    }
}

impl ProtocolFamily for Mock {
    /// The route's target, through the host's own sockets; an address with
    /// no route is refused, so nothing unrouted reaches the host network.
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        let dial = {
            let mut state = self.state();
            state.dials.push(addr);
            state.routes.get(&addr).copied()
        };
        match dial {
            Some(Dial::To(target)) => HostInet.connect(target),
            Some(Dial::Refused(errno)) => Err(io::Error::from_raw_os_error(errno)),
            None => Err(io::ErrorKind::ConnectionRefused.into()),
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
    fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        let key = to_key(name);
        let mut state = self.state();
        state.lookups.push(name.to_string());
        while state.held.contains(&key) {
            state = self
                .released
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        Ok(state.answers.get(&key).cloned().unwrap_or_default())
    }
}

/// A name as the table keys it, in any case and with or without its root.
fn to_key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}
