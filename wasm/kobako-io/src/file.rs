//! `::File < IO`: the files beneath the paths a Host App mounted.
//!
//! A path resolves through wasi-libc against the preopened directories
//! the host presents, one per Mount, and the host decides what each open
//! may do. With no Mount at all there is nothing to open, so
//! every call that touches a path raises `NotImplementedError`, as CRuby
//! does for a call its platform lacks. The path helpers (`join`,
//! `basename`, `dirname`, `extname`) touch nothing and always answer.

use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::fs;
use std::io::{self, Write};

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, IntoValue, Mrb, Object, RArray, RClass, RString, Symbol, Value};

use crate::io::{argument_error, close_after, required_block, Stream};
use crate::socket::string_value_of;
use crate::stream::FileLines;
use crate::{descriptors, errno, lines};

pub(crate) mod mode;
mod stat;

pub(crate) use stat::fstat_of;
use stat::Stat;

/// What a `File` standing for a descriptor this guest may no longer reach is
/// refused with.
const REACH_REFUSAL: &str = "kobako File reaches only a file this guest opened";

pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    use beni::Module;

    let file = mrb.define_class(c"File", io)?;
    file.const_set(mrb, c"SEPARATOR", mrb.str_new(b"/").as_value())?;
    mode::define_constants(mrb, file, io)?;
    file.define_method(mrb, c"initialize", beni::method!(file_initialize, -1))?;
    file.define_method(mrb, c"path", beni::method!(file_path, 0))?;
    file.define_method(mrb, c"size", beni::method!(file_size, 0))?;
    file.define_method(mrb, c"truncate", beni::method!(file_truncate, 1))?;
    file.define_singleton_method(mrb, c"truncate", beni::method!(file_s_truncate, 2))?;

    file.define_singleton_method(mrb, c"read", beni::method!(file_s_read, 1))?;
    file.define_singleton_method(mrb, c"readlines", beni::method!(file_s_readlines, -1))?;
    file.define_singleton_method(mrb, c"foreach", beni::method!(file_s_foreach, -1))?;
    file.define_singleton_method(mrb, c"write", beni::method!(file_s_write, 2))?;
    file.define_singleton_method(mrb, c"exist?", beni::method!(file_s_exist_p, 1))?;
    file.define_singleton_method(mrb, c"file?", beni::method!(file_s_file_p, 1))?;
    file.define_singleton_method(mrb, c"directory?", beni::method!(file_s_directory_p, 1))?;
    file.define_singleton_method(mrb, c"size", beni::method!(file_s_size, 1))?;
    file.define_singleton_method(mrb, c"delete", beni::method!(file_s_delete, -1))?;
    file.define_singleton_method(mrb, c"unlink", beni::method!(file_s_delete, -1))?;
    file.define_singleton_method(mrb, c"rename", beni::method!(file_s_rename, 2))?;
    file.define_singleton_method(mrb, c"join", beni::method!(file_s_join, -1))?;
    file.define_singleton_method(mrb, c"basename", beni::method!(file_s_basename, -1))?;
    file.define_singleton_method(mrb, c"dirname", beni::method!(file_s_dirname, 1))?;
    file.define_singleton_method(mrb, c"extname", beni::method!(file_s_extname, 1))?;
    stat::init(mrb, file)
}

/// Whether the host presents any mounted directory: a preopen is always
/// descriptor 3 onwards.
#[cfg(target_os = "wasi")]
fn is_mounted() -> bool {
    #[link(wasm_import_module = "wasi_snapshot_preview1")]
    unsafe extern "C" {
        #[link_name = "fd_prestat_get"]
        fn fd_prestat_get(fd: i32, prestat: *mut u32) -> i32;
    }
    let mut prestat = [0u32; 2];
    // SAFETY: `prestat` is an 8-byte, 4-aligned buffer, the size and
    // alignment of preview1's `prestat`.
    unsafe { fd_prestat_get(3, prestat.as_mut_ptr()) == 0 }
}

#[cfg(not(target_os = "wasi"))]
fn is_mounted() -> bool {
    true
}

pub(crate) fn require_mount(mrb: &Mrb, call: &str) -> Result<(), Error> {
    if is_mounted() {
        return Ok(());
    }
    Err(errno::not_implemented(
        mrb,
        &format!("{call} is unimplemented: this Sandbox mounts no filesystem"),
    ))
}

/// A path a call hands the host, as `path_text` reads it. An empty one
/// raises `Errno::ENOENT`, as Linux's `getname` refuses one, since
/// wasi-libc would take it for the working directory.
pub(crate) fn path_arg(mrb: &Mrb, value: Value) -> Result<String, Error> {
    let path = path_text(mrb, value)?;
    match path.is_empty() {
        true => Err(errno::error_by_name(mrb, c"ENOENT", &path)),
        false => Ok(path),
    }
}

/// A path as a String names it. One holding a NUL raises `ArgumentError`,
/// as CRuby refuses one for every path call. One whose bytes are no valid
/// UTF-8 names nothing WASI can reach, so it fails as the engine would
/// answer it.
pub(crate) fn path_text(mrb: &Mrb, value: Value) -> Result<String, Error> {
    String::from_utf8(path_bytes_of(mrb, value)?).map_err(|err| {
        let path = String::from_utf8_lossy(err.as_bytes());
        errno::error_by_name(mrb, c"EILSEQ", &path)
    })
}

/// `path`, which `path_arg` answered, as the C string a libc call takes.
pub(crate) fn to_c_path(path: &str) -> CString {
    CString::new(path).expect("path_arg refuses a path holding a NUL")
}

unsafe extern "C" {
    /// wasi-libc `open(2)`, which hands the host every flag a mode asks
    /// for, as `std::fs::OpenOptions` refuses a create without a write.
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;

    /// wasi-libc `ftruncate(2)` and `truncate(2)`, which refuse a negative
    /// length as Linux does rather than taking it as a huge one.
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn truncate(path: *const c_char, length: i64) -> c_int;
}

fn file_initialize(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<(), Error> {
    require_mount(mrb, "File.open")?;
    let args = scan_args::<(Value,), (Option<Value>, Option<Value>), (), (), (), ()>(mrb)?;
    let (path,) = args.required;
    let (mode, _perm) = args.optional;
    let path = path_arg(mrb, path)?;
    let flags = mode::open_flags(mrb, mode, &path)?;
    let c_path = to_c_path(&path);
    // SAFETY: `c_path` is NUL-terminated and outlives the call.
    let fd = unsafe { open(c_path.as_ptr(), flags) };
    if fd < 0 {
        return Err(errno::from_io(mrb, &io::Error::last_os_error(), &path));
    }
    descriptors::remember(fd);
    crate::io::point_at(mrb, self_, fd, Some(path));
    Ok(())
}

fn file_path(mrb: &Mrb, self_: Value) -> Value {
    match Stream::state(mrb, self_).and_then(|state| state.path) {
        Some(path) => mrb.str_new(path.as_bytes()).as_value(),
        None => qnil().as_value(),
    }
}

fn file_size(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = crate::io::checked_fd(mrb, self_, REACH_REFUSAL)?;
    let len = stat::fstat_of(fd)
        .map_err(|err| errno::from_io(mrb, &err, &crate::stream::label_of(mrb, self_)))?
        .size();
    Ok(i64::try_from(len).unwrap_or(i64::MAX).into_value(mrb))
}

fn file_truncate(mrb: &Mrb, self_: Value, length: i64) -> Result<Value, Error> {
    let fd = crate::io::checked_fd(mrb, self_, REACH_REFUSAL)?;
    if !crate::io::is_opened_to_write(fd) {
        return Err(errno::io_error(mrb, "not opened for writing"));
    }
    // SAFETY: `fd` is a file this guest opened; the call takes no buffer.
    if unsafe { ftruncate(fd, length) } != 0 {
        let err = io::Error::last_os_error();
        let label = crate::stream::label_of(mrb, self_);
        return Err(errno::from_transfer(mrb, &err, &label, "writing"));
    }
    Ok(0.into_value(mrb))
}

fn file_s_truncate(mrb: &Mrb, _self: Value, path: Value, length: i64) -> Result<Value, Error> {
    require_mount(mrb, "File.truncate")?;
    let path = path_arg(mrb, path)?;
    let c_path = to_c_path(&path);
    // SAFETY: `c_path` is NUL-terminated and outlives the call.
    if unsafe { truncate(c_path.as_ptr(), length) } != 0 {
        return Err(errno::from_io(mrb, &io::Error::last_os_error(), &path));
    }
    Ok(0.into_value(mrb))
}

fn file_s_read(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.read")?;
    let path = path_arg(mrb, path)?;
    let bytes = fs::read(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(mrb.str_new(&bytes).as_value())
}

/// `readlines(name, sep = $/, limit = nil, chomp: false)`: every line of the
/// file `name` opens for reading, as CRuby's `IO.readlines`.
fn file_s_readlines(mrb: &Mrb, file: Value, args: &[Value]) -> Result<Value, Error> {
    let (path, rest) = path_and_line_args(mrb, "File.readlines", args)?;
    with_opened(mrb, file, path, |opened| {
        lines::readlines(mrb, rest, || FileLines::new(mrb, opened))
    })
}

/// `foreach(name, sep = $/, limit = nil, chomp: false) { |line| }`: each line
/// of the file `name` opens, yielded in turn, as CRuby's `IO.foreach`; with
/// no `Enumerator`, a call without a block raises.
fn file_s_foreach(mrb: &Mrb, file: Value, args: &[Value]) -> Result<Value, Error> {
    required_block(mrb, "File.foreach")?;
    let (path, rest) = path_and_line_args(mrb, "File.foreach", args)?;
    with_opened(mrb, file, path, |opened| {
        lines::each_line(mrb, opened, rest, || FileLines::new(mrb, opened))
    })?;
    Ok(qnil().as_value())
}

/// The path a class-level line reader opens, and the line arguments after it.
fn path_and_line_args<'a>(
    mrb: &Mrb,
    call: &str,
    args: &'a [Value],
) -> Result<(Value, &'a [Value]), Error> {
    args.split_first()
        .map(|(path, rest)| (*path, rest))
        .ok_or_else(|| argument_error(mrb, &format!("{call} needs a path")))
}

/// `read` over the file `path` opens for reading, closed again however `read`
/// ends, as CRuby's `rb_io_s_readlines` ensures.
fn with_opened(
    mrb: &Mrb,
    file: Value,
    path: Value,
    read: impl FnOnce(Value) -> Result<Value, Error>,
) -> Result<Value, Error> {
    close_after(mrb, file.funcall(mrb, c"new", [path])?, read)
}

fn file_s_write(mrb: &Mrb, _self: Value, path: Value, data: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.write")?;
    let path = path_arg(mrb, path)?;
    let data = data.to_r_string(mrb)?;
    // SAFETY: the bytes are copied out before the next mruby call.
    let bytes = unsafe { data.as_slice(mrb) }.to_vec();
    fs::File::create(&path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(i64::try_from(bytes.len())
        .unwrap_or(i64::MAX)
        .into_value(mrb))
}

/// What the entry `path` names reports, none where nothing answers, for a
/// predicate that answers false rather than raise.
pub(crate) fn stat_or_none(mrb: &Mrb, call: &str, path: Value) -> Result<Option<Stat>, Error> {
    require_mount(mrb, call)?;
    let path = path_text(mrb, path)?;
    Ok(stat::stat_of(&path).ok().filter(|_| !path.is_empty()))
}

fn file_s_exist_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(stat_or_none(mrb, "File.exist?", path)?.is_some())
}

fn file_s_file_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(stat_or_none(mrb, "File.file?", path)?.is_some_and(Stat::is_file))
}

fn file_s_directory_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(stat_or_none(mrb, "File.directory?", path)?.is_some_and(Stat::is_dir))
}

fn file_s_size(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.size")?;
    let path = path_arg(mrb, path)?;
    let len = stat::stat_of(&path)
        .map_err(|err| errno::from_io(mrb, &err, &path))?
        .size();
    Ok(i64::try_from(len).unwrap_or(i64::MAX).into_value(mrb))
}

fn file_s_delete(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    require_mount(mrb, "File.delete")?;
    for &path in args {
        let path = path_arg(mrb, path)?;
        fs::remove_file(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    }
    Ok(i64::try_from(args.len())
        .unwrap_or(i64::MAX)
        .into_value(mrb))
}

fn file_s_rename(mrb: &Mrb, _self: Value, from: Value, to: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.rename")?;
    let (from, to) = (path_arg(mrb, from)?, path_arg(mrb, to)?);
    fs::rename(&from, &to).map_err(|err| errno::from_io(mrb, &err, &format!("({from}, {to})")))?;
    Ok(0.into_value(mrb))
}

/// The bytes of the path `value` names, a String or what its `to_path`, else
/// its `to_str`, answers, as CRuby's `rb_get_path` reads one. One holding a
/// NUL raises `ArgumentError`, and any other byte is taken as it stands, as
/// CRuby's `check_path_encoding` refuses only an encoding with no ASCII, so a
/// helper that touches nothing reads any path.
fn path_bytes_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    let to_path = Symbol::new(mrb, "to_path")?.as_value();
    let path = match RString::from_value(value) {
        Some(path) => path,
        None if value.funcall::<_, _, bool>(mrb, c"respond_to?", [to_path])? => {
            string_value_of(mrb, value.funcall::<_, _, Value>(mrb, c"to_path", ())?)?
        }
        None => string_value_of(mrb, value)?,
    };
    // SAFETY: the bytes are copied before the next mruby call.
    let bytes = unsafe { path.as_slice(mrb) }.to_vec();
    if bytes.contains(&0) {
        return Err(argument_error(mrb, "string contains null byte"));
    }
    Ok(bytes)
}

/// `File.join(*parts)`, an Array among them joined in its place, as CRuby's
/// `rb_file_join` does: a part leading with `/` replaces the separators the
/// joined path ends in, and one without is set after a single `/` unless
/// the path already ends in one.
fn file_s_join(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let mut within = Vec::new();
    let joined = join_parts(mrb, args, &mut within)?;
    Ok(mrb.str_new(&joined).as_value())
}

fn join_parts(mrb: &Mrb, parts: &[Value], within: &mut Vec<i64>) -> Result<Vec<u8>, Error> {
    let mut joined = Vec::new();
    for (index, &part) in parts.iter().enumerate() {
        let part = match RArray::from_value(part) {
            Some(nested) => {
                let id = part.object_id();
                if within.contains(&id) {
                    return Err(argument_error(mrb, "recursive array"));
                }
                within.push(id);
                let nested: Vec<Value> = nested.entries(mrb).collect();
                let part = join_parts(mrb, &nested, within)?;
                within.pop();
                part
            }
            None => path_bytes_of(mrb, part)?,
        };
        if index > 0 {
            let tail = trimmed_path_of(&joined).len();
            if part.first() == Some(&b'/') {
                joined.truncate(tail);
            } else if tail == joined.len() {
                joined.push(b'/');
            }
        }
        joined.extend_from_slice(&part);
    }
    Ok(joined)
}

/// `path` without the separators it ends in, as CRuby's `chompdirsep` finds
/// where they start.
fn trimmed_path_of(path: &[u8]) -> &[u8] {
    let separators = path.iter().rev().take_while(|b| **b == b'/').count();
    &path[..path.len() - separators]
}

/// The last component, trailing separators dropped; `/` for the root.
fn last_component_of(path: &[u8]) -> &[u8] {
    let trimmed = trimmed_path_of(path);
    if trimmed.is_empty() {
        return if path.is_empty() { b"" } else { b"/" };
    }
    trimmed.rsplit(|b| *b == b'/').next().unwrap_or(trimmed)
}

/// Where a base's extension starts, as CRuby's `ruby_enc_find_extname` reads
/// it: its leading dots skipped, then its last dot, a dot at the end naming
/// `.` alone.
fn extname_start_of(base: &[u8]) -> Option<usize> {
    let leading = base.iter().take_while(|b| **b == b'.').count();
    base[leading..]
        .iter()
        .rposition(|b| *b == b'.')
        .map(|dot| leading + dot)
}

fn file_s_basename(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(Value,), (Option<Value>,), (), (), (), ()>(mrb)?;
    let path = path_bytes_of(mrb, args.required.0)?;
    let mut base = last_component_of(&path).to_vec();
    if let Some(suffix) = args.optional.0 {
        let suffix = path_bytes_of(mrb, suffix)?;
        if suffix == b".*" {
            if let Some(dot) = extname_start_of(&base) {
                base.truncate(dot);
            }
        } else if base.len() > suffix.len() && base.ends_with(&suffix) {
            base.truncate(base.len() - suffix.len());
        }
    }
    Ok(mrb.str_new(&base).as_value())
}

fn file_s_dirname(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let path = path_bytes_of(mrb, path)?;
    let trimmed = trimmed_path_of(&path);
    let dir: &[u8] = match trimmed.iter().rposition(|b| *b == b'/') {
        Some(0) => b"/",
        Some(at) => trimmed_path_of(&trimmed[..at]),
        None if path.first() == Some(&b'/') => b"/",
        None => b".",
    };
    let dir: &[u8] = if dir.is_empty() { b"/" } else { dir };
    Ok(mrb.str_new(dir).as_value())
}

fn file_s_extname(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let path = path_bytes_of(mrb, path)?;
    let base = last_component_of(&path);
    let ext = extname_start_of(base).map_or(&b""[..], |dot| &base[dot..]);
    Ok(mrb.str_new(ext).as_value())
}
