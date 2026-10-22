//! The read, seek, and close half of `::IO`, which a file this guest
//! opened answers: `#read`, `#gets`, `#each_line`, `#readlines`,
//! `#eof?`, `#seek`, `#rewind`, `#pos` / `#tell`, `#close`, `#closed?`.
//!
//! Each reaches the descriptor through `std::fs::File` without taking it
//! over, so the descriptor stays the `IO`'s until `#close`. The host
//! keeps the position, so a seek here is a seek there. The guest has no
//! `Enumerator`, so `#each_line` without a block raises where CRuby would
//! answer one.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, IntoValue, Module, Mrb, Proc, RArray, RClass, Value};

use crate::io::{argument_error, checked_fd, is_closed, release, Stream};
use crate::{descriptors, errno};

/// What a stream that may not be read from is refused with.
const READ_REFUSAL: &str = "kobako IO reads only from a file this guest opened";

/// The most bytes one read asks the host for.
const CHUNK: usize = 4096;

pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    io.const_set(mrb, c"SEEK_SET", 0)?;
    io.const_set(mrb, c"SEEK_CUR", 1)?;
    io.const_set(mrb, c"SEEK_END", 2)?;
    io.define_method(mrb, c"read", beni::method!(io_read, -1))?;
    io.define_method(mrb, c"gets", beni::method!(io_gets, 0))?;
    io.define_method(mrb, c"each_line", beni::method!(io_each_line, -1))?;
    io.define_method(mrb, c"readlines", beni::method!(io_readlines, 0))?;
    io.define_method(mrb, c"eof?", beni::method!(io_eof_p, 0))?;
    io.define_method(mrb, c"eof", beni::method!(io_eof_p, 0))?;
    io.define_method(mrb, c"seek", beni::method!(io_seek, -1))?;
    io.define_method(mrb, c"rewind", beni::method!(io_rewind, 0))?;
    io.define_method(mrb, c"pos", beni::method!(io_pos, 0))?;
    io.define_method(mrb, c"tell", beni::method!(io_pos, 0))?;
    io.define_method(mrb, c"close", beni::method!(io_close, 0))?;
    io.define_method(mrb, c"closed?", beni::method!(io_closed_p, 0))?;
    Ok(())
}

/// The path a stream's errors name: a `File`'s own, or its descriptor.
pub(crate) fn label(mrb: &Mrb, self_: Value) -> String {
    Stream::state(mrb, self_)
        .and_then(|state| state.path)
        .unwrap_or_else(|| "IO".to_string())
}

/// Run `body` over the descriptor as a `std::fs::File` it does not own.
pub(crate) fn with_file<R>(
    fd: i32,
    body: impl FnOnce(&mut fs::File) -> io::Result<R>,
) -> io::Result<R> {
    // SAFETY: `fd` is a descriptor this guest holds open; `ManuallyDrop`
    // keeps the borrowed `File` from closing it.
    let mut file = ManuallyDrop::new(unsafe { fs::File::from_raw_fd(fd) });
    body(&mut file)
}

fn readable(mrb: &Mrb, self_: Value) -> Result<i32, Error> {
    let fd = checked_fd(mrb, self_, READ_REFUSAL)?;
    if descriptors::is_channel(fd) {
        return Err(errno::io_error(mrb, "not opened for reading"));
    }
    Ok(fd)
}

fn fail(mrb: &Mrb, self_: Value, err: &io::Error) -> Error {
    errno::from_transfer(mrb, err, &label(mrb, self_), "reading")
}

/// `read` with no length reads to the end, "" at the end; with one, up
/// to that many bytes, `nil` at the end.
fn io_read(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    let (length,) = scan_args::<(), (Option<Value>,), (), (), (), ()>(mrb)?.optional;
    let length = match length.filter(|v| !v.is_nil()) {
        None => None,
        Some(v) => Some(
            i64::from_value(v)
                .filter(|n| *n >= 0)
                .ok_or_else(|| argument_error(mrb, "negative length given"))? as usize,
        ),
    };
    let bytes = with_file(fd, |file| match length {
        None => {
            let mut all = Vec::new();
            file.read_to_end(&mut all)?;
            Ok(all)
        }
        Some(len) => {
            // Room for the first chunk up front, so one read can answer a
            // length the file holds instead of a small probe and regrowth.
            let mut some = Vec::with_capacity(len.min(CHUNK));
            file.take(len as u64).read_to_end(&mut some)?;
            Ok(some)
        }
    })
    .map_err(|err| fail(mrb, self_, &err))?;
    if bytes.is_empty() && length.is_some_and(|len| len > 0) {
        return Ok(qnil().as_value());
    }
    Ok(mrb.str_new(&bytes).as_value())
}

/// One line, its newline kept, or `None` at the end. A read past the
/// line is given back by seeking, since the host keeps the position.
fn read_line(fd: i32) -> io::Result<Option<Vec<u8>>> {
    with_file(fd, |file| {
        let mut line = Vec::new();
        let mut buf = [0u8; CHUNK];
        loop {
            let read = file.read(&mut buf)?;
            if read == 0 {
                return Ok((!line.is_empty()).then_some(line));
            }
            if let Some(at) = buf[..read].iter().position(|&b| b == b'\n') {
                line.extend_from_slice(&buf[..=at]);
                file.seek(SeekFrom::Current(-((read - at - 1) as i64)))?;
                return Ok(Some(line));
            }
            line.extend_from_slice(&buf[..read]);
        }
    })
}

fn io_gets(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    let line = read_line(fd).map_err(|err| fail(mrb, self_, &err))?;
    Ok(line.map_or(qnil().as_value(), |line| mrb.str_new(&line).as_value()))
}

fn io_each_line(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "each_line needs a block"))?;
    let fd = readable(mrb, self_)?;
    while let Some(line) = read_line(fd).map_err(|err| fail(mrb, self_, &err))? {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(&line).as_value()])?;
    }
    Ok(self_)
}

fn io_readlines(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    let lines = mrb.ary_new();
    while let Some(line) = read_line(fd).map_err(|err| fail(mrb, self_, &err))? {
        lines.push(mrb, mrb.str_new(&line).as_value())?;
    }
    Ok(lines.as_value())
}

fn io_eof_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    let fd = readable(mrb, self_)?;
    with_file(fd, |file| {
        Ok(file.stream_position()? >= file.metadata()?.len())
    })
    .map_err(|err| fail(mrb, self_, &err))
}

fn io_seek(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    let args = scan_args::<(i64,), (Option<i64>,), (), (), (), ()>(mrb)?;
    let (offset,) = args.required;
    let (whence,) = args.optional;
    let to = match whence.unwrap_or(0) {
        0 => SeekFrom::Start(
            u64::try_from(offset).map_err(|_| argument_error(mrb, "negative offset"))?,
        ),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        other => return Err(argument_error(mrb, &format!("unknown whence: {other}"))),
    };
    with_file(fd, |file| file.seek(to)).map_err(|err| fail(mrb, self_, &err))?;
    Ok(0.into_value(mrb))
}

fn io_rewind(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    with_file(fd, |file| file.rewind()).map_err(|err| fail(mrb, self_, &err))?;
    Ok(0.into_value(mrb))
}

fn io_pos(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = readable(mrb, self_)?;
    let pos = with_file(fd, |file| file.stream_position()).map_err(|err| fail(mrb, self_, &err))?;
    Ok(i64::try_from(pos).unwrap_or(i64::MAX).into_value(mrb))
}

unsafe extern "C" {
    /// wasi-libc `close(2)`: dropping a `std::fs::File` closes it but
    /// discards the failure a Filesystem reports.
    fn close(fd: core::ffi::c_int) -> core::ffi::c_int;
}

/// Closing a file this guest opened releases its descriptor, raising what
/// the Filesystem reported if releasing it failed; closing one already
/// closed does nothing. A captured stream is the host's and stays open.
fn io_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    if is_closed(mrb, self_) {
        return Ok(qnil().as_value());
    }
    let fd = checked_fd(mrb, self_, READ_REFUSAL)?;
    if descriptors::is_channel(fd) {
        return Err(errno::io_error(
            mrb,
            "kobako IO cannot close a captured stream",
        ));
    }
    // SAFETY: this guest opened `fd` and has just stopped tracking it, so
    // nothing else closes it.
    let failed = descriptors::forget(fd) && unsafe { close(fd) } != 0;
    let refusal = failed.then(io::Error::last_os_error);
    release(mrb, self_);
    match refusal {
        Some(err) => Err(fail(mrb, self_, &err)),
        None => Ok(qnil().as_value()),
    }
}

fn io_closed_p(mrb: &Mrb, self_: Value) -> bool {
    is_closed(mrb, self_)
}
