//! `::File < IO`: the files beneath the paths a Host App mounted.
//!
//! A path resolves through wasi-libc against the preopened directories
//! the host presents, one per Mount, and the host decides what each open
//! may do. With no Mount at all there is nothing to open, so
//! every call that touches a path raises `NotImplementedError`, as CRuby
//! does for a call its platform lacks. The path helpers (`join`,
//! `basename`, `dirname`, `extname`) touch nothing and always answer.

use std::fs;
use std::io::Write;
use std::os::fd::IntoRawFd;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, IntoValue, Mrb, Object, Proc, RArray, RClass, Value};

use crate::io::{argument_error, Stream};
use crate::stream::with_file;
use crate::{descriptors, errno};

pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    use beni::Module;

    let file = mrb.define_class(c"File", io)?;
    file.const_set(mrb, c"SEPARATOR", mrb.str_new(b"/").as_value())?;
    file.define_method(mrb, c"initialize", beni::method!(file_initialize, -1))?;
    file.define_method(mrb, c"path", beni::method!(file_path, 0))?;
    file.define_method(mrb, c"size", beni::method!(file_size, 0))?;

    file.define_singleton_method(mrb, c"open", beni::method!(file_s_open, -1))?;
    file.define_singleton_method(mrb, c"read", beni::method!(file_s_read, 1))?;
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
    Ok(())
}

/// Whether the host presents any mounted directory: a preopen is always
/// descriptor 3 onwards.
#[cfg(target_os = "wasi")]
fn mounted() -> bool {
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
fn mounted() -> bool {
    true
}

fn require_mount(mrb: &Mrb, call: &str) -> Result<(), Error> {
    if mounted() {
        return Ok(());
    }
    Err(errno::not_implemented(
        mrb,
        &format!("{call} is unimplemented: this Sandbox mounts no filesystem"),
    ))
}

/// A path as a String names it. One whose bytes are no valid UTF-8 names
/// nothing WASI can reach, so it fails as the engine would answer it.
fn path_arg(mrb: &Mrb, value: Value) -> Result<String, Error> {
    if let Some(bytes) = Vec::<u8>::from_value(value) {
        return String::from_utf8(bytes).map_err(|err| {
            let path = String::from_utf8_lossy(err.as_bytes());
            errno::named(mrb, c"EILSEQ", &path)
        });
    }
    Err(match mrb.exception_type_error() {
        Ok(cls) => Error::new(
            mrb,
            cls,
            &format!(
                "no implicit conversion of {} into String",
                value.classname(mrb)
            ),
        ),
        Err(err) => err,
    })
}

/// A CRuby access mode: `r`, `w`, or `a`, each with an optional `+`, and
/// `b` / `t` accepted and ignored.
fn options(mrb: &Mrb, mode: &str) -> Result<fs::OpenOptions, Error> {
    let plain: String = mode.chars().filter(|c| *c != 'b' && *c != 't').collect();
    let mut options = fs::OpenOptions::new();
    match plain.as_str() {
        "r" => options.read(true),
        "r+" => options.read(true).write(true),
        "w" => options.write(true).create(true).truncate(true),
        "w+" => options.read(true).write(true).create(true).truncate(true),
        "a" => options.append(true).create(true),
        "a+" => options.read(true).append(true).create(true),
        _ => return Err(argument_error(mrb, &format!("invalid access mode {mode}"))),
    };
    Ok(options)
}

fn file_initialize(mrb: &Mrb, self_: Value, _args: &[Value]) -> Result<(), Error> {
    require_mount(mrb, "File.open")?;
    let args = scan_args::<(Value,), (Option<Value>, Option<Value>), (), (), (), ()>(mrb)?;
    let (path,) = args.required;
    let (mode, _perm) = args.optional;
    let path = path_arg(mrb, path)?;
    let mode = match mode.filter(|v| !v.is_nil()) {
        Some(mode) => path_arg(mrb, mode)?,
        None => "r".to_string(),
    };
    let file = options(mrb, &mode)?
        .open(&path)
        .map_err(|err| errno::from_io(mrb, &err, &path))?;
    let fd = file.into_raw_fd();
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
    let fd = crate::io::checked_fd(
        mrb,
        self_,
        "kobako File reaches only a file this guest opened",
    )?;
    let len = with_file(fd, |file| Ok(file.metadata()?.len()))
        .map_err(|err| errno::from_io(mrb, &err, &crate::stream::label(mrb, self_)))?;
    Ok(i64::try_from(len).unwrap_or(i64::MAX).into_value(mrb))
}

/// `File.open` with a block yields the file and closes it however the
/// block ends, answering the block's value.
fn file_s_open(mrb: &Mrb, self_: RClass, args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?.block;
    let file = crate::io::new(mrb, self_, args)?;
    let Some(block) = block else {
        return Ok(file);
    };
    let answer = block.call(mrb, [file]);
    file.funcall::<_, _, Value>(mrb, c"close", ())?;
    answer
}

fn file_s_read(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.read")?;
    let path = path_arg(mrb, path)?;
    let bytes = fs::read(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(mrb.str_new(&bytes).as_value())
}

fn file_s_write(mrb: &Mrb, _self: Value, path: Value, data: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.write")?;
    let path = path_arg(mrb, path)?;
    let data = data.to_r_string(mrb)?;
    // SAFETY: the bytes are copied out before the next mruby call.
    let bytes = unsafe { data.as_bytes(mrb) }.to_vec();
    fs::File::create(&path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(i64::try_from(bytes.len())
        .unwrap_or(i64::MAX)
        .into_value(mrb))
}

fn metadata(mrb: &Mrb, call: &str, path: Value) -> Result<Option<fs::Metadata>, Error> {
    require_mount(mrb, call)?;
    let path = path_arg(mrb, path)?;
    Ok(fs::metadata(path).ok())
}

fn file_s_exist_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(metadata(mrb, "File.exist?", path)?.is_some())
}

fn file_s_file_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(metadata(mrb, "File.file?", path)?.is_some_and(|m| m.is_file()))
}

fn file_s_directory_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(metadata(mrb, "File.directory?", path)?.is_some_and(|m| m.is_dir()))
}

fn file_s_size(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    require_mount(mrb, "File.size")?;
    let path = path_arg(mrb, path)?;
    let len = fs::metadata(&path)
        .map_err(|err| errno::from_io(mrb, &err, &path))?
        .len();
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

/// Join with single separators between the parts, as CRuby does.
fn file_s_join(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let mut joined = String::new();
    for (index, &part) in args.iter().enumerate() {
        let part = path_arg(mrb, part)?;
        if index == 0 {
            joined.push_str(&part);
            continue;
        }
        match (joined.ends_with('/'), part.starts_with('/')) {
            (true, true) => joined.push_str(&part[1..]),
            (false, false) => {
                joined.push('/');
                joined.push_str(&part);
            }
            _ => joined.push_str(&part),
        }
    }
    Ok(mrb.str_new(joined.as_bytes()).as_value())
}

/// The last component, trailing separators dropped; `/` for the root.
fn last_component(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "" } else { "/" };
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

fn file_s_basename(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(Value,), (Option<Value>,), (), (), (), ()>(mrb)?;
    let path = path_arg(mrb, args.required.0)?;
    let mut base = last_component(&path).to_string();
    if let Some(suffix) = args.optional.0 {
        let suffix = path_arg(mrb, suffix)?;
        if suffix == ".*" {
            if let Some(dot) = base.rfind('.').filter(|dot| *dot > 0) {
                base.truncate(dot);
            }
        } else if base.len() > suffix.len() && base.ends_with(&suffix) {
            base.truncate(base.len() - suffix.len());
        }
    }
    Ok(mrb.str_new(base.as_bytes()).as_value())
}

fn file_s_dirname(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let path = path_arg(mrb, path)?;
    let trimmed = path.trim_end_matches('/');
    let dir = match trimmed.rfind('/') {
        Some(0) => "/",
        Some(at) => trimmed[..at].trim_end_matches('/'),
        None if path.starts_with('/') => "/",
        None => ".",
    };
    let dir = if dir.is_empty() { "/" } else { dir };
    Ok(mrb.str_new(dir.as_bytes()).as_value())
}

fn file_s_extname(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let path = path_arg(mrb, path)?;
    let base = last_component(&path);
    let ext = match base.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < base.len() => &base[dot..],
        _ => "",
    };
    Ok(mrb.str_new(ext.as_bytes()).as_value())
}
