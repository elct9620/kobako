//! The reads that go through the buffer CRuby's `io.c` keeps before a
//! socket: what an earlier read held back comes first, and what a read took
//! past what it answered stays for the next.

use std::task::Poll;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, Integer, Mrb, Proc, RArray, RClass, RHash, Symbol, TryConvert, Value};

use super::basic::{exception_option, length_of, source_of, waited, SocketIo, Wait};
use crate::io::{argument_error, would_wait, Direction};

/// The most a read through the buffer asks of one receive: the size of the
/// host's socket buffer, and past the largest IPv4 datagram, so a datagram
/// comes whole.
const READ_CHUNK: usize = 64 * 1024;

pub(super) fn init(mrb: &Mrb, basic: RClass) -> Result<(), Error> {
    basic.define_method(mrb, c"read", beni::method!(socket_read, -1))?;
    basic.define_method(mrb, c"readpartial", beni::method!(socket_readpartial, -1))?;
    basic.define_method(
        mrb,
        c"read_nonblock",
        beni::method!(socket_read_nonblock, -1),
    )?;
    basic.define_method(mrb, c"gets", beni::method!(socket_gets, -1))?;
    basic.define_method(mrb, c"each_line", beni::method!(socket_each_line, -1))?;
    basic.define_method(mrb, c"readlines", beni::method!(socket_readlines, -1))?;
    basic.define_method(mrb, c"eof?", beni::method!(socket_eof_p, 0))?;
    basic.define_method(mrb, c"eof", beni::method!(socket_eof_p, 0))?;
    Ok(())
}

/// Up to `len` bytes, what an earlier read held back first, else what the
/// host brings next, holding back what the read did not take; `None` at the
/// end, and `Pending` when nothing came and `wait` does not wait.
fn read_some(
    mrb: &Mrb,
    source: &dyn SocketIo,
    len: usize,
    wait: Wait,
) -> Result<Poll<Option<Vec<u8>>>, Error> {
    let mut held = source.held().borrow_mut();
    if held.is_empty() {
        drop(held);
        let bytes = match source.receive(mrb, READ_CHUNK, wait)? {
            Poll::Ready(Some(bytes)) => bytes,
            Poll::Ready(None) => return Ok(Poll::Ready(None)),
            Poll::Pending => return Ok(Poll::Pending),
        };
        held = source.held().borrow_mut();
        *held = bytes;
    }
    let take = held.len().min(len);
    Ok(Poll::Ready(Some(held.drain(..take).collect())))
}

/// `value`, a `String` buffer a read is given, made to hold `read`, or
/// emptied when the read answers `nil`, as CRuby fills `outbuf`.
pub(super) fn fill(mrb: &Mrb, outbuf: Option<Value>, read: Value) -> Result<Value, Error> {
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
        Some(v) => Some(length_of(mrb, v)?),
        None => None,
    };
    let source = source_of(mrb, self_)?;
    let mut read = Vec::new();
    while length.is_none_or(|length| read.len() < length) {
        let want = length.map_or(READ_CHUNK, |length| length - read.len());
        match waited(read_some(mrb, source, want, Wait::Block)?) {
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
    read_partial(mrb, self_, args, Wait::Block, true)
}

/// `read_nonblock(maxlen, outbuf = nil, exception: true)` answers as
/// `readpartial` does, at once: with nothing come yet it raises
/// `IO::EAGAINWaitReadable`. With `exception: false` it answers
/// `:wait_readable` instead, and `nil` at the end.
fn socket_read_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option(mrb, args)?;
    read_partial(mrb, self_, args, Wait::DontWait, exception)
}

/// What one read brings, as CRuby's `io_getpartial` serves `readpartial` and
/// `read_nonblock` alike, apart from whether it waits.
fn read_partial(
    mrb: &Mrb,
    self_: Value,
    args: &[Value],
    wait: Wait,
    exception: bool,
) -> Result<Value, Error> {
    let (maxlen, outbuf) = match args {
        [maxlen] => (*maxlen, None),
        [maxlen, outbuf] => (*maxlen, Some(*outbuf).filter(|buf| !buf.is_nil())),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let maxlen = length_of(mrb, maxlen)?;
    if maxlen == 0 {
        return fill(mrb, outbuf, mrb.str_new(b"").as_value());
    }
    match read_some(mrb, source_of(mrb, self_)?, maxlen, wait)? {
        Poll::Ready(Some(bytes)) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        Poll::Ready(None) if !exception => fill(mrb, outbuf, qnil().as_value()),
        Poll::Ready(None) => Err(Error::new(
            mrb,
            mrb.exc_get(c"EOFError")?,
            "end of file reached",
        )),
        Poll::Pending => would_wait(mrb, Direction::Read, "read would block", exception),
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
        match waited(read_some(mrb, source, READ_CHUNK, Wait::Block)?) {
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
        match waited(source.receive(mrb, READ_CHUNK, Wait::Block)?) {
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
    match waited(source.receive(mrb, READ_CHUNK, Wait::Block)?) {
        Some(bytes) => {
            *source.held().borrow_mut() = bytes;
            Ok(false)
        }
        None => Ok(true),
    }
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
