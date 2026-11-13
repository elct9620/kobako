//! `BasicSocket`, what every socket answers as an IO, as CRuby's
//! `ext/socket/basicsocket.c` stands over `io.c`: the socket type's own
//! receiving and sending, and the calls that reach them past the buffer.

use std::cell::{Cell, RefCell};
use std::net::{Ipv4Addr, Shutdown};
use std::task::Poll;

use beni::prelude::*;
use beni::scan_args::get_kwargs;
use beni::value::qnil;
use beni::{Error, Integer, IntoValue, Mrb, Qfalse, RClass, RString, Symbol, TryConvert, Value};

use super::address::to_sockaddr;
use super::buffer::{eof_error, fill};
use super::ip::to_addr_array;
use super::option::Refusal;
use super::tcp::Connection;
use crate::errno;
use crate::io::{argument_error, split_options, would_wait, Direction};
use crate::wasi::wasi::io::poll::Pollable;

/// Whether one call waits for the socket, as Linux's `MSG_DONTWAIT` chooses
/// for one `recvmsg` or `sendmsg`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wait {
    Blocking,
    DontWait,
}

impl Wait {
    /// How long a wait under it may last: unbounded, or not at all.
    pub(super) fn timeout(self) -> Option<std::time::Duration> {
        match self {
            Wait::Blocking => None,
            Wait::DontWait => Some(std::time::Duration::ZERO),
        }
    }
}

/// When a wait on a socket ends.
pub(crate) enum Readiness {
    Ready,
    /// Ready once the pollable is.
    Pending(Pollable),
    /// Never ready, as a socket with no peer never reads.
    Inert,
}

/// How far above its preview2 handle a socket's `fileno` sits. A guest built
/// for preview1 numbers its files through preview1, which never reaches this
/// many, so no socket's number names a file.
const SOCKET_FILENO_OFFSET: i64 = 1 << 20;

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
    /// `shutdown` shut the reading side, as `sk_shutdown`'s `RCV_SHUTDOWN`;
    /// a wait to read ends at once, as `tcp_poll` and `datagram_poll` report
    /// it. The guest keeps it itself: WASI 0.2's TCP `shutdown(receive)`
    /// discards what arrived, where Linux's `SHUT_RD` still reads it, and its
    /// UDP names no shutdown.
    pub(super) rcv_shut: Cell<bool>,
}

impl IoState {
    pub(super) fn new() -> IoState {
        IoState {
            rbuf: RefCell::new(Vec::new()),
            sync: Cell::new(true),
            read_closed: Cell::new(false),
            write_closed: Cell::new(false),
            rcv_shut: Cell::new(false),
        }
    }
}

/// Bytes a receive took, beside the end that sent them when the socket names
/// one, as `recvfrom(2)` fills its address.
pub(super) type Received = (Vec<u8>, Option<(Ipv4Addr, u16)>);

/// What a `BasicSocket` call reaches: the socket's receiving and sending, as
/// Linux's `proto_ops` gives each socket type its own, and what CRuby keeps
/// beside it.
pub(super) trait SocketIo {
    /// The buffer and modes CRuby keeps beside the socket.
    fn io(&self) -> &IoState;

    /// Up to `max` of the next bytes the host brings, `None` at the end;
    /// `Pending` when none have come and `wait` does not wait.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error>;

    /// What `receive` answers, beside the end that sent it, as `recvfrom(2)`
    /// names one: a stream names none, as CRuby's answers `nil` for it. An
    /// `Errno` it raises names `call`.
    fn receive_from(
        &self,
        mrb: &Mrb,
        max: usize,
        wait: Wait,
        _call: &str,
    ) -> Result<Poll<Option<Received>>, Error> {
        let received = self.receive(mrb, max, wait)?;
        Ok(received.map(|bytes| bytes.map(|bytes| (bytes, None))))
    }

    /// Send `bytes`, all of them when `wait` waits, else what there is room
    /// for, answering how many were sent; `Pending` when there is no room and
    /// `wait` does not wait. An `Errno` it raises names `call`, as CRuby's
    /// `send` names `send(2)`; `None` names none, as its `IO#write` does.
    fn send(
        &self,
        mrb: &Mrb,
        bytes: &[u8],
        wait: Wait,
        call: Option<&str>,
    ) -> Result<Poll<usize>, Error>;

    /// Shut the side `how` names, as `shutdown(2)` does: the reading side is
    /// marked first, as `inet_shutdown` sets `sk_shutdown` before the
    /// protocol's own shutdown, which may still fail.
    fn shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error> {
        if how != Shutdown::Write {
            self.io().rcv_shut.set(true);
        }
        self.protocol_shutdown(mrb, how)
    }

    /// Shut the side `how` names as the protocol's own shutdown does, the
    /// reading side marked already.
    fn protocol_shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error>;

    /// The socket's own end, as `getsockname(2)` answers it: an unbound one
    /// answers the unspecified address.
    fn local_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error>;

    /// The peer as the guest named it, as `getpeername(2)` answers it; with
    /// none it raises `Errno::ENOTCONN`.
    fn peer_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error>;

    /// What `option`, a level and a name, reads on the socket, as the
    /// protocol's `getsockopt` answers it.
    fn option_of(&self, mrb: &Mrb, option: (i32, i32)) -> Result<Result<i32, Refusal>, Error>;

    /// Set `option` to `int` on the socket, as the protocol's `setsockopt`
    /// takes it.
    fn set_option(
        &self,
        mrb: &Mrb,
        option: (i32, i32),
        int: i32,
    ) -> Result<Result<(), Refusal>, Error>;

    /// The socket's kind and protocol, as an `Addrinfo` of one of its ends
    /// carries them.
    fn kind(&self) -> (i64, i64);

    /// The socket's preview2 handle.
    fn handle(&self, mrb: &Mrb) -> Result<u32, Error>;

    /// When a wait for the socket to read or write ends, as its `poll` would
    /// answer: a shut reading side is ready at once, as Linux's `tcp_poll`
    /// and `datagram_poll` report `RCV_SHUTDOWN`, and otherwise as the
    /// protocol finds it.
    fn readiness(&self, mrb: &Mrb, direction: Direction) -> Result<Readiness, Error> {
        if matches!(direction, Direction::Read) && self.io().rcv_shut.get() {
            return Ok(Readiness::Ready);
        }
        self.protocol_readiness(mrb, direction)
    }

    /// When a wait ends as the protocol's own `poll` would answer, its
    /// reading side open.
    fn protocol_readiness(&self, mrb: &Mrb, direction: Direction) -> Result<Readiness, Error>;

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
/// that would wait raises, as CRuby's socket `*_nonblock` methods take it:
/// `true` unless given `false`, as `init.c` asks only `ex == Qfalse`.
pub(super) fn exception_option_of<'a>(
    mrb: &Mrb,
    args: &'a [Value],
) -> Result<(&'a [Value], bool), Error> {
    let (rest, Some(options)) = split_options(args) else {
        return Ok((args, true));
    };
    let kw = get_kwargs::<_, (), (Option<Value>,), ()>(mrb, options, &[], &["exception"])?;
    let (given,) = kw.optional;
    Ok((
        rest,
        !given.is_some_and(|given| Qfalse::from_value(given).is_some()),
    ))
}

/// The bytes `object` spells, as CRuby's `IO#write` converts it.
fn written_bytes_of(mrb: &Mrb, object: Value) -> Result<Vec<u8>, Error> {
    let data = object
        .funcall::<_, _, Value>(mrb, c"to_s", ())?
        .to_r_string(mrb)?;
    // SAFETY: the bytes are copied before the next mruby call.
    Ok(unsafe { data.as_slice(mrb) }.to_vec())
}

/// The length `read` and `readpartial` are given, refusing a negative one as
/// CRuby's `io_getpartial` does.
pub(super) fn read_length_of(mrb: &Mrb, length: Value) -> Result<usize, Error> {
    let length = i64::try_convert(length, mrb)?;
    usize::try_from(length)
        .map_err(|_| argument_error(mrb, &format!("negative length {length} given")))
}

/// The length a socket read sizes its buffer by, as CRuby's `rsock_strbuf`
/// makes one, so a negative one raises what `rb_str_new` raises.
pub(super) fn strbuf_length_of(mrb: &Mrb, length: Value) -> Result<usize, Error> {
    usize::try_from(i64::try_convert(length, mrb)?)
        .map_err(|_| argument_error(mrb, "negative string size (or size too big)"))
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
    basic.define_method(mrb, c"send", beni::method!(socket_send, -1))?;
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
    for name in [c"rewind", c"pos", c"tell"] {
        basic.define_method(mrb, name, beni::method!(refuse_position, 0))?;
    }
    basic.define_method(mrb, c"pos=", beni::method!(refuse_set_position, 1))?;
    basic.define_method(mrb, c"seek", beni::method!(refuse_seek, -1))?;
    basic.define_method(mrb, c"pread", beni::method!(refuse_pread, -1))?;
    basic.define_method(mrb, c"pwrite", beni::method!(refuse_pwrite, 2))?;
    basic.define_method(mrb, c"fsync", beni::method!(refuse_sync, 0))?;
    basic.define_method(mrb, c"fdatasync", beni::method!(refuse_sync, 0))?;
    basic.define_method(mrb, c"advise", beni::method!(socket_advise, -1))?;
    basic.define_method(mrb, c"sync", beni::method!(socket_sync, 0))?;
    basic.define_method(mrb, c"sync=", beni::method!(socket_sync_set, 1))?;
    for name in ["dup", "clone"] {
        basic.funcall::<_, _, Value>(mrb, c"undef_method", [Symbol::new(mrb, name)?.as_value()])?;
    }
    Ok(())
}

/// `fileno` answers the socket's number, above every file's.
fn socket_fileno(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(SOCKET_FILENO_OFFSET + i64::from(source_of(mrb, self_)?.handle(mrb)?))
}

/// `write(*objects)` sends what the objects spell, answering how many bytes
/// it sent, as CRuby's `IO#write` joins them.
fn socket_write(mrb: &Mrb, self_: Value, objects: &[Value]) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    for object in objects {
        bytes.extend(written_bytes_of(mrb, *object)?);
    }
    // Nothing to write answers 0 before the IO is taken, as CRuby's
    // `io_write` returns for an empty String, so no empty datagram leaves.
    if bytes.is_empty() {
        return Ok(0.into_value(mrb));
    }
    let sent = ready_of(writer_of(mrb, self_)?.send(mrb, &bytes, Wait::Blocking, None)?);
    Ok((sent as i64).into_value(mrb))
}

/// `write_nonblock(string, exception: true)` sends what there is room for at
/// once, answering how many bytes it sent; with no room it raises
/// `IO::EAGAINWaitWritable`, or answers `:wait_writable` with
/// `exception: false`, as CRuby's does.
fn socket_write_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option_of(mrb, args)?;
    let [string] = args else {
        return Err(Error::argnum(mrb, args.len(), 1, 1));
    };
    let bytes = written_bytes_of(mrb, *string)?;
    match writer_of(mrb, self_)?.send(mrb, &bytes, Wait::DontWait, None)? {
        Poll::Ready(sent) => Ok((sent as i64).into_value(mrb)),
        Poll::Pending => would_wait(mrb, Direction::Write, "write would block", exception),
    }
}

/// `syswrite(string)` sends every byte of `string` past any buffer,
/// answering how many it sent.
fn socket_syswrite(mrb: &Mrb, self_: Value, string: Value) -> Result<Value, Error> {
    let bytes = written_bytes_of(mrb, string)?;
    let sent = ready_of(writer_of(mrb, self_)?.send(mrb, &bytes, Wait::Blocking, None)?);
    Ok((sent as i64).into_value(mrb))
}

/// `value` as CRuby's `StringValue` takes it: a String, or what its `to_str`
/// answers; anything else raises `TypeError`.
pub(crate) fn string_value_of(mrb: &Mrb, value: Value) -> Result<RString, Error> {
    check_string_type(mrb, value)?.ok_or_else(|| errno::type_error(mrb, value))
}

/// `value` as CRuby's `rb_check_string_type` takes it: a String, or what its
/// `to_str` answers, `None` for a value with no `to_str`; a `to_str` answering
/// no String raises `TypeError`.
pub(super) fn check_string_type(mrb: &Mrb, value: Value) -> Result<Option<RString>, Error> {
    if let Some(string) = RString::from_value(value) {
        return Ok(Some(string));
    }
    let to_str = Symbol::new(mrb, "to_str")?.as_value();
    if !value.funcall::<_, _, bool>(mrb, c"respond_to?", [to_str])? {
        return Ok(None);
    }
    let string = value.funcall::<_, _, Value>(mrb, c"to_str", ())?;
    RString::from_value(string)
        .map(Some)
        .ok_or_else(|| errno::type_error(mrb, value))
}

/// The bytes of `value` as CRuby's `StringValue` takes it.
pub(super) fn string_bytes_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    let string = string_value_of(mrb, value)?;
    // SAFETY: the bytes are copied out before any later mruby call.
    Ok(unsafe { string.as_slice(mrb) }.to_vec())
}

/// The bytes of `mesg`, as CRuby's `send` takes them through `StringValue`.
/// The buffer a read is given, `nil` naming none, read as CRuby's
/// `io_setstrbuf` reads it with `StringValue` before any byte is taken, so a
/// buffer that is no String raises `TypeError` and leaves what came in place.
pub(super) fn outbuf_of(mrb: &Mrb, outbuf: Option<Value>) -> Result<Option<Value>, Error> {
    outbuf
        .filter(|buf| !buf.is_nil())
        .map(|buf| string_value_of(mrb, buf).map(|buf| buf.as_value()))
        .transpose()
}

pub(super) fn mesg_bytes_of(mrb: &Mrb, mesg: Value) -> Result<Vec<u8>, Error> {
    let mesg = string_value_of(mrb, mesg)?;
    // SAFETY: the bytes are copied before the next mruby call.
    Ok(unsafe { mesg.as_slice(mrb) }.to_vec())
}

/// The `sockaddr` a send's `dest` names, as CRuby's `SockAddrStringValue`
/// reads it: an `Addrinfo`'s own, or a String.
pub(super) fn dest_sockaddr_of(mrb: &Mrb, dest: Value) -> Result<Value, Error> {
    match super::addrinfo::Addrinfo::of(mrb, dest) {
        Ok(_) => dest.funcall(mrb, c"to_sockaddr", ()),
        Err(_) => string_value_of(mrb, dest).map(|string| string.as_value()),
    }
}

/// The flags a send or a receive takes, read as CRuby's `NUM2INT` reads
/// them: a Float drops its fraction, and NaN, an infinity, or a number past
/// C's `int` raises `RangeError`.
pub(super) fn flags_of(mrb: &Mrb, flags: Value) -> Result<i64, Error> {
    let whole = match f64::from_value(flags) {
        Some(float) if float.abs() < i64::MAX as f64 => {
            Integer::try_convert((float.trunc() as i64).into_value(mrb), mrb)?
        }
        Some(_) => {
            let shown = flags
                .funcall::<_, _, Value>(mrb, c"to_s", ())?
                .to_r_string(mrb)?;
            let message = format!("float {} out of range of integer", shown.to_string(mrb)?);
            return Err(Error::new(mrb, mrb.exception_range_error()?, &message));
        }
        None => Integer::try_convert(flags, mrb)?,
    };
    Ok(i64::from(whole.to_i32(mrb)?))
}

/// `send(mesg, flags[, dest])` sends every byte of `mesg` past any buffer,
/// waiting for room, as CRuby's `BasicSocket#send`: `dest`, an `Addrinfo` or
/// a String, changes nothing on a connected stream, as Linux's TCP ignores
/// one, and the flags read as `NUM2INT` reads them after the open file. A
/// shut writing side raises `Errno::EPIPE` naming `send(2)`, or `sendto(2)`
/// with a `dest`, as CRuby's does.
fn socket_send(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (mesg, flags, dest) = match args {
        [mesg, flags] => (*mesg, *flags, None),
        [mesg, flags, dest] => (*mesg, *flags, Some(*dest)),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 3)),
    };
    let send = send_args_of(mrb, self_, mesg, flags, dest, false)?;
    let sent = ready_of(
        send.socket
            .send(mrb, &send.bytes, Wait::Blocking, Some(send.call))?,
    );
    Ok((sent as i64).into_value(mrb))
}

/// What a send reads of its arguments, in CRuby's `rsock_bsock_send` order.
pub(super) struct SendArgs<'a> {
    pub(super) bytes: Vec<u8>,
    /// The `dest` sockaddr String, once it converted.
    pub(super) dest: Option<Value>,
    pub(super) socket: &'a dyn SocketIo,
    /// The call its `Errno` names: `sendto(2)` once a destination is named.
    pub(super) call: &'static str,
}

/// Read a send's arguments as CRuby's `rsock_bsock_send` reads them: the
/// message as `StringValue`, a `dest` as a sockaddr String, the open socket,
/// then the flags as `NUM2INT`, refusing one WASI carries none of. A send
/// `addressed` some other way names `sendto(2)` too.
pub(super) fn send_args_of<'a>(
    mrb: &Mrb,
    self_: Value,
    mesg: Value,
    flags: Value,
    dest: Option<Value>,
    addressed: bool,
) -> Result<SendArgs<'a>, Error> {
    let bytes = mesg_bytes_of(mrb, mesg)?;
    let dest = dest.map(|dest| dest_sockaddr_of(mrb, dest)).transpose()?;
    let call = match addressed || dest.is_some() {
        true => "sendto(2)",
        false => "send(2)",
    };
    let socket = open_socket_of(mrb, self_)?;
    ensure_flags_carried(mrb, flags_of(mrb, flags)?, call)?;
    Ok(SendArgs {
        bytes,
        dest,
        socket,
        call,
    })
}

/// Linux's `MSG_DONTWAIT`, the one flag a guest's receive or send takes; it
/// waits all the same, as CRuby's `recv` waits again on `EAGAIN`.
const MSG_DONTWAIT: i64 = 0x40;

/// Refuse a `flags` WASI carries none of with `Errno::ENOTSUP`, naming
/// `call`, as a receive and a send both take only `MSG_DONTWAIT`.
pub(super) fn ensure_flags_carried(mrb: &Mrb, flags: i64, call: &str) -> Result<(), Error> {
    match flags & !MSG_DONTWAIT {
        0 => Ok(()),
        _ => Err(errno::error_by_name(mrb, c"ENOTSUP", call)),
    }
}

/// What a receive past a socket's buffer takes, as CRuby's
/// `rsock_s_recvfrom` reads it before it takes the open socket: `flags` as
/// `NUM2INT`, then `maxlen`, and an `outbuf` as `StringValue`.
pub(super) struct DirectArgs {
    pub(super) maxlen: usize,
    flags: i64,
    pub(super) outbuf: Option<Value>,
}

pub(super) fn direct_args_of(mrb: &Mrb, args: &[Value]) -> Result<DirectArgs, Error> {
    let (maxlen, flags, outbuf) = match args {
        [maxlen] => (*maxlen, None, None),
        [maxlen, flags] => (*maxlen, Some(*flags), None),
        [maxlen, flags, outbuf] => (*maxlen, Some(*flags), Some(*outbuf)),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 3)),
    };
    let flags = match flags.filter(|flags| !flags.is_nil()) {
        Some(flags) => flags_of(mrb, flags)?,
        None => 0,
    };
    let maxlen = strbuf_length_of(mrb, maxlen)?;
    Ok(DirectArgs {
        maxlen,
        flags,
        outbuf: outbuf_of(mrb, outbuf)?,
    })
}

impl DirectArgs {
    /// Refuse a read `source` cannot take past its buffer: bytes a read held
    /// back raise `IOError` naming `refusal`, as CRuby's, and then a flag WASI
    /// carries none of raises `Errno::ENOTSUP`.
    pub(super) fn ensure_takes(
        &self,
        mrb: &Mrb,
        source: &dyn SocketIo,
        refusal: &str,
    ) -> Result<(), Error> {
        if !source.io().rbuf.borrow().is_empty() {
            return Err(errno::io_error(mrb, refusal));
        }
        ensure_flags_carried(mrb, self.flags, "recvfrom(2)")
    }
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
    // A number reads as `NUM2INT` reads it, past `int` a RangeError.
    match flags_of(mrb, how)? {
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

/// `close_read` shuts the reading side and refuses a later read, or closes
/// the socket when its writing side is closed already, as CRuby's
/// `bsock_close_read` does; the shutdown's own failure is no answer, as
/// CRuby ignores it.
fn socket_close_read(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    close_side(mrb, self_, Shutdown::Read)
}

/// `close_write` sends the FIN once what was written is sent and refuses a
/// later write, or closes the socket when its reading side is closed already,
/// as CRuby's `bsock_close_write` does.
fn socket_close_write(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    close_side(mrb, self_, Shutdown::Write)
}

/// Close the side `how` names, `Read` or `Write`, as `bsock_close_read` and
/// `bsock_close_write` do alike.
fn close_side(mrb: &Mrb, self_: Value, how: Shutdown) -> Result<Value, Error> {
    let source = open_socket_of(mrb, self_)?;
    let io = source.io();
    let (own, other) = match how {
        Shutdown::Read => (&io.read_closed, &io.write_closed),
        _ => (&io.write_closed, &io.read_closed),
    };
    if other.get() {
        source.close();
        return Ok(qnil().as_value());
    }
    let _ = source.shutdown(mrb, how);
    own.set(true);
    Ok(qnil().as_value())
}

/// `getsockname` answers the socket's own end packed as a `sockaddr_in`.
fn socket_getsockname(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (ip, port) = open_socket_of(mrb, self_)?.local_end(mrb)?;
    Ok(mrb.str_new(&to_sockaddr(ip.into(), port)).as_value())
}

/// `getpeername` answers the peer packed as a `sockaddr_in`.
fn socket_getpeername(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (ip, port) = open_socket_of(mrb, self_)?.peer_end(mrb)?;
    Ok(mrb.str_new(&to_sockaddr(ip.into(), port)).as_value())
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
    let past = PastBuffer::new(Wait::Blocking, false);
    receive_past_buffer(mrb, self_, args, past)
}

/// How a receive past the buffer reads: whether it waits, and whether it
/// answers the sender too.
pub(super) struct PastBuffer {
    wait: Wait,
    exception: bool,
    with_sender: bool,
}

impl PastBuffer {
    pub(super) fn new(wait: Wait, with_sender: bool) -> Self {
        Self {
            wait,
            exception: true,
            with_sender,
        }
    }

    /// The refusal a buffer holding bytes raises, which CRuby's blocking
    /// `rsock_s_recvfrom` names `recv` and its non-blocking one `recvfrom`.
    fn buffered_refusal(&self) -> &'static str {
        match self.wait {
            Wait::Blocking => "recv for buffered IO",
            _ => "recvfrom for buffered IO",
        }
    }

    /// The same read, answering `:wait_readable` rather than raising when
    /// it would wait and `exception` is false.
    pub(super) fn with_exception(self, exception: bool) -> Self {
        Self { exception, ..self }
    }
}

/// What `recv`, `recv_nonblock`, `recvfrom`, and `recvfrom_nonblock` answer,
/// as CRuby's one `rsock_s_recvfrom` serves them: up to `maxlen` of what
/// comes next into `outbuf`, past the buffer, which raises `IOError` while it
/// holds bytes; `nil` at a stream's end. With the sender, the answer is the
/// message and the sender's address, `nil` for a stream.
pub(super) fn receive_past_buffer(
    mrb: &Mrb,
    self_: Value,
    args: &[Value],
    past: PastBuffer,
) -> Result<Value, Error> {
    let direct = direct_args_of(mrb, args)?;
    let source = open_socket_of(mrb, self_)?;
    direct.ensure_takes(mrb, source, past.buffered_refusal())?;
    let (maxlen, outbuf) = (direct.maxlen, direct.outbuf);
    // Every receive is CRuby's one `recvfrom(2)`.
    let call = "recvfrom(2)";
    let Poll::Ready(received) = source.receive_from(mrb, maxlen, past.wait, call)? else {
        let blocked = format!("{call} would block");
        return would_wait(mrb, Direction::Read, &blocked, past.exception);
    };
    let Some((bytes, sender)) = received else {
        return Ok(qnil().as_value());
    };
    let mesg = fill(mrb, outbuf, mrb.str_new(&bytes).as_value())?;
    if !past.with_sender {
        return Ok(mesg);
    }
    let sender = match sender {
        Some(sender) => to_addr_array(mrb, sender)?,
        None => qnil().as_value(),
    };
    Ok(mrb.ary_new_from_values(&[mesg, sender]).as_value())
}

/// `sysread(maxlen, outbuf = nil)` answers up to `maxlen` of what comes next,
/// waiting for some, past any buffer; it raises `EOFError` at the end. Asked
/// for nothing it answers `""` before it takes the socket, as CRuby's
/// `rb_io_sysread` does.
fn socket_sysread(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let args = match args {
        [maxlen] => [*maxlen, qnil().as_value(), qnil().as_value()],
        [maxlen, outbuf] => [*maxlen, qnil().as_value(), *outbuf],
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let direct = direct_args_of(mrb, &args)?;
    let (maxlen, outbuf) = (direct.maxlen, direct.outbuf);
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    let source = reader_of(mrb, self_)?;
    direct.ensure_takes(mrb, source, "sysread for buffered IO")?;
    match ready_of(source.receive(mrb, maxlen, Wait::Blocking)?) {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Err(eof_error(mrb)),
    }
}

/// `recv_nonblock(maxlen, flags = 0, outbuf = nil, exception: true)` answers
/// up to `maxlen` of what has come, `nil` at the end. With nothing come yet it
/// raises `IO::EAGAINWaitReadable`, or answers `:wait_readable` with
/// `exception: false`. Bytes a read held back raise `IOError`, as CRuby's.
fn socket_recv_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option_of(mrb, args)?;
    let past = PastBuffer::new(Wait::DontWait, false).with_exception(exception);
    receive_past_buffer(mrb, self_, args, past)
}

/// A call a socket has no position for raises `Errno::ESPIPE`, as Linux's
/// `lseek`, `pread`, and `pwrite` answer one; a closed socket raises
/// `IOError` first, as CRuby checks the stream before the call.
fn refuse_position(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    open_socket_of(mrb, self_)?;
    Err(errno::error_alone(mrb, c"ESPIPE"))
}

/// `pos=(offset)`, refused as every position call is.
fn refuse_set_position(mrb: &Mrb, self_: Value, _offset: Value) -> Result<Value, Error> {
    refuse_position(mrb, self_)
}

/// `pwrite(string, offset)`, refused as every position call is.
fn refuse_pwrite(mrb: &Mrb, self_: Value, _string: Value, _offset: Value) -> Result<Value, Error> {
    refuse_position(mrb, self_)
}

/// `seek(offset, whence = SEEK_SET)`, its arity checked first, as CRuby's.
fn refuse_seek(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if !(1..=2).contains(&args.len()) {
        return Err(Error::argnum(mrb, args.len(), 1, 2));
    }
    refuse_position(mrb, self_)
}

/// `pread(maxlen, offset, outbuf = nil)`, its arity checked first.
fn refuse_pread(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if !(2..=3).contains(&args.len()) {
        return Err(Error::argnum(mrb, args.len(), 2, 3));
    }
    refuse_position(mrb, self_)
}

/// A socket keeps nothing to sync, so `fsync` raises `Errno::EINVAL`, as
/// Linux's answers it; a closed socket raises `IOError` first.
fn refuse_sync(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    open_socket_of(mrb, self_)?;
    Err(errno::error_alone(mrb, c"EINVAL"))
}

/// `advise(advice, offset = 0, len = 0)` checks the advice as a file's does,
/// then is taken and ignored, as Linux's `fadvise` takes advice on a socket;
/// past the advice, a closed socket raises `IOError`, as CRuby's checks.
fn socket_advise(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let Some(advice) = args.first().filter(|_| args.len() <= 3) else {
        return Err(Error::argnum(mrb, args.len(), 1, 3));
    };
    crate::stream::advice_of(mrb, *advice)?;
    open_socket_of(mrb, self_)?;
    // The offset and the length read as `NUM2OFFT`, after the open IO.
    for number in args[1..].iter().filter(|number| !number.is_nil()) {
        i64::try_convert(*number, mrb)?;
    }
    Ok(qnil().as_value())
}

/// `sync` and `sync=` read and set the socket's own flag; a closed socket
/// raises `IOError`, as CRuby's take the open IO first.
fn socket_sync(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(open_socket_of(mrb, self_)?.io().sync.get())
}

fn socket_sync_set(mrb: &Mrb, self_: Value, sync: Value) -> Result<Value, Error> {
    open_socket_of(mrb, self_)?.io().sync.set(sync.to_bool());
    Ok(sync)
}

/// When a wait on the socket `value` ends, `None` when `value` is no socket.
/// A closed socket, or a side `close_read` or `close_write` closed, raises
/// `IOError`, as CRuby checks an IO before it waits on it.
pub(crate) fn readiness_of(
    mrb: &Mrb,
    value: Value,
    direction: Direction,
) -> Result<Option<Readiness>, Error> {
    if source_of(mrb, value).is_err() {
        return Ok(None);
    }
    let source = match direction {
        Direction::Read => reader_of(mrb, value)?,
        Direction::Write => writer_of(mrb, value)?,
    };
    source.readiness(mrb, direction).map(Some)
}

/// Whether `value` is a socket a select may wait on; a closed one raises
/// `IOError`, as CRuby's `select` checks each IO open before it waits.
pub(crate) fn is_open_socket(mrb: &Mrb, value: Value) -> Result<bool, Error> {
    if source_of(mrb, value).is_err() {
        return Ok(false);
    }
    open_socket_of(mrb, value).map(|_| true)
}

/// When a select's wait on the socket `value` ends. Only a closed socket
/// raises `IOError`, as CRuby's `select` checks nothing of a stream's modes.
pub(crate) fn select_readiness_of(
    mrb: &Mrb,
    value: Value,
    direction: Direction,
) -> Result<Readiness, Error> {
    open_socket_of(mrb, value)?.readiness(mrb, direction)
}
