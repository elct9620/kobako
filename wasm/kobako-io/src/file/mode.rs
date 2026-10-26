//! How `File.open` reads its mode into what the open asks of the host.

use std::fs::OpenOptions;

use beni::{Error, Mrb};

use crate::io::argument_error;

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
