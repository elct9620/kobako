//! `TCPSocket`, one connection over `wasi:sockets/tcp`, as CRuby's
//! `ext/socket/tcpsocket.c` opens one to a peer.

use std::cell::RefCell;
use std::net::{Ipv4Addr, Shutdown};
use std::task::Poll;
use std::time::Duration;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{
    Error, ExceptionClass, Module, Mrb, Proc, RArray, RClass, RHash, Symbol, TryConvert, TypedData,
    Value,
};

use super::address::{local_of, peer_of, to_ip_socket_address};
use super::addrinfo::{IPPROTO_TCP, SOCK_STREAM};
use super::basic::{IoState, SocketIo, Wait};
use super::ip::local_end_of;
use super::{seconds_of, stream_error, to_error};
use crate::errno;
use crate::io::argument_error;
use crate::wasi::wait_within;
use crate::wasi::wasi::io::streams::{InputStream, OutputStream, StreamError};
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::network::{ErrorCode, IpAddressFamily};
use crate::wasi::wasi::sockets::tcp::{ShutdownType, TcpSocket};
use crate::wasi::wasi::sockets::tcp_create_socket::create_tcp_socket;

/// The most `blocking-write-and-flush` takes in one call, as WASI bounds it.
const WRITE_CHUNK: usize = 4096;

/// One connection, its streams dropped before the socket, as WASI asks of a
/// child resource.
struct Open {
    input: InputStream,
    output: OutputStream,
    socket: TcpSocket,
    peer: (Ipv4Addr, u16),
}

/// What a `TCPSocket` stands for, kept in its payload so a script's instance
/// variables cannot rewrite it; `None` once closed.
#[beni::wrap(class = "TCPSocket", name = "Kobako::TCPSocket")]
pub(super) struct Connection {
    open: RefCell<Option<Open>>,
    io: IoState,
}

impl SocketIo for Connection {
    fn io(&self) -> &IoState {
        &self.io
    }

    /// Asking for no bytes waits until the stream is readable, then answers
    /// as the end, as a stream's `recv(2)` returning 0 reads to CRuby.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error> {
        if max == 0 {
            return self.with(mrb, |open| {
                let readable = open.input.subscribe();
                let limit = match wait {
                    Wait::Block => None,
                    Wait::DontWait => Some(Duration::ZERO),
                };
                Ok(match wait_within(&readable, limit) {
                    true => Poll::Ready(None),
                    false => Poll::Pending,
                })
            });
        }
        self.with(mrb, |open| loop {
            let read = match wait {
                Wait::Block => open.input.blocking_read(max as u64),
                Wait::DontWait => open.input.read(max as u64),
            };
            match read {
                Ok(bytes) if bytes.is_empty() && wait == Wait::DontWait => {
                    return Ok(Poll::Pending)
                }
                Ok(bytes) if bytes.is_empty() => continue,
                Ok(bytes) => return Ok(Poll::Ready(Some(bytes))),
                Err(StreamError::Closed) => return Ok(Poll::Ready(None)),
                Err(err) => return Err(stream_error(mrb, err)),
            }
        })
    }

    /// The bytes, as CRuby's `IO#write` sends all it is given and
    /// `write_nonblock` what there is room for.
    fn send(&self, mrb: &Mrb, bytes: &[u8], wait: Wait) -> Result<Poll<usize>, Error> {
        self.with(mrb, |open| {
            let failed = |err| stream_error(mrb, err);
            if wait == Wait::Block {
                for chunk in bytes.chunks(WRITE_CHUNK) {
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

    /// The host shuts the side `how` names; shutting the writing side sends
    /// the FIN once what was written is sent.
    fn shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error> {
        let how = match how {
            Shutdown::Read => ShutdownType::Receive,
            Shutdown::Write => ShutdownType::Send,
            Shutdown::Both => ShutdownType::Both,
        };
        self.with(mrb, |open| {
            open.socket
                .shutdown(how)
                .map_err(|code| to_error(mrb, code, "TCPSocket", "shutdown(2)"))
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

    fn with<T>(&self, mrb: &Mrb, call: impl FnOnce(&Open) -> Result<T, Error>) -> Result<T, Error> {
        match self.open.borrow().as_ref() {
            Some(open) => call(open),
            None => Err(errno::io_error(mrb, "closed stream")),
        }
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
    let (ip, port, quoted) = peer_of(mrb, host, port, resolv, TCP, "connect(2)")?;
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
        let refused = |code| to_error(mrb, code, TCP, &bind_call);
        socket
            .start_bind(&network, to_ip_socket_address((local_ip, local_port)))
            .map_err(refused)?;
        socket.finish_bind().map_err(refused)?;
    }
    socket
        .start_connect(&network, to_ip_socket_address((ip, port)))
        .map_err(failed)?;
    let pollable = socket.subscribe();
    let (input, output) = loop {
        match socket.finish_connect() {
            Ok(streams) => break streams,
            Err(ErrorCode::WouldBlock) => {
                if wait_within(&pollable, connect) {
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
        open: RefCell::new(Some(Open {
            input,
            output,
            socket,
            peer: (ip, port),
        })),
        io: IoState::new(),
    })
}

/// The limits `TCPSocket.new`'s keywords set on resolving a name and on
/// connecting.
struct Timeouts {
    resolv: Option<Duration>,
    connect: Option<Duration>,
}

/// The limits `resolv_timeout:` and `connect_timeout:` set, refusing any
/// keyword CRuby's does not take. `fast_fallback:` chooses between IPv6 and
/// IPv4 attempts, so on a guest that speaks only IPv4 it changes nothing.
fn timeouts_of(mrb: &Mrb, options: Option<RHash>) -> Result<Timeouts, Error> {
    let mut timeouts = Timeouts {
        resolv: None,
        connect: None,
    };
    let Some(options) = options else {
        return Ok(timeouts);
    };
    let keys: RArray = options.funcall(mrb, c"keys", ())?;
    for i in 0..keys.len() {
        let given: Value = keys.entry(mrb, i as isize)?;
        let name = Symbol::from_value(given).and_then(|name| name.name(mrb).ok());
        let limit = match name.as_deref() {
            Some("resolv_timeout") => &mut timeouts.resolv,
            Some("connect_timeout") => &mut timeouts.connect,
            Some("fast_fallback") => continue,
            _ => {
                let name = given.inspect(mrb);
                return Err(argument_error(mrb, &format!("unknown keyword: {name}")));
            }
        };
        *limit = seconds_of(mrb, options.aref(mrb, given)?)?;
    }
    Ok(timeouts)
}

fn io_timeout_error(mrb: &Mrb) -> Result<ExceptionClass, Error> {
    mrb.class_get(c"IO")?.const_get(mrb, c"TimeoutError")
}
