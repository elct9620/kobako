//! A stub resolver, as glibc's `res_send` is: a name in `/etc/hosts` answers
//! from there, as nsswitch's `files` comes before `dns`; any other is asked of
//! each `nameserver` in `/etc/resolv.conf` over UDP, and over TCP when a reply
//! comes truncated. Its sockets wait on the reactor, so dropping the query
//! closes them, which glibc's blocking `getaddrinfo` cannot offer.

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::future::poll_fn;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::task::Poll;
use std::time::{Duration, Instant};

use mio::event::Source;
use simple_dns::rdata::RData;
use simple_dns::{Name, Packet, PacketFlag, Question, CLASS, QCLASS, QTYPE, RCODE, TYPE};

use super::ReadyWaker;
use crate::net::lookup::{LookupError, Resolving};
use crate::net::reactor::{Reactor, Ready, Registration};

/// glibc's `RES_TIMEOUT`, `RES_DFLRETRY`, `RES_MAXRETRANS`, `RES_MAXRETRY`,
/// and `MAXNS`.
const RES_TIMEOUT: u64 = 5;
const RES_DFLRETRY: u32 = 2;
const RES_MAXRETRANS: u64 = 30;
const RES_MAXRETRY: u32 = 5;
const MAXNS: usize = 3;

/// The port a name server answers on.
const NAMESERVER_PORT: u16 = 53;

/// glibc's `PACKETSZ`: the largest reply a UDP read takes, past which a
/// server truncates.
const PACKETSZ: usize = 512;

/// glibc's `NS_MAXCDNAME` and `NS_MAXLABEL`: the most bytes a name takes on
/// the wire, and a label holds.
const NS_MAXCDNAME: usize = 255;
const NS_MAXLABEL: usize = 63;

pub(super) fn resolve(name: &str) -> Resolving {
    let bare = name.strip_suffix('.').unwrap_or(name).to_string();
    if let Some(found) = hosts_answer_of(&read_config("/etc/hosts"), &bare) {
        return Box::pin(std::future::ready(Ok(found)));
    }
    if !is_host_name(&bare) {
        return Box::pin(std::future::ready(Err(LookupError::NoName)));
    }
    let conf = Conf::parse(&read_config("/etc/resolv.conf"));
    Box::pin(ask(conf, bare))
}

/// The text of the configuration at `path`, read as bytes as glibc reads
/// it, so a byte no encoding spells voids that byte alone, not the file.
fn read_config(path: &str) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// Whether `name` is a host name glibc's DNS backend asks about, as
/// `res_hnok` judges: one `ns_name_pton` can encode, each label of 1 to 63
/// letters, digits, `-`, and `_`, the whole at most `NS_MAXCDNAME` bytes on
/// the wire, the name not starting with `-`. `check_name` answers any other
/// `HOST_NOT_FOUND` with no query.
fn is_host_name(name: &str) -> bool {
    !name.starts_with('-')
        && name.len() + 2 <= NS_MAXCDNAME
        && name.split('.').all(|label| {
            (1..=NS_MAXLABEL).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
}

/// What `/etc/resolv.conf` says about the servers to ask, as `res_init`
/// reads it.
#[derive(Clone, Debug, PartialEq)]
struct Conf {
    nameservers: Vec<SocketAddr>,
    timeout: Duration,
    attempts: u32,
}

impl Conf {
    /// How long the server at `place` in the list is waited for, as
    /// `send_dg` gives it in whole seconds: doubled for each place down the
    /// list, shared among the servers past the first, and at least one.
    fn timeout_of(&self, place: usize) -> Duration {
        match place {
            0 => self.timeout,
            _ => Duration::from_secs(
                ((self.timeout.as_secs() << place) / self.nameservers.len() as u64).max(1),
            ),
        }
    }

    fn parse(text: &str) -> Self {
        let mut nameservers = Vec::new();
        let mut timeout = RES_TIMEOUT;
        let mut attempts = RES_DFLRETRY;
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("nameserver") if nameservers.len() < MAXNS => {
                    let address = words.next().and_then(|word| word.parse::<IpAddr>().ok());
                    if let Some(ip) = address {
                        nameservers.push(SocketAddr::new(ip, NAMESERVER_PORT));
                    }
                }
                Some("options") => {
                    for option in words {
                        if let Some(n) =
                            option.strip_prefix("timeout:").and_then(|n| n.parse().ok())
                        {
                            timeout = u64::min(n, RES_MAXRETRANS);
                        }
                        if let Some(n) = option
                            .strip_prefix("attempts:")
                            .and_then(|n| n.parse().ok())
                        {
                            attempts = u32::min(n, RES_MAXRETRY);
                        }
                    }
                }
                _ => {}
            }
        }
        if nameservers.is_empty() {
            nameservers.push(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), NAMESERVER_PORT));
        }
        Self {
            nameservers,
            timeout: Duration::from_secs(timeout.max(1)),
            attempts: attempts.max(1),
        }
    }
}

/// The addresses `/etc/hosts` holds for `name`, in its order, or `None` when
/// it names none.
fn hosts_answer_of(hosts: &str, name: &str) -> Option<Vec<IpAddr>> {
    let mut found: Vec<IpAddr> = Vec::new();
    for line in hosts.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut words = line.split_whitespace();
        let Some(ip) = words.next().and_then(|word| word.parse::<IpAddr>().ok()) else {
            continue;
        };
        if words.any(|alias| alias.eq_ignore_ascii_case(name)) {
            found.push(ip);
        }
    }
    (!found.is_empty()).then_some(found)
}

/// What one server said of one question.
#[derive(Debug, PartialEq)]
enum Reply {
    /// The addresses the name led to, none when it does not exist.
    Answered(Vec<IpAddr>),
    /// Too long for UDP, to be asked again over TCP.
    Truncated,
    /// The server could not answer, so the next is asked.
    Failed,
}

/// Each server in turn, `attempts` rounds over all, as `res_send` tries
/// them, until one answers; a server it cannot reach gives way to the next.
/// None answering is `Again`, as `res_query` reads `res_send`'s failure as
/// `TRY_AGAIN`.
async fn ask(conf: Conf, name: String) -> Result<Vec<IpAddr>, LookupError> {
    for _ in 0..conf.attempts {
        for (place, server) in conf.nameservers.iter().enumerate() {
            if let Ok(Some(found)) = exchange(*server, &name, conf.timeout_of(place)).await {
                return Ok(found);
            }
        }
    }
    Err(LookupError::Again)
}

/// Ask `server` for `name`'s AAAA and A records, as glibc's `send_dg` asks:
/// both at once, then, when the time runs out with one replied, the other
/// alone, and once more on a fresh socket, before taking the one reply it
/// has. A server that fails one question gives the other's answer at once.
/// `None` when neither was answered, so the next server is asked.
async fn exchange(
    server: SocketAddr,
    name: &str,
    timeout: Duration,
) -> io::Result<Option<Vec<IpAddr>>> {
    let questions = [TYPE::AAAA, TYPE::A].map(|rtype| (draw_id(), rtype));
    let mut replies: [Option<Reply>; 2] = [None, None];
    let mut socket = WatchedSocket::connect_udp(server)?;
    let deadline = Instant::now() + timeout;
    for (id, rtype) in questions {
        socket
            .send(&encode_query(id, name, rtype)?, deadline)
            .await?;
    }
    socket
        .collect(&mut replies, questions, name, deadline)
        .await?;
    for reopen in [false, true] {
        let replied = replies.iter().any(Option::is_some);
        let Some(unreplied) = replies.iter().position(Option::is_none) else {
            break;
        };
        if !replied {
            break;
        }
        if reopen {
            socket = WatchedSocket::connect_udp(server)?;
        }
        let deadline = Instant::now() + timeout;
        let (id, rtype) = questions[unreplied];
        socket
            .send(&encode_query(id, name, rtype)?, deadline)
            .await?;
        socket
            .collect(&mut replies, questions, name, deadline)
            .await?;
    }
    let mut found = None::<Vec<IpAddr>>;
    for (reply, (id, rtype)) in replies.into_iter().zip(questions) {
        let reply = match reply {
            Some(Reply::Truncated) => {
                ask_over_tcp(server, id, name, rtype, Instant::now() + timeout)
                    .await
                    .ok()
                    .flatten()
            }
            reply => reply,
        };
        if let Some(Reply::Answered(addresses)) = reply {
            found.get_or_insert_with(Vec::new).extend(addresses);
        }
    }
    Ok(found)
}

/// Ask `server` the one question again over TCP, as `send_vc` does.
async fn ask_over_tcp(
    server: SocketAddr,
    id: u16,
    name: &str,
    rtype: TYPE,
    deadline: Instant,
) -> io::Result<Option<Reply>> {
    let query = encode_query(id, name, rtype)?;
    let mut framed = (query.len() as u16).to_be_bytes().to_vec();
    framed.extend(query);
    let mut stream = WatchedSocket::connect_tcp(server)?;
    stream.write_all(&framed, deadline).await?;
    let mut length = [0u8; 2];
    if !stream.read_exact(&mut length, deadline).await? {
        return Ok(None);
    }
    let mut reply = vec![0u8; usize::from(u16::from_be_bytes(length))];
    if !stream.read_exact(&mut reply, deadline).await? {
        return Ok(None);
    }
    Ok(reply_of(&reply, id, name, rtype))
}

/// Encode a query for `name`'s `rtype` records, recursion desired.
fn encode_query(id: u16, name: &str, rtype: TYPE) -> io::Result<Vec<u8>> {
    let mut packet = Packet::new_query(id);
    packet.set_flags(PacketFlag::RECURSION_DESIRED);
    let qname = Name::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
    packet.questions.push(Question::new(
        qname,
        QTYPE::TYPE(rtype),
        QCLASS::CLASS(CLASS::IN),
        false,
    ));
    packet.build_bytes_vec().map_err(io::Error::other)
}

/// What `bytes` says of the question `id` asked, or `None` when it is no
/// reply to that question, as `res_send` discards a reply whose ID or
/// question does not match. An answer counts only on the chain of CNAMEs from
/// the name asked, so a record for any other name is never taken.
fn reply_of(bytes: &[u8], id: u16, name: &str, rtype: TYPE) -> Option<Reply> {
    // The ID is read before the rest, so a stray datagram is never parsed.
    if bytes.get(..2)? != id.to_be_bytes() {
        return None;
    }
    let packet = Packet::parse(bytes).ok()?;
    let question = packet.questions.first()?;
    let asked = packet.has_flags(PacketFlag::RESPONSE)
        && packet.questions.len() == 1
        && question.qtype == QTYPE::TYPE(rtype)
        && question.qclass == QCLASS::CLASS(CLASS::IN)
        && is_same_name(&question.qname, name);
    if !asked {
        return None;
    }
    if packet.has_flags(PacketFlag::TRUNCATION) {
        return Some(Reply::Truncated);
    }
    match packet.rcode() {
        RCODE::NoError => {}
        RCODE::NameError => return Some(Reply::Answered(Vec::new())),
        _ => return Some(Reply::Failed),
    }
    let aliases: HashMap<String, String> = packet
        .answers
        .iter()
        .filter_map(|record| match &record.rdata {
            RData::CNAME(target) => Some((
                record.name.to_string().to_ascii_lowercase(),
                target.0.to_string().to_ascii_lowercase(),
            )),
            _ => None,
        })
        .collect();
    let mut chain = HashSet::from([name.to_ascii_lowercase()]);
    let mut at = name.to_ascii_lowercase();
    while let Some(target) = aliases.get(&at).filter(|target| !chain.contains(*target)) {
        chain.insert(target.clone());
        at = target.clone();
    }
    let mut found = Vec::new();
    for record in &packet.answers {
        if record.class != CLASS::IN || !is_on_chain(&chain, &record.name) {
            continue;
        }
        let ip = match &record.rdata {
            RData::A(a) if rtype == TYPE::A => IpAddr::from(Ipv4Addr::from(a.address)),
            RData::AAAA(aaaa) if rtype == TYPE::AAAA => IpAddr::from(Ipv6Addr::from(aaaa.address)),
            _ => continue,
        };
        found.push(ip);
    }
    Some(Reply::Answered(found))
}

fn is_same_name(qname: &Name<'_>, name: &str) -> bool {
    qname.to_string().eq_ignore_ascii_case(name)
}

fn is_on_chain(chain: &HashSet<String>, name: &Name<'_>) -> bool {
    chain.contains(&name.to_string().to_ascii_lowercase())
}

/// Draw a query ID no off-path sender can guess, as `res_mkquery` does.
fn draw_id() -> u16 {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.finish() as u16
}

/// A socket the reactor watches, waited on until a deadline.
struct WatchedSocket<S: Source> {
    socket: S,
    registration: Registration,
    waker: Arc<ReadyWaker>,
}

impl WatchedSocket<mio::net::UdpSocket> {
    /// A UDP socket connected to `server`, so the host takes replies from
    /// that server alone.
    fn connect_udp(server: SocketAddr) -> io::Result<Self> {
        let socket = mio::net::UdpSocket::bind(super::any_address_of(server))?;
        socket.connect(server)?;
        Self::watch(socket)
    }

    async fn send(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        loop {
            match self.socket.send(bytes) {
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                sent => return sent.map(drop),
            }
            if !self.wait_for_change(deadline).await {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }

    /// Fill each empty slot of `replies` with the reply to its question,
    /// until all are filled or `deadline` passes.
    async fn collect(
        &mut self,
        replies: &mut [Option<Reply>; 2],
        questions: [(u16, TYPE); 2],
        name: &str,
        deadline: Instant,
    ) -> io::Result<()> {
        let mut buffer = [0u8; PACKETSZ];
        while replies.iter().any(Option::is_none) {
            let Some(read) = self.recv(&mut buffer, deadline).await? else {
                return Ok(());
            };
            for (slot, (id, rtype)) in replies.iter_mut().zip(questions) {
                if slot.is_none() {
                    *slot = reply_of(&buffer[..read], id, name, rtype);
                }
            }
        }
        Ok(())
    }

    /// The next datagram's length, or `None` at the deadline.
    async fn recv(&mut self, buffer: &mut [u8], deadline: Instant) -> io::Result<Option<usize>> {
        loop {
            match self.socket.recv(buffer) {
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                // An ICMP error from the server reads as a refusal; the next
                // server is asked.
                Err(_) => return Ok(None),
                Ok(read) => return Ok(Some(read)),
            }
            if !self.wait_for_change(deadline).await {
                return Ok(None);
            }
        }
    }
}

impl WatchedSocket<mio::net::TcpStream> {
    fn connect_tcp(server: SocketAddr) -> io::Result<Self> {
        Self::watch(mio::net::TcpStream::connect(server)?)
    }

    async fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            match self.socket.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => bytes = &bytes[written..],
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected
                    ) =>
                {
                    if !self.wait_for_change(deadline).await {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// Fill `buffer`; `false` when the stream ends or the deadline passes
    /// first.
    async fn read_exact(&mut self, mut buffer: &mut [u8], deadline: Instant) -> io::Result<bool> {
        while !buffer.is_empty() {
            match self.socket.read(buffer) {
                Ok(0) => return Ok(false),
                Ok(read) => buffer = &mut buffer[read..],
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    if !self.wait_for_change(deadline).await {
                        return Ok(false);
                    }
                }
                Err(err) => return Err(err),
            }
        }
        Ok(true)
    }
}

impl<S: Source> WatchedSocket<S> {
    fn watch(mut socket: S) -> io::Result<Self> {
        let waker = Arc::new(ReadyWaker::default());
        let ready = Arc::downgrade(&waker) as Weak<dyn Ready>;
        let registration = Reactor::current()?.register(&mut socket, ready)?;
        Ok(Self {
            socket,
            registration,
            waker,
        })
    }

    /// Wait until the socket changes, `false` once `deadline` passes first.
    /// The timer goes with the wait it bounds, whether it ends or is
    /// cancelled.
    async fn wait_for_change(&self, deadline: Instant) -> bool {
        let mut timer = None;
        poll_fn(|cx| {
            if self.waker.poll_changed(cx).is_ready() {
                return Poll::Ready(true);
            }
            if Instant::now() >= deadline {
                return Poll::Ready(false);
            }
            if timer.is_none() {
                timer = Reactor::current()
                    .ok()
                    .map(|reactor| reactor.wake_at(deadline, cx.waker().clone()));
            }
            Poll::Pending
        })
        .await
    }
}

impl<S: Source> Drop for WatchedSocket<S> {
    fn drop(&mut self) {
        self.registration.deregister(&mut self.socket);
    }
}

#[cfg(test)]
mod tests;
