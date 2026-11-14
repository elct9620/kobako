//! `::Dir`: the directories beneath the paths a Host App mounted, listed,
//! made, removed, matched by pattern, and changed into as CRuby's `Dir` does
//! on Linux.
//!
//! A path resolves as `File`'s does, against the working directory wasi-libc
//! keeps, which starts at `/` in every invocation. With no Mount every call
//! that reaches a path raises `NotImplementedError`. An opened `Dir` lists its
//! entries when it opens and again on each walk and `rewind`, as POSIX leaves
//! a change made since then unspecified. The guest has no `Enumerator`, so a
//! call CRuby answers with one without a block raises instead.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::Path;

use beni::prelude::*;
use beni::scan_args::{get_kwargs, scan_args};
use beni::value::qnil;
use beni::{
    Error, IntoValue, Module, Mrb, Proc, Qfalse, Qtrue, RArray, RClass, RHash, TryConvert,
    TypedData, Value,
};

use crate::errno;
use crate::file::{path_arg, path_text, require_mount, stat_or_none};
use crate::io::{argument_error, to_int};

mod glob;

/// What an opened `Dir` stands for, kept in its payload so a script's
/// instance variables cannot rewrite it. Its path outlives the close, as
/// CRuby's does; its entries are `None` once closed.
#[beni::wrap(class = "Dir", name = "Kobako::Dir")]
struct Listing {
    path: String,
    opened: RefCell<Option<OpenEntries>>,
}

struct OpenEntries {
    entries: Vec<Vec<u8>>,
    next: usize,
}

/// The two names every listing opens with, as `readdir(3)` answers them.
const DOTS: [&[u8]; 2] = [b".", b".."];

impl Listing {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Listing, Error> {
        <&Listing>::try_convert(value, mrb)
    }
}

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let dir = mrb.define_class(c"Dir", mrb.class_object())?;
    Listing::mark_carriers(mrb)?;
    dir.include_module(mrb, mrb.module_get(c"Enumerable")?)?;
    dir.define_singleton_method(mrb, c"new", beni::method!(dir_s_new, 1))?;
    dir.define_singleton_method(mrb, c"open", beni::method!(dir_s_open, -1))?;
    dir.define_singleton_method(mrb, c"children", beni::method!(dir_s_children, 1))?;
    dir.define_singleton_method(mrb, c"entries", beni::method!(dir_s_entries, 1))?;
    dir.define_singleton_method(mrb, c"each_child", beni::method!(dir_s_each_child, -1))?;
    dir.define_singleton_method(mrb, c"exist?", beni::method!(dir_s_exist_p, 1))?;
    dir.define_singleton_method(mrb, c"empty?", beni::method!(dir_s_empty_p, 1))?;
    dir.define_singleton_method(mrb, c"mkdir", beni::method!(dir_s_mkdir, -1))?;
    dir.define_singleton_method(mrb, c"rmdir", beni::method!(dir_s_rmdir, 1))?;
    dir.define_singleton_method(mrb, c"delete", beni::method!(dir_s_rmdir, 1))?;
    dir.define_singleton_method(mrb, c"unlink", beni::method!(dir_s_rmdir, 1))?;
    dir.define_singleton_method(mrb, c"glob", beni::method!(dir_s_glob, -1))?;
    dir.define_singleton_method(mrb, c"[]", beni::method!(dir_s_aref, -1))?;
    dir.define_singleton_method(mrb, c"pwd", beni::method!(dir_s_pwd, 0))?;
    dir.define_singleton_method(mrb, c"getwd", beni::method!(dir_s_pwd, 0))?;
    dir.define_singleton_method(mrb, c"chdir", beni::method!(dir_s_chdir, -1))?;
    let constants = mrb.class_get(c"File")?.define_module(mrb, c"Constants")?;
    for (name, value) in glob::FNM {
        constants.const_set(mrb, name, value)?;
    }
    dir.define_method(mrb, c"path", beni::method!(dir_path, 0))?;
    dir.define_method(mrb, c"to_path", beni::method!(dir_path, 0))?;
    dir.define_method(mrb, c"inspect", beni::method!(dir_inspect, 0))?;
    dir.define_method(mrb, c"read", beni::method!(dir_read, 0))?;
    dir.define_method(mrb, c"each", beni::method!(dir_each, 0))?;
    dir.define_method(mrb, c"children", beni::method!(dir_children, 0))?;
    dir.define_method(mrb, c"each_child", beni::method!(dir_each_child, 0))?;
    dir.define_method(mrb, c"rewind", beni::method!(dir_rewind, 0))?;
    dir.define_method(mrb, c"close", beni::method!(dir_close, 0))?;
    Ok(())
}

/// `value` as a path a call beneath a Mount may take.
fn dir_path_arg(mrb: &Mrb, call: &str, value: Value) -> Result<String, Error> {
    require_mount(mrb, call)?;
    path_arg(mrb, value)
}

/// The names in `path` but `.` and `..`, as the host lists them, each beside
/// whether it is a directory and not a link to one.
fn listed(path: impl AsRef<Path>) -> io::Result<Vec<(Vec<u8>, bool)>> {
    fs::read_dir(path)?
        .map(|entry| {
            entry.map(|entry| {
                let real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
                (entry.file_name().into_encoded_bytes(), real_dir)
            })
        })
        .collect()
}

/// The names in `path` but `.` and `..`, as the host lists them.
fn children(path: &str) -> io::Result<Vec<Vec<u8>>> {
    Ok(listed(path)?.into_iter().map(|(name, _)| name).collect())
}

/// The names in `path` behind `.` and `..`, as `readdir(3)` answers them.
fn entries(path: &str) -> io::Result<Vec<Vec<u8>>> {
    let mut all: Vec<Vec<u8>> = DOTS.iter().map(|dot| dot.to_vec()).collect();
    all.extend(children(path)?);
    Ok(all)
}

/// Lists the path `value` names for `call` with `list`.
fn list_path(
    mrb: &Mrb,
    call: &str,
    value: Value,
    list: fn(&str) -> io::Result<Vec<Vec<u8>>>,
) -> Result<Vec<Vec<u8>>, Error> {
    let path = dir_path_arg(mrb, call, value)?;
    list(&path).map_err(|err| errno::from_io(mrb, &err, &path))
}

fn names_value(mrb: &Mrb, names: &[Vec<u8>]) -> Result<Value, Error> {
    let array = mrb.ary_new();
    for name in names {
        array.push(mrb, mrb.str_new(name).as_value())?;
    }
    Ok(array.as_value())
}

fn yield_each(mrb: &Mrb, block: Proc, names: &[Vec<u8>]) -> Result<(), Error> {
    for name in names {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(name).as_value()])?;
    }
    Ok(())
}

/// Opens the directory `value` names for `call`, listing it now.
fn open_listing(mrb: &Mrb, class: RClass, call: &str, value: Value) -> Result<Value, Error> {
    let path = dir_path_arg(mrb, call, value)?;
    let entries = entries(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    let listing = Listing {
        path,
        opened: RefCell::new(Some(OpenEntries { entries, next: 0 })),
    };
    Ok(mrb.wrap_as(listing, class).as_value())
}

fn dir_s_new(mrb: &Mrb, class: RClass, path: Value) -> Result<Value, Error> {
    open_listing(mrb, class, "Dir.new", path)
}

/// `Dir.open` with a block yields the directory and closes it however the
/// block ends, answering the block's value.
fn dir_s_open(mrb: &Mrb, class: RClass, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(Value,), (), (), (), (), Option<Proc>>(mrb)?;
    let dir = open_listing(mrb, class, "Dir.open", args.required.0)?;
    let Some(block) = args.block else {
        return Ok(dir);
    };
    let answer = block.call(mrb, [dir]);
    dir_close(mrb, dir)?;
    answer
}

/// `glob(pattern, flags = 0, base: nil, sort: true) { |path| }`: the paths
/// `pattern`, or each pattern an Array holds, names.
fn dir_s_glob(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(Value,), (Option<Value>,), (), (), RHash, Option<Proc>>(mrb)?;
    let (patterns,) = args.required;
    let (flags,) = args.optional;
    let flags = match flags {
        Some(flags) => to_int(mrb, flags)?.to_i64(mrb)?,
        None => 0,
    };
    let patterns = match RArray::from_value(patterns) {
        Some(each) => each.entries(mrb).collect(),
        None => vec![patterns],
    };
    globbed(mrb, "Dir.glob", &patterns, args.keywords, flags, args.block)
}

/// `Dir[*patterns, base: nil, sort: true]`: the paths each pattern names.
fn dir_s_aref(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(), (), RArray, (), RHash, ()>(mrb)?;
    let patterns: Vec<Value> = args.splat.entries(mrb).collect();
    globbed(mrb, "Dir[]", &patterns, args.keywords, 0, None)
}

/// The paths each of `patterns` names in turn, each yielded to `block` with
/// `nil` answered, or all of them answered without one.
fn globbed(
    mrb: &Mrb,
    call: &str,
    patterns: &[Value],
    keywords: RHash,
    flags: i64,
    block: Option<Proc>,
) -> Result<Value, Error> {
    require_mount(mrb, call)?;
    let kw = get_kwargs::<_, (), (Option<Value>, Option<Value>), ()>(
        mrb,
        keywords,
        &[],
        &["base", "sort"],
    )?;
    let (base, sort) = kw.optional;
    let base = match base.filter(|base| !base.is_nil()) {
        Some(base) => Some(path_text(mrb, base)?).filter(|base| !base.is_empty()),
        None => None,
    };
    let sort = match sort {
        None => true,
        Some(sort) if Qtrue::from_value(sort).is_some() => true,
        Some(sort) if Qfalse::from_value(sort).is_some() => false,
        Some(_) => return Err(argument_error(mrb, "expected true or false as sort:")),
    };
    let options = glob::Options::new(flags, sort);
    let mut found = Vec::new();
    for pattern in patterns {
        let pattern = path_text(mrb, *pattern)?;
        found.extend(glob::glob(
            pattern.as_bytes(),
            base.as_deref().map(str::as_bytes),
            options,
        ));
    }
    match block {
        Some(block) => {
            yield_each(mrb, block, &found)?;
            Ok(qnil().as_value())
        }
        None => names_value(mrb, &found),
    }
}

/// The working directory, `/` until a script changes it.
fn dir_s_pwd(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Ok(mrb.str_new(&cwd(mrb)?).as_value())
}

/// The working directory's name, as wasi-libc's `getcwd` keeps it.
fn cwd(mrb: &Mrb) -> Result<Vec<u8>, Error> {
    let cwd = std::env::current_dir().map_err(|err| errno::from_io(mrb, &err, "."))?;
    Ok(cwd.into_os_string().into_encoded_bytes())
}

/// `chdir(path) { |path| }`: make `path` the working directory, `0`
/// answered; with a block, only while it runs, the block's value answered
/// and the earlier directory restored however it ends. The guest has no
/// `HOME`, so a call naming no path raises, as CRuby's does without one.
fn dir_s_chdir(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(), (Option<Value>,), (), (), (), Option<Proc>>(mrb)?;
    let (target,) = args.optional;
    let Some(target) = target else {
        return Err(argument_error(mrb, "HOME/LOGDIR not set"));
    };
    let path = dir_path_arg(mrb, "Dir.chdir", target)?;
    let Some(block) = args.block else {
        change_dir(mrb, &path)?;
        return Ok(0.into_value(mrb));
    };
    let previous = cwd(mrb)?;
    change_dir(mrb, &path)?;
    let answer = block.call(mrb, [mrb.str_new(path.as_bytes()).as_value()]);
    name_cwd(&previous);
    answer
}

/// Change into `path`, the host judging it as Linux's `chdir(2)` does, then
/// name the result with no `.` or `..` and one separator between names, as
/// Linux's `getcwd(3)` answers. wasi-libc keeps the path as joined, and the
/// guest reads no link's target, so a `..` steps back by name.
fn change_dir(mrb: &Mrb, path: &str) -> Result<(), Error> {
    std::env::set_current_dir(path).map_err(|err| errno::from_io(mrb, &err, path))?;
    let joined = cwd(mrb)?;
    let mut names: Vec<&[u8]> = Vec::new();
    for name in joined.split(|&byte| byte == b'/') {
        match name {
            b"" | b"." => {}
            b".." => {
                names.pop();
            }
            name => names.push(name),
        }
    }
    name_cwd(&[b"/".as_slice(), &names.join(&b'/')].concat());
    Ok(())
}

/// Name the working directory `name` without asking the host again: it
/// names a directory a change already reached, and the starting `/` is one
/// no preopen covers, so a `chdir` could not return to it.
#[cfg(target_os = "wasi")]
fn name_cwd(name: &[u8]) {
    use core::ffi::{c_char, c_void};

    unsafe extern "C" {
        /// wasi-libc's working directory, which `getcwd` answers and every
        /// relative path joins onto; `chdir` frees the one it replaces once
        /// it has set one with `malloc`.
        static mut __wasilibc_cwd: *mut c_char;
        fn strdup(name: *const c_char) -> *mut c_char;
        fn free(name: *mut c_void);
    }
    let Ok(name) = std::ffi::CString::new(name) else {
        return;
    };
    // SAFETY: the guest runs one thread, so nothing reads the global while it
    // changes. Only a change reaches here, so the name it replaces is one
    // `chdir` allocated, and `strdup` allocates the next with the same
    // `malloc`, which the next `chdir` frees.
    unsafe {
        let copied = strdup(name.as_ptr());
        if copied.is_null() {
            return;
        }
        let replaced = __wasilibc_cwd;
        __wasilibc_cwd = copied;
        free(replaced.cast());
    }
}

#[cfg(not(target_os = "wasi"))]
fn name_cwd(name: &[u8]) {
    // SAFETY: `name` is spliced from the bytes `current_dir` answered at
    // separators, which keeps each name's encoding whole.
    let name = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(name) };
    let _ = std::env::set_current_dir(name);
}

fn dir_s_children(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    names_value(mrb, &list_path(mrb, "Dir.children", path, children)?)
}

fn dir_s_entries(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    names_value(mrb, &list_path(mrb, "Dir.entries", path, entries)?)
}

fn dir_s_each_child(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let args = scan_args::<(Value,), (), (), (), (), Option<Proc>>(mrb)?;
    let block = args
        .block
        .ok_or_else(|| argument_error(mrb, "Dir.each_child needs a block"))?;
    let names = list_path(mrb, "Dir.each_child", args.required.0, children)?;
    yield_each(mrb, block, &names)?;
    Ok(qnil().as_value())
}

fn dir_s_exist_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    Ok(stat_or_none(mrb, "Dir.exist?", path)?.is_some_and(|found| found.is_dir()))
}

/// Anything but a directory is not an empty one, as CRuby answers; a
/// missing path raises.
fn dir_s_empty_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    let path = dir_path_arg(mrb, "Dir.empty?", path)?;
    match fs::read_dir(&path) {
        Ok(mut names) => Ok(names.next().is_none()),
        Err(err) if err.kind() == io::ErrorKind::NotADirectory => Ok(false),
        Err(err) => Err(errno::from_io(mrb, &err, &path)),
    }
}

/// The permission is read as CRuby reads it, then left unused, as preview1
/// makes a directory with none of its own.
fn dir_s_mkdir(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    let (path,) = scan_args::<(Value,), (Option<i64>,), (), (), (), ()>(mrb)?.required;
    let path = dir_path_arg(mrb, "Dir.mkdir", path)?;
    fs::create_dir(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(0.into_value(mrb))
}

fn dir_s_rmdir(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let path = dir_path_arg(mrb, "Dir.rmdir", path)?;
    fs::remove_dir(&path).map_err(|err| errno::from_io(mrb, &err, &path))?;
    Ok(0.into_value(mrb))
}

/// What `body` makes of the open listing, or `IOError` once it is closed.
fn with_opened<R>(
    mrb: &Mrb,
    self_: Value,
    body: impl FnOnce(&mut OpenEntries) -> R,
) -> Result<R, Error> {
    let listing = Listing::of(mrb, self_)?;
    let mut held = listing.opened.borrow_mut();
    let opened = held
        .as_mut()
        .ok_or_else(|| errno::io_error(mrb, "closed directory"))?;
    Ok(body(opened))
}

/// Lists the open directory again from its start, as CRuby rewinds before
/// each walk, and leaves it at the end.
fn relist(mrb: &Mrb, self_: Value) -> Result<Vec<Vec<u8>>, Error> {
    let path = &Listing::of(mrb, self_)?.path;
    with_opened(mrb, self_, |_| ())?;
    let names = entries(path).map_err(|err| errno::from_io(mrb, &err, path))?;
    with_opened(mrb, self_, |opened| {
        opened.entries = names.clone();
        opened.next = names.len();
    })?;
    Ok(names)
}

fn dir_path(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let path = &Listing::of(mrb, self_)?.path;
    Ok(mrb.str_new(path.as_bytes()).as_value())
}

fn dir_inspect(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let path = &Listing::of(mrb, self_)?.path;
    Ok(mrb.str_new(format!("#<Dir:{path}>").as_bytes()).as_value())
}

fn dir_read(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let name = with_opened(mrb, self_, |opened| {
        let name = opened.entries.get(opened.next).cloned();
        opened.next += usize::from(name.is_some());
        name
    })?;
    Ok(name.map_or(qnil().as_value(), |name| mrb.str_new(&name).as_value()))
}

fn dir_each(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = scan_args::<(), (), (), (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "Dir#each needs a block"))?;
    let names = relist(mrb, self_)?;
    yield_each(mrb, block, &names)?;
    Ok(self_)
}

fn dir_children(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let names = relist(mrb, self_)?;
    names_value(mrb, &names[DOTS.len()..])
}

fn dir_each_child(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let block = scan_args::<(), (), (), (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "Dir#each_child needs a block"))?;
    let names = relist(mrb, self_)?;
    yield_each(mrb, block, &names[DOTS.len()..])?;
    Ok(self_)
}

/// Lists the directory again and reads it from its first entry, as
/// `rewinddir(3)` lets a read see what changed since.
fn dir_rewind(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    relist(mrb, self_)?;
    with_opened(mrb, self_, |opened| opened.next = 0)?;
    Ok(self_)
}

fn dir_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Listing::of(mrb, self_)?.opened.borrow_mut().take();
    Ok(qnil().as_value())
}
