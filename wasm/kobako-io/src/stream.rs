//! The read, seek, sync, and close half of `::IO`, which a file this guest
//! opened answers: `#read`, `#gets`, `#each_line`, `#readlines`,
//! `#eof?`, `#seek`, `#rewind`, `#pos` / `#tell`, `#pos=`, `#pread`,
//! `#pwrite`, `#fsync`, `#fdatasync`, `#advise`, `#close`, `#closed?`.
//!
//! A sync, a position, a positioned write, and an advice reach a captured
//! stream too, which answers as the pipe it is on Linux.
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
use core::ffi::c_int;

use beni::{Error, Integer, IntoValue, Module, Mrb, RClass, RString, Symbol, Value};

use crate::file::fstat_of;
use crate::io::{argument_error, checked_fd, is_closed, release, to_int, Stream};
use crate::lines::{self, LineSource};
use crate::{descriptors, errno};

/// What a stream that may not be read from is refused with.
const READ_REFUSAL: &str = "kobako IO reads only from a file this guest opened";

/// What a stream standing for a descriptor this guest may no longer reach is
/// refused with.
pub(crate) const REACH_REFUSAL: &str =
    "kobako IO reaches only fd 1 (stdout), fd 2 (stderr), or a file this guest opened";

/// The advices `#advise` takes, in the order wasi-libc numbers them.
const ADVICES: [&str; 6] = [
    "normal",
    "sequential",
    "random",
    "willneed",
    "dontneed",
    "noreuse",
];

/// The most bytes one read asks the host for.
const CHUNK: usize = 4096;

pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    io.const_set(mrb, c"SEEK_SET", 0)?;
    io.const_set(mrb, c"SEEK_CUR", 1)?;
    io.const_set(mrb, c"SEEK_END", 2)?;
    io.define_method(mrb, c"read", beni::method!(io_read, -1))?;
    io.define_method(mrb, c"gets", beni::method!(io_gets, -1))?;
    io.define_method(mrb, c"each_line", beni::method!(io_each_line, -1))?;
    io.define_method(mrb, c"readlines", beni::method!(io_readlines, -1))?;
    io.define_method(mrb, c"eof?", beni::method!(io_eof_p, 0))?;
    io.define_method(mrb, c"eof", beni::method!(io_eof_p, 0))?;
    io.define_method(mrb, c"seek", beni::method!(io_seek, -1))?;
    io.define_method(mrb, c"rewind", beni::method!(io_rewind, 0))?;
    io.define_method(mrb, c"pos", beni::method!(io_pos, 0))?;
    io.define_method(mrb, c"pos=", beni::method!(io_set_pos, 1))?;
    io.define_method(mrb, c"tell", beni::method!(io_pos, 0))?;
    io.define_method(mrb, c"pread", beni::method!(io_pread, -1))?;
    io.define_method(mrb, c"pwrite", beni::method!(io_pwrite, 2))?;
    io.define_method(mrb, c"fsync", beni::method!(io_fsync, 0))?;
    io.define_method(mrb, c"fdatasync", beni::method!(io_fdatasync, 0))?;
    io.define_method(mrb, c"advise", beni::method!(io_advise, -1))?;
    io.define_method(mrb, c"close", beni::method!(io_close, 0))?;
    io.define_method(mrb, c"closed?", beni::method!(io_closed_p, 0))?;
    Ok(())
}

/// The path a stream's errors name: a `File`'s own, or its descriptor.
pub(crate) fn label_of(mrb: &Mrb, self_: Value) -> String {
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

fn readable_fd(mrb: &Mrb, self_: Value) -> Result<i32, Error> {
    let fd = checked_fd(mrb, self_, READ_REFUSAL)?;
    if descriptors::is_channel(fd) {
        return Err(errno::io_error(mrb, "not opened for reading"));
    }
    Ok(fd)
}

/// Whether the stream stands for a directory opened as a `File`.
fn is_directory(mrb: &Mrb, self_: Value) -> bool {
    checked_fd(mrb, self_, REACH_REFUSAL)
        .is_ok_and(|fd| fstat_of(fd).is_ok_and(|stat| stat.is_dir()))
}

/// The error a failed read raises: a stream not opened for reading names
/// that, and a directory, which has nothing to read, `EISDIR`, as CRuby's
/// `read(2)` does.
fn read_error(mrb: &Mrb, self_: Value, err: &io::Error) -> Error {
    let path = label_of(mrb, self_);
    if is_directory(mrb, self_) {
        return errno::error_by_name(mrb, c"EISDIR", &path);
    }
    errno::from_transfer(mrb, err, &path, "reading")
}

/// The error a failed seek or position raises. WASI keeps no position for
/// a directory, where Linux's would seek, so a directory raises `EISDIR`;
/// any other failure its own name.
fn position_error(mrb: &Mrb, self_: Value, err: &io::Error) -> Error {
    let path = label_of(mrb, self_);
    if is_directory(mrb, self_) {
        return errno::error_by_name(mrb, c"EISDIR", &path);
    }
    errno::from_io(mrb, err, &path)
}

/// `read` with no length reads to the end, "" at the end; with one, up
/// to that many bytes, `nil` at the end.
fn io_read(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let fd = readable_fd(mrb, self_)?;
    let (length,) = scan_args::<(), (Option<Value>,), (), (), (), ()>(mrb)?.optional;
    let length = match length.filter(|v| !v.is_nil()) {
        None => None,
        Some(v) => Some(
            Integer::from_value(v)
                .map(|int| int.to_i64(mrb))
                .transpose()?
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
    .map_err(|err| read_error(mrb, self_, &err))?;
    if bytes.is_empty() && length.is_some_and(|len| len > 0) {
        return Ok(qnil().as_value());
    }
    Ok(mrb.str_new(&bytes).as_value())
}

/// A file read line by line: a chunk at a time from its position, a read
/// past the line given back by seeking, since the host keeps the position.
struct FileLines {
    fd: i32,
    stream: Value,
}

impl LineSource for FileLines {
    fn fill(&mut self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error> {
        with_file(self.fd, |file| {
            let mut buf = vec![0u8; CHUNK];
            let read = file.read(&mut buf)?;
            buf.truncate(read);
            Ok((read > 0).then_some(buf))
        })
        .map_err(|err| read_error(mrb, self.stream, &err))
    }

    fn give_back(&mut self, mrb: &Mrb, rest: Vec<u8>) -> Result<(), Error> {
        with_file(self.fd, |file| {
            file.seek(SeekFrom::Current(-(rest.len() as i64))).map(drop)
        })
        .map_err(|err| read_error(mrb, self.stream, &err))
    }
}

impl FileLines {
    /// The lines `self_` reads, as every IO reads them.
    fn new(mrb: &Mrb, self_: Value) -> Result<Self, Error> {
        Ok(FileLines {
            fd: readable_fd(mrb, self_)?,
            stream: self_,
        })
    }
}

fn io_gets(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    lines::gets(mrb, args, || FileLines::new(mrb, self_))
}

fn io_each_line(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    lines::each_line(mrb, self_, args, || FileLines::new(mrb, self_))
}

fn io_readlines(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    lines::readlines(mrb, args, || FileLines::new(mrb, self_))
}

/// Whether the position stands at the end; a stream not opened for reading
/// raises, as CRuby's `eof?` reads before it answers.
fn io_eof_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    let fd = readable_fd(mrb, self_)?;
    with_file(fd, |file| {
        // A read of nothing asks the host only whether the file reads at all.
        let _read_nothing = file.read(&mut [])?;
        Ok(file.stream_position()? >= fstat_of(fd)?.size())
    })
    .map_err(|err| read_error(mrb, self_, &err))
}

fn io_seek(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(i64,), (Option<Value>,), (), (), (), ()>(mrb)?;
    let (offset,) = args.required;
    let (whence,) = args.optional;
    let whence = match whence {
        Some(whence) => whence_of(mrb, whence)?,
        None => 0,
    };
    seek(mrb, self_, offset, whence)?;
    Ok(0.into_value(mrb))
}

/// `whence` as a number, `:SET`, `:CUR`, and `:END` read as the constants
/// they name, as CRuby's `interpret_seek_whence` reads them; any other value
/// converts as an Integer, as its `NUM2INT` does.
fn whence_of(mrb: &Mrb, whence: Value) -> Result<i64, Error> {
    if let Some(name) = Symbol::from_value(whence) {
        match name.name(mrb)?.as_str() {
            "SET" => return Ok(0),
            "CUR" => return Ok(1),
            "END" => return Ok(2),
            _ => {}
        }
    }
    to_int(mrb, whence)?.to_i64(mrb)
}

fn io_set_pos(mrb: &Mrb, self_: Value, offset: i64) -> Result<Value, Error> {
    seek(mrb, self_, offset, 0)?;
    Ok(offset.into_value(mrb))
}

/// A position before the start, or a `whence` no constant names, is
/// `EINVAL`, as `lseek(2)` answers it.
fn seek(mrb: &Mrb, self_: Value, offset: i64, whence: i64) -> Result<(), Error> {
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    let invalid = || errno::error_by_name(mrb, c"EINVAL", &label_of(mrb, self_));
    let to = match whence {
        0 => SeekFrom::Start(u64::try_from(offset).map_err(|_| invalid())?),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return Err(invalid()),
    };
    with_file(fd, |file| file.seek(to)).map_err(|err| position_error(mrb, self_, &err))?;
    Ok(())
}

fn io_rewind(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    with_file(fd, |file| file.rewind()).map_err(|err| position_error(mrb, self_, &err))?;
    Ok(0.into_value(mrb))
}

fn io_pos(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    let pos = with_file(fd, |file| file.stream_position())
        .map_err(|err| position_error(mrb, self_, &err))?;
    Ok(i64::try_from(pos).unwrap_or(i64::MAX).into_value(mrb))
}

/// The calls `std` keeps behind an unstable feature on this target.
#[cfg(target_os = "wasi")]
mod raw {
    use core::ffi::{c_int, c_void};

    unsafe extern "C" {
        pub(super) fn pread(fd: c_int, buf: *mut c_void, len: usize, offset: i64) -> isize;
        pub(super) fn pwrite(fd: c_int, buf: *const c_void, len: usize, offset: i64) -> isize;
        /// Answers the failure's number instead of setting `errno`.
        pub(super) fn posix_fadvise(fd: c_int, offset: i64, len: i64, advice: c_int) -> c_int;
    }
}

/// The same calls on the host target the guest crates' unit tests link
/// against, whose libc may lack `posix_fadvise`; an advice changes nothing
/// read, so taking none there answers alike.
#[cfg(not(target_os = "wasi"))]
mod raw {
    use core::ffi::{c_int, c_void};

    unsafe extern "C" {
        pub(super) fn pread(fd: c_int, buf: *mut c_void, len: usize, offset: i64) -> isize;
        pub(super) fn pwrite(fd: c_int, buf: *const c_void, len: usize, offset: i64) -> isize;
    }

    pub(super) unsafe fn posix_fadvise(
        _fd: c_int,
        _offset: i64,
        _len: i64,
        _advice: c_int,
    ) -> c_int {
        0
    }
}

/// `pread(maxlen, offset, outbuf)` reads at `offset` without moving the
/// position, raising `EOFError` where nothing is left, as CRuby does. It
/// reads a chunk at a time, so `maxlen` claims no memory the file does not
/// fill, and replaces what `outbuf` held with what it read.
fn io_pread(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(i64, i64), (Option<Value>,), (), (), (), ()>(mrb)?;
    let (length, offset) = args.required;
    let (outbuf,) = args.optional;
    let outbuf = match outbuf.filter(|value| !value.is_nil()) {
        Some(value) => {
            Some(RString::from_value(value).ok_or_else(|| errno::type_error(mrb, value))?)
        }
        None => None,
    };
    let fd = readable_fd(mrb, self_)?;
    let length = usize::try_from(length)
        .map_err(|_| argument_error(mrb, "negative string size (or size too big)"))?;
    let mut bytes = Vec::with_capacity(length.min(CHUNK));
    let mut chunk = [0u8; CHUNK];
    while bytes.len() < length {
        let want = (length - bytes.len()).min(CHUNK);
        let at = offset + bytes.len() as i64;
        // SAFETY: `chunk` holds `want` writable bytes for the call.
        let read = unsafe { raw::pread(fd, chunk.as_mut_ptr().cast(), want, at) };
        if read < 0 {
            return Err(read_error(mrb, self_, &io::Error::last_os_error()));
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
    if bytes.is_empty() && length > 0 {
        let eof = mrb.exc_get(c"EOFError")?;
        return Err(Error::new(mrb, eof, "end of file reached"));
    }
    let Some(outbuf) = outbuf else {
        return Ok(mrb.str_new(&bytes).as_value());
    };
    outbuf.resize(mrb, 0)?;
    outbuf.cat(mrb, &bytes)?;
    Ok(outbuf.as_value())
}

/// `pwrite(string, offset)` writes at `offset` without moving the position,
/// answering how many bytes one call took, as CRuby does.
fn io_pwrite(mrb: &Mrb, self_: Value, data: Value, offset: i64) -> Result<Value, Error> {
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    let data = data.to_r_string(mrb)?;
    // SAFETY: the slice is consumed before the next mruby call.
    let bytes = unsafe { data.as_slice(mrb) };
    // SAFETY: `bytes` is a live buffer the call only reads.
    let written = unsafe { raw::pwrite(fd, bytes.as_ptr().cast(), bytes.len(), offset) };
    if written < 0 {
        let err = io::Error::last_os_error();
        return Err(errno::from_transfer(
            mrb,
            &err,
            &label_of(mrb, self_),
            "writing",
        ));
    }
    Ok((written as i64).into_value(mrb))
}

fn sync(mrb: &Mrb, self_: Value, data_only: bool) -> Result<Value, Error> {
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    with_file(fd, |file| match data_only {
        true => file.sync_data(),
        false => file.sync_all(),
    })
    .map_err(|err| errno::from_io(mrb, &err, &label_of(mrb, self_)))?;
    Ok(0.into_value(mrb))
}

fn io_fsync(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    sync(mrb, self_, false)
}

fn io_fdatasync(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    sync(mrb, self_, true)
}

/// The `posix_fadvise` advice `advice` names, as CRuby checks it before any
/// call: one that is no Symbol raises `TypeError`, and an unknown one
/// `NotImplementedError`.
pub(crate) fn advice_of(mrb: &Mrb, advice: Value) -> Result<usize, Error> {
    let symbol = Symbol::from_value(advice).ok_or_else(|| {
        let type_error = mrb.exception_type_error();
        match type_error {
            Ok(class) => Error::new(mrb, class, "advice must be a Symbol"),
            Err(err) => err,
        }
    })?;
    let name = symbol.name(mrb)?;
    ADVICES
        .iter()
        .position(|known| *known == name)
        .ok_or_else(|| errno::not_implemented(mrb, &format!("Unsupported advice: :{name}")))
}

/// `advise(advice, offset = 0, len = 0)` checks the advice as CRuby does
/// before it reaches the stream, and answers `nil`.
fn io_advise(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<Value, Error> {
    // A `nil` offset or length is 0, as CRuby reads one.
    let args =
        scan_args::<(Value,), (Option<Option<i64>>, Option<Option<i64>>), (), (), (), ()>(mrb)?;
    let (advice,) = args.required;
    let (offset, len) = (args.optional.0.flatten(), args.optional.1.flatten());
    let advice = advice_of(mrb, advice)?;
    let fd = checked_fd(mrb, self_, REACH_REFUSAL)?;
    // SAFETY: `fd` is a descriptor this guest may reach; the call only reads
    // its arguments.
    let refused =
        unsafe { raw::posix_fadvise(fd, offset.unwrap_or(0), len.unwrap_or(0), advice as c_int) };
    if refused != 0 {
        let err = io::Error::from_raw_os_error(refused);
        return Err(errno::from_io(mrb, &err, &label_of(mrb, self_)));
    }
    Ok(qnil().as_value())
}

unsafe extern "C" {
    /// wasi-libc `close(2)`: dropping a `std::fs::File` closes it but
    /// discards the failure a Filesystem reports.
    fn close(fd: core::ffi::c_int) -> core::ffi::c_int;
}

/// Closing a file this guest opened releases its descriptor, raising what
/// the Filesystem reported if releasing it failed; closing one already
/// closed, or a copy whose descriptor another closed, only marks it
/// closed, as CRuby's close answers `nil`. A captured stream is the host's
/// and stays open.
fn io_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    if is_closed(mrb, self_) {
        return Ok(qnil().as_value());
    }
    let Ok(fd) = checked_fd(mrb, self_, REACH_REFUSAL) else {
        release(mrb, self_);
        return Ok(qnil().as_value());
    };
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
        Some(err) => Err(errno::from_io(mrb, &err, &label_of(mrb, self_))),
        None => Ok(qnil().as_value()),
    }
}

fn io_closed_p(mrb: &Mrb, self_: Value) -> bool {
    is_closed(mrb, self_)
}
