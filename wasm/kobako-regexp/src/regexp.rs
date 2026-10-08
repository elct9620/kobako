//! The guest `Regexp` class — a CDATA carrier over a compiled
//! `fancy_regex::Regex` plus the source and MRI option bits.
//!
//! `Regexp.compile` is the entry a `/.../ ` literal compiles to, so it and
//! `Regexp.new` are singleton methods that build the carrier directly —
//! through a bounded per-invocation cache that lets an identical
//! pattern reuse its compiled engine instead of rebuilding it every time the
//! literal is evaluated. Each successful match refreshes the `$~` / `$1..$9` /
//! `$&` / `` $` `` / `$'` globals, mirroring the curated regexp engine's
//! always-on behaviour.
//!
//! This file owns the class surface; the helpers fan out per responsibility:
//! `globals` (match-global refresh), `render` (inspect / to_s / escape
//! text), `replace` (owned match spans + replacement expansion).

mod globals;
mod render;
mod replace;

pub(crate) use globals::set_span_globals;
pub(crate) use replace::{expand_replacement, match_spans, MatchSpan};

use crate::errors::{argument_error, regexp_error, type_error};
use crate::translate;
use beni::prelude::*;
use beni::typed_data::Obj;
use beni::value::qnil;
use beni::value::Lazy;
use beni::{
    DataType, Error, IntoValue, Mrb, Proc, RClass, RString, Symbol, TryConvert, TypedData, Value,
};
use lru::LruCache;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Compiled pattern plus the metadata `#source` / `#options` / `#casefold?`
/// report without an engine getter.
#[derive(Clone)]
#[beni::wrap(class = "Regexp", name = "Kobako::Regexp")]
pub(crate) struct RegexpState {
    regex: Arc<fancy_regex::Regex>,
    source: String,
    options: i64,
}

pub(crate) fn state_of(mrb: &Mrb, value: Value) -> Option<&RegexpState> {
    <&RegexpState>::try_convert(value, mrb).ok()
}

/// Per-invocation memoization of compiled patterns, keyed by
/// `(source, options)`. Bounded so it cannot grow without limit within
/// an invocation; held per interpreter, it is freed with the interpreter
/// when the invocation ends.
struct CompileCache {
    entries: RefCell<LruCache<(String, i64), Arc<fancy_regex::Regex>>>,
}

static COMPILE_CACHE_TYPE: DataType<CompileCache> = DataType::new(c"Kobako::RegexpCompileCache");

// Written out rather than declared with `#[beni::wrap]`, unlike the two
// carriers above: the macro prepares each class it names when a gem marks
// its carriers, and this one wraps as `Object` — which mruby exempts from
// the data mark, and whose allocator every other object still needs. Its
// carriers are therefore never marked.
//
// SAFETY: as above, the exemption is what lets the wrap allocate a
// carrier rather than raise.
unsafe impl TypedData for CompileCache {
    fn class(mrb: &Mrb) -> RClass {
        mrb.object_class()
    }

    fn data_type() -> &'static DataType<Self> {
        &COMPILE_CACHE_TYPE
    }
}

/// Each interpreter's compile cache, built on its first compile and kept
/// where no guest program can read or overwrite it.
static COMPILE_CACHE: Lazy<Obj<CompileCache>> = Lazy::new(|mrb| mrb.obj_wrap(CompileCache::new()));

impl CompileCache {
    fn new() -> Self {
        Self {
            entries: RefCell::new(LruCache::new(cache_capacity())),
        }
    }
}

/// Compiled-pattern cache capacity — 64 by default, overridable at build time
/// with `KOBAKO_REGEXP_CACHE_CAP` to trade guest memory for fewer recompiles.
fn cache_capacity() -> NonZeroUsize {
    option_env!("KOBAKO_REGEXP_CACHE_CAP")
        .and_then(|raw| raw.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or(NonZeroUsize::new(64).expect("64 is non-zero"))
}

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    // RegexpError is the guest exception a bad pattern or a blown
    // backtracking limit raises; the gem owns it as a StandardError subclass.
    mrb.define_error(c"RegexpError", mrb.exception_standard_error())?;

    let cls = mrb.define_class(c"Regexp", mrb.object_class())?;
    RegexpState::mark_carriers(mrb)?;

    // The archive is built MRB_INT32, so an option flag crosses into the
    // value domain as the width mruby actually carries.
    cls.const_set(mrb, c"IGNORECASE", translate::IGNORECASE as i32)?;
    cls.const_set(mrb, c"EXTENDED", translate::EXTENDED as i32)?;
    cls.const_set(mrb, c"MULTILINE", translate::MULTILINE as i32)?;

    cls.define_singleton_method(mrb, c"new", beni::method!(rx_compile, -1))?;
    cls.define_singleton_method(mrb, c"compile", beni::method!(rx_compile, -1))?;
    cls.define_singleton_method(mrb, c"escape", beni::method!(rx_escape, -1))?;
    cls.define_singleton_method(mrb, c"quote", beni::method!(rx_escape, -1))?;
    cls.define_singleton_method(mrb, c"last_match", beni::method!(rx_last_match, 0))?;
    cls.define_singleton_method(mrb, c"last_match=", beni::method!(rx_set_last_match, 1))?;

    cls.define_method(mrb, c"match", beni::method!(rx_match, -1))?;
    cls.define_method(mrb, c"match?", beni::method!(rx_match_p, -1))?;
    cls.define_method(mrb, c"=~", beni::method!(rx_eqtilde, 1))?;
    cls.define_method(mrb, c"===", beni::method!(rx_eqq, 1))?;
    cls.define_method(mrb, c"source", beni::method!(rx_source, 0))?;
    cls.define_method(mrb, c"options", beni::method!(rx_options, 0))?;
    cls.define_method(mrb, c"casefold?", beni::method!(rx_casefold, 0))?;
    cls.define_method(mrb, c"named_captures", beni::method!(rx_named_captures, 0))?;
    cls.define_method(mrb, c"names", beni::method!(rx_names, 0))?;
    cls.define_method(mrb, c"inspect", beni::method!(rx_inspect, 0))?;
    cls.define_method(mrb, c"to_s", beni::method!(rx_to_s, 0))?;
    cls.define_method(mrb, c"==", beni::method!(rx_eq, 1))?;
    // A copy carries a clone of the compiled pattern rather than
    // recompiling it; a carrier's payload cannot be replaced after the
    // fact, so the copy is made where it is allocated.
    cls.define_method(
        mrb,
        c"dup",
        beni::method!(<RegexpState as beni::typed_data::Dup>::dup, 0),
    )?;
    cls.define_method(
        mrb,
        c"clone",
        beni::method!(<RegexpState as beni::typed_data::Dup>::clone, -1),
    )?;
    Ok(())
}

/// Fancy-mode backtracking ceiling. A pattern that exceeds it fails with
/// `RegexpError` instead of burning the invocation's wall-clock budget;
/// non-fancy patterns delegate to the linear engine and never backtrack.
/// The host fuel / epoch / memory caps remain the
/// ultimate compute bound.
const BACKTRACK_LIMIT: usize = 1_000_000;

fn rx_compile(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    if args.is_empty() {
        return Err(argument_error(
            mrb,
            "wrong number of arguments (given 0, expected 1..3)",
        ));
    }
    let source = text_of(mrb, args[0])?;
    let options = parse_options(mrb, args.get(1).copied())?;
    compile(mrb, source, options)
}

/// The one construction path, so every pattern goes through the
/// per-invocation compile cache and an identical one reuses its engine.
fn compile(mrb: &Mrb, source: String, options: i64) -> Result<Value, Error> {
    if let Some(regex) = cache_get(mrb, &source, options) {
        return Ok(wrap_regexp(mrb, regex, source, options));
    }
    let pattern = translate::build_pattern(&source, options);
    match fancy_regex::RegexBuilder::new(&pattern)
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
    {
        Ok(regex) => {
            let regex = Arc::new(regex);
            cache_put(mrb, source.clone(), options, &regex);
            Ok(wrap_regexp(mrb, regex, source, options))
        }
        Err(error) => Err(regexp_error(mrb, &source, &error.to_string())),
    }
}

fn wrap_regexp(mrb: &Mrb, regex: Arc<fancy_regex::Regex>, source: String, options: i64) -> Value {
    mrb.wrap(RegexpState {
        regex,
        source,
        options,
    })
    .as_value()
}

fn cache_get(mrb: &Mrb, source: &str, options: i64) -> Option<Arc<fancy_regex::Regex>> {
    mrb.get_inner(&COMPILE_CACHE)
        .entries
        .borrow_mut()
        .get(&(source.to_string(), options))
        .cloned()
}

fn cache_put(mrb: &Mrb, source: String, options: i64, regex: &Arc<fancy_regex::Regex>) {
    mrb.get_inner(&COMPILE_CACHE)
        .entries
        .borrow_mut()
        .put((source, options), Arc::clone(regex));
}

fn parse_options(mrb: &Mrb, flags: Option<Value>) -> Result<i64, Error> {
    match flags {
        Some(value) if !value.is_nil() => match i32::from_value(value) {
            Some(mask) => Ok(i64::from(mask)),
            None => Ok(translate::parse_flag_string(&text_of(mrb, value)?)),
        },
        _ => Ok(0),
    }
}

/// MRI-style: a negative `pos` counts back from the end, and one out of
/// range is no match. A valid offset snaps down to a char boundary so the
/// engine never receives a mid-codepoint offset.
pub(crate) fn resolve_pos(subject: &str, pos: i64) -> Option<usize> {
    let len = subject.len() as i64;
    let pos = if pos < 0 { pos + len } else { pos };
    if pos < 0 || pos > len {
        return None;
    }
    let mut p = pos as usize;
    while p > 0 && !subject.is_char_boundary(p) {
        p -= 1;
    }
    Some(p)
}

fn match_pos(subject: &str, args: &[Value]) -> Option<usize> {
    let raw = args.get(1).and_then(|v| i32::from_value(*v)).unwrap_or(0);
    resolve_pos(subject, i64::from(raw))
}

fn rx_match(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let block = crate::args::block(mrb)?;
    let Some(&arg) = args.first() else {
        return Ok(qnil().as_value());
    };
    if arg.is_nil() {
        return Ok(qnil().as_value());
    }
    let subject = subject_string(mrb, arg)?;
    let Some(pos) = match_pos(&subject, args) else {
        return Ok(qnil().as_value());
    };
    let md = do_match(mrb, self_, subject, pos)?;
    yield_match(mrb, md, block)
}

/// Mirrors `Regexp#match`'s block form: the block is never called on a miss.
pub(crate) fn yield_match(mrb: &Mrb, md: Value, block: Option<Proc>) -> Result<Value, Error> {
    match block {
        Some(b) if !md.is_nil() => b.call(mrb, &[md]),
        _ => Ok(md),
    }
}

fn rx_match_p(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<bool, Error> {
    let Some(&arg) = args.first() else {
        return Ok(false);
    };
    if arg.is_nil() {
        return Ok(false);
    }
    let subject = subject_string(mrb, arg)?;
    let Some(pos) = match_pos(&subject, args) else {
        return Ok(false);
    };
    let Some(state) = state_of(mrb, self_) else {
        return Ok(false);
    };
    match state.regex.find_from_pos(&subject, pos) {
        Ok(found) => Ok(found.is_some()),
        Err(error) => Err(regexp_error(mrb, &state.source, &error.to_string())),
    }
}

fn rx_eqtilde(mrb: &Mrb, self_: Value, arg: Value) -> Result<Value, Error> {
    if arg.is_nil() {
        return Ok(qnil().as_value());
    }
    let subject = subject_string(mrb, arg)?;
    let md = do_match(mrb, self_, subject, 0)?;
    if md.is_nil() {
        Ok(qnil().as_value())
    } else {
        md.funcall(mrb, c"begin", &[0i32.into_value(mrb)])
    }
}

fn rx_eqq(mrb: &Mrb, self_: Value, arg: Value) -> Result<bool, Error> {
    if arg.is_nil() {
        return Ok(false);
    }
    let Ok(subject) = subject_string(mrb, arg) else {
        return Ok(false);
    };
    Ok(!do_match(mrb, self_, subject, 0)?.is_nil())
}

fn rx_source(mrb: &Mrb, state: &RegexpState) -> Value {
    mrb.str_new(state.source.as_bytes()).as_value()
}

fn rx_options(mrb: &Mrb, state: &RegexpState) -> Value {
    (state.options as i32).into_value(mrb)
}

fn rx_casefold(_mrb: &Mrb, state: &RegexpState) -> bool {
    state.options & translate::IGNORECASE != 0
}

fn rx_named_captures(mrb: &Mrb, state: &RegexpState) -> Result<Value, Error> {
    let map = mrb.hash_new();
    for (name, indexes) in named_groups(state) {
        let array = mrb.ary_new();
        for index in indexes {
            array.push(mrb, (index as i32).into_value(mrb))?;
        }
        map.set(
            mrb,
            mrb.str_new(name.as_bytes()).as_value(),
            array.as_value(),
        )?;
    }
    Ok(map.as_value())
}

fn rx_names(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = state_of(mrb, self_) else {
        return Ok(qnil().as_value());
    };
    let names = mrb.ary_new();
    for (name, _) in named_groups(state) {
        names.push(mrb, mrb.str_new(name.as_bytes()).as_value())?;
    }
    Ok(names.as_value())
}

/// A name shared by several groups collects every index, as MRI's
/// `named_captures` does.
fn named_groups(state: &RegexpState) -> Vec<(&str, Vec<usize>)> {
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (index, name) in state.regex.capture_names().enumerate() {
        let Some(name) = name else { continue };
        match groups.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, indexes)) => indexes.push(index),
            None => groups.push((name, vec![index])),
        }
    }
    groups
}

fn rx_inspect(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = state_of(mrb, self_) else {
        return qnil().as_value();
    };
    mrb.str_new(
        format!(
            "/{}/{}",
            render::inspect_source(&state.source),
            render::enabled_flags(state.options)
        )
        .as_bytes(),
    )
    .as_value()
}

fn rx_to_s(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = state_of(mrb, self_) else {
        return qnil().as_value();
    };
    let (options, body) = match render::lift_inline_group(&state.source) {
        Some((enabled, disabled, inner)) => ((state.options | enabled) & !disabled, inner),
        None => (state.options, state.source.as_str()),
    };
    let (on, off) = render::on_off_flags(options);
    let rendered = if off.is_empty() {
        format!("(?{on}:{body})")
    } else {
        format!("(?{on}-{off}:{body})")
    };
    mrb.str_new(rendered.as_bytes()).as_value()
}

fn rx_eq(mrb: &Mrb, self_: Value, arg: Value) -> bool {
    let (Some(this), Some(other)) = (state_of(mrb, self_), state_of(mrb, arg)) else {
        return false;
    };
    this.source == other.source && this.options == other.options
}

/// Read straight from `$~`: MRI keeps the two in lock-step and every match
/// refreshes `$~`, so no separate state is needed.
fn rx_last_match(mrb: &Mrb, _self: Value) -> Value {
    mrb.gv_get(c"$~")
}

/// Refreshes the derived globals too, so a caller can save and restore the
/// whole match set around an inner match (`String#slice!` relies on this).
fn rx_set_last_match(mrb: &Mrb, _self: Value, value: Value) -> Value {
    globals::set_last_match(mrb, value);
    value
}

fn rx_escape(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    if args.is_empty() {
        return Err(argument_error(
            mrb,
            "wrong number of arguments (given 0, expected 1)",
        ));
    }
    Ok(mrb
        .str_new(render::escape_str(&text_of(mrb, args[0])?).as_bytes())
        .as_value())
}

fn do_match(mrb: &Mrb, regexp: Value, subject: String, pos: usize) -> Result<Value, Error> {
    let Some(state) = state_of(mrb, regexp) else {
        return Ok(qnil().as_value());
    };
    match state.regex.captures_from_pos(&subject, pos) {
        Ok(Some(captures)) => {
            let count = state.regex.captures_len();
            let groups: Vec<Option<(usize, usize)>> = (0..count)
                .map(|i| captures.get(i).map(|m| (m.start(), m.end())))
                .collect();
            let names: Vec<(String, usize)> = state
                .regex
                .capture_names()
                .enumerate()
                .filter_map(|(i, name)| name.map(|n| (n.to_string(), i)))
                .collect();
            // `captures` (and its borrow of `subject`) ends here, so the match
            // can take ownership of `subject` instead of cloning it.
            Ok(globals::finalize(mrb, regexp, subject, groups, names))
        }
        Ok(None) => {
            globals::clear_globals(mrb);
            Ok(qnil().as_value())
        }
        Err(error) => Err(regexp_error(mrb, &state.source, &error.to_string())),
    }
}

pub(crate) fn is_regexp(mrb: &Mrb, value: Value) -> bool {
    state_of(mrb, value).is_some()
}

/// A non-`Regexp` pattern compiles as a literal (escaped) pattern, as in
/// MRI.
pub(crate) fn coerce_regexp(mrb: &Mrb, arg: Value) -> Result<Value, Error> {
    if is_regexp(mrb, arg) {
        return Ok(arg);
    }
    compile(mrb, render::escape_str(&text_of(mrb, arg)?), 0)
}

/// Everything here works over `&str`, so non-UTF-8 bytes are refused rather
/// than rendered as an empty string, where an empty subject silently
/// matches nothing and an empty pattern silently matches everywhere.
pub(crate) fn text_of(mrb: &Mrb, val: Value) -> Result<String, Error> {
    let rendered = val.funcall(mrb, c"to_s", &[])?;
    String::from_value(rendered)
        .ok_or_else(|| argument_error(mrb, "invalid byte sequence in UTF-8"))
}

/// A `String`'s characters; anything else raises the `TypeError` mruby
/// raises for a value it cannot convert to a String.
fn string_text(mrb: &Mrb, val: Value) -> Result<String, Error> {
    RString::try_convert(val, mrb)?;
    text_of(mrb, val)
}

/// Coerces like the C `reg_operand`: a `String` or `Symbol` yields its
/// characters, anything else raises `TypeError`.
fn subject_string(mrb: &Mrb, arg: Value) -> Result<String, Error> {
    if Symbol::from_value(arg).is_some() {
        text_of(mrb, arg)
    } else {
        string_text(mrb, arg)
    }
}

/// A String is not coerced here, so a non-`Regexp` raises `TypeError`.
pub(crate) fn require_regexp(mrb: &Mrb, arg: Value) -> Result<Value, Error> {
    if is_regexp(mrb, arg) {
        Ok(arg)
    } else {
        Err(type_error(
            mrb,
            &format!(
                "wrong argument type {} (expected Regexp)",
                arg.classname(mrb)
            ),
        ))
    }
}
