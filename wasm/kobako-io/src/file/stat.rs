//! `File::Stat`: what an entry reported when it was asked, kept from then on.
//!
//! It answers what a preview1 `filestat` holds and wasi-libc passes on. It
//! defines no method for the rest, so a script fails where it would otherwise
//! read a wrong answer: wasi-libc fills in no owner or permission, a FIFO and a
//! block device reach it as no kind at all, the guest has no `Time` to give a
//! time in, and the host shows every entry on one device.

use std::ffi::{CStr, CString};
use std::io;

use beni::prelude::*;
use beni::value::qnil;
use beni::{Error, IntoValue, Module, Mrb, RClass, RTypedData, TryConvert, TypedData, Value};

use super::{path_arg, require_mount};
use crate::errno;
use crate::io::argument_error;

// wasi-libc's `st_mode` kinds.
const S_IFMT: u32 = 0xf000;
const S_IFCHR: u32 = 0x2000;
const S_IFDIR: u32 = 0x4000;
const S_IFREG: u32 = 0x8000;
const S_IFLNK: u32 = 0xa000;
const S_IFSOCK: u32 = 0xc000;

/// What an entry reported, kept in the payload so a script's instance
/// variables cannot rewrite it.
#[derive(Clone, Copy)]
#[beni::wrap(class = "File::Stat", name = "Kobako::File::Stat")]
struct Stat {
    ino: u64,
    nlink: u64,
    size: u64,
    mode: u32,
}

impl Stat {
    fn of(mrb: &Mrb, value: Value) -> Result<Self, Error> {
        <&Stat>::try_convert(value, mrb).copied()
    }

    fn is(self, kind: u32) -> bool {
        self.mode & S_IFMT == kind
    }

    /// `File::Stat#ftype`'s name for the kind, which reads `"unknown"` for a
    /// FIFO and a block device, as no kind reaches wasi-libc for either.
    fn ftype(self) -> &'static str {
        match self.mode & S_IFMT {
            S_IFREG => "file",
            S_IFDIR => "directory",
            S_IFLNK => "link",
            S_IFCHR => "characterSpecial",
            S_IFSOCK => "socket",
            _ => "unknown",
        }
    }

    /// The size, `nil` for an empty entry, as `size?` answers.
    fn size_value(self, mrb: &Mrb) -> Value {
        match self.size {
            0 => qnil().as_value(),
            size => size.into_value(mrb),
        }
    }
}

/// The calls that ask the host about an entry. wasi-libc's `struct stat` is
/// spelled out here, as `std` keeps the inode number and the count of names
/// behind an unstable feature on this target.
#[cfg(target_os = "wasi")]
mod raw {
    use core::ffi::{c_char, c_int};
    use std::ffi::CStr;
    use std::io;
    use std::mem::MaybeUninit;

    use super::Stat;

    /// The fields of wasi-libc's `struct stat` up to `st_size`, then the rest
    /// of its 144 bytes.
    #[repr(C)]
    struct CStat {
        dev: u64,
        ino: u64,
        nlink: u64,
        mode: u32,
        owner: [u32; 3],
        rdev: u64,
        size: i64,
        rest: [u64; 11],
    }

    unsafe extern "C" {
        fn stat(path: *const c_char, buf: *mut CStat) -> c_int;
        fn lstat(path: *const c_char, buf: *mut CStat) -> c_int;
        fn fstat(fd: c_int, buf: *mut CStat) -> c_int;
    }

    /// What `call` left in the buffer it was handed, or the failure it set.
    fn asked(call: impl FnOnce(*mut CStat) -> c_int) -> io::Result<Stat> {
        let mut found = MaybeUninit::<CStat>::uninit();
        if call(found.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded, so it filled the buffer.
        let found = unsafe { found.assume_init() };
        Ok(Stat {
            ino: found.ino,
            nlink: found.nlink,
            size: u64::try_from(found.size).unwrap_or(0),
            mode: found.mode,
        })
    }

    pub(super) fn following(path: &CStr) -> io::Result<Stat> {
        // SAFETY: `path` is NUL-terminated and `buf` is as large as `stat` writes.
        asked(|buf| unsafe { stat(path.as_ptr(), buf) })
    }

    pub(super) fn not_following(path: &CStr) -> io::Result<Stat> {
        // SAFETY: `path` is NUL-terminated and `buf` is as large as `lstat` writes.
        asked(|buf| unsafe { lstat(path.as_ptr(), buf) })
    }

    pub(super) fn of_descriptor(fd: i32) -> io::Result<Stat> {
        // SAFETY: `buf` is as large as `fstat` writes, and a bad `fd` only fails.
        asked(|buf| unsafe { fstat(fd, buf) })
    }
}

/// The same calls on the host target the guest crates' unit tests link
/// against, which reads `std`'s spelling of them.
#[cfg(not(target_os = "wasi"))]
mod raw {
    use std::ffi::{CStr, OsStr};
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::Stat;

    fn reported(metadata: fs::Metadata) -> Stat {
        Stat {
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            size: metadata.size(),
            mode: metadata.mode(),
        }
    }

    fn named(path: &CStr) -> &Path {
        Path::new(OsStr::from_bytes(path.to_bytes()))
    }

    pub(super) fn following(path: &CStr) -> io::Result<Stat> {
        fs::metadata(named(path)).map(reported)
    }

    pub(super) fn not_following(path: &CStr) -> io::Result<Stat> {
        fs::symlink_metadata(named(path)).map(reported)
    }

    pub(super) fn of_descriptor(fd: i32) -> io::Result<Stat> {
        crate::stream::with_file(fd, |file| file.metadata().map(reported))
    }
}

pub(super) fn init(mrb: &Mrb, file: RClass) -> Result<(), Error> {
    let stat = file.define_class(mrb, c"Stat", mrb.class_object())?;
    Stat::mark_carriers(mrb)?;
    stat.define_singleton_method(mrb, c"new", beni::method!(stat_s_new, 1))?;
    stat.define_method(mrb, c"initialize_copy", beni::method!(stat_copy, 1))?;
    stat.define_method(mrb, c"ino", beni::method!(stat_ino, 0))?;
    stat.define_method(mrb, c"nlink", beni::method!(stat_nlink, 0))?;
    stat.define_method(mrb, c"size", beni::method!(stat_size, 0))?;
    stat.define_method(mrb, c"size?", beni::method!(stat_size_p, 0))?;
    stat.define_method(mrb, c"zero?", beni::method!(stat_zero_p, 0))?;
    stat.define_method(mrb, c"ftype", beni::method!(stat_ftype, 0))?;
    stat.define_method(mrb, c"file?", beni::method!(stat_file_p, 0))?;
    stat.define_method(mrb, c"directory?", beni::method!(stat_directory_p, 0))?;
    stat.define_method(mrb, c"symlink?", beni::method!(stat_symlink_p, 0))?;
    stat.define_method(mrb, c"chardev?", beni::method!(stat_chardev_p, 0))?;
    stat.define_method(mrb, c"socket?", beni::method!(stat_socket_p, 0))?;

    file.define_method(mrb, c"stat", beni::method!(file_stat, 0))?;
    file.define_method(mrb, c"lstat", beni::method!(file_lstat, 0))?;
    file.define_singleton_method(mrb, c"stat", beni::method!(file_s_stat, 1))?;
    file.define_singleton_method(mrb, c"lstat", beni::method!(file_s_lstat, 1))?;
    file.define_singleton_method(mrb, c"ftype", beni::method!(file_s_ftype, 1))?;
    file.define_singleton_method(mrb, c"symlink?", beni::method!(file_s_symlink_p, 1))?;
    file.define_singleton_method(mrb, c"size?", beni::method!(file_s_size_p, 1))?;
    file.define_singleton_method(mrb, c"zero?", beni::method!(file_s_zero_p, 1))?;
    file.define_singleton_method(mrb, c"empty?", beni::method!(file_s_zero_p, 1))?;
    Ok(())
}

/// What `ask` answers for `path`, kept apart from a failure of the path
/// itself, which every call raises.
fn asked_of(
    mrb: &Mrb,
    call: &str,
    path: Value,
    ask: fn(&CStr) -> io::Result<Stat>,
) -> Result<(String, io::Result<Stat>), Error> {
    require_mount(mrb, call)?;
    let path = path_arg(mrb, path)?;
    let c_path = CString::new(path.as_str())
        .map_err(|_| argument_error(mrb, "string contains null byte"))?;
    let found = ask(&c_path);
    Ok((path, found))
}

/// As `asked_of`, the host's failure raising the `Errno` it names.
fn required(
    mrb: &Mrb,
    call: &str,
    path: Value,
    ask: fn(&CStr) -> io::Result<Stat>,
) -> Result<Stat, Error> {
    let (path, found) = asked_of(mrb, call, path, ask)?;
    found.map_err(|err| errno::from_io(mrb, &err, &path))
}

fn wrapped(mrb: &Mrb, stat: Stat) -> Value {
    mrb.wrap(stat).as_value()
}

fn stat_s_new(mrb: &Mrb, class: RClass, path: Value) -> Result<Value, Error> {
    let stat = required(mrb, "File::Stat.new", path, raw::following)?;
    Ok(mrb.wrap_as(stat, class).as_value())
}

/// `dup` and `clone` copy the payload, which nothing changes afterward.
fn stat_copy(mrb: &Mrb, self_: RTypedData, orig: Value) -> Result<Value, Error> {
    // A receiver already holding a report keeps it, as a report never changes.
    let _ = self_.init(mrb, Stat::of(mrb, orig)?);
    Ok(self_.as_value())
}

fn stat_ino(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Stat::of(mrb, self_)?.ino.into_value(mrb))
}

fn stat_nlink(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Stat::of(mrb, self_)?.nlink.into_value(mrb))
}

fn stat_size(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Stat::of(mrb, self_)?.size.into_value(mrb))
}

fn stat_size_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Stat::of(mrb, self_)?.size_value(mrb))
}

fn stat_zero_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.size == 0)
}

fn stat_ftype(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let ftype = Stat::of(mrb, self_)?.ftype();
    Ok(mrb.str_new(ftype.as_bytes()).as_value())
}

fn stat_file_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.is(S_IFREG))
}

fn stat_directory_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.is(S_IFDIR))
}

fn stat_symlink_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.is(S_IFLNK))
}

fn stat_chardev_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.is(S_IFCHR))
}

fn stat_socket_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Stat::of(mrb, self_)?.is(S_IFSOCK))
}

fn file_stat(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = crate::io::checked_fd(
        mrb,
        self_,
        "kobako File reaches only a file this guest opened",
    )?;
    let stat = raw::of_descriptor(fd)
        .map_err(|err| errno::from_io(mrb, &err, &crate::stream::label(mrb, self_)))?;
    Ok(wrapped(mrb, stat))
}

/// The entry the path it was opened by names now, as `File#lstat` asks by the
/// path rather than the descriptor.
fn file_lstat(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    crate::io::checked_fd(
        mrb,
        self_,
        "kobako File reaches only a file this guest opened",
    )?;
    let path = crate::stream::label(mrb, self_);
    let path = mrb.str_new(path.as_bytes()).as_value();
    Ok(wrapped(
        mrb,
        required(mrb, "File#lstat", path, raw::not_following)?,
    ))
}

fn file_s_stat(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    Ok(wrapped(
        mrb,
        required(mrb, "File.stat", path, raw::following)?,
    ))
}

fn file_s_lstat(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    Ok(wrapped(
        mrb,
        required(mrb, "File.lstat", path, raw::not_following)?,
    ))
}

/// Linux's CRuby reads a link as itself here, through `lstat`.
fn file_s_ftype(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let ftype = required(mrb, "File.ftype", path, raw::not_following)?.ftype();
    Ok(mrb.str_new(ftype.as_bytes()).as_value())
}

fn file_s_symlink_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    let (_, found) = asked_of(mrb, "File.symlink?", path, raw::not_following)?;
    Ok(found.is_ok_and(|stat| stat.is(S_IFLNK)))
}

fn file_s_size_p(mrb: &Mrb, _self: Value, path: Value) -> Result<Value, Error> {
    let (_, found) = asked_of(mrb, "File.size?", path, raw::following)?;
    Ok(found.map_or(qnil().as_value(), |stat| stat.size_value(mrb)))
}

fn file_s_zero_p(mrb: &Mrb, _self: Value, path: Value) -> Result<bool, Error> {
    let (_, found) = asked_of(mrb, "File.zero?", path, raw::following)?;
    Ok(found.is_ok_and(|stat| stat.size == 0))
}
