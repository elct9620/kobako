//! `BasicSocket`, what every socket answers as an IO, as CRuby's
//! `ext/socket/basicsocket.c` stands over `io.c`: reads through a buffer
//! before the socket.

use std::cell::{Cell, RefCell};

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{
    Error, Integer, IntoValue, Mrb, Proc, RArray, RClass, RHash, Symbol, TryConvert, Value,
};

use super::tcp::Connection;
use crate::errno;
use crate::io::argument_error;

/// The most a read without a length asks of one blocking read, the size of
/// the host's socket buffer.
pub(super) const READ_CHUNK: usize = 64 * 1024;

/// What a `BasicSocket` call reaches: the socket's receiving and sending, as
/// Linux's `proto_ops` gives each socket type its own, and the read buffer
/// CRuby keeps before it, whose bytes come first.
pub(super) trait SocketIo {
    /// What a read took past what it answered.
    fn held(&self) -> &RefCell<Vec<u8>>;

    /// The next bytes the host brings, waiting for some; `None` at the end.
    fn receive(&self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error>;

    /// Send `bytes`, waiting for room, answering how many were sent.
    fn send(&self, mrb: &Mrb, bytes: &[u8]) -> Result<usize, Error>;

    /// Whether a write is sent at once, as `IO#sync` reads; a socket's is.
    fn sync(&self) -> &Cell<bool>;
}

/// The socket `value` stands for, a `TCPSocket` or a `UDPSocket`.
fn source_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
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
    basic.define_method(mrb, c"read", beni::method!(socket_read, -1))?;
    basic.define_method(mrb, c"readpartial", beni::method!(socket_readpartial, -1))?;
    basic.define_method(mrb, c"gets", beni::method!(socket_gets, -1))?;
    basic.define_method(mrb, c"each_line", beni::method!(socket_each_line, -1))?;
    basic.define_method(mrb, c"readlines", beni::method!(socket_readlines, -1))?;
    basic.define_method(mrb, c"eof?", beni::method!(socket_eof_p, 0))?;
    basic.define_method(mrb, c"eof", beni::method!(socket_eof_p, 0))?;
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
        let data = object
            .funcall::<_, _, Value>(mrb, c"to_s", ())?
            .to_r_string(mrb)?;
        // SAFETY: the bytes are copied before the next mruby call.
        bytes.extend_from_slice(unsafe { data.as_bytes(mrb) });
    }
    let sent = source_of(mrb, self_)?.send(mrb, &bytes)?;
    Ok((sent as i64).into_value(mrb))
}

/// Up to `len` bytes, what an earlier read held back first, else what the
/// host brings next, holding back what the read did not take; `None` at the
/// end.
fn read_some(mrb: &Mrb, source: &dyn SocketIo, len: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut held = source.held().borrow_mut();
    if held.is_empty() {
        drop(held);
        let Some(bytes) = source.receive(mrb)? else {
            return Ok(None);
        };
        held = source.held().borrow_mut();
        *held = bytes;
    }
    let take = held.len().min(len);
    Ok(Some(held.drain(..take).collect()))
}

/// `value`, a `String` buffer a read is given, made to hold `read`, or
/// emptied when the read answers `nil`, as CRuby fills `outbuf`.
fn fill(mrb: &Mrb, outbuf: Option<Value>, read: Value) -> Result<Value, Error> {
    let Some(outbuf) = outbuf else {
        return Ok(read);
    };
    let filling = match read.is_nil() {
        true => mrb.str_new(b"").as_value(),
        false => read,
    };
    outbuf.funcall::<_, _, Value>(mrb, c"replace", [filling])?;
    Ok(match read.is_nil() {
        true => read,
        false => outbuf,
    })
}

/// `read(length = nil, outbuf = nil)` reads `length` bytes, or to the end
/// without one, into `outbuf` when given, as CRuby's `IO#read` does; `nil` at
/// the end when a length was asked for.
fn socket_read(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (length, outbuf) = match args {
        [] => (None, None),
        [length] => (Some(*length), None),
        [length, outbuf] => (Some(*length), Some(*outbuf).filter(|buf| !buf.is_nil())),
        _ => return Err(Error::argnum(mrb, args.len(), 0, 2)),
    };
    let length = match length.filter(|v| !v.is_nil()) {
        Some(v) => Some(
            usize::try_from(i64::try_convert(v, mrb)?)
                .map_err(|_| argument_error(mrb, "negative length given"))?,
        ),
        None => None,
    };
    let source = source_of(mrb, self_)?;
    let mut read = Vec::new();
    while length.is_none_or(|length| read.len() < length) {
        let want = length.map_or(READ_CHUNK, |length| length - read.len());
        match read_some(mrb, source, want)? {
            Some(bytes) => read.extend(bytes),
            None => break,
        }
    }
    let answer = match read.is_empty() && length.is_some_and(|length| length > 0) {
        true => qnil().as_value(),
        false => mrb.str_new(&read).as_value(),
    };
    fill(mrb, outbuf, answer)
}

/// `readpartial(maxlen, outbuf = nil)` answers what one read brings, into
/// `outbuf` when given, raising `EOFError` at the end, as CRuby does.
fn socket_readpartial(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (maxlen, outbuf) = match args {
        [maxlen] => (*maxlen, None),
        [maxlen, outbuf] => (*maxlen, Some(*outbuf).filter(|buf| !buf.is_nil())),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let maxlen = usize::try_from(i64::try_convert(maxlen, mrb)?)
        .map_err(|_| argument_error(mrb, "negative length given"))?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match read_some(mrb, source_of(mrb, self_)?, maxlen)? {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Err(Error::new(
            mrb,
            mrb.exc_get(c"EOFError")?,
            "end of file reached",
        )),
    }
}

/// The next line, its separator kept unless `chomp`, or `None` at the end.
/// A limit of zero answers an empty line at once; an empty separator reads a
/// paragraph, skipping the newlines before it, as CRuby's `rspara` does.
fn next_line(
    mrb: &Mrb,
    source: &dyn SocketIo,
    (separator, limit, chomp): &LineArgs,
) -> Result<Option<Vec<u8>>, Error> {
    if *limit == Some(0) {
        return Ok(Some(Vec::new()));
    }
    let paragraph = separator.as_deref().is_some_and(<[u8]>::is_empty);
    if paragraph {
        skip_newlines(mrb, source)?;
    }
    let separator = match paragraph {
        true => Some(&b"\n\n"[..]),
        false => separator.as_deref(),
    };
    let mut line = Vec::new();
    loop {
        if let Some(end) = line_end_of(&line, separator, *limit) {
            let rest = line.split_off(end);
            source.held().borrow_mut().splice(0..0, rest);
            break;
        }
        match read_some(mrb, source, READ_CHUNK)? {
            Some(bytes) => line.extend(bytes),
            None => break,
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    if let Some(separator) = separator.filter(|_| *chomp) {
        if line.ends_with(separator) {
            line.truncate(line.len() - separator.len());
        }
    }
    Ok(Some(line))
}

/// Drop the newlines that stand next, reading on until another byte or the
/// end comes, as CRuby's `swallow` does before a paragraph.
fn skip_newlines(mrb: &Mrb, source: &dyn SocketIo) -> Result<(), Error> {
    loop {
        {
            let mut held = source.held().borrow_mut();
            let newlines = held.iter().take_while(|byte| **byte == b'\n').count();
            held.drain(..newlines);
            if !held.is_empty() {
                return Ok(());
            }
        }
        match source.receive(mrb)? {
            Some(bytes) => *source.held().borrow_mut() = bytes,
            None => return Ok(()),
        }
    }
}

/// A limit of zero raises `ArgumentError` for `name`, as CRuby's `readlines`
/// and `each_line` refuse one that would never end.
fn ensure_limit(mrb: &Mrb, (_, limit, _): &LineArgs, name: &str) -> Result<(), Error> {
    match limit {
        Some(0) => Err(argument_error(mrb, &format!("invalid limit: 0 for {name}"))),
        _ => Ok(()),
    }
}

/// `gets(sep = $/, limit = nil, chomp: false)` answers the next line, or
/// `nil` at the end.
fn socket_gets(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    Ok(match next_line(mrb, source_of(mrb, self_)?, &line_args)? {
        Some(line) => mrb.str_new(&line).as_value(),
        None => qnil().as_value(),
    })
}

/// `each_line(sep = $/, limit = nil, chomp: false) { |line| }` yields each
/// line to the end.
fn socket_each_line(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "each_line needs a block"))?;
    let line_args = line_args_of(mrb, args)?;
    ensure_limit(mrb, &line_args, "each_line")?;
    let source = source_of(mrb, self_)?;
    while let Some(line) = next_line(mrb, source, &line_args)? {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(&line).as_value()])?;
    }
    Ok(self_)
}

/// `readlines(sep = $/, limit = nil, chomp: false)` answers every line to the
/// end.
fn socket_readlines(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    ensure_limit(mrb, &line_args, "readlines")?;
    let source = source_of(mrb, self_)?;
    let lines = mrb.ary_new();
    while let Some(line) = next_line(mrb, source, &line_args)? {
        lines.push(mrb, mrb.str_new(&line).as_value())?;
    }
    Ok(lines.as_value())
}

/// `eof?` waits for the next bytes, holding them for the next read, and
/// answers whether the end came instead, as CRuby's fills its buffer.
fn socket_eof_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    let source = source_of(mrb, self_)?;
    if !source.held().borrow().is_empty() {
        return Ok(false);
    }
    match source.receive(mrb)? {
        Some(bytes) => {
            *source.held().borrow_mut() = bytes;
            Ok(false)
        }
        None => Ok(true),
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

/// What `gets` and its kin take: the separator, `None` to read to the end,
/// the limit, and whether to chomp the separator.
type LineArgs = (Option<Vec<u8>>, Option<usize>, bool);

/// `gets(sep = $/, limit = nil, chomp: false)`'s arguments; an empty
/// separator reads a paragraph, as CRuby's does.
fn line_args_of(mrb: &Mrb, args: &[Value]) -> Result<LineArgs, Error> {
    let (args, chomp) = match args.split_last() {
        Some((last, rest)) if RHash::try_convert(*last, mrb).is_ok() => {
            let options = RHash::try_convert(*last, mrb)?;
            let chomp: Value = options.aref(mrb, Symbol::new(mrb, "chomp")?.as_value())?;
            (rest, chomp.to_bool())
        }
        _ => (args, false),
    };
    let (separator, limit) = separator_and_limit_of(mrb, args)?;
    Ok((separator, limit, chomp))
}

/// `gets(sep = $/, limit = nil)`'s separator, `None` to read to the end, and
/// its limit.
fn separator_and_limit_of(
    mrb: &Mrb,
    args: &[Value],
) -> Result<(Option<Vec<u8>>, Option<usize>), Error> {
    let limit_of = |value: Value| -> Result<Option<usize>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        Ok(usize::try_from(i64::try_convert(value, mrb)?).ok())
    };
    let separator_of = |value: Value| -> Result<Option<Vec<u8>>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        let text = value.to_r_string(mrb)?;
        // SAFETY: the bytes are copied before the next mruby call.
        Ok(Some(unsafe { text.as_bytes(mrb) }.to_vec()))
    };
    match args {
        [] => Ok((Some(b"\n".to_vec()), None)),
        [only] if Integer::from_value(*only).is_some() => {
            Ok((Some(b"\n".to_vec()), limit_of(*only)?))
        }
        [separator] => Ok((separator_of(*separator)?, None)),
        [separator, limit] => Ok((separator_of(*separator)?, limit_of(*limit)?)),
        _ => Err(Error::argnum(mrb, args.len(), 0, 2)),
    }
}

/// Where the line `line` starts ends: past its separator, or at its limit,
/// whichever comes first; `None` while neither is in it.
fn line_end_of(line: &[u8], separator: Option<&[u8]>, limit: Option<usize>) -> Option<usize> {
    let by_separator = separator.and_then(|separator| {
        line.windows(separator.len())
            .position(|window| window == separator)
            .map(|at| at + separator.len())
    });
    let by_limit = limit.filter(|limit| line.len() >= *limit);
    match (by_separator, by_limit) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (end, None) | (None, end) => end,
    }
}

/// Whether `value` is a socket holding bytes a `gets` read past, which a read
/// answers before the socket is asked, as CRuby's read buffer is.
pub(crate) fn holds_buffered(mrb: &Mrb, value: Value) -> bool {
    source_of(mrb, value).is_ok_and(|source| !source.held().borrow().is_empty())
}
