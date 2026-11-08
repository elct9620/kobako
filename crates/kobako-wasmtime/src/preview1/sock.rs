//! The 4 `sock_*` calls. A file's descriptor answers `NOTSOCK`, as
//! wasmtime-wasi's preview1 does, and one standing only for its path `BADF`,
//! as Linux's socket calls take theirs through `fdget`. A socket's number
//! answers `NOTSUP`: its reads and writes go through `fd_read` and
//! `fd_write`, and the rest of a socket's calls through preview2.

use super::errno::{Answer, Errno};
use super::fd::file;
use crate::descriptors::Descriptors;

pub(crate) fn refuse<T>(table: &Descriptors, fd: u32) -> Answer<T> {
    if table.socket(fd).is_some() {
        return Err(Errno::Notsup.into());
    }
    file(table, fd)?;
    Err(Errno::Notsock.into())
}

#[cfg(test)]
mod tests {
    //! Witness that a preview1 call on a socket's number reaches the socket
    //! file its preview2 handle opened.
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use kobako_runtime::grant::Grant;
    use kobako_runtime::network;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;
    use wiggle::GuestPtr;

    use super::super::types::{Advice, Errno, Fdflags, Filetype, Sdflags, Whence};
    use super::super::wasi_snapshot_preview1::WasiSnapshotPreview1;
    use crate::config::Config;
    use crate::invocation::Invocation;
    use crate::preview2::bindings::wasi_sockets_network_0_2_12::{
        IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
    };
    use crate::preview2::bindings::{IoHost, SocketsHost};
    use crate::wasi::{sandbox_connections, Wasi};

    /// A hermetic guest whose one network entry reaches `127.0.0.1:port` over
    /// TCP.
    fn networked(port: u16) -> Invocation {
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            open_connection_limit: None,
            connection_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let mut entry = network::Builder::default();
        entry.endpoint(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp),
        );
        let grant = Grant::Network(network::Network::new("local", Arc::new(entry)).unwrap());
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[grant], &sandbox_connections())
            .expect("WASI context must build");
        let mut guest = Invocation::new(None, wasi);
        // wiggle hands each call the guest's fuel; every call here gets plenty.
        guest.set_hostcall_fuel(usize::MAX);
        guest
    }

    /// A hermetic guest whose socket connected to `listener`, the number its
    /// preview2 handle took, and the peer's end.
    fn connected(listener: &TcpListener) -> (Invocation, u32, TcpStream) {
        let port = listener.local_addr().unwrap().port();
        let mut guest = networked(port);
        let socket = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port,
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)?
                    .expect("an endpoint names the address");
                let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
                guest.poll_pollable_block(connecting)?;
                guest
                    .tcp_tcp_socket_finish_connect(socket)?
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
    fn answers_on(guest: &mut Invocation, fd: u32) -> (Option<u16>, Option<Errno>, Option<Errno>) {
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
        iovec(&mut memory, 1);
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
    fn file_calls_on(guest: &mut Invocation, fd: u32) -> (Vec<Option<Errno>>, Option<Filetype>) {
        let mut memory = vec![0u8; 64];
        iovec(&mut memory, 4);
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
        let filetype = guest
            .fd_fdstat_get(&mut mem, fd.into())
            .ok()
            .map(|stat| stat.fs_filetype);
        (calls, filetype)
    }

    // @behavior RT-363
    #[test]
    fn a_socket_number_answers_a_files_calls_as_linux_answers_a_socket() {
        let mut guest = networked(9);
        let tcp = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .unwrap()
            .expect("a network makes a TCP socket");
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
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
                file_calls_on(&mut guest, tcp.0),
                file_calls_on(&mut guest, udp.0)
            ],
            [
                (linux.clone(), Some(Filetype::SocketStream)),
                (linux, Some(Filetype::SocketDgram))
            ],
            "fd_seek, fd_tell, fd_pread, and fd_pwrite on a socket's number must answer ESPIPE, \
             fd_sync, fd_datasync, and fd_filestat_set_size EINVAL, fd_advise succeed, and \
             fd_readdir ENOTDIR, as Linux answers each on a socket; fd_fdstat_get must report a \
             TCP socket as a stream and a UDP socket as datagrams"
        );
    }

    // @behavior RT-352
    #[test]
    fn a_socket_number_answers_preview1_as_linux_before_it_connects() {
        let mut guest = networked(9);
        let tcp = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .unwrap()
            .expect("a network makes a TCP socket");
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .unwrap()
            .expect("a network makes a UDP socket");
        assert_eq!(
            [answers_on(&mut guest, tcp.0), answers_on(&mut guest, udp.0)],
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
    fn iovec(memory: &mut [u8], len: u32) {
        memory[0..4].copy_from_slice(&16u32.to_le_bytes());
        memory[4..8].copy_from_slice(&len.to_le_bytes());
    }

    // @behavior RT-349
    #[test]
    fn a_preview1_read_and_write_of_a_socket_number_reach_its_socket_file() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (mut guest, fd, mut peer) = connected(&listener);
        let mut memory = vec![0u8; 64];
        iovec(&mut memory, 5);
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
            iovec(&mut memory, 5);
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
        let shutdown = guest
            .sock_shutdown(
                &mut wiggle::GuestMemory::Unshared(&mut []),
                fd.into(),
                Sdflags::WR,
            )
            .err()
            .and_then(|err| err.downcast().ok());
        assert_eq!(
            (written, &heard, read, stat, shutdown),
            (
                Some(5),
                b"hello",
                b"world".to_vec(),
                Some((Filetype::SocketStream, true)),
                Some(Errno::Notsup)
            ),
            "fd_write and fd_read through preview1 on a connected preview2 socket's number must \
             reach its peer, fd_fdstat_get must report a non-blocking socket stream, and \
             sock_shutdown must answer ENOTSUP"
        );
    }

    // @behavior RT-355
    #[test]
    fn a_socket_whose_connect_failed_polls_ready_to_read() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let mut guest = networked(port);
        let fd = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port,
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)?
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
        let (mut guest, fd, mut peer) = connected(&listener);
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
