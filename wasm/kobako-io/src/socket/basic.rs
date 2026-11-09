//! `BasicSocket`, what every socket answers as an IO, as CRuby's
//! `ext/socket/basicsocket.c` stands over `io.c`: the socket type's own
//! receiving and sending, and the calls that reach them past the buffer.

use std::cell::{Cell, RefCell};
use std::task::Poll;

use beni::prelude::*;
use beni::value::qnil;
use beni::{
    Error, IntoValue, Mrb, Qfalse, Qtrue, RArray, RClass, RHash, Symbol, TryConvert, Value,
};

use super::buffer::fill;
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

/// What a `BasicSocket` call reaches: the socket's receiving and sending, as
/// Linux's `proto_ops` gives each socket type its own, and the read buffer
/// CRuby keeps before it, whose bytes come first.
pub(super) trait SocketIo {
    /// What a read took past what it answered.
    fn held(&self) -> &RefCell<Vec<u8>>;

    /// Up to `max` of the next bytes the host brings, `None` at the end;
    /// `Pending` when none have come and `wait` does not wait.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error>;

    /// Send `bytes`, all of them when `wait` waits, else what there is room
    /// for, answering how many were sent; `Pending` when there is no room and
    /// `wait` does not wait.
    fn send(&self, mrb: &Mrb, bytes: &[u8], wait: Wait) -> Result<Poll<usize>, Error>;

    /// Whether a write is sent at once, as `IO#sync` reads; a socket's is.
    fn sync(&self) -> &Cell<bool>;
}

/// What a call that waits answers, which it does only once it is ready.
pub(super) fn waited<T>(poll: Poll<T>) -> T {
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
fn bytes_of(mrb: &Mrb, object: Value) -> Result<Vec<u8>, Error> {
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

/// `write(*objects)` sends what the objects spell, answering how many bytes
/// it sent, as CRuby's `IO#write` joins them.
fn socket_write(mrb: &Mrb, self_: Value, objects: &[Value]) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    for object in objects {
        bytes.extend(bytes_of(mrb, *object)?);
    }
    let sent = waited(source_of(mrb, self_)?.send(mrb, &bytes, Wait::Block)?);
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
    let bytes = bytes_of(mrb, *string)?;
    match source_of(mrb, self_)?.send(mrb, &bytes, Wait::DontWait)? {
        Poll::Ready(sent) => Ok((sent as i64).into_value(mrb)),
        Poll::Pending => would_wait(mrb, Direction::Write, "write would block", exception),
    }
}

/// `syswrite(string)` sends every byte of `string` past any buffer,
/// answering how many it sent.
fn socket_syswrite(mrb: &Mrb, self_: Value, string: Value) -> Result<Value, Error> {
    let bytes = bytes_of(mrb, string)?;
    let sent = waited(source_of(mrb, self_)?.send(mrb, &bytes, Wait::Block)?);
    Ok((sent as i64).into_value(mrb))
}

/// Linux's `MSG_DONTWAIT`, the one receive flag a guest's receive takes; it
/// waits all the same, as CRuby's `recv` waits again on `EAGAIN`.
const MSG_DONTWAIT: i64 = 0x40;

/// What a receive past the buffer takes: `maxlen`, `flags`, and an `outbuf`,
/// refusing a flag WASI carries none of with `Errno::ENOTSUP`. Bytes a read
/// held back raise `IOError` naming `refusal`, as CRuby's.
pub(super) fn direct_args<'a>(
    mrb: &Mrb,
    self_: Value,
    args: &[Value],
    refusal: &str,
) -> Result<(&'a dyn SocketIo, usize, Option<Value>), Error> {
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
    let source = source_of(mrb, self_)?;
    if !source.held().borrow().is_empty() {
        return Err(errno::io_error(mrb, refusal));
    }
    Ok((source, maxlen, outbuf))
}

/// `recv(maxlen, flags = 0, outbuf = nil)` answers up to `maxlen` of what
/// comes next, waiting for some, past any buffer; `nil` at the end.
fn socket_recv(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (source, maxlen, outbuf) = direct_args(mrb, self_, args, "recv for buffered IO")?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match waited(source.receive(mrb, maxlen, Wait::Block)?) {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => fill(mrb, outbuf, qnil().as_value()),
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
    let (source, maxlen, outbuf) = direct_args(mrb, self_, &args, "sysread for buffered IO")?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match waited(source.receive(mrb, maxlen, Wait::Block)?) {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Err(Error::new(
            mrb,
            mrb.exc_get(c"EOFError")?,
            "end of file reached",
        )),
    }
}

/// `recv_nonblock(maxlen, flags = 0, outbuf = nil, exception: true)` answers
/// up to `maxlen` of what has come, `nil` at the end. With nothing come yet it
/// raises `IO::EAGAINWaitReadable`, or answers `:wait_readable` with
/// `exception: false`. Bytes a read held back raise `IOError`, as CRuby's.
fn socket_recv_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option(mrb, args)?;
    let (source, maxlen, outbuf) = direct_args(mrb, self_, args, "recvfrom for buffered IO")?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match source.receive(mrb, maxlen, Wait::DontWait)? {
        Poll::Ready(Some(bytes)) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        Poll::Ready(None) => fill(mrb, outbuf, qnil().as_value()),
        Poll::Pending => would_wait(mrb, Direction::Read, "recvfrom(2) would block", exception),
    }
}

/// A call a socket has no position for raises `Errno::ESPIPE`, as Linux's
/// `lseek`, `pread`, and `pwrite` answer one.
fn socket_unseekable(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    Err(errno::error_alone(mrb, c"ESPIPE"))
}

/// A socket keeps nothing to sync, so `fsync` raises `Errno::EINVAL`, as
/// Linux's answers it.
fn socket_unsyncable(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Err(errno::error_alone(mrb, c"EINVAL"))
}

/// `advise(advice, offset = 0, len = 0)` checks the advice as a file's does,
/// then is taken and ignored, as Linux's `fadvise` takes advice on a socket.
fn socket_advise(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let Some(advice) = args.first().filter(|_| args.len() <= 3) else {
        return Err(Error::argnum(mrb, args.len(), 1, 3));
    };
    crate::stream::advice_of(mrb, *advice)?;
    Ok(qnil().as_value())
}

fn socket_sync(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(source_of(mrb, self_)?.sync().get())
}

fn socket_sync_set(mrb: &Mrb, self_: Value, sync: Value) -> Result<Value, Error> {
    source_of(mrb, self_)?.sync().set(sync.to_bool());
    Ok(sync)
}

/// Whether `value` is a `TCPSocket` or a `UDPSocket`.
pub(crate) fn is_socket(mrb: &Mrb, value: Value) -> bool {
    source_of(mrb, value).is_ok()
}
