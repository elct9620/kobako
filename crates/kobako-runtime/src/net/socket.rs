//! A socket is a file, as Linux's `net/socket.c` makes one: every socket
//! stands on sockfs, and its file hands each call to the protocol's operations,
//! as `socket_file_ops` hands them to `proto_ops`.

use std::io;
use std::sync::{Arc, LazyLock};

use crate::vfs::{
    alloc_file_pseudo, Cred, Dentry, FMode, File, FileOperations, FileSystemType, FileType,
    FilesystemError, Inode, InodeOperations, Kiocb, Kstat, OpenHow, PollMask, SuperBlock, VfsMount,
};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// What one protocol answers for its socket, as Linux's `struct proto_ops`:
/// the file's reads and writes, what a poll finds, and the release once every
/// view is gone.
pub trait ProtoOps: Send + Sync {
    /// Read what the socket holds; `WouldBlock` while it holds nothing yet.
    fn recvmsg(&self, buf: &mut [u8]) -> Result<usize, FilesystemError>;
    /// Write what the socket takes now; `WouldBlock` while it takes nothing.
    fn sendmsg(&self, buf: &[u8]) -> Result<usize, FilesystemError>;
    fn poll_mask(&self) -> PollMask;
    /// What a guest's poll waits on.
    fn wait_queue(&self) -> &Arc<WaitQueueHead>;
    /// Let go of the host socket, as `sock_close` runs on the last `fput`.
    fn release(&self);
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
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        self.0.recvmsg(buf)
    }

    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        self.0.sendmsg(buf)
    }

    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(self.0.wait_queue(), table);
        self.0.poll_mask()
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
