//! Reading a stream line by line, as CRuby's `rb_io_getline` does for every
//! IO: `gets`, `readline`, `each_line`, and `readlines` take one separator,
//! limit, and `chomp:` form whatever the stream reads from, so a File and a
//! socket answer alike.

use beni::prelude::*;
use beni::value::qnil;
use beni::{Error, Mrb, RHash, RString, Symbol, TryConvert, Value};

use crate::io::{argument_error, required_block, to_int};
use crate::socket::string_value_of;

/// What a stream reads lines from: the next bytes it has, `None` at the end,
/// and a way to hand back bytes read past a line for the next read.
pub(crate) trait LineSource {
    fn fill(&mut self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error>;

    fn give_back(&mut self, mrb: &Mrb, rest: Vec<u8>) -> Result<(), Error>;
}

/// What `gets` and its kin take: the separator, `None` to read to the end,
/// the limit, and whether to chomp the separator.
pub(crate) type LineArgs = (Option<Vec<u8>>, Option<usize>, bool);

/// `gets(sep = $/, limit = nil, chomp: false)`'s arguments; an empty
/// separator reads a paragraph, as CRuby's does.
pub(crate) fn line_args_of(mrb: &Mrb, args: &[Value]) -> Result<LineArgs, Error> {
    let (args, chomp) = match args.split_last() {
        Some((last, rest)) if RHash::try_convert(*last, mrb).is_ok() => {
            let options = RHash::try_convert(*last, mrb)?;
            let chomp: Value = options.aref(mrb, Symbol::new(mrb, "chomp")?.as_value())?;
            (rest, chomp.to_bool())
        }
        _ => (args, false),
    };
    let (separator, limit) = separator_and_limit_of(mrb, args)?;
    Ok((separator, limit, chomp))
}

/// `gets(sep = $/, limit = nil)`'s separator, `None` to read to the end, and
/// its limit.
fn separator_and_limit_of(
    mrb: &Mrb,
    args: &[Value],
) -> Result<(Option<Vec<u8>>, Option<usize>), Error> {
    let limit_of = |value: Value| -> Result<Option<usize>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        Ok(usize::try_from(to_int(mrb, value)?.to_i64(mrb)?).ok())
    };
    let separator_of = |value: Value| -> Result<Option<Vec<u8>>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        let text = string_value_of(mrb, value)?;
        // SAFETY: the bytes are copied before the next mruby call.
        Ok(Some(unsafe { text.as_slice(mrb) }.to_vec()))
    };
    // One argument that `rb_check_string_type` finds no String in, through
    // `to_str`, is the limit, as CRuby's `rb_io_getline_args` reads it, so a
    // Symbol raises TypeError as a limit.
    let is_limit = |value: Value| -> Result<bool, Error> {
        if value.is_nil() || RString::from_value(value).is_some() {
            return Ok(false);
        }
        let to_str = Symbol::new(mrb, "to_str")?.as_value();
        Ok(!value.funcall::<_, _, bool>(mrb, c"respond_to?", [to_str])?)
    };
    match args {
        [] => Ok((Some(b"\n".to_vec()), None)),
        [only] if is_limit(*only)? => Ok((Some(b"\n".to_vec()), limit_of(*only)?)),
        [separator] => Ok((separator_of(*separator)?, None)),
        [separator, limit] => Ok((separator_of(*separator)?, limit_of(*limit)?)),
        _ => Err(Error::argnum(mrb, args.len(), 0, 2)),
    }
}

/// A limit of zero raises `ArgumentError` for `name`, as CRuby's `readlines`
/// and `each_line` refuse one that would never end.
pub(crate) fn ensure_limit(mrb: &Mrb, (_, limit, _): &LineArgs, name: &str) -> Result<(), Error> {
    match limit {
        Some(0) => Err(argument_error(mrb, &format!("invalid limit: 0 for {name}"))),
        _ => Ok(()),
    }
}

/// `gets(sep = $/, limit = nil, chomp: false)`: the next line the stream
/// `source` opens reads, or `nil` at the end.
pub(crate) fn gets<S: LineSource>(
    mrb: &Mrb,
    args: &[Value],
    source: impl FnOnce() -> Result<S, Error>,
) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    let line = take_line(mrb, &mut source()?, &line_args)?;
    Ok(line.map_or(qnil().as_value(), |line| mrb.str_new(&line).as_value()))
}

/// `each_line(sep = $/, limit = nil, chomp: false) { |line| }`: yields each
/// line the stream `source` opens reads to the end, answering `self_`.
pub(crate) fn each_line<S: LineSource>(
    mrb: &Mrb,
    self_: Value,
    args: &[Value],
    source: impl FnOnce() -> Result<S, Error>,
) -> Result<Value, Error> {
    let block = required_block(mrb, "each_line")?;
    let line_args = line_args_of(mrb, args)?;
    ensure_limit(mrb, &line_args, "each_line")?;
    let mut source = source()?;
    while let Some(line) = take_line(mrb, &mut source, &line_args)? {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(&line).as_value()])?;
    }
    Ok(self_)
}

/// `readlines(sep = $/, limit = nil, chomp: false)`: every line the stream
/// `source` opens reads to the end.
pub(crate) fn readlines<S: LineSource>(
    mrb: &Mrb,
    args: &[Value],
    source: impl FnOnce() -> Result<S, Error>,
) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    ensure_limit(mrb, &line_args, "readlines")?;
    let mut source = source()?;
    let lines = mrb.ary_new();
    while let Some(line) = take_line(mrb, &mut source, &line_args)? {
        lines.push(mrb, mrb.str_new(&line).as_value())?;
    }
    Ok(lines.as_value())
}

/// The next line, its separator kept unless `chomp`, or `None` at the end.
/// A limit of zero answers an empty line at once; an empty separator reads a
/// paragraph, skipping the newlines before and after it, as CRuby's `rspara`
/// does.
pub(crate) fn take_line(
    mrb: &Mrb,
    source: &mut dyn LineSource,
    (separator, limit, chomp): &LineArgs,
) -> Result<Option<Vec<u8>>, Error> {
    if *limit == Some(0) {
        return Ok(Some(Vec::new()));
    }
    let paragraph = separator.as_deref().is_some_and(<[u8]>::is_empty);
    if paragraph {
        skip_newlines(mrb, source)?;
    }
    let separator = match paragraph {
        true => Some(&b"\n\n"[..]),
        false => separator.as_deref(),
    };
    let mut line = Vec::new();
    loop {
        if let Some(end) = line_end_of(&line, separator, *limit) {
            let rest = line.split_off(end);
            source.give_back(mrb, rest)?;
            break;
        }
        match source.fill(mrb)? {
            Some(bytes) => line.extend(bytes),
            None => break,
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    if paragraph {
        skip_newlines(mrb, source)?;
    }
    if let Some(separator) = separator.filter(|_| *chomp) {
        if line.ends_with(separator) {
            line.truncate(line.len() - separator.len());
            // A lone "\n" separator takes the "\r" before it too, as CRuby's
            // `chomp_cr` drops a CRLF line ending.
            if separator == b"\n" && line.ends_with(b"\r") {
                line.pop();
            }
        }
    }
    Ok(Some(line))
}

/// Drop the newlines that stand next, reading on until another byte or the
/// end comes, as CRuby's `swallow` does around a paragraph.
fn skip_newlines(mrb: &Mrb, source: &mut dyn LineSource) -> Result<(), Error> {
    while let Some(mut bytes) = source.fill(mrb)? {
        let newlines = bytes.iter().take_while(|byte| **byte == b'\n').count();
        if newlines < bytes.len() {
            source.give_back(mrb, bytes.split_off(newlines))?;
            break;
        }
    }
    Ok(())
}

/// Where the line `line` starts ends: past its separator, or at its limit,
/// whichever comes first; `None` while neither is in it.
fn line_end_of(line: &[u8], separator: Option<&[u8]>, limit: Option<usize>) -> Option<usize> {
    let by_separator = separator.and_then(|separator| {
        line.windows(separator.len())
            .position(|window| window == separator)
            .map(|at| at + separator.len())
    });
    let by_limit = limit.filter(|limit| line.len() >= *limit);
    match (by_separator, by_limit) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (end, None) | (None, end) => end,
    }
}
