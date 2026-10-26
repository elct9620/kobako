//! How `File.open` reads its mode into the flags its open hands wasi-libc.

use core::ffi::c_int;
use std::ffi::CStr;

use beni::prelude::*;
use beni::{Error, Integer, Module, Mrb, RClass, Value};

use crate::errno;
use crate::io::argument_error;

// CRuby's values on Linux, the platform a script most likely assumes.
const RDONLY: i64 = 0;
const WRONLY: i64 = 1;
const RDWR: i64 = 2;
const CREAT: i64 = 0o100;
const EXCL: i64 = 0o200;
const TRUNC: i64 = 0o1000;
const APPEND: i64 = 0o2000;

/// The flags an open honors, each named as `File::Constants` names it,
/// beside the wasi-libc `O_*` it asks for.
const FLAGS: [(&CStr, i64, c_int); 7] = [
    (c"RDONLY", RDONLY, 0x0400_0000),
    (c"WRONLY", WRONLY, 0x1000_0000),
    (c"RDWR", RDWR, 0x0400_0000 | 0x1000_0000),
    (c"CREAT", CREAT, 1 << 12),
    (c"EXCL", EXCL, 4 << 12),
    (c"TRUNC", TRUNC, 8 << 12),
    (c"APPEND", APPEND, 1),
];

/// `File::Constants`, which `IO` includes as CRuby's does.
pub(super) fn define_constants(mrb: &Mrb, file: RClass, io: RClass) -> Result<(), Error> {
    let constants = file.define_module(mrb, c"Constants")?;
    for (name, value, _) in FLAGS {
        constants.const_set(mrb, name, value)?;
    }
    io.include_module(mrb, constants)
}

/// The wasi-libc flags opening `path` with `mode` asks for: an Integer of
/// `File::Constants` flags, an access-mode String, or `nil` to read.
pub(super) fn open_flags(mrb: &Mrb, mode: Option<Value>, path: &str) -> Result<c_int, Error> {
    let flags = match mode.filter(|mode| !mode.is_nil()) {
        None => RDONLY,
        Some(mode) => match Integer::from_value(mode) {
            Some(flags) => flags.to_i64(mrb)?,
            None => match Vec::<u8>::from_value(mode) {
                Some(mode) => from_string(mrb, &String::from_utf8_lossy(&mode))?,
                None => return Err(errno::type_error(mrb, mode)),
            },
        },
    };
    to_libc(flags).ok_or_else(|| errno::error_by_name(mrb, c"EINVAL", path))
}

/// The wasi-libc flags for CRuby's `flags`, or `None` for a bit no flag
/// names or `WRONLY` beside `RDWR`, either of which would open differently
/// than CRuby does.
fn to_libc(flags: i64) -> Option<c_int> {
    let (access, others) = FLAGS.split_at(3);
    let (_, _, mut libc) = *access
        .iter()
        .find(|(_, flag, _)| *flag == flags & (WRONLY | RDWR))?;
    let mut rest = flags & !(WRONLY | RDWR);
    for (_, flag, o_flag) in others {
        if rest & flag != 0 {
            libc |= o_flag;
            rest &= !flag;
        }
    }
    (rest == 0).then_some(libc)
}

/// CRuby's access mode as `File::Constants` flags: `r`, `w`, or `a`, then
/// any of `+`, `b`, and `t`, with `x` after `w` alone for an exclusive
/// create. `b` and `t` exclude each other.
fn from_string(mrb: &Mrb, mode: &str) -> Result<i64, Error> {
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
    let write = if plus { RDWR } else { WRONLY };
    Ok(match access {
        'r' if plus => RDWR,
        'r' => RDONLY,
        'w' if exclusive => write | CREAT | TRUNC | EXCL,
        'w' => write | CREAT | TRUNC,
        _ => write | CREAT | APPEND,
    })
}
