//! `BasicSocket`, what every socket answers as an IO, as CRuby's
//! `ext/socket/basicsocket.c` stands over `io.c`: the socket type's own
//! receiving and sending, and the calls that reach them past the buffer.

use std::cell::{Cell, RefCell};
use std::net::{Ipv4Addr, Shutdown};
use std::task::Poll;

use beni::prelude::*;
use beni::value::qnil;
use beni::{
    Error, IntoValue, Mrb, Qfalse, Qtrue, RArray, RClass, RHash, Symbol, TryConvert, Value,
};

use super::address::sockaddr_of;
use super::buffer::{end_of_file, fill};
use super::tcp::Connection;
use crate::errno;
use crate::io::{argument_error, would_wait, Direction};

/// Whether one call waits for the socket, as Linux's `MSG_DONTWAIT` chooses
/// for one `recvmsg` or `sendmsg`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wait {
    Block,
    DontWait,
}

impl Wait {
    /// How long a wait under it may last: unbounded, or not at all.
    pub(super) fn limit(self) -> Option<std::time::Duration> {
        match self {
            Wait::Block => None,
            Wait::DontWait => Some(std::time::Duration::ZERO),
        }
    }
}

/// What CRuby's `rb_io_t` keeps beside a socket: the read buffer, whose
/// bytes come first, whether writes are sent at once, and which sides
/// `close_read` and `close_write` closed.
pub(super) struct IoState {
    /// What a read took past what it answered.
    pub(super) rbuf: RefCell<Vec<u8>>,
    /// Whether a write is sent at once, as `IO#sync` reads; a socket's is.
    pub(super) sync: Cell<bool>,
    /// `close_read` closed the reading side, as CRuby's IO clears
    /// `FMODE_READABLE`.
    read_closed: Cell<bool>,
    /// `close_write` closed the writing side, as CRuby's IO clears
    /// `FMODE_WRITABLE`.
    write_closed: Cell<bool>,
}

impl IoState {
    pub(super) fn new() -> IoState {
        IoState {
            rbuf: RefCell::new(Vec::new()),
            sync: Cell::new(true),
            read_closed: Cell::new(false),
            write_closed: Cell::new(false),
        }
    }
}

/// What a `BasicSocket` call reaches: the socket's receiving and sending, as
/// Linux's `proto_ops` gives each socket type its own, and what CRuby keeps
/// beside it.
pub(super) trait SocketIo {
    /// The buffer and modes CRuby keeps beside the socket.
    fn io(&self) -> &IoState;

    /// Up to `max` of the next bytes the host brings, `None` at the end;
    /// `Pending` when none have come and `wait` does not wait.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error>;

    /// Send `bytes`, all of them when `wait` waits, else what there is room
    /// for, answering how many were sent; `Pending` when there is no room and
    /// `wait` does not wait.
    fn send(&self, mrb: &Mrb, bytes: &[u8], wait: Wait) -> Result<Poll<usize>, Error>;

    /// Shut the side `how` names, as `shutdown(2)` does.
    fn shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error>;

    /// The socket's own end, as `getsockname(2)` answers it: an unbound one
    /// answers the unspecified address.
    fn local_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error>;

    /// The peer as the guest named it, as `getpeername(2)` answers it; with
    /// none it raises `Errno::ENOTCONN`.
    fn peer_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error>;

    /// The socket's kind and protocol, as an `Addrinfo` of one of its ends
    /// carries them.
    fn kind(&self) -> (i64, i64);

    /// The socket's preview2 handle, which preview1's calls and `IO.select`
    /// reach as one fdtable holds a socket beside a file.
    fn handle(&self, mrb: &Mrb) -> Result<u32, Error>;

    /// Release the socket, its streams before it.
    fn close(&self);

    fn is_closed(&self) -> bool;
}

/// The socket `value` stands for, open: a closed one raises `IOError`.
pub(super) fn open_socket_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
    let source = source_of(mrb, value)?;
    match source.is_closed() {
        true => Err(errno::io_error(mrb, "closed stream")),
        false => Ok(source),
    }
}

/// The socket `value` stands for, to read: a closed one, or one whose reading
/// side `close_read` closed, raises `IOError`, as CRuby's checks it before
/// any read, the buffer's included.
pub(super) fn reader_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
    let source = open_socket_of(mrb, value)?;
    match source.io().read_closed.get() {
        true => Err(errno::io_error(mrb, "not opened for reading")),
        false => Ok(source),
    }
}

/// The socket `value` stands for, to write: a closed one, or one whose
/// writing side `close_write` closed, raises `IOError`.
fn writer_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
    let source = open_socket_of(mrb, value)?;
    match source.io().write_closed.get() {
        true => Err(errno::io_error(mrb, "not opened for writing")),
        false => Ok(source),
    }
}

/// What a call that waits answers, which it does only once it is ready.
pub(super) fn ready_of<T>(poll: Poll<T>) -> T {
    match poll {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("a call that waits returns once it is ready"),
    }
}

/// The arguments before a trailing `exception:` keyword, and whether a call
/// that would wait raises, as CRuby's `*_nonblock` methods take it: `true`
/// unless given `false`.
pub(super) fn exception_option<'a>(
    mrb: &Mrb,
    args: &'a [Value],
) -> Result<(&'a [Value], bool), Error> {
    let Some((last, rest)) = args.split_last() else {
        return Ok((args, true));
    };
    let Some(options) = RHash::from_value(*last) else {
        return Ok((args, true));
    };
    let keys: RArray = options.funcall(mrb, c"keys", ())?;
    let mut exception = true;
    for i in 0..keys.len() {
        let key: Value = keys.entry(mrb, i as isize)?;
        let named = Symbol::from_value(key).and_then(|key| key.name(mrb).ok());
        if named.as_deref() != Some("exception") {
            let message = format!("unknown keyword: {}", key.inspect(mrb));
            return Err(argument_error(mrb, &message));
        }
        let given: Value = options.aref(mrb, key)?;
        exception = match (Qtrue::from_value(given), Qfalse::from_value(given)) {
            (Some(_), _) => true,
            (_, Some(_)) => false,
            _ => {
                let message = format!(
                    "expected true or false as exception: {}",
                    given.inspect(mrb)
                );
                return Err(argument_error(mrb, &message));
            }
        };
    }
    Ok((rest, exception))
}

/// The bytes `object` spells, as CRuby's `IO#write` converts it.
fn written_bytes_of(mrb: &Mrb, object: Value) -> Result<Vec<u8>, Error> {
    let data = object
        .funcall::<_, _, Value>(mrb, c"to_s", ())?
        .to_r_string(mrb)?;
    // SAFETY: the bytes are copied before the next mruby call.
    Ok(unsafe { data.as_bytes(mrb) }.to_vec())
}

/// A length a read is given, refusing a negative one as CRuby's does.
pub(super) fn length_of(mrb: &Mrb, length: Value) -> Result<usize, Error> {
    usize::try_from(i64::try_convert(length, mrb)?)
        .map_err(|_| argument_error(mrb, "negative length given"))
}

/// The socket `value` stands for, a `TCPSocket` or a `UDPSocket`.
pub(super) fn source_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
    match Connection::of(mrb, value) {
        Ok(connection) => Ok(connection),
        Err(_) => super::udp::source_of(mrb, value),
    }
}

/// What every socket answers as an IO, as CRuby's `BasicSocket` does over its
/// one descriptor: reads through a buffer before the socket, a position, sync,
/// or listing it has none of answered as Linux answers each, and no copy,
/// since WASI dups no socket.
pub(super) fn init(mrb: &Mrb, basic: RClass) -> Result<(), Error> {
    basic.define_method(mrb, c"fileno", beni::method!(socket_fileno, 0))?;
    basic.define_method(mrb, c"to_i", beni::method!(socket_fileno, 0))?;
    basic.define_method(mrb, c"write", beni::method!(socket_write, -1))?;
    basic.define_method(
        mrb,
        c"write_nonblock",
        beni::method!(socket_write_nonblock, -1),
    )?;
    basic.define_method(mrb, c"syswrite", beni::method!(socket_syswrite, 1))?;
    basic.define_method(mrb, c"recv", beni::method!(socket_recv, -1))?;
    basic.define_method(
        mrb,
        c"recv_nonblock",
        beni::method!(socket_recv_nonblock, -1),
    )?;
    basic.define_method(mrb, c"sysread", beni::method!(socket_sysread, -1))?;
    basic.define_method(mrb, c"shutdown", beni::method!(socket_shutdown, -1))?;
    basic.define_method(mrb, c"close_read", beni::method!(socket_close_read, 0))?;
    basic.define_method(mrb, c"close_write", beni::method!(socket_close_write, 0))?;
    basic.define_method(mrb, c"close", beni::method!(socket_close, 0))?;
    basic.define_method(mrb, c"closed?", beni::method!(socket_closed_p, 0))?;
    basic.define_method(mrb, c"getsockname", beni::method!(socket_getsockname, 0))?;
    basic.define_method(mrb, c"getpeername", beni::method!(socket_getpeername, 0))?;
    super::buffer::init(mrb, basic)?;
    for name in [
        c"seek", c"rewind", c"pos", c"pos=", c"tell", c"pread", c"pwrite",
    ] {
        basic.define_method(mrb, name, beni::method!(socket_unseekable, -1))?;
    }
    basic.define_method(mrb, c"fsync", beni::method!(socket_unsyncable, 0))?;
    basic.define_method(mrb, c"fdatasync", beni::method!(socket_unsyncable, 0))?;
    basic.define_method(mrb, c"advise", beni::method!(socket_advise, -1))?;
    basic.define_method(mrb, c"sync", beni::method!(socket_sync, 0))?;
    basic.define_method(mrb, c"sync=", beni::method!(socket_sync_set, 1))?;
    for name in ["dup", "clone"] {
        basic.funcall::<_, _, Value>(mrb, c"undef_method", [Symbol::new(mrb, name)?.as_value()])?;
    }
    Ok(())
}

/// `fileno` answers the socket's descriptor, its preview2 handle.
fn socket_fileno(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(i64::from(source_of(mrb, self_)?.handle(mrb)?))
}

/// `write(*objects)` sends what the objects spell, answering how many bytes
/// it sent, as CRuby's `IO#write` joins them.
fn socket_write(mrb: &Mrb, self_: Value, objects: &[Value]) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    for object in objects {
        bytes.extend(written_bytes_of(mrb, *object)?);
    }
    let sent = ready_of(writer_of(mrb, self_)?.send(mrb, &bytes, Wait::Block)?);
    Ok((sent as i64).into_value(mrb))
}

/// `write_nonblock(string, exception: true)` sends what there is room for at
/// once, answering how many bytes it sent; with no room it raises
/// `IO::EAGAINWaitWritable`, or answers `:wait_writable` with
/// `exception: false`, as CRuby's does.
fn socket_write_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option(mrb, args)?;
    let [string] = args else {
        return Err(Error::argnum(mrb, args.len(), 1, 1));
    };
    let bytes = written_bytes_of(mrb, *string)?;
    match writer_of(mrb, self_)?.send(mrb, &bytes, Wait::DontWait)? {
        Poll::Ready(sent) => Ok((sent as i64).into_value(mrb)),
        Poll::Pending => would_wait(mrb, Direction::Write, "write would block", exception),
    }
}

/// `syswrite(string)` sends every byte of `string` past any buffer,
/// answering how many it sent.
fn socket_syswrite(mrb: &Mrb, self_: Value, string: Value) -> Result<Value, Error> {
    let bytes = written_bytes_of(mrb, string)?;
    let sent = ready_of(writer_of(mrb, self_)?.send(mrb, &bytes, Wait::Block)?);
    Ok((sent as i64).into_value(mrb))
}

/// Linux's `MSG_DONTWAIT`, the one receive flag a guest's receive takes; it
/// waits all the same, as CRuby's `recv` waits again on `EAGAIN`.
const MSG_DONTWAIT: i64 = 0x40;

/// What a receive past `source`'s buffer takes: `maxlen`, `flags`, and an
/// `outbuf`, refusing a flag WASI carries none of with `Errno::ENOTSUP`.
/// Bytes a read held back raise `IOError` naming `refusal`, as CRuby's.
pub(super) fn direct_args(
    mrb: &Mrb,
    source: &dyn SocketIo,
    args: &[Value],
    refusal: &str,
) -> Result<(usize, Option<Value>), Error> {
    let (maxlen, flags, outbuf) = match args {
        [maxlen] => (*maxlen, None, None),
        [maxlen, flags] => (*maxlen, Some(*flags), None),
        [maxlen, flags, outbuf] => (
            *maxlen,
            Some(*flags),
            Some(*outbuf).filter(|buf| !buf.is_nil()),
        ),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 3)),
    };
    let maxlen = length_of(mrb, maxlen)?;
    if let Some(flags) = flags.filter(|flags| !flags.is_nil()) {
        if i64::try_convert(flags, mrb)? & !MSG_DONTWAIT != 0 {
            return Err(errno::error_by_name(mrb, c"ENOTSUP", "recvfrom(2)"));
        }
    }
    if !source.io().rbuf.borrow().is_empty() {
        return Err(errno::io_error(mrb, refusal));
    }
    Ok((maxlen, outbuf))
}

/// The side a `shutdown` argument names, as CRuby's `rsock_shutdown_how_arg`
/// reads it: `nil` for both, a number, or a name with or without `SHUT_`.
fn shutdown_of(mrb: &Mrb, how: Option<Value>) -> Result<Shutdown, Error> {
    let Some(how) = how.filter(|how| !how.is_nil()) else {
        return Ok(Shutdown::Both);
    };
    if let Some(name) = super::name_of(mrb, how)? {
        return match name.strip_prefix("SHUT_").unwrap_or(&name) {
            "RD" => Ok(Shutdown::Read),
            "WR" => Ok(Shutdown::Write),
            "RDWR" => Ok(Shutdown::Both),
            _ => Err(super::socket_error(
                mrb,
                &format!("unknown shutdown argument: {name}"),
            )),
        };
    }
    match i64::try_convert(how, mrb)? {
        0 => Ok(Shutdown::Read),
        1 => Ok(Shutdown::Write),
        2 => Ok(Shutdown::Both),
        _ => Err(argument_error(
            mrb,
            "`how' should be either :SHUT_RD, :SHUT_WR, :SHUT_RDWR",
        )),
    }
}

/// `shutdown(how = Socket::SHUT_RDWR)` shuts the side `how` names, answering
/// 0. What has arrived still reads after the reading side shuts, and a write
/// after the writing side shuts raises `Errno::EPIPE`, as Linux's.
fn socket_shutdown(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if args.len() > 1 {
        return Err(Error::argnum(mrb, args.len(), 0, 1));
    }
    let how = shutdown_of(mrb, args.first().copied())?;
    open_socket_of(mrb, self_)?.shutdown(mrb, how)?;
    Ok(0.into_value(mrb))
}

/// `close_read` shuts the reading side and refuses a later read, closing the
/// socket once its writing side is closed too, as CRuby's `rb_io_close_read`
/// does for a socket.
fn socket_close_read(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let source = open_socket_of(mrb, self_)?;
    source.shutdown(mrb, Shutdown::Read)?;
    let io = source.io();
    io.read_closed.set(true);
    if io.write_closed.get() {
        source.close();
    }
    Ok(qnil().as_value())
}

/// `close_write` sends the FIN once what was written is sent and refuses a
/// later write, closing the socket once its reading side is closed too.
fn socket_close_write(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let source = open_socket_of(mrb, self_)?;
    source.shutdown(mrb, Shutdown::Write)?;
    let io = source.io();
    io.write_closed.set(true);
    if io.read_closed.get() {
        source.close();
    }
    Ok(qnil().as_value())
}

/// `getsockname` answers the socket's own end packed as a `sockaddr_in`.
fn socket_getsockname(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (ip, port) = open_socket_of(mrb, self_)?.local_end(mrb)?;
    Ok(mrb.str_new(&sockaddr_of(ip.into(), port)).as_value())
}

/// `getpeername` answers the peer packed as a `sockaddr_in`.
fn socket_getpeername(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (ip, port) = open_socket_of(mrb, self_)?.peer_end(mrb)?;
    Ok(mrb.str_new(&sockaddr_of(ip.into(), port)).as_value())
}

/// `close` releases the socket; closing it again does nothing.
fn socket_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    source_of(mrb, self_)?.close();
    Ok(qnil().as_value())
}

fn socket_closed_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(source_of(mrb, self_)?.is_closed())
}

/// `recv(maxlen, flags = 0, outbuf = nil)` answers up to `maxlen` of what
/// comes next, waiting for some, past any buffer, and `nil` at the end,
/// leaving `outbuf` as it was. Like CRuby's `recvfrom(2)`, it reads whether
/// or not `close_read` closed the reading side.
fn socket_recv(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let source = open_socket_of(mrb, self_)?;
    let (maxlen, outbuf) = direct_args(mrb, source, args, "recv for buffered IO")?;
    match ready_of(source.receive(mrb, maxlen, Wait::Block)?) {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Ok(qnil().as_value()),
    }
}

/// `sysread(maxlen, outbuf = nil)` answers up to `maxlen` of what comes next,
/// waiting for some, past any buffer; it raises `EOFError` at the end.
fn socket_sysread(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let args = match args {
        [maxlen] => [*maxlen, qnil().as_value(), qnil().as_value()],
        [maxlen, outbuf] => [*maxlen, qnil().as_value(), *outbuf],
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let source = reader_of(mrb, self_)?;
    let (maxlen, outbuf) = direct_args(mrb, source, &args, "sysread for buffered IO")?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match ready_of(source.receive(mrb, maxlen, Wait::Block)?) {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Err(end_of_file(mrb)),
    }
}

/// `recv_nonblock(maxlen, flags = 0, outbuf = nil, exception: true)` answers
/// up to `maxlen` of what has come, `nil` at the end. With nothing come yet it
/// raises `IO::EAGAINWaitReadable`, or answers `:wait_readable` with
/// `exception: false`. Bytes a read held back raise `IOError`, as CRuby's.
fn socket_recv_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option(mrb, args)?;
    let source = open_socket_of(mrb, self_)?;
    let (maxlen, outbuf) = direct_args(mrb, source, args, "recvfrom for buffered IO")?;
    match source.receive(mrb, maxlen, Wait::DontWait)? {
        Poll::Ready(Some(bytes)) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        Poll::Ready(None) => Ok(qnil().as_value()),
        Poll::Pending => would_wait(mrb, Direction::Read, "recvfrom(2) would block", exception),
    }
}

/// A call a socket has no position for raises `Errno::ESPIPE`, as Linux's
/// `lseek`, `pread`, and `pwrite` answer one; a closed socket raises
/// `IOError` first, as CRuby checks the stream before the call.
fn socket_unseekable(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    open_socket_of(mrb, self_)?;
    Err(errno::error_alone(mrb, c"ESPIPE"))
}

/// A socket keeps nothing to sync, so `fsync` raises `Errno::EINVAL`, as
/// Linux's answers it; a closed socket raises `IOError` first.
fn socket_unsyncable(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    open_socket_of(mrb, self_)?;
    Err(errno::error_alone(mrb, c"EINVAL"))
}

/// `advise(advice, offset = 0, len = 0)` checks the advice as a file's does,
/// then is taken and ignored, as Linux's `fadvise` takes advice on a socket;
/// a closed socket raises `IOError` first.
fn socket_advise(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let Some(advice) = args.first().filter(|_| args.len() <= 3) else {
        return Err(Error::argnum(mrb, args.len(), 1, 3));
    };
    open_socket_of(mrb, self_)?;
    crate::stream::advice_of(mrb, *advice)?;
    Ok(qnil().as_value())
}

fn socket_sync(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(source_of(mrb, self_)?.io().sync.get())
}

fn socket_sync_set(mrb: &Mrb, self_: Value, sync: Value) -> Result<Value, Error> {
    source_of(mrb, self_)?.io().sync.set(sync.to_bool());
    Ok(sync)
}

/// The handle a wait on the socket `value` polls, `None` when `value` is no
/// socket. A closed socket, or a side `close_read` or `close_write` closed,
/// raises `IOError`, as CRuby checks an IO before it waits on it.
pub(crate) fn waitable_handle(
    mrb: &Mrb,
    value: Value,
    direction: Direction,
) -> Result<Option<u32>, Error> {
    if source_of(mrb, value).is_err() {
        return Ok(None);
    }
    let source = match direction {
        Direction::Read => reader_of(mrb, value)?,
        Direction::Write => writer_of(mrb, value)?,
    };
    source.handle(mrb).map(Some)
}
