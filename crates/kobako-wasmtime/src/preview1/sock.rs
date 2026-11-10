//! The 4 `sock_*` calls. A socket's number answers through the protocol
//! behind its file, as Linux's `sock_from_file` reaches a file's
//! `proto_ops`; a file's descriptor answers `NOTSOCK`, as wasmtime-wasi's
//! preview1 does, and one standing only for its path `BADF`, as Linux's
//! socket calls take theirs through `fdget`.

use std::net::Shutdown;
use std::sync::Arc;

use kobako_runtime::net::ProtoOps;

use super::errno::{Answer, Errno};
use super::fd::file;
use super::memory::GuestMemory;
use super::types;
use crate::descriptors::{Descriptors, SocketType};

/// The protocol behind the socket `fd` stands for.
fn socket(table: &Descriptors, fd: u32) -> Answer<Arc<dyn ProtoOps>> {
    if let Some(ops) = table.proto_ops(fd) {
        return Ok(ops);
    }
    file(table, fd)?;
    Err(Errno::Notsock.into())
}

/// No socket of the guest's listens, so a stream answers `INVAL`, as
/// `inet_accept` does on a socket not listening, and a datagram one `NOTSUP`,
/// as `sock_no_accept` does.
pub(crate) fn accept<T>(table: &Descriptors, fd: u32) -> Answer<T> {
    socket(table, fd)?;
    match table.socket_of(fd).map(|(_, kind)| kind) {
        Some(SocketType::Stream) => Err(Errno::Inval.into()),
        _ => Err(Errno::Notsup.into()),
    }
}

/// What the socket holds, into the first buffer with room, as `fd_read`
/// reads one. Unlike Linux's `recvmsg`, it scatters nothing across the other
/// buffers and reports no cut datagram, since the socket answers through the
/// one read its file takes. A flag WASI names, peeking or waiting for all,
/// answers `NOTSUP`, as the guest's own receive answers a flag it cannot carry.
pub(crate) fn recv(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    ri_data: types::IovecArray,
    ri_flags: types::Riflags,
) -> Answer<(types::Size, types::Roflags)> {
    let ops = socket(table, fd)?;
    if !ri_flags.is_empty() {
        return Err(Errno::Notsup.into());
    }
    let buf = mem.first_non_empty_iovec(ri_data)?;
    let read = ops.recvmsg(mem.bytes_mut(buf)?)?;
    Ok((read as u32, types::Roflags::empty()))
}

/// The first buffer with bytes, sent as `sendmsg` with no flags.
pub(crate) fn send(
    table: &Descriptors,
    mem: &mut GuestMemory<'_, '_>,
    fd: u32,
    si_data: types::CiovecArray,
) -> Answer<types::Size> {
    let ops = socket(table, fd)?;
    let buf = mem.first_non_empty_ciovec(si_data)?;
    Ok(ops.sendmsg(mem.bytes(buf)?)? as u32)
}

/// Shut the sides `how` names down, as `shutdown(2)`; naming neither
/// answers `INVAL`, as `inet_shutdown` refuses an out-of-range `how`.
pub(crate) fn shutdown(table: &Descriptors, fd: u32, how: types::Sdflags) -> Answer {
    let ops = socket(table, fd)?;
    let how = match (
        how.contains(types::Sdflags::RD),
        how.contains(types::Sdflags::WR),
    ) {
        (true, true) => Shutdown::Both,
        (true, false) => Shutdown::Read,
        (false, true) => Shutdown::Write,
        (false, false) => return Err(Errno::Inval.into()),
    };
    Ok(ops.shutdown(how)?)
}

#[cfg(test)]
mod tests {
    //! Witness that a preview1 call on a socket's number reaches the socket
    //! file its preview2 handle opened.
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    use kobako_runtime::network;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;
    use wiggle::GuestPtr;

    use super::super::types::{Advice, Errno, Fdflags, Filetype, Riflags, Sdflags, Whence};
    use super::super::wasi_snapshot_preview1::WasiSnapshotPreview1;
    use crate::config::Config;
    use crate::invocation::Invocation;
    use crate::preview2::bindings::wasi_sockets_network_0_2_12::{
        IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
    };
    use crate::preview2::bindings::{IoHost, SocketsHost};
    use crate::preview2::IntoAnswer;

    /// A hermetic guest whose one network entry reaches `127.0.0.1:port` over
    /// TCP.
    fn loopback_guest(port: u16) -> Invocation {
        let endpoint = network::Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_protocol(network::Protocol::Tcp);
        let mut guest =
            Invocation::with_network(&Config::for_profile(Profile::Hermetic), [endpoint]);
        // wiggle hands each call the guest's fuel; every call here gets plenty.
        guest.set_hostcall_fuel(usize::MAX);
        guest
    }

    /// A hermetic guest whose socket connected to `listener`, the number its
    /// preview2 handle took, and the peer's end.
    fn connected_guest(listener: &TcpListener) -> (Invocation, u32, TcpStream) {
        let port = listener.local_addr().unwrap().port();
        let mut guest = loopback_guest(port);
        let socket = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port,
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)
                    .into_answer()?
                    .expect("an endpoint names the address");
                let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
                guest.poll_pollable_block(connecting)?;
                guest
                    .tcp_tcp_socket_finish_connect(socket)
                    .into_answer()?
                    .expect("the listener accepts");
                wasmtime::Result::Ok(socket)
            },
        )
        .expect("the connect answers");
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (guest, socket.0, peer)
    }

    /// What preview1 answers on `fd`: the flags of a write poll's event, then
    /// the errors a one-byte read and a one-byte write answer.
    fn probe_stream_calls(
        guest: &mut Invocation,
        fd: u32,
    ) -> (Option<u16>, Option<Errno>, Option<Errno>) {
        // One fd_write subscription at 0, its event written at 64.
        let mut memory = vec![0u8; 128];
        memory[8] = 2;
        memory[16..20].copy_from_slice(&fd.to_le_bytes());
        let polled = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            guest
                .poll_oneoff(
                    &mut wiggle::GuestMemory::Unshared(&mut memory),
                    GuestPtr::new(0),
                    GuestPtr::new(64),
                    1,
                )
                .ok()
        });
        let flags = (polled == Some(1)).then(|| u16::from_le_bytes([memory[88], memory[89]]));
        let mut memory = vec![0u8; 64];
        write_iovec(&mut memory, 1);
        let read = guest
            .fd_read(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
            )
            .err()
            .and_then(|err| err.downcast().ok());
        let write = guest
            .fd_write(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
            )
            .err()
            .and_then(|err| err.downcast().ok());
        (flags, read, write)
    }

    /// What preview1 answers on `fd` for the calls a file's position, sync,
    /// size, advice, and listing take, then the filetype it reports.
    fn probe_file_calls(guest: &mut Invocation, fd: u32) -> (Vec<Option<Errno>>, Option<Filetype>) {
        let mut memory = vec![0u8; 64];
        write_iovec(&mut memory, 4);
        let errno = |answer: Result<_, super::super::types::Error>| {
            answer.err().and_then(|err| err.downcast().ok())
        };
        let mut mem = wiggle::GuestMemory::Unshared(&mut memory);
        let calls = vec![
            errno(guest.fd_seek(&mut mem, fd.into(), 0, Whence::Set).map(drop)),
            errno(guest.fd_tell(&mut mem, fd.into()).map(drop)),
            errno(
                guest
                    .fd_pread(&mut mem, fd.into(), GuestPtr::new((0, 1)), 0)
                    .map(drop),
            ),
            errno(
                guest
                    .fd_pwrite(&mut mem, fd.into(), GuestPtr::new((0, 1)), 0)
                    .map(drop),
            ),
            errno(guest.fd_sync(&mut mem, fd.into())),
            errno(guest.fd_datasync(&mut mem, fd.into())),
            errno(guest.fd_filestat_set_size(&mut mem, fd.into(), 0)),
            errno(guest.fd_advise(&mut mem, fd.into(), 0, 0, Advice::Normal)),
            errno(
                guest
                    .fd_readdir(&mut mem, fd.into(), GuestPtr::new(16), 8, 0)
                    .map(drop),
            ),
        ];
        let fdstat = guest
            .fd_fdstat_get(&mut mem, fd.into())
            .ok()
            .map(|stat| stat.fs_filetype);
        let filestat = guest
            .fd_filestat_get(&mut mem, fd.into())
            .ok()
            .map(|stat| stat.filetype);
        (calls, fdstat.filter(|kind| filestat == Some(*kind)))
    }

    // @behavior RT-363
    #[test]
    fn a_socket_number_answers_a_files_calls_as_linux_answers_a_socket() {
        let mut guest = loopback_guest(9);
        let tcp = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a TCP socket");
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a UDP socket");
        let linux = vec![
            Some(Errno::Spipe),
            Some(Errno::Spipe),
            Some(Errno::Spipe),
            Some(Errno::Spipe),
            Some(Errno::Inval),
            Some(Errno::Inval),
            Some(Errno::Inval),
            None,
            Some(Errno::Notdir),
        ];
        assert_eq!(
            [
                probe_file_calls(&mut guest, tcp.0),
                probe_file_calls(&mut guest, udp.0)
            ],
            [
                (linux.clone(), Some(Filetype::SocketStream)),
                (linux, Some(Filetype::SocketDgram))
            ],
            "fd_seek, fd_tell, fd_pread, and fd_pwrite on a socket's number must answer ESPIPE, \
             fd_sync, fd_datasync, and fd_filestat_set_size EINVAL, fd_advise succeed, and \
             fd_readdir ENOTDIR, as Linux answers each on a socket; fd_fdstat_get and \
             fd_filestat_get must both report a TCP socket as a stream and a UDP socket as \
             datagrams"
        );
    }

    // @behavior RT-352
    #[test]
    fn a_socket_number_answers_preview1_as_linux_before_it_connects() {
        let mut guest = loopback_guest(9);
        let tcp = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a TCP socket");
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a UDP socket");
        assert_eq!(
            [
                probe_stream_calls(&mut guest, tcp.0),
                probe_stream_calls(&mut guest, udp.0)
            ],
            [
                (Some(1), Some(Errno::Notconn), Some(Errno::Pipe)),
                (Some(0), Some(Errno::Again), Some(Errno::Destaddrreq)),
            ],
            "preview1 on a new TCP socket's number must poll writable and hung up, read ENOTCONN, \
             and write EPIPE; on a new UDP socket's number it must poll writable, read EAGAIN, \
             and write EDESTADDRREQ"
        );
    }

    /// One iovec at 0 naming `len` bytes at 16.
    fn write_iovec(memory: &mut [u8], len: u32) {
        memory[0..4].copy_from_slice(&16u32.to_le_bytes());
        memory[4..8].copy_from_slice(&len.to_le_bytes());
    }

    // @behavior RT-349
    #[test]
    fn a_preview1_read_and_write_of_a_socket_number_reach_its_socket_file() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (mut guest, fd, mut peer) = connected_guest(&listener);
        let mut memory = vec![0u8; 64];
        write_iovec(&mut memory, 5);
        memory[16..21].copy_from_slice(b"hello");
        let written = guest
            .fd_write(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
            )
            .ok();
        let mut heard = [0u8; 5];
        let _ = peer.read_exact(&mut heard);
        peer.write_all(b"world").unwrap();
        let started = Instant::now();
        let read = loop {
            let mut memory = vec![0u8; 64];
            write_iovec(&mut memory, 5);
            let answer = guest.fd_read(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
            );
            if let Ok(n) = answer {
                break memory[16..16 + n as usize].to_vec();
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "fd_read through preview1 on a connected socket's number must read the peer's reply"
            );
            thread::sleep(Duration::from_millis(1));
        };
        let stat = guest
            .fd_fdstat_get(&mut wiggle::GuestMemory::Unshared(&mut []), fd.into())
            .ok()
            .map(|stat| (stat.fs_filetype, stat.fs_flags.contains(Fdflags::NONBLOCK)));
        assert_eq!(
            (written, &heard, read, stat),
            (
                Some(5),
                b"hello",
                b"world".to_vec(),
                Some((Filetype::SocketStream, true))
            ),
            "fd_write and fd_read through preview1 on a connected preview2 socket's number must \
             reach its peer, and fd_fdstat_get must report a non-blocking socket stream"
        );
    }

    // @behavior RT-372
    #[test]
    fn a_preview1_socket_call_reaches_the_socket_behind_its_number() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (mut guest, fd, mut peer) = connected_guest(&listener);
        let errno = |answer: Result<_, super::super::types::Error>| {
            answer.err().and_then(|err| err.downcast().ok())
        };
        let mut memory = vec![0u8; 64];
        write_iovec(&mut memory, 4);
        memory[16..20].copy_from_slice(b"ping");
        let sent = guest
            .sock_send(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
                0,
            )
            .ok();
        let mut heard = [0u8; 4];
        let _ = peer.read_exact(&mut heard);
        peer.write_all(b"pong").unwrap();
        let peeked = errno(
            guest
                .sock_recv(
                    &mut wiggle::GuestMemory::Unshared(&mut memory),
                    fd.into(),
                    GuestPtr::new((0, 1)),
                    Riflags::RECV_PEEK,
                )
                .map(drop),
        );
        let started = Instant::now();
        let received = loop {
            let mut memory = vec![0u8; 64];
            write_iovec(&mut memory, 4);
            let answer = guest.sock_recv(
                &mut wiggle::GuestMemory::Unshared(&mut memory),
                fd.into(),
                GuestPtr::new((0, 1)),
                Riflags::empty(),
            );
            if let Ok((n, _)) = answer {
                break memory[16..16 + n as usize].to_vec();
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "sock_recv on a connected socket's number must read the peer's reply"
            );
            thread::sleep(Duration::from_millis(1));
        };
        let shut = guest
            .sock_shutdown(
                &mut wiggle::GuestMemory::Unshared(&mut []),
                fd.into(),
                Sdflags::WR,
            )
            .is_ok();
        let ended = peer.read_to_end(&mut Vec::new()).ok();
        let accepted = errno(
            guest
                .sock_accept(
                    &mut wiggle::GuestMemory::Unshared(&mut []),
                    fd.into(),
                    Fdflags::empty(),
                )
                .map(drop),
        );
        let neither = errno(guest.sock_shutdown(
            &mut wiggle::GuestMemory::Unshared(&mut []),
            fd.into(),
            Sdflags::empty(),
        ));
        assert_eq!(
            (sent, &heard, peeked, received, shut, ended, accepted, neither),
            (
                Some(4),
                b"ping",
                Some(Errno::Notsup),
                b"pong".to_vec(),
                true,
                Some(0),
                Some(Errno::Inval),
                Some(Errno::Inval)
            ),
            "sock_send and sock_recv on a connected preview2 socket's number must reach its peer, \
             a peek must answer ENOTSUP, sock_accept on a stream not listening must answer \
             EINVAL, sock_shutdown of the writing side must end the peer's read, and a shutdown \
             naming neither side must answer EINVAL"
        );
    }

    // @behavior RT-355
    #[test]
    fn a_socket_whose_connect_failed_polls_ready_to_read() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let mut guest = loopback_guest(port);
        let fd = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port,
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)
                    .into_answer()?
                    .expect("an endpoint names the address");
                let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
                guest.poll_pollable_block(connecting)?;
                Ok(socket.0)
            },
        )
        .expect("the connect settles");
        // One fd_read subscription at 0, its event written at 64.
        let mut memory = vec![0u8; 128];
        memory[8] = 1;
        memory[16..20].copy_from_slice(&fd.to_le_bytes());
        let answer = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            guest
                .poll_oneoff(
                    &mut wiggle::GuestMemory::Unshared(&mut memory),
                    GuestPtr::new(0),
                    GuestPtr::new(64),
                    1,
                )
                .ok()
        });
        assert_eq!(
            (answer, memory[74]),
            (Some(1), 1),
            "poll_oneoff through preview1 of fd_read on a socket whose connect was refused must \
             answer its event, as poll reports an error whatever was asked"
        );
    }

    // @behavior RT-350
    #[test]
    fn a_preview1_poll_of_a_socket_number_waits_for_its_peer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (mut guest, fd, mut peer) = connected_guest(&listener);
        // One fd_read subscription at 0, its event written at 64.
        let mut memory = vec![0u8; 128];
        memory[8] = 1;
        memory[16..20].copy_from_slice(&fd.to_le_bytes());
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            peer.write_all(b"x").unwrap();
            peer
        });
        let started = Instant::now();
        let answer = run_task(Some(started + Duration::from_secs(10)), || {
            guest
                .poll_oneoff(
                    &mut wiggle::GuestMemory::Unshared(&mut memory),
                    GuestPtr::new(0),
                    GuestPtr::new(64),
                    1,
                )
                .ok()
        });
        let spent = started.elapsed();
        let _peer = writer.join().unwrap();
        assert!(
            answer == Some(1) && memory[74] == 1 && spent >= Duration::from_millis(50),
            "poll_oneoff through preview1 of fd_read on a silent socket's number must answer its \
             event once the peer writes, got {answer:?} after {spent:?}"
        );
    }
}
