//! The reads that go through the buffer CRuby's `io.c` keeps before a
//! socket: what an earlier read held back comes first, and what a read took
//! past what it answered stays for the next.

use std::task::Poll;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{
    Error, Integer, IntoValue, Mrb, Proc, RArray, RClass, RHash, RString, Symbol, TryConvert, Value,
};

use super::basic::{exception_option, length_of, reader_of, ready_of, source_of, SocketIo, Wait};
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
    basic.define_method(mrb, c"readline", beni::method!(socket_readline, -1))?;
    basic.define_method(mrb, c"getc", beni::method!(socket_getc, 0))?;
    basic.define_method(mrb, c"readchar", beni::method!(socket_readchar, 0))?;
    basic.define_method(mrb, c"getbyte", beni::method!(socket_getbyte, 0))?;
    basic.define_method(mrb, c"readbyte", beni::method!(socket_readbyte, 0))?;
    basic.define_method(mrb, c"each_char", beni::method!(socket_each_char, 0))?;
    basic.define_method(mrb, c"each_byte", beni::method!(socket_each_byte, 0))?;
    basic.define_method(mrb, c"ungetc", beni::method!(socket_ungetc, 1))?;
    basic.define_method(mrb, c"ungetbyte", beni::method!(socket_ungetbyte, 1))?;
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
    match refill(mrb, source, wait)? {
        Poll::Ready(true) => {}
        Poll::Ready(false) => return Ok(Poll::Ready(None)),
        Poll::Pending => return Ok(Poll::Pending),
    }
    let mut held = source.io().rbuf.borrow_mut();
    let take = held.len().min(len);
    Ok(Poll::Ready(Some(held.drain(..take).collect())))
}

/// Fill the buffer from the host when it holds nothing, as CRuby's
/// `io_fillbuf` does, answering whether bytes came before the end; `Pending`
/// when none have come and `wait` does not wait.
fn refill(mrb: &Mrb, source: &dyn SocketIo, wait: Wait) -> Result<Poll<bool>, Error> {
    if !source.io().rbuf.borrow().is_empty() {
        return Ok(Poll::Ready(true));
    }
    Ok(match source.receive(mrb, READ_CHUNK, wait)? {
        Poll::Ready(Some(bytes)) => {
            *source.io().rbuf.borrow_mut() = bytes;
            Poll::Ready(true)
        }
        Poll::Ready(None) => Poll::Ready(false),
        Poll::Pending => Poll::Pending,
    })
}

/// `outbuf`, a `String` buffer a read is given, made to hold `read`, or
/// emptied when the read answers `nil`, as CRuby fills it.
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
    let source = reader_of(mrb, self_)?;
    let mut read = Vec::new();
    while length.is_none_or(|length| read.len() < length) {
        let want = length.map_or(READ_CHUNK, |length| length - read.len());
        match ready_of(read_some(mrb, source, want, Wait::Block)?) {
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
    match read_some(mrb, reader_of(mrb, self_)?, maxlen, wait)? {
        Poll::Ready(Some(bytes)) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        Poll::Ready(None) if !exception => fill(mrb, outbuf, qnil().as_value()),
        Poll::Ready(None) => {
            fill(mrb, outbuf, qnil().as_value())?;
            Err(end_of_file(mrb))
        }
        Poll::Pending => would_wait(mrb, Direction::Read, "read would block", exception),
    }
}

/// The next line, its separator kept unless `chomp`, or `None` at the end.
/// A limit of zero answers an empty line at once; an empty separator reads a
/// paragraph, skipping the newlines before and after it, as CRuby's `rspara`
/// does.
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
            source.io().rbuf.borrow_mut().splice(0..0, rest);
            break;
        }
        match ready_of(read_some(mrb, source, READ_CHUNK, Wait::Block)?) {
            Some(bytes) => line.extend(bytes),
            None => break,
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    if paragraph {
        skip_newlines(mrb, source)?;
    }
    if let Some(separator) = separator.filter(|_| *chomp) {
        if line.ends_with(separator) {
            line.truncate(line.len() - separator.len());
        }
    }
    Ok(Some(line))
}

/// Drop the newlines that stand next, reading on until another byte or the
/// end comes, as CRuby's `swallow` does around a paragraph.
fn skip_newlines(mrb: &Mrb, source: &dyn SocketIo) -> Result<(), Error> {
    while ready_of(refill(mrb, source, Wait::Block)?) {
        let mut held = source.io().rbuf.borrow_mut();
        let newlines = held.iter().take_while(|byte| **byte == b'\n').count();
        held.drain(..newlines);
        if !held.is_empty() {
            break;
        }
    }
    Ok(())
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
    Ok(match next_line(mrb, reader_of(mrb, self_)?, &line_args)? {
        Some(line) => mrb.str_new(&line).as_value(),
        None => qnil().as_value(),
    })
}

/// The `EOFError` a `read` form raises at the end, as CRuby's.
pub(super) fn end_of_file(mrb: &Mrb) -> Error {
    match mrb.exc_get(c"EOFError") {
        Ok(class) => Error::new(mrb, class, "end of file reached"),
        Err(err) => err,
    }
}

/// `readline(sep = $/, limit = nil, chomp: false)` answers as `gets` does,
/// raising `EOFError` at the end.
fn socket_readline(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    match next_line(mrb, reader_of(mrb, self_)?, &line_args)? {
        Some(line) => Ok(mrb.str_new(&line).as_value()),
        None => Err(end_of_file(mrb)),
    }
}

/// The next byte, waiting for one, without taking it; `None` at the end.
fn peek_byte(mrb: &Mrb, source: &dyn SocketIo) -> Result<Option<u8>, Error> {
    ready_of(refill(mrb, source, Wait::Block)?);
    Ok(source.io().rbuf.borrow().first().copied())
}

/// The next byte, taking it; `None` at the end.
fn next_byte(mrb: &Mrb, source: &dyn SocketIo) -> Result<Option<u8>, Error> {
    let byte = peek_byte(mrb, source)?;
    if byte.is_some() {
        source.io().rbuf.borrow_mut().remove(0);
    }
    Ok(byte)
}

/// The next character: a UTF-8 sequence whole, or a byte that starts none
/// alone, as CRuby's `getc` reads its external encoding; `None` at the end.
fn next_char(mrb: &Mrb, source: &dyn SocketIo) -> Result<Option<Vec<u8>>, Error> {
    let Some(first) = next_byte(mrb, source)? else {
        return Ok(None);
    };
    let width = match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    };
    let mut char = vec![first];
    while char.len() < width {
        match peek_byte(mrb, source)? {
            Some(byte) if byte & 0xC0 == 0x80 => {
                source.io().rbuf.borrow_mut().remove(0);
                char.push(byte);
            }
            _ => break,
        }
    }
    Ok(Some(char))
}

/// `getc` answers the next character, `nil` at the end.
fn socket_getc(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(match next_char(mrb, reader_of(mrb, self_)?)? {
        Some(char) => mrb.str_new(&char).as_value(),
        None => qnil().as_value(),
    })
}

/// `readchar` answers the next character, raising `EOFError` at the end.
fn socket_readchar(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    match next_char(mrb, reader_of(mrb, self_)?)? {
        Some(char) => Ok(mrb.str_new(&char).as_value()),
        None => Err(end_of_file(mrb)),
    }
}

/// `getbyte` answers the next byte as an Integer, `nil` at the end.
fn socket_getbyte(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(match next_byte(mrb, reader_of(mrb, self_)?)? {
        Some(byte) => i64::from(byte).into_value(mrb),
        None => qnil().as_value(),
    })
}

/// `readbyte` answers the next byte as an Integer, raising `EOFError` at the
/// end.
fn socket_readbyte(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    match next_byte(mrb, reader_of(mrb, self_)?)? {
        Some(byte) => Ok(i64::from(byte).into_value(mrb)),
        None => Err(end_of_file(mrb)),
    }
}

/// The block a call that yields to the end needs, as `each_line` asks for
/// one.
fn required_block(mrb: &Mrb, name: &str) -> Result<Proc, Error> {
    scan_args::<(), (), (), (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, &format!("{name} needs a block")))
}

/// `each_char { |char| }` yields each character to the end.
fn socket_each_char(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = required_block(mrb, "each_char")?;
    let source = reader_of(mrb, self_)?;
    while let Some(char) = next_char(mrb, source)? {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(&char).as_value()])?;
    }
    Ok(self_)
}

/// `each_byte { |byte| }` yields each byte as an Integer to the end.
fn socket_each_byte(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = required_block(mrb, "each_byte")?;
    let source = reader_of(mrb, self_)?;
    while let Some(byte) = next_byte(mrb, source)? {
        block.call::<_, Value>(mrb, [i64::from(byte).into_value(mrb)])?;
    }
    Ok(self_)
}

/// Put `bytes` back before what the buffer holds, for the next read to take
/// first.
fn push_back(source: &dyn SocketIo, bytes: &[u8]) {
    source
        .io()
        .rbuf
        .borrow_mut()
        .splice(0..0, bytes.iter().copied());
}

/// `ungetc(string)` puts `string`, or the character an Integer numbers,
/// back for the next read, as CRuby's does.
fn socket_ungetc(mrb: &Mrb, self_: Value, pushed: Value) -> Result<Value, Error> {
    let source = reader_of(mrb, self_)?;
    let bytes = match Integer::from_value(pushed) {
        Some(_) => {
            let code = u32::try_convert(pushed, mrb)?;
            let char = char::from_u32(code)
                .ok_or_else(|| argument_error(mrb, &format!("invalid character: {code}")))?;
            char.to_string().into_bytes()
        }
        None => {
            let text = RString::try_convert(pushed, mrb)
                .map_err(|_| crate::errno::type_error(mrb, pushed))?;
            // SAFETY: the bytes are copied before the next mruby call.
            unsafe { text.as_bytes(mrb) }.to_vec()
        }
    };
    push_back(source, &bytes);
    Ok(qnil().as_value())
}

/// `ungetbyte(byte)` puts back an Integer's low byte, or a String's bytes;
/// `nil` puts back nothing, as CRuby's does.
fn socket_ungetbyte(mrb: &Mrb, self_: Value, pushed: Value) -> Result<Value, Error> {
    let source = reader_of(mrb, self_)?;
    if pushed.is_nil() {
        return Ok(qnil().as_value());
    }
    match Integer::from_value(pushed) {
        Some(_) => push_back(source, &[i64::try_convert(pushed, mrb)? as u8]),
        None => {
            let text = RString::try_convert(pushed, mrb)
                .map_err(|_| crate::errno::type_error(mrb, pushed))?;
            // SAFETY: the bytes are copied before the next mruby call.
            let bytes = unsafe { text.as_bytes(mrb) }.to_vec();
            push_back(source, &bytes);
        }
    }
    Ok(qnil().as_value())
}

/// `each_line(sep = $/, limit = nil, chomp: false) { |line| }` yields each
/// line to the end.
fn socket_each_line(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "each_line needs a block"))?;
    let line_args = line_args_of(mrb, args)?;
    ensure_limit(mrb, &line_args, "each_line")?;
    let source = reader_of(mrb, self_)?;
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
    let source = reader_of(mrb, self_)?;
    let lines = mrb.ary_new();
    while let Some(line) = next_line(mrb, source, &line_args)? {
        lines.push(mrb, mrb.str_new(&line).as_value())?;
    }
    Ok(lines.as_value())
}

/// `eof?` waits for the next bytes, holding them for the next read, and
/// answers whether the end came instead, as CRuby's fills its buffer.
fn socket_eof_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(!ready_of(refill(mrb, reader_of(mrb, self_)?, Wait::Block)?))
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
    source_of(mrb, value).is_ok_and(|source| !source.io().rbuf.borrow().is_empty())
}
