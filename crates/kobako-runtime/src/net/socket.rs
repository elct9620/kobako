//! A socket is a file, as Linux's `net/socket.c` makes one: every socket
//! stands on sockfs, and its file hands each call to the protocol's operations,
//! as `socket_file_ops` hands them to `proto_ops`.

use std::io;
use std::net::{Shutdown, SocketAddr};
use std::sync::{Arc, LazyLock};

use crate::vfs::{
    alloc_file_pseudo, Cred, Dentry, FMode, File, FileOperations, FileSystemType, FileType,
    FilesystemError, Inode, InodeOperations, Kiocb, Kstat, OpenHow, PollMask, SuperBlock, VfsMount,
};
use crate::wait::PollTable;

use super::MAX_SOCKET_BUFFER;

/// What one protocol answers for its socket, as Linux's `struct proto_ops`:
/// the file's reads and writes, what a poll finds, its shutdown, names, and
/// options, and the release once every view is gone. A Wasm Driver reaches
/// every socket through it, whichever protocol stands behind.
pub trait ProtoOps: Send + Sync {
    /// Read what the socket holds; `WouldBlock` while it holds nothing yet.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError>;
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
    /// The port the guest reads for its own end, once its namespace gave one
    /// as the connect began.
    fn virtual_port(&self) -> Option<u16>;
    /// The value `opt` holds on this socket, never the host's; one the
    /// protocol has not answers `ProtocolNotAvailable`.
    fn getsockopt(&self, opt: SockOpt) -> Result<u32, FilesystemError>;
    /// Set `opt` to `value`, as `setsockopt` does, refusing what Linux
    /// refuses; a host socket already dialed takes it at once.
    fn setsockopt(&self, opt: SockOpt, value: u32) -> Result<(), FilesystemError>;
    /// Let go of the host socket, as `sock_close` runs on the last `fput`.
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

/// What every inet socket holds beside its protocol's own state, as Linux's
/// `inet_sock` sits beneath `tcp_sock` and `udp_sock`.
pub(crate) struct InetSock {
    /// The port the guest reads for its own end, which its namespace gave as
    /// the connect began, as a NAT's tuple pairs it with the host's.
    pub(crate) virtual_port: Option<u16>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    pub(crate) named: Option<SocketAddr>,
    /// How much each queue holds, as `sk_rcvbuf` and `sk_sndbuf`.
    pub(crate) buffers: Buffers,
    /// The hop limit sent with, as `inet_sock` keeps `uc_ttl`.
    pub(crate) ttl: u8,
}

impl InetSock {
    pub(crate) fn new() -> Self {
        Self {
            virtual_port: None,
            named: None,
            buffers: Buffers::default(),
            ttl: IPDEFTTL,
        }
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

    /// Set a buffer `opt` to twice `value`, as `sock_setsockopt` does; the
    /// hop limit and TCP's options are the protocol's to set.
    pub(crate) fn set_buffer(&mut self, opt: SockOpt, value: u32) -> Result<(), FilesystemError> {
        let bytes = usize::try_from(value).unwrap_or(usize::MAX);
        match opt {
            SockOpt::RcvBuf => self.buffers.set_rcvbuf(bytes),
            SockOpt::SndBuf => self.buffers.set_sndbuf(bytes),
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
    fn to_doubled(bytes: usize, least: usize) -> usize {
        (bytes.min(MAX_SOCKET_BUFFER / 2) * 2).max(least)
    }

    pub(crate) fn set_rcvbuf(&mut self, bytes: usize) {
        self.rcvbuf = Self::to_doubled(bytes, SOCK_MIN_RCVBUF);
    }

    pub(crate) fn set_sndbuf(&mut self, bytes: usize) {
        self.sndbuf = Self::to_doubled(bytes, SOCK_MIN_SNDBUF);
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

/// `value` within 1 to `max`, else EINVAL, as Linux bounds a TCP option.
pub(crate) fn ensure_within(value: u32, max: u32) -> Result<(), FilesystemError> {
    match (1..=max).contains(&value) {
        true => Ok(()),
        false => Err(FilesystemError::InvalidInput),
    }
}

/// The memory a producer's buffers may hold, reserved against the Invocation's
/// as the producer is made and given back as it is let go, as
/// `sk_forward_alloc` reserves what the memory cgroup charges a socket.
pub(crate) struct SocketMemory {
    cred: Arc<Cred>,
    bytes: usize,
}

impl SocketMemory {
    /// Reserve `bytes` for `cred`, refused past its memory as `ENOMEM`.
    pub(crate) fn charge(cred: &Arc<Cred>, bytes: usize) -> Result<Self, FilesystemError> {
        cred.security_vm_enough_memory(bytes)?;
        Ok(Self {
            cred: cred.clone(),
            bytes,
        })
    }
}

impl Drop for SocketMemory {
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

/// The POSIX name a host socket's failure stands for; one with none carries
/// only its kind, so no host address reaches the guest.
pub(crate) fn to_error(err: &io::Error) -> FilesystemError {
    match err.kind() {
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
/// socket. The file is opened non-blocking, as a WASI 0.2 socket is, so a
/// read or write that would wait answers EAGAIN and the guest polls.
struct SocketOps(Arc<dyn ProtoOps>);

impl FileOperations for SocketOps {
    /// A read of nothing answers zero before the socket is asked, as
    /// `sock_read_iter` does, so it takes no datagram.
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.0.recvmsg(buf)
    }

    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.0.sendmsg(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        self.0.poll(table)
    }

    fn is_stream(&self) -> bool {
        true
    }
}

impl Drop for SocketOps {
    fn drop(&mut self) {
        self.0.release();
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
        Box::new(SocketOps(sock)),
        cred,
    )
}
