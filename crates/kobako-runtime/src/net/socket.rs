//! A socket is a file, as Linux's `net/socket.c` makes one: every socket
//! stands on sockfs, and its file hands each call to the protocol's operations,
//! as `socket_file_ops` hands them to `proto_ops`.

use std::io;
use std::net::{Ipv4Addr, Shutdown, SocketAddr};
use std::sync::{Arc, LazyLock};

use socket2::SockRef;

use crate::vfs::{
    alloc_file_pseudo, Cred, Dentry, FMode, File, FileOperations, FileSystemType, FileType,
    FilesystemError, Inode, InodeOperations, Kiocb, Kstat, OpenHow, PollMask, SuperBlock, VfsMount,
};
use crate::wait::PollTable;

use super::connections::{open, OpenConnections, Place};

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
    /// as `->bind` runs after `__sys_bind` asks `security_socket_bind`: an
    /// IPv6 address answers `AddressFamilyNotSupported`, and a socket bound
    /// or connected already `InvalidInput`, as `__inet_bind` refuses one.
    fn bind(&self, addr: SocketAddr) -> Result<(), FilesystemError>;
    /// Read what the socket holds and who sent it, as `->recvmsg` fills
    /// `msg_name`: a datagram's peer as the guest named it, none for a
    /// stream; `WouldBlock` while it holds nothing yet.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddr>), FilesystemError>;
    /// Write what the socket takes now; `WouldBlock` while it takes nothing.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError>;
    /// What a call would find ready, the socket's wait queue registered on
    /// `table`, as a protocol's `->poll` calls `sock_poll_wait`.
    fn poll(&self, table: Option<&mut PollTable>) -> PollMask;
    /// Shut the reading side, the writing side, or both down, as
    /// `inet_shutdown` marks `sk_shutdown`; a socket connected to nothing
    /// answers `ENOTCONN`.
    fn shutdown(&self, how: Shutdown) -> Result<(), FilesystemError>;
    /// The peer as the guest named it, once connected, as `->getname` maps
    /// the real peer back through cgroup `getpeername4`.
    fn getpeername(&self) -> Option<SocketAddr>;
    /// The guest's own end, as `->getname` answers `getsockname` through
    /// cgroup `getsockname4`: the guest's own virtual address and the port its
    /// namespace gave as the connect began, or the unspecified address
    /// before then, as Linux answers an unbound socket.
    fn getsockname(&self) -> SocketAddr;
    /// The value `opt` holds on this socket, never the host's; one the
    /// protocol has not answers `ProtocolNotAvailable`.
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError>;
    /// Set `opt` to `value`, as `setsockopt` does, refusing what Linux
    /// refuses; a host socket already dialed takes it at once.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), FilesystemError>;
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
}

/// The guest's own end of every socket it connects, kept out of what a lookup
/// hands out, as a container in its own network namespace reads its veth's
/// address while the host translates. It sits below the virtual range's last
/// address, the one a broadcast would take.
pub(crate) const LOCAL_ADDRESS: Ipv4Addr = Ipv4Addr::new(198, 19, 255, 254);

/// What every inet socket holds beside its protocol's own state, as Linux's
/// `inet_sock` sits beneath `tcp_sock` and `udp_sock`.
pub(crate) struct InetSock {
    /// The port the guest reads for its own end, which its namespace gave as
    /// the connect began, as a NAT's tuple pairs it with the host's.
    pub(crate) virtual_port: Option<u16>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    pub(crate) peer: Option<SocketAddr>,
    /// How much each queue holds, as `sk_rcvbuf` and `sk_sndbuf`.
    pub(crate) buffers: Buffers,
    /// The hop limit sent with, as `inet_sock` keeps `uc_ttl`.
    pub(crate) ttl: u8,
    /// The socket's pending failure, as `sk_err`: answered once, then
    /// cleared, as `sock_error` swaps it with zero.
    pub(crate) error: Option<FilesystemError>,
    /// The guest shut its reading side down, as `sk_shutdown`'s
    /// `RCV_SHUTDOWN`.
    pub(crate) rcv_shut: bool,
    /// The guest shut its writing side down, as `SEND_SHUTDOWN`.
    pub(crate) send_shut: bool,
    /// The socket's place against its Sandbox's cap and the process's, given
    /// back as it is released.
    pub(crate) place: Option<Place>,
    /// Whether a bind took the socket's own end, as `inet_num` records one.
    bound: bool,
    /// Whether the bind named its port, which the socket then keeps once it
    /// closes, as `SOCK_BINDPORT_LOCK` does.
    port_locked: bool,
    /// What its queues may hold, reserved against the Invocation's memory
    /// until the socket is released.
    memory: Option<MemoryCharge>,
}

impl InetSock {
    /// A socket whose queues hold what `memory` reserved.
    pub(crate) fn new(memory: MemoryCharge) -> Self {
        Self {
            virtual_port: None,
            peer: None,
            buffers: Buffers::default(),
            ttl: IPDEFTTL,
            error: None,
            rcv_shut: false,
            send_shut: false,
            place: None,
            bound: false,
            port_locked: false,
            memory: Some(memory),
        }
    }

    /// Take `addr` as the socket's own end, as `inet_bind` does: an IPv6
    /// address answers `EAFNOSUPPORT`, and a socket already bound, or
    /// `connected` and so bound by its connect, `EINVAL`, as `__inet_bind`
    /// refuses one past `TCP_CLOSE` or holding a port.
    pub(crate) fn bind(
        &mut self,
        addr: SocketAddr,
        connected: bool,
    ) -> Result<(), FilesystemError> {
        if addr.is_ipv6() {
            return Err(FilesystemError::AddressFamilyNotSupported);
        }
        if self.bound || connected {
            return Err(FilesystemError::InvalidInput);
        }
        self.bound = true;
        self.port_locked = addr.port() != 0;
        Ok(())
    }

    /// Give back the end a bind took once the socket closes, unless the bind
    /// named its port, as `tcp_set_state` puts the port on entering
    /// `TCP_CLOSE` without `SOCK_BINDPORT_LOCK`.
    pub(crate) fn put_port(&mut self) {
        self.bound &= self.port_locked;
    }

    /// The guest's own end, at its own virtual address once a port is held.
    pub(crate) fn getsockname(&self) -> SocketAddr {
        match self.virtual_port {
            Some(port) => SocketAddr::from((LOCAL_ADDRESS, port)),
            None => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        }
    }

    /// Hold the port `alloc` gives as a connect begins, unless the socket
    /// still holds one, as a hash connect picks a port only when `inet_num`
    /// is zero; a port put back once the socket closed is then replaced,
    /// while reading the socket's own end still answers the old one until
    /// then, as `inet_sport` keeps it.
    pub(crate) fn hold_virtual_port(&mut self, alloc: &mut dyn FnMut() -> u16) {
        if !self.bound || self.virtual_port.is_none() {
            self.virtual_port = Some(alloc());
        }
        self.bound = true;
    }

    /// A new place against `open_connections` and the process, or `None`
    /// when the socket holds one already, so a socket connecting again
    /// counts once; either cap full answers `EAGAIN`.
    pub(crate) fn open_place(
        &self,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Option<Place>, FilesystemError> {
        match self.place {
            Some(_) => Ok(None),
            None => open(open_connections)
                .map(Some)
                .ok_or(FilesystemError::WouldBlock),
        }
    }

    /// Mark the sides `how` names shut, as `inet_shutdown` sets
    /// `sk_shutdown`.
    pub(crate) fn mark_shutdown(&mut self, how: Shutdown) {
        self.rcv_shut |= matches!(how, Shutdown::Read | Shutdown::Both);
        self.send_shut |= matches!(how, Shutdown::Write | Shutdown::Both);
    }

    /// Shut the sides `how` names, as `inet_shutdown` does for TCP and UDP
    /// alike: a socket not `connected` marks them still and answers
    /// `ENOTCONN`.
    pub(crate) fn shutdown(
        &mut self,
        how: Shutdown,
        connected: bool,
    ) -> Result<(), FilesystemError> {
        self.mark_shutdown(how);
        match connected {
            true => Ok(()),
            false => Err(FilesystemError::NotConnected),
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
    pub(crate) fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError> {
        let value = match opt {
            SockOpt::RcvBuf => self.buffers.rcvbuf,
            SockOpt::SndBuf => self.buffers.sndbuf,
            SockOpt::Ttl => usize::from(self.ttl),
            _ => return Err(FilesystemError::ProtocolNotAvailable),
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
    ) -> Result<(), FilesystemError> {
        let bytes = usize::try_from(value).unwrap_or(usize::MAX);
        match opt {
            SockOpt::RcvBuf => self.buffers.set_rcvbuf(bytes),
            SockOpt::SndBuf => self.buffers.set_sndbuf(bytes),
            SockOpt::Ttl => {
                let ttl = to_ttl(value)?;
                if let Some(host) = host {
                    set_hop_limit(host, ttl).map_err(|err| to_error(&err))?;
                }
                self.ttl = ttl;
            }
            _ => return Err(FilesystemError::ProtocolNotAvailable),
        }
        Ok(())
    }
}

/// The least `SO_RCVBUF` and `SO_SNDBUF` leave a queue, Linux's
/// `SOCK_MIN_RCVBUF` and `SOCK_MIN_SNDBUF`.
const SOCK_MIN_RCVBUF: usize = 2304;
const SOCK_MIN_SNDBUF: usize = 4608;

/// How much a socket's queues hold, as `sk_rcvbuf` and `sk_sndbuf`. Each
/// starts at, and never passes, the buffer its memory reserved, which plays
/// `rmem_max` and `wmem_max` as twice the most a set takes.
pub(crate) struct Buffers {
    pub(crate) rcvbuf: usize,
    pub(crate) sndbuf: usize,
}

impl Default for Buffers {
    fn default() -> Self {
        Self {
            rcvbuf: MAX_SOCKET_BUFFER,
            sndbuf: MAX_SOCKET_BUFFER,
        }
    }
}

impl Buffers {
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
    }
}

/// The hop limit a socket sends with until the guest sets one, Linux's
/// `IPDEFTTL`; the host's own default never reaches the guest.
pub(crate) const IPDEFTTL: u8 = 64;

/// `ttl` as a hop limit Linux takes, 1 to 255, refusing the rest with EINVAL
/// as `do_ip_setsockopt` does.
pub(crate) fn to_ttl(ttl: u32) -> Result<u8, FilesystemError> {
    match u8::try_from(ttl) {
        Ok(0) | Err(_) => Err(FilesystemError::InvalidInput),
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
pub(crate) fn ensure_within(value: u32, max: u32) -> Result<(), FilesystemError> {
    match (1..=max).contains(&value) {
        true => Ok(()),
        false => Err(FilesystemError::InvalidInput),
    }
}

/// Memory the host holds for a guest, reserved against the Invocation's as it
/// is taken and given back as it is let go, as `sk_forward_alloc` reserves
/// what the memory cgroup charges a socket: a producer's buffers, or a part of
/// a request the guest builds.
pub struct MemoryCharge {
    cred: Arc<Cred>,
    bytes: usize,
}

impl MemoryCharge {
    /// Reserve `bytes` for `cred`, refused past its memory as `ENOMEM`.
    pub fn new(cred: &Arc<Cred>, bytes: usize) -> Result<Self, FilesystemError> {
        cred.security_vm_enough_memory(bytes)?;
        Ok(Self {
            cred: cred.clone(),
            bytes,
        })
    }

    /// A charge for `cred` that reserves nothing yet.
    pub fn empty(cred: &Arc<Cred>) -> Self {
        Self {
            cred: cred.clone(),
            bytes: 0,
        }
    }

    /// The bytes it reserves.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Reserve `bytes` in place of what it held, taking only the growth and
    /// giving back the rest; refused past its memory as `ENOMEM`, keeping what
    /// it held.
    pub fn resize(&mut self, bytes: usize) -> Result<(), FilesystemError> {
        match bytes.checked_sub(self.bytes) {
            Some(growth) => self.cred.security_vm_enough_memory(growth)?,
            None => self.cred.vm_unacct_memory(self.bytes - bytes),
        }
        self.bytes = bytes;
        Ok(())
    }
}

impl Drop for MemoryCharge {
    fn drop(&mut self) {
        self.cred.vm_unacct_memory(self.bytes);
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
pub(crate) fn to_error(err: &io::Error) -> FilesystemError {
    #[cfg(unix)]
    {
        use rustix::io::Errno;
        match Errno::from_io_error(err) {
            Some(Errno::NOBUFS) => return FilesystemError::NoBufferSpace,
            Some(Errno::HOSTDOWN) => return FilesystemError::HostUnreachable,
            #[cfg(target_os = "linux")]
            Some(Errno::NONET) => return FilesystemError::NetworkUnreachable,
            _ => {}
        }
    }
    match err.kind() {
        io::ErrorKind::NetworkDown => FilesystemError::NetworkDown,
        io::ErrorKind::ConnectionRefused => FilesystemError::ConnectionRefused,
        io::ErrorKind::ConnectionReset => FilesystemError::ConnectionReset,
        io::ErrorKind::ConnectionAborted => FilesystemError::ConnectionAborted,
        io::ErrorKind::TimedOut => FilesystemError::TimedOut,
        io::ErrorKind::HostUnreachable => FilesystemError::HostUnreachable,
        io::ErrorKind::NetworkUnreachable => FilesystemError::NetworkUnreachable,
        io::ErrorKind::AddrNotAvailable => FilesystemError::AddressNotAvailable,
        io::ErrorKind::AddrInUse => FilesystemError::AddressInUse,
        io::ErrorKind::BrokenPipe => FilesystemError::BrokenPipe,
        io::ErrorKind::NotConnected => FilesystemError::NotConnected,
        io::ErrorKind::PermissionDenied => FilesystemError::PermissionDenied,
        kind => FilesystemError::Other(format!("{kind:?}")),
    }
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
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), FilesystemError> {
        sb.d_make_root(Inode::new(&Kstat::new(FileType::Dir, 0), self));
        Ok(())
    }
}

impl InodeOperations for SockFs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
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
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.ops.recvmsg(buf).map(|(n, _)| n)
    }

    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
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
        use rustix::io::Errno;
        let answers = [Errno::NETDOWN, Errno::NOBUFS, Errno::HOSTDOWN]
            .map(|errno| to_error(&io::Error::from_raw_os_error(errno.raw_os_error())));
        assert_eq!(
            answers,
            [
                FilesystemError::NetworkDown,
                FilesystemError::NoBufferSpace,
                FilesystemError::HostUnreachable
            ],
            "a host socket failing with ENETDOWN, ENOBUFS, or EHOSTDOWN must answer ENETDOWN, \
             ENOBUFS, and EHOSTUNREACH, read from its errno where std names no kind"
        );
    }
}
