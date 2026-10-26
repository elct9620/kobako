//! How `File.open` reads its mode into what the open asks of the host.

use std::ffi::CStr;
use std::fs::OpenOptions;

use beni::{Error, Module, Mrb, RClass};

use crate::errno;
use crate::io::argument_error;

// wasi-libc's `O_*` values, as CRuby's own differ by platform.
const RDONLY: i64 = 0x0400_0000;
const WRONLY: i64 = 0x1000_0000;
const RDWR: i64 = RDONLY | WRONLY;
const CREAT: i64 = 1 << 12;
const EXCL: i64 = 4 << 12;
const TRUNC: i64 = 8 << 12;
const APPEND: i64 = 1;

/// The flags an open honors, each named as `File::Constants` names it.
const FLAGS: [(&CStr, i64); 7] = [
    (c"RDONLY", RDONLY),
    (c"WRONLY", WRONLY),
    (c"RDWR", RDWR),
    (c"CREAT", CREAT),
    (c"EXCL", EXCL),
    (c"TRUNC", TRUNC),
    (c"APPEND", APPEND),
];

/// `File::Constants`, which `IO` includes as CRuby's does.
pub(super) fn define_constants(mrb: &Mrb, file: RClass, io: RClass) -> Result<(), Error> {
    let constants = file.define_module(mrb, c"Constants")?;
    for (name, value) in FLAGS {
        constants.const_set(mrb, name, value)?;
    }
    io.include_module(mrb, constants)
}

/// An Integer mode of `File::Constants` flags. A bit no flag names raises
/// `EINVAL` rather than opening differently than CRuby would, and a mode
/// with no access flag reads, as CRuby's zero `RDONLY` does.
pub(super) fn from_flags(mrb: &Mrb, flags: i64, path: &str) -> Result<OpenOptions, Error> {
    if flags & !(RDWR | CREAT | EXCL | TRUNC | APPEND) != 0 {
        return Err(errno::error_by_name(mrb, c"EINVAL", path));
    }
    let has = |flag| flags & flag == flag;
    let write = has(WRONLY);
    let mut options = OpenOptions::new();
    options
        .read(has(RDONLY) || !write)
        .write(write)
        .append(write && has(APPEND))
        .create(has(CREAT))
        .create_new(has(CREAT | EXCL))
        .truncate(has(TRUNC));
    Ok(options)
}

/// CRuby's access mode: `r`, `w`, or `a`, then any of `+`, `b`, and `t`,
/// with `x` after `w` alone for an exclusive create. `b` and `t` exclude
/// each other.
pub(super) fn from_string(mrb: &Mrb, mode: &str) -> Result<OpenOptions, Error> {
    let invalid = || argument_error(mrb, &format!("invalid access mode {mode}"));
    let mut chars = mode.chars();
    let access = chars
        .next()
        .filter(|c| matches!(c, 'r' | 'w' | 'a'))
        .ok_or_else(invalid)?;
    let (mut plus, mut binary, mut text, mut exclusive) = (false, false, false, false);
    for c in chars {
        match c {
            '+' => plus = true,
            'b' => binary = true,
            't' => text = true,
            'x' if access == 'w' => exclusive = true,
            _ => return Err(invalid()),
        }
    }
    if binary && text {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    match access {
        'r' => options.read(true).write(plus),
        'w' => options
            .read(plus)
            .write(true)
            .create(true)
            .truncate(true)
            .create_new(exclusive),
        _ => options.read(plus).append(true).create(true),
    };
    Ok(options)
}
