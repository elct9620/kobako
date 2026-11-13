//! A socket is a file, as Linux's `net/socket.c` makes one: every socket
//! stands on sockfs, and its file hands each call to the protocol's operations,
//! as `socket_file_ops` hands them to `proto_ops`.

use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use socket2::SockRef;

use crate::cred::Cred;
use crate::memcg::MemoryCharge;
use crate::vfs::{
    alloc_file_pseudo, Dentry, FMode, File, FileOperations, FileSystemType, FileType, Inode,
    InodeOperations, Kiocb, Kstat, OpenHow, PollMask, SuperBlock, VfsMount,
};
use crate::wait::PollTable;
use crate::Errno;

use super::connections::{open, OpenConnections, Place};
use super::family::admitted_addresses_of;
use super::namespace::TcpSockets;
use super::{Namespace, ProtocolFamily, TcpSock, UdpSock};
use crate::network::{Connection, Protocol, Ruleset};

/// How much a socket holds each way before it stops reading and refuses
/// more, as `sk_rcvbuf` and `sk_sndbuf` bound a Linux socket and net core's
/// `rmem_max` and `wmem_max` bound what either may be set to, so a full
/// buffer pushes back on the peer.
pub const MAX_SOCKET_BUFFER: usize = 64 * 1024;

/// What one protocol answers for its socket, as Linux's `struct proto_ops`:
/// its bind, the file's reads and writes, what a poll finds, its shutdown,
/// names, and options, and the release once every view is gone, whichever
/// protocol stands behind.
pub trait ProtoOps: Send + Sync {
    /// Take `addr` as the socket's own end once the socket hooks passed it,
    /// its port from `net`'s ports for the protocol, as `->bind` runs after
    /// `__sys_bind` asks `security_socket_bind`: an IPv6 address answers
    /// `AddressFamilyNotSupported`, a socket bound or connected already
    /// `InvalidInput`, and a port held elsewhere `AddressInUse`, as
    /// `__inet_bind` refuses one.
    fn bind(&self, net: &Namespace, addr: SocketAddr) -> Result<(), Errno>;
    /// Connect to `addr`, the address the guest named, once the socket hooks
    /// passed it, as `->connect` runs after `__sys_connect` asks
    /// `security_socket_connect`: the socket's own state is checked first,
    /// then `net` translates `addr` as cgroup `connect4` does and its ruleset
    /// judges the real destination.
    fn connect(&self, net: &Namespace, addr: SocketAddr) -> Result<(), Errno>;
    /// Read what the socket holds and who sent it, as `->recvmsg` fills
    /// `msg_name`: a datagram's peer as the guest named it, none for a
    /// stream; `WouldBlock` while it holds nothing yet.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddr>), Errno>;
    /// Write what the socket takes now; `WouldBlock` while it takes nothing.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, Errno>;
    /// What a call would find ready, the socket's wait queue registered on
    /// `table`, as a protocol's `->poll` calls `sock_poll_wait`.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask;
    /// Shut the reading side, the writing side, or both down, as
    /// `inet_shutdown` marks `sk_shutdown`; a socket connected to nothing
    /// answers `ENOTCONN`.
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno>;
    /// The `peer` end, or the guest's own, as `->getname` answers each. The
    /// peer is the address the guest named, mapped back from the real one
    /// through cgroup `getpeername4`; a socket connected to nothing answers
    /// `ENOTCONN`. The own end passes cgroup `getsockname4`: the guest's own
    /// virtual address and the port its namespace gave as the connect began,
    /// or the unspecified address before then, as Linux answers an unbound
    /// socket.
    fn getname(&self, peer: bool) -> Result<SocketAddr, Errno>;
    /// The protocol the socket speaks, as `sk_protocol` names it, which the
    /// socket hooks judge a connect by.
    fn protocol(&self) -> Protocol;
    /// The value `opt` holds on this socket, never the host's; one the
    /// protocol has not answers `ProtocolNotAvailable`.
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, Errno>;
    /// Set `opt` to `value`, as `setsockopt` does, refusing what Linux
    /// refuses; a host socket already dialed takes it at once.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), Errno>;
    /// Close the socket once every view is gone, as `sock_close` runs on the
    /// last `fput`; a protocol may finish sending before its host socket goes.
    fn release(&self);
}

/// A socket option a protocol may answer, as `setsockopt`'s level and name
/// pick one. Sizes are bytes, times are seconds, and a flag is 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SockOpt {
    /// `SO_RCVBUF`, which reads twice what was set.
    RcvBuf,
    /// `SO_SNDBUF`, which reads twice what was set.
    SndBuf,
    /// `IP_TTL`.
    Ttl,
    /// `SO_KEEPALIVE`.
    KeepAlive,
    /// `TCP_KEEPIDLE`.
    KeepIdle,
    /// `TCP_KEEPINTVL`.
    KeepIntvl,
    /// `TCP_KEEPCNT`.
    KeepCnt,
    /// `TCP_NODELAY`, which sends each write without Nagle's delay.
    NoDelay,
}

/// The guest's own end of every socket it connects, kept out of what a lookup
/// hands out, as a container in its own network namespace reads its veth's
/// address while the host translates. It sits below the virtual range's last
/// address, the one a broadcast would take.
pub(crate) const LOCAL_ADDRESS: Ipv4Addr = Ipv4Addr::new(198, 19, 255, 254);

/// The ports a bind to port 0 or a connect gives the guest for its own end,
/// Linux's default `ip_local_port_range`, taken in turn from the bottom and
/// around again, so no host port reaches the guest and a script reads the
/// same ports each run.
const VIRTUAL_PORTS: std::ops::RangeInclusive<u16> = 32768..=60999;

/// The ports one protocol's sockets hold in a namespace, as Linux's `bhash`
/// keeps a bucket for each bound port: a port is held by one socket at a
/// time, and goes back as that socket puts it.
pub(crate) struct BindHash {
    ports: Mutex<Ports>,
}

/// The ports held, and where the next search for a free one starts.
struct Ports {
    held: HashSet<u16>,
    next: u16,
}

impl BindHash {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            ports: Mutex::new(Ports {
                held: HashSet::new(),
                next: *VIRTUAL_PORTS.start(),
            }),
        })
    }

    fn ports(&self) -> MutexGuard<'_, Ports> {
        self.ports.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `port` held for one socket, or `EADDRINUSE` while another holds it,
    /// as `inet_csk_get_port` finds its bucket taken.
    fn get_port(self: &Arc<Self>, port: u16) -> Result<BoundPort, Errno> {
        match self.ports().held.insert(port) {
            true => Ok(BoundPort {
                hash: self.clone(),
                port,
            }),
            false => Err(Errno::AddressInUse),
        }
    }

    /// The first free port of `VIRTUAL_PORTS` from where the last search
    /// left off, as `inet_csk_find_open_port` and `inet_hash_connect` walk
    /// `ip_local_port_range` past each port held; `None` once every one is
    /// held.
    fn find_open_port(self: &Arc<Self>) -> Option<BoundPort> {
        let mut ports = self.ports();
        let start = ports.next;
        let mut port = start;
        loop {
            let after = match port == *VIRTUAL_PORTS.end() {
                true => *VIRTUAL_PORTS.start(),
                false => port + 1,
            };
            if ports.held.insert(port) {
                ports.next = after;
                return Some(BoundPort {
                    hash: self.clone(),
                    port,
                });
            }
            if after == start {
                return None;
            }
            port = after;
        }
    }
}

/// One port a socket holds in its protocol's `BindHash`, given back as it is
/// dropped, as `__inet_put_port` takes a socket off its bucket.
pub(crate) struct BoundPort {
    hash: Arc<BindHash>,
    port: u16,
}

impl Drop for BoundPort {
    fn drop(&mut self) {
        self.hash.ports().held.remove(&self.port);
    }
}

/// Where a socket stands as the calls TCP and UDP share read Linux's
/// `sk_state`: a UDP socket naming a peer is `TCP_ESTABLISHED`, as
/// `__ip4_datagram_connect` sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkState {
    /// `TCP_CLOSE`: connected to nothing, or back from a failure.
    Close,
    /// `TCP_SYN_SENT`: a connect under way.
    SynSent,
    /// `TCP_ESTABLISHED`.
    Established,
}

/// What every inet socket holds beside its protocol's own state, as Linux's
/// `inet_sock` sits beneath `tcp_sock` and `udp_sock`.
pub(crate) struct InetSock {
    /// The port the socket holds, as `inet_num`, which a bind or a connect
    /// takes and closing puts back.
    num: Option<BoundPort>,
    /// The port the guest reads for its own end, as `inet_sport`, kept after
    /// the port goes back until another is taken.
    sport: Option<u16>,
    /// The address a bind named, as `inet_rcv_saddr`.
    rcv_saddr: Ipv4Addr,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    pub(crate) peer: Option<SocketAddr>,
    /// How much each queue holds, as `sk_rcvbuf` and `sk_sndbuf`.
    pub(crate) buffers: Buffers,
    /// The hop limit sent with, as `inet_sock` keeps `uc_ttl`.
    pub(crate) uc_ttl: u8,
    /// The socket's pending failure, as `sk_err`: answered once, then
    /// cleared, as `sock_error` swaps it with zero.
    pub(crate) err: Option<Errno>,
    /// The guest shut its reading side down, as `sk_shutdown`'s
    /// `RCV_SHUTDOWN`.
    pub(crate) rcv_shutdown: bool,
    /// The guest shut its writing side down, as `SEND_SHUTDOWN`.
    pub(crate) send_shutdown: bool,
    /// The socket's place against its Sandbox's cap and the process's, given
    /// back as it is released.
    pub(crate) place: Option<Place>,
    /// Whether the bind named its port, which the socket then keeps once it
    /// closes, as `SOCK_BINDPORT_LOCK` does.
    bindport_locked: bool,
    /// What its queues may hold, reserved against the Invocation's memory
    /// until the socket is released.
    memory: Option<MemoryCharge>,
}

impl InetSock {
    /// A socket whose queues start at `buffers`, within what `memory`
    /// reserved.
    pub(crate) fn new(memory: MemoryCharge, buffers: Buffers) -> Self {
        Self {
            num: None,
            sport: None,
            rcv_saddr: Ipv4Addr::UNSPECIFIED,
            peer: None,
            buffers,
            uc_ttl: IPDEFTTL,
            err: None,
            rcv_shutdown: false,
            send_shutdown: false,
            place: None,
            bindport_locked: false,
            memory: Some(memory),
        }
    }

    /// Take `addr` as the socket's own end, its port from `hash`, as
    /// `__inet_bind` does: an IPv6 address answers `EAFNOSUPPORT`, and a
    /// socket holding a port, or past `TCP_CLOSE` and so bound by its connect,
    /// `EINVAL`. Port 0 takes a free virtual port, and a named port one no
    /// other socket holds, either answering `EADDRINUSE` once none is left,
    /// as `inet_csk_get_port` does.
    pub(crate) fn bind(
        &mut self,
        addr: SocketAddr,
        state: SkState,
        hash: &Arc<BindHash>,
    ) -> Result<(), Errno> {
        let IpAddr::V4(ip) = addr.ip() else {
            return Err(Errno::AddressFamilyNotSupported);
        };
        if self.num.is_some() || state != SkState::Close {
            return Err(Errno::InvalidInput);
        }
        let bound = match addr.port() {
            0 => hash.find_open_port().ok_or(Errno::AddressInUse)?,
            port => hash.get_port(port)?,
        };
        self.sport = Some(bound.port);
        self.num = Some(bound);
        self.bindport_locked = addr.port() != 0;
        self.rcv_saddr = ip;
        Ok(())
    }

    /// Give back the port the socket holds once it closes, unless a bind
    /// named it, as `tcp_set_state` puts the port on entering `TCP_CLOSE`
    /// without `SOCK_BINDPORT_LOCK`.
    pub(crate) fn put_port(&mut self) {
        if !self.bindport_locked {
            self.num = None;
        }
    }

    /// The peer as the guest named it, or the guest's own end, as
    /// `inet_getname` answers TCP and UDP alike. A peer answers `ENOTCONN`
    /// unless one is named and the socket is past `TCP_SYN_SENT`. The own end
    /// reads `inet_rcv_saddr`, or the guest's own virtual address once it
    /// names a peer, and `inet_sport`.
    pub(crate) fn getname(&self, peer: bool, state: SkState) -> Result<SocketAddr, Errno> {
        if peer {
            return match state {
                SkState::Established => self.peer.ok_or(Errno::NotConnected),
                SkState::Close | SkState::SynSent => Err(Errno::NotConnected),
            };
        }
        let ip = match (self.rcv_saddr.is_unspecified(), self.peer) {
            (true, Some(_)) => LOCAL_ADDRESS,
            _ => self.rcv_saddr,
        };
        Ok(SocketAddr::from((ip, self.sport.unwrap_or(0))))
    }

    /// Hold a free port from `hash` as a connect begins, unless the socket
    /// holds one, as `inet_hash_connect` picks a port only when `inet_num` is
    /// zero, answering `EADDRNOTAVAIL` once none is left. A port put back once
    /// the socket closed is then replaced, while reading the socket's own end
    /// still answers the old one until then, as `inet_sport` keeps it.
    pub(crate) fn hash_connect(&mut self, hash: &Arc<BindHash>) -> Result<(), Errno> {
        if self.num.is_none() {
            let bound = hash.find_open_port().ok_or(Errno::AddressNotAvailable)?;
            self.sport = Some(bound.port);
            self.num = Some(bound);
        }
        Ok(())
    }

    /// A new place against `open_connections` and the process, or `None`
    /// when the socket holds one already, so a socket connecting again
    /// counts once; either cap full answers `EAGAIN`.
    pub(crate) fn open_place(
        &self,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Option<Place>, Errno> {
        match self.place {
            Some(_) => Ok(None),
            None => open(open_connections).map(Some).ok_or(Errno::WouldBlock),
        }
    }

    /// Mark the sides `how` names shut, as `inet_shutdown` sets
    /// `sk_shutdown`.
    pub(crate) fn mark_shutdown(&mut self, how: Shutdown) {
        self.rcv_shutdown |= matches!(how, Shutdown::Read | Shutdown::Both);
        self.send_shutdown |= matches!(how, Shutdown::Write | Shutdown::Both);
    }

    /// Shut the sides `how` names, as `inet_shutdown` does for TCP and UDP
    /// alike: a socket in `TCP_CLOSE` marks them still and answers
    /// `ENOTCONN`. A protocol disconnects one in `TCP_SYN_SENT` itself.
    pub(crate) fn shutdown(&mut self, how: Shutdown, state: SkState) -> Result<(), Errno> {
        self.mark_shutdown(how);
        match state {
            SkState::Close => Err(Errno::NotConnected),
            SkState::SynSent | SkState::Established => Ok(()),
        }
    }

    /// Give back the socket's place and its memory, as `sk_free` does once
    /// the socket is released.
    pub(crate) fn release(&mut self) {
        self.place = None;
        self.memory = None;
    }

    /// What `opt` reads at the socket and IP levels, as `sk_getsockopt` and
    /// `do_ip_getsockopt` answer; a TCP option answers `ProtocolNotAvailable`.
    pub(crate) fn getsockopt(&self, opt: SockOpt) -> Result<u32, Errno> {
        let value = match opt {
            SockOpt::RcvBuf => self.buffers.rcvbuf,
            SockOpt::SndBuf => self.buffers.sndbuf,
            SockOpt::Ttl => usize::from(self.uc_ttl),
            _ => return Err(Errno::ProtocolNotAvailable),
        };
        Ok(u32::try_from(value).unwrap_or(u32::MAX))
    }

    /// Set `opt` at the socket and IP levels, as `sk_setsockopt` and
    /// `do_ip_setsockopt` do for every inet protocol: a buffer to twice
    /// `value`, and the hop limit, which reaches `host`, a host socket
    /// already dialed, at once; a host refusing it leaves the old value, as a
    /// failed `setsockopt` changes nothing. A TCP option answers
    /// `ProtocolNotAvailable`.
    pub(crate) fn setsockopt(
        &mut self,
        opt: SockOpt,
        value: u32,
        host: Option<SockRef<'_>>,
    ) -> Result<(), Errno> {
        let bytes = usize::try_from(value).unwrap_or(usize::MAX);
        match opt {
            SockOpt::RcvBuf => self.buffers.set_rcvbuf(bytes),
            SockOpt::SndBuf => self.buffers.set_sndbuf(bytes),
            SockOpt::Ttl => {
                let ttl = to_ttl(value)?;
                if let Some(host) = host {
                    set_hop_limit(host, ttl).map_err(|err| to_error(&err))?;
                }
                self.uc_ttl = ttl;
            }
            _ => return Err(Errno::ProtocolNotAvailable),
        }
        Ok(())
    }
}

/// The least `SO_RCVBUF` and `SO_SNDBUF` leave a queue, Linux's
/// `SOCK_MIN_RCVBUF` and `SOCK_MIN_SNDBUF`.
const SOCK_MIN_RCVBUF: usize = 2304;
const SOCK_MIN_SNDBUF: usize = 4608;

/// What a TCP socket's queues hold until it connects, Linux's `tcp_wmem[1]`
/// and `tcp_rmem[1]`.
const TCP_WMEM_DEFAULT: usize = 16 * 1024;
const TCP_RMEM_DEFAULT: usize = 128 * 1024;

/// How much a socket's queues hold, as `sk_rcvbuf` and `sk_sndbuf`. Each
/// starts at Linux's default and never passes the buffer its memory
/// reserved, which plays `rmem_max` and `wmem_max` as twice the most a set
/// takes.
pub(crate) struct Buffers {
    pub(crate) rcvbuf: usize,
    pub(crate) sndbuf: usize,
    /// Whether the guest set `SO_SNDBUF`, which Linux's `SOCK_SNDBUF_LOCK`
    /// keeps from growing.
    sndbuf_locked: bool,
}

impl Default for Buffers {
    /// A datagram socket's, Linux's `rmem_default` and `wmem_default`, both
    /// past the reserved buffer.
    fn default() -> Self {
        Self::within_reserve(MAX_SOCKET_BUFFER, MAX_SOCKET_BUFFER)
    }
}

impl Buffers {
    /// A TCP socket's, until it connects.
    pub(crate) fn tcp() -> Self {
        Self::within_reserve(TCP_RMEM_DEFAULT, TCP_WMEM_DEFAULT)
    }

    fn within_reserve(rcvbuf: usize, sndbuf: usize) -> Self {
        Self {
            rcvbuf: rcvbuf.min(MAX_SOCKET_BUFFER),
            sndbuf: sndbuf.min(MAX_SOCKET_BUFFER),
            sndbuf_locked: false,
        }
    }

    /// Grow a connected TCP socket's send buffer, as `tcp_sndbuf_expand` makes
    /// room for its first window: two of `TCP_INIT_CWND` segments, which pass
    /// the reserved buffer at any MSS. One the guest set stays.
    pub(crate) fn sndbuf_expand(&mut self) {
        if !self.sndbuf_locked {
            self.sndbuf = MAX_SOCKET_BUFFER;
        }
    }

    /// Twice `bytes`, within the cap, as `sock_setsockopt` doubles a value to
    /// leave room for its bookkeeping.
    fn to_buffer_room(bytes: usize, least: usize) -> usize {
        (bytes.min(MAX_SOCKET_BUFFER / 2) * 2).max(least)
    }

    pub(crate) fn set_rcvbuf(&mut self, bytes: usize) {
        self.rcvbuf = Self::to_buffer_room(bytes, SOCK_MIN_RCVBUF);
    }

    pub(crate) fn set_sndbuf(&mut self, bytes: usize) {
        self.sndbuf = Self::to_buffer_room(bytes, SOCK_MIN_SNDBUF);
        self.sndbuf_locked = true;
    }
}

/// The hop limit a socket sends with until the guest sets one, Linux's
/// `IPDEFTTL`; the host's own default never reaches the guest.
pub(crate) const IPDEFTTL: u8 = 64;

/// `ttl` as a hop limit Linux takes, 1 to 255, refusing the rest with EINVAL
/// as `do_ip_setsockopt` does.
pub(crate) fn to_ttl(ttl: u32) -> Result<u8, Errno> {
    match u8::try_from(ttl) {
        Ok(0) | Err(_) => Err(Errno::InvalidInput),
        Ok(ttl) => Ok(ttl),
    }
}

/// Give the host socket `socket` the hop limit `ttl`, as `IP_TTL` on an IPv4
/// one and `IPV6_UNICAST_HOPS` on an IPv6 one, since the guest's IPv4 socket
/// may reach the host over IPv6 and each family keeps its own.
pub(crate) fn set_hop_limit(socket: SockRef<'_>, ttl: u8) -> io::Result<()> {
    match socket.local_addr()?.is_ipv6() {
        true => socket.set_unicast_hops_v6(ttl.into()),
        false => socket.set_ttl_v4(ttl.into()),
    }
}

/// `value` within 1 to `max`, else EINVAL, as Linux bounds a TCP option.
pub(crate) fn ensure_within(value: u32, max: u32) -> Result<(), Errno> {
    match (1..=max).contains(&value) {
        true => Ok(()),
        false => Err(Errno::InvalidInput),
    }
}

/// Take how a non-blocking connect on `stream` stands, as reading `SO_ERROR`
/// clears it and then the peer's name tells: `None` while it is under way, and
/// its outcome once it settled. Every dialer of a host stream reads it the same
/// way.
pub(crate) fn take_connect_outcome(stream: &mio::net::TcpStream) -> Option<io::Result<()>> {
    match (stream.take_error(), stream.peer_addr()) {
        (Ok(Some(err)), _) | (Err(err), _) => Some(Err(err)),
        (Ok(None), Ok(_)) => Some(Ok(())),
        (Ok(None), Err(err)) if err.kind() == io::ErrorKind::NotConnected => None,
        (Ok(None), Err(err)) => Some(Err(err)),
    }
}

/// The POSIX name a host socket's failure stands for, read from its errno
/// first where std names no kind for it; one with none carries only its kind,
/// so no host address reaches the guest.
pub(crate) fn to_error(err: &io::Error) -> Errno {
    #[cfg(unix)]
    {
        use rustix::io::Errno as HostErrno;
        match HostErrno::from_io_error(err) {
            Some(HostErrno::NOBUFS) => return Errno::NoBufferSpace,
            Some(HostErrno::HOSTDOWN) => return Errno::HostUnreachable,
            #[cfg(target_os = "linux")]
            Some(HostErrno::NONET) => return Errno::NetworkUnreachable,
            _ => {}
        }
    }
    Errno::from_io_kind(err.kind()).unwrap_or_else(|| Errno::Other(format!("{:?}", err.kind())))
}

/// The one Filesystem every socket stands on, as Linux's `sock_mnt`. No path
/// reaches it: a socket is only ever open.
static SOCK_MNT: LazyLock<Arc<VfsMount>> = LazyLock::new(|| {
    let sb = SuperBlock::new(Arc::new(SockFs)).expect("sockfs fills in nothing that can fail");
    VfsMount::kern_mount(sb)
});

/// A socket reports what it is and no length, as `sock_alloc` makes one.
struct SockFs;

impl FileSystemType for SockFs {
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), Errno> {
        sb.d_make_root(Inode::new(&Kstat::new(FileType::Dir, 0), self));
        Ok(())
    }
}

impl InodeOperations for SockFs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, Errno> {
        Ok(Kstat::new(FileType::Sock, 0))
    }
}

/// What answers for a socket file, as `socket_file_ops` hands each call to the
/// socket's protocol operations. The file never waits: a read or write that
/// would wait answers EAGAIN and the guest polls, since WASI's sockets never
/// block and no encoding clears the file's `O_NONBLOCK`.
struct SocketFile {
    ops: Arc<dyn ProtoOps>,
}

impl FileOperations for SocketFile {
    /// A read of nothing answers zero before the socket is asked, as
    /// `sock_read_iter` does, so it takes no datagram.
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, Errno> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.ops.recvmsg(buf).map(|(n, _)| n)
    }

    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, Errno> {
        self.ops.sendmsg(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        self.ops.poll(table)
    }

    fn is_stream(&self) -> bool {
        true
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        self.ops.release();
    }
}

/// The socket file of `sock`, opened as `cred` for reading and writing, as
/// Linux's `sock_alloc_file` makes one; dropping it releases the socket.
pub fn sock_alloc_file(sock: Arc<dyn ProtoOps>, cred: &Arc<Cred>) -> File {
    let inode = Inode::new(&Kstat::new(FileType::Sock, 0), Arc::new(SockFs));
    alloc_file_pseudo(
        inode,
        &SOCK_MNT,
        &OpenHow {
            read: true,
            write: true,
            nonblock: true,
            ..OpenHow::default()
        },
        Box::new(SocketFile { ops: sock }),
        cred,
    )
}

/// The address family a socket is made in, as `socket()`'s `domain`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddressFamily {
    /// `AF_INET`.
    Inet,
    /// `AF_INET6`, which the guest has none of.
    Inet6,
}

impl AddressFamily {
    /// The family `ip` is an address of, as an address's `sa_family` names it.
    pub(crate) fn of(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::Inet,
            IpAddr::V6(_) => Self::Inet6,
        }
    }
}

/// Refuse a family the guest has no socket of, as `__sock_create` answers
/// `EAFNOSUPPORT` for a family no `net_proto_family` registered; an address of
/// such a family reaches no socket, so a connect to one answers the same.
pub(crate) fn check_family(family: AddressFamily) -> Result<(), Errno> {
    match family {
        AddressFamily::Inet => Ok(()),
        AddressFamily::Inet6 => Err(Errno::AddressFamilyNotSupported),
    }
}

impl TcpSock {
    /// A TCP socket of `family`, connected to nothing, in `net` for `cred`,
    /// as `__sock_create` makes one; it connects through `net`'s Protocol
    /// Family, and its buffers count against `cred`'s memory, as a socket's
    /// do against its creator's memory cgroup.
    pub fn create(
        net: &Namespace,
        cred: &Arc<Cred>,
        family: AddressFamily,
    ) -> Result<Arc<Self>, Errno> {
        check_family(family)?;
        Self::create_kern(net.family(), net.tcp_sockets(), cred)
    }

    /// A TCP socket the runtime makes for itself, as `sock_create_kern` does:
    /// it connects through `family`, `sockets` holds it until the Invocation
    /// ends, and its buffers count against `cred`'s memory.
    pub(crate) fn create_kern(
        family: &Arc<dyn ProtocolFamily>,
        sockets: &TcpSockets,
        cred: &Arc<Cred>,
    ) -> Result<Arc<Self>, Errno> {
        let sock = Self::new(family.clone(), cred)?;
        sockets.hold(&sock);
        Ok(sock)
    }
}

impl UdpSock {
    /// A UDP socket of `family`, connected to nothing, in `net` for `cred`,
    /// as `__sock_create` makes one; its receive queue counts against
    /// `cred`'s memory.
    pub fn create(
        _net: &Namespace,
        cred: &Arc<Cred>,
        family: AddressFamily,
    ) -> Result<Arc<Self>, Errno> {
        check_family(family)?;
        Self::new(cred)
    }
}

/// Bind `sock` to `addr` in `net` as `cred`, as `__sys_bind` asks
/// `security_socket_bind` before the protocol's `->bind`; the host socket
/// takes its own address as it connects.
pub fn sys_bind(
    net: &Namespace,
    cred: &Cred,
    sock: &dyn ProtoOps,
    addr: SocketAddr,
) -> Result<(), Errno> {
    cred.security_socket_bind(addr)?;
    sock.bind(net, addr)
}

/// Connect `sock` in `net` to `addr`, the address the guest named, as `cred`,
/// as `__sys_connect` does: the socket hooks judge what the guest named
/// first, then the socket's `->connect` takes it. `None` stands for
/// `AF_UNSPEC`, which Linux takes as a disconnect; a socket naming no peer
/// would send each datagram anywhere, so it is refused.
pub fn sys_connect(
    net: &Namespace,
    cred: &Cred,
    sock: &dyn ProtoOps,
    addr: Option<SocketAddr>,
) -> Result<(), Errno> {
    let addr = addr.ok_or(Errno::PermissionDenied)?;
    security_socket_connect(net, cred, addr, sock.protocol())?;
    sock.connect(net, addr)
}

/// Judge a `protocol` connection to `addr` through the socket hooks `cred`
/// holds, the name a virtual address in `net` stands for in place of the
/// address, as `security_socket_connect` runs before the protocol's
/// `connect`.
fn security_socket_connect(
    net: &Namespace,
    cred: &Cred,
    addr: SocketAddr,
    protocol: Protocol,
) -> Result<(), Errno> {
    let host = match net.translation_of(addr) {
        Some(Some((name, _))) => name,
        _ => addr.ip().to_string(),
    };
    cred.security_socket_connect(&Connection::new(&host, addr.port(), protocol))
}

/// Where a socket's connect leads once cgroup `connect4` ran: the literal
/// address the guest named, or the name a virtual address stands for, the
/// real address it stands for, and the port.
pub(crate) enum Destination {
    Literal(SocketAddr),
    Named {
        name: String,
        real: IpAddr,
        port: u16,
    },
}

/// What a socket's connect to `addr` reaches in `net` once its state check
/// passed, as cgroup `connect4` runs inside the protocol's `connect`: an IPv6
/// address answers `EAFNOSUPPORT`, as `tcp_v4_connect` and
/// `__ip4_datagram_connect` refuse one; a
/// virtual address turns into the name it stands for and the one real
/// address it stands for, one no name holds answering `EACCES`; any other
/// stands as itself.
pub(crate) fn connect4(net: &Namespace, addr: SocketAddr) -> Result<Destination, Errno> {
    check_family(AddressFamily::of(addr.ip()))?;
    match net.translation_of(addr) {
        None => Ok(Destination::Literal(addr)),
        Some(None) => Err(Errno::PermissionDenied),
        Some(Some((name, real))) => Ok(Destination::Named {
            name,
            real,
            port: addr.port(),
        }),
    }
}

/// The real address a `protocol` connection to `dest` dials, once `net`'s
/// ruleset lets it, as the filter judges what leaves after the socket took
/// its port; `EACCES` when it does not.
pub(crate) fn filter(
    net: &Namespace,
    dest: &Destination,
    protocol: Protocol,
) -> Result<SocketAddr, Errno> {
    filter_with(net.ruleset(), &**net.family(), dest, protocol)
}

/// The real address a `protocol` connection to `dest` dials once `ruleset`
/// lets it, judged against the host's own addresses as `family` reads them;
/// `EACCES` when it does not.
pub(crate) fn filter_with(
    ruleset: &Ruleset,
    family: &dyn ProtocolFamily,
    dest: &Destination,
    protocol: Protocol,
) -> Result<SocketAddr, Errno> {
    match dest {
        Destination::Literal(addr) => {
            ruleset.admit_literal(*addr, protocol)?;
            Ok(*addr)
        }
        Destination::Named { name, real, port } => {
            admitted_addresses_of(family, ruleset, name, &[*real], *port, protocol)
                .pop()
                .ok_or(Errno::PermissionDenied)
        }
    }
}

/// What the socket tests share.
#[cfg(test)]
pub(crate) mod testing {
    use crate::sched::testing::within_max_wait;
    use crate::vfs::PollMask;
    use crate::wait::{do_poll, PollTable};

    /// Wait on `poll`, a socket's or its file's, until it finds `mask`; past
    /// the longest a test waits, the test fails.
    pub(crate) fn wait_for(poll: impl Fn(Option<&mut PollTable>) -> PollMask, mask: PollMask) {
        let found =
            within_max_wait(|| do_poll(None, |table| poll(table).contains(mask).then_some(())));
        assert_eq!(found, Ok(Some(())), "the socket's poll must find {mask:?}");
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // @behavior NW-074
    #[test]
    fn a_host_socket_failure_answers_the_posix_name_its_errno_holds() {
        use rustix::io::Errno as HostErrno;
        let answers = [
            HostErrno::NETDOWN,
            HostErrno::NOBUFS,
            HostErrno::HOSTDOWN,
            HostErrno::AGAIN,
            HostErrno::INVAL,
        ]
        .map(|errno| to_error(&io::Error::from_raw_os_error(errno.raw_os_error())));
        assert_eq!(
            answers,
            [
                Errno::NetworkDown,
                Errno::NoBufferSpace,
                Errno::HostUnreachable,
                Errno::WouldBlock,
                Errno::InvalidInput
            ],
            "a host socket failing with ENETDOWN, ENOBUFS, EHOSTDOWN, EAGAIN, or EINVAL must \
             answer ENETDOWN, ENOBUFS, EHOSTUNREACH, EAGAIN, and EINVAL, read from its errno \
             where std names no kind, as a file's failure is otherwise"
        );
    }

    // @behavior NW-066
    #[test]
    fn a_socket_of_an_ipv6_family_answers_eafnosupport() {
        let network =
            crate::network::Network::new("api", Arc::new(crate::network::Builder::default()))
                .unwrap();
        let net = Namespace::for_network(
            &network,
            Arc::new(super::super::family::testing::CountsDials::default()),
            Arc::new(super::super::HyperOutbound::new()),
        );
        let cred = crate::security::prepare_creds_within(&[], None);
        let answers = (
            TcpSock::create(&net, &cred, AddressFamily::Inet6).err(),
            UdpSock::create(&net, &cred, AddressFamily::Inet6).err(),
            TcpSock::create(&net, &cred, AddressFamily::Inet).is_ok(),
        );
        assert_eq!(
            answers,
            (
                Some(Errno::AddressFamilyNotSupported),
                Some(Errno::AddressFamilyNotSupported),
                true
            ),
            "a TCP or UDP socket asked for in the IPv6 family must answer EAFNOSUPPORT, as the \
             guest carries IPv4 alone, while one in the IPv4 family is made"
        );
    }
}
