//! `TCPSocket`, one connection over `wasi:sockets/tcp`, as CRuby's
//! `ext/socket/tcpsocket.c` opens one to a peer.

use std::cell::RefCell;
use std::net::{Ipv4Addr, Shutdown};
use std::task::Poll;
use std::time::Duration;

use beni::prelude::*;
use beni::scan_args::{get_kwargs, scan_args};
use beni::value::qnil;
use beni::{
    Error, ExceptionClass, Module, Mrb, Proc, RArray, RClass, RHash, TryConvert, TypedData, Value,
};

use super::address::{local_of, peer_of, to_ip_socket_address};
use super::addrinfo::{IPPROTO_TCP, SOCK_STREAM};
use super::basic::{IoState, Readiness, SocketIo, Wait};
use super::ip::local_end_of;
use super::{bind, stream_error, to_error};
use crate::errno;
use crate::io::{time_interval_of, Direction};
use crate::wasi::wasi::io::streams::{InputStream, OutputStream, StreamError};
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::network::{ErrorCode, IpAddressFamily};
use crate::wasi::wasi::sockets::tcp::{ShutdownType, TcpSocket};
use crate::wasi::wasi::sockets::tcp_create_socket::create_tcp_socket;
use crate::wasi::{read_within, wait_within};

/// The most `blocking-write-and-flush` takes in one call, as WASI bounds it.
const MAX_BLOCKING_WRITE: usize = 4096;

/// One connection, its streams dropped before the socket, as WASI asks of a
/// child resource.
struct OpenSocket {
    input: InputStream,
    output: OutputStream,
    socket: TcpSocket,
    peer: (Ipv4Addr, u16),
}

/// What a `TCPSocket` stands for, kept in its payload so a script's instance
/// variables cannot rewrite it; `None` once closed.
#[beni::wrap(class = "TCPSocket", name = "Kobako::TCPSocket")]
pub(super) struct Connection {
    open: RefCell<Option<OpenSocket>>,
    io: IoState,
}

impl SocketIo for Connection {
    fn io(&self) -> &IoState {
        &self.io
    }

    /// Asking for no bytes waits until the stream is readable, then answers
    /// as the end, as a stream's `recv(2)` returning 0 reads to CRuby. Once
    /// the reading side is shut, a read takes what has arrived and answers
    /// the end rather than wait, as Linux's `tcp_recvmsg` stops at
    /// `RCV_SHUTDOWN`.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error> {
        if self.io().rcv_shut.get() {
            let received = match max {
                0 => Poll::Pending,
                _ => self.receive_from_stream(mrb, max, Wait::DontWait)?,
            };
            return Ok(match received {
                Poll::Pending => Poll::Ready(None),
                ready => ready,
            });
        }
        self.receive_from_stream(mrb, max, wait)
    }

    /// The bytes, as CRuby's `IO#write` sends all it is given and
    /// `write_nonblock` what there is room for.
    fn send(&self, mrb: &Mrb, bytes: &[u8], wait: Wait) -> Result<Poll<usize>, Error> {
        self.with(mrb, |open| {
            let failed = |err| stream_error(mrb, err);
            if wait == Wait::Block {
                for chunk in bytes.chunks(MAX_BLOCKING_WRITE) {
                    open.output
                        .blocking_write_and_flush(chunk)
                        .map_err(failed)?;
                }
                return Ok(Poll::Ready(bytes.len()));
            }
            if bytes.is_empty() {
                return Ok(Poll::Ready(0));
            }
            let room = open.output.check_write().map_err(failed)? as usize;
            if room == 0 {
                return Ok(Poll::Pending);
            }
            let now = &bytes[..room.min(bytes.len())];
            open.output.write(now).map_err(failed)?;
            open.output.flush().map_err(failed)?;
            Ok(Poll::Ready(now.len()))
        })
    }

    /// The host shuts the writing side, sending the FIN once what was written
    /// is sent.
    fn protocol_shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error> {
        self.with(mrb, |open| {
            if how != Shutdown::Read {
                open.socket
                    .shutdown(ShutdownType::Send)
                    .map_err(|code| to_error(mrb, code, "TCPSocket", "shutdown(2)"))?;
            }
            Ok(())
        })
    }

    fn local_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error> {
        let local = self.with(mrb, |open| Ok(open.socket.local_address()))?;
        local_end_of(mrb, local, "TCPSocket")
    }

    fn peer_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error> {
        self.with(mrb, |open| Ok(open.peer))
    }

    fn kind(&self) -> (i64, i64) {
        (SOCK_STREAM, IPPROTO_TCP)
    }

    fn handle(&self, mrb: &Mrb) -> Result<u32, Error> {
        self.with(mrb, |open| Ok(open.socket.handle()))
    }

    fn protocol_readiness(&self, mrb: &Mrb, direction: Direction) -> Result<Readiness, Error> {
        self.with(mrb, |open| {
            Ok(match direction {
                Direction::Read => Readiness::Pending(open.input.subscribe()),
                Direction::Write => Readiness::Pending(open.output.subscribe()),
            })
        })
    }

    fn close(&self) {
        self.open.borrow_mut().take();
    }

    fn is_closed(&self) -> bool {
        self.open.borrow().is_none()
    }
}

/// What `call` answers for the TCP socket `value` stands for, `None` when it
/// stands for no `TCPSocket`; a closed one raises `IOError`.
pub(super) fn with_tcp_socket<T>(
    mrb: &Mrb,
    value: Value,
    call: impl FnOnce(&TcpSocket) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    match Connection::of(mrb, value) {
        Ok(connection) => connection.with(mrb, |open| call(&open.socket)).map(Some),
        Err(_) => Ok(None),
    }
}

impl Connection {
    pub(super) fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Connection, Error> {
        <&Connection>::try_convert(value, mrb)
    }

    fn with<T>(
        &self,
        mrb: &Mrb,
        call: impl FnOnce(&OpenSocket) -> Result<T, Error>,
    ) -> Result<T, Error> {
        match self.open.borrow().as_ref() {
            Some(open) => call(open),
            None => Err(errno::io_error(mrb, "closed stream")),
        }
    }

    /// Up to `max` bytes from the input stream, `None` at its end.
    fn receive_from_stream(
        &self,
        mrb: &Mrb,
        max: usize,
        wait: Wait,
    ) -> Result<Poll<Option<Vec<u8>>>, Error> {
        let timeout = wait.timeout();
        self.with(mrb, |open| {
            if max == 0 {
                return Ok(match wait_within(&open.input.subscribe(), timeout) {
                    true => Poll::Ready(None),
                    false => Poll::Pending,
                });
            }
            match read_within(&open.input, max as u64, timeout) {
                Ok(Some(bytes)) => Ok(Poll::Ready(Some(bytes))),
                Ok(None) => Ok(Poll::Pending),
                Err(StreamError::Closed) => Ok(Poll::Ready(None)),
                Err(err) => Err(stream_error(mrb, err)),
            }
        })
    }
}

/// Define `TCPSocket` under `ip`, its `IPSocket`, and `Socket.tcp`.
pub(super) fn init(mrb: &Mrb, socket_class: RClass, ip: RClass) -> Result<(), Error> {
    socket_class.define_singleton_method(mrb, c"tcp", beni::method!(socket_s_tcp, -1))?;
    let socket = mrb.define_class(c"TCPSocket", ip)?;
    Connection::mark_carriers(mrb)?;
    socket.define_singleton_method(mrb, c"new", beni::method!(tcp_socket_s_new, -1))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(tcp_socket_s_new, -1))?;
    Ok(())
}

/// `TCPSocket.new(remote_host, remote_port, local_host = nil, local_port = nil,
/// connect_timeout: nil)` connects to the IPv4 address `remote_host` names, or
/// the one its name leads to, as the network the Sandbox gives allows. A local
/// end the script chooses is bound first, as the host lets a bind take it,
/// and a connect slower than `connect_timeout` raises `IO::TimeoutError`, as
/// CRuby's does.
fn tcp_socket_s_new(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    let connection = dial(mrb, args, Caller::TcpSocketNew)?;
    Ok(mrb.wrap_as(connection, class).as_value())
}

/// `Socket.tcp(host, port, local_host = nil, local_port = nil,
/// connect_timeout: nil, resolv_timeout: nil) { |socket| }` connects as
/// `TCPSocket.new` does, answering a `TCPSocket` where CRuby's answers a
/// `Socket`, since the guest makes no raw socket. With a block it yields the
/// socket and closes it after, answering what the block answers. A connect
/// slower than `connect_timeout` raises `Errno::ETIMEDOUT`, as CRuby's does.
fn socket_s_tcp(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?.block;
    let connection = dial(mrb, args, Caller::SocketTcp)?;
    let socket = mrb
        .wrap_as(connection, mrb.class_get(c"TCPSocket")?)
        .as_value();
    let Some(block) = block else {
        return Ok(socket);
    };
    let answer = block.call::<_, Value>(mrb, [socket]);
    socket.funcall::<_, _, Value>(mrb, c"close", ())?;
    answer
}

/// Which call a connect answers for: `Socket.tcp` names its end as an
/// `Addrinfo` shows it and times out as `Errno::ETIMEDOUT`, where
/// `TCPSocket.new` quotes the host and raises `IO::TimeoutError`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    TcpSocketNew,
    SocketTcp,
}

/// The connection `TCPSocket.new` and `Socket.tcp` open from their
/// arguments: to the IPv4 address `remote_host` names, or the one its name
/// leads to, from a local end the script chooses, bound first as the host
/// lets a bind take it.
fn dial(mrb: &Mrb, args: &[Value], caller: Caller) -> Result<Connection, Error> {
    const TCP: &str = "TCPSocket";
    let mut args = args.to_vec();
    let options = args.last().copied().and_then(RHash::from_value);
    if options.is_some() {
        args.pop();
    }
    let (host, port, local) = match args.as_slice() {
        [host, port, local @ ..] if local.len() <= 2 => (*host, *port, local),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 4)),
    };
    let Timeouts { resolv, connect } = timeouts_of(mrb, options)?;
    let resolv_limit = || match (resolv, caller) {
        (None, _) => Ok(None),
        (Some(sec), Caller::TcpSocketNew) => time_interval_of(mrb, sec),
        (Some(sec), Caller::SocketTcp) => deadline_of(mrb, sec),
    };
    let (ip, port, quoted) = peer_of(mrb, host, port, &resolv_limit, TCP, "connect(2)")?;
    let named = |call: &str, quoted: String, (ip, port): (Ipv4Addr, u16)| match caller {
        Caller::TcpSocketNew => quoted,
        Caller::SocketTcp => format!("{call} for {ip}:{port}"),
    };
    let call = named("connect(2)", quoted, (ip, port));
    let failed = |code| to_error(mrb, code, TCP, &call);
    let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(failed)?;
    let network = instance_network();
    if local.iter().any(|end| !end.is_nil()) {
        let none = qnil().as_value();
        let (local_host, local_port) = (
            local.first().copied().unwrap_or(none),
            local.get(1).copied().unwrap_or(none),
        );
        let (local_ip, local_port, quoted) = local_of(mrb, local_host, local_port, TCP)?;
        let bind_call = named("bind(2)", quoted, (local_ip, local_port));
        bind(&socket, (local_ip, local_port))
            .map_err(|code| to_error(mrb, code, TCP, &bind_call))?;
    }
    socket
        .start_connect(&network, to_ip_socket_address((ip, port)))
        .map_err(failed)?;
    let pollable = socket.subscribe();
    let (input, output) = loop {
        match socket.finish_connect() {
            Ok(streams) => break streams,
            Err(ErrorCode::WouldBlock) => {
                let limit = match connect {
                    Some(sec) => time_interval_of(mrb, sec)?,
                    None => None,
                };
                if wait_within(&pollable, limit) {
                    continue;
                }
                return Err(match caller {
                    Caller::TcpSocketNew => Error::new(
                        mrb,
                        io_timeout_error(mrb)?,
                        &format!("user specified timeout for {ip} port {port}"),
                    ),
                    Caller::SocketTcp => {
                        errno::error_by_name(mrb, c"ETIMEDOUT", "user specified timeout")
                    }
                });
            }
            Err(code) => return Err(failed(code)),
        }
    };
    Ok(Connection {
        open: RefCell::new(Some(OpenSocket {
            input,
            output,
            socket,
            peer: (ip, port),
        })),
        io: IoState::new(),
    })
}

/// The limits `TCPSocket.new`'s keywords set on resolving a name, read only
/// for a name, and on connecting, read only once a connect must wait, as
/// CRuby's `rsock_connect` reaches `wait_connectable` only then.
struct Timeouts {
    resolv: Option<Value>,
    connect: Option<Value>,
}

/// The limits `resolv_timeout:` and `connect_timeout:` set, refusing any
/// keyword CRuby's does not take. `fast_fallback:` chooses between IPv6 and
/// IPv4 attempts, so on a guest that speaks only IPv4 it changes nothing.
fn timeouts_of(mrb: &Mrb, options: Option<RHash>) -> Result<Timeouts, Error> {
    let Some(options) = options else {
        return Ok(Timeouts {
            resolv: None,
            connect: None,
        });
    };
    let kw = get_kwargs::<_, (), (Option<Value>, Option<Value>, Option<Value>), ()>(
        mrb,
        options,
        &[],
        &["resolv_timeout", "connect_timeout", "fast_fallback"],
    )?;
    let (resolv, connect, _fast_fallback) = kw.optional;
    Ok(Timeouts { resolv, connect })
}

/// The wait `sec` leaves, as `Socket.tcp` reads `resolv_timeout:` into the
/// deadline `now + sec`: one already past ends at once, and NaN, which no
/// time reaches, never.
fn deadline_of(mrb: &Mrb, sec: Value) -> Result<Option<Duration>, Error> {
    let seconds = f64::try_convert(sec, mrb)?;
    Ok(match seconds {
        _ if seconds.is_nan() => None,
        _ if seconds <= 0.0 => Some(Duration::ZERO),
        _ => Some(Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)),
    })
}

fn io_timeout_error(mrb: &Mrb) -> Result<ExceptionClass, Error> {
    mrb.class_get(c"IO")?.const_get(mrb, c"TimeoutError")
}
