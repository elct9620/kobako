//! `::Dir`: the directories beneath the paths a Host App mounted, listed,
//! made, and removed as CRuby's `Dir` does on Linux.
//!
//! A path resolves as `File`'s does, and with no Mount every call raises
//! `NotImplementedError`. An opened `Dir` lists its entries when it opens
//! and again on each walk and `rewind`, as POSIX leaves a change made since
//! then unspecified. The guest has no `Enumerator`, so a call CRuby answers with
//! one without a block raises instead.

use std::cell::RefCell;
use std::fs;
use std::io;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{Error, IntoValue, Module, Mrb, Proc, RClass, TryConvert, TypedData, Value};

use crate::errno;
use crate::file::{path_arg, require_mount, stat_or_none};
use crate::io::argument_error;

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

/// The names in `path` but `.` and `..`, as the host lists them.
fn children(path: &str) -> io::Result<Vec<Vec<u8>>> {
    fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.file_name().into_encoded_bytes()))
        .collect()
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
