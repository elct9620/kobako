//! The guest `Regexp` class — a CDATA carrier over a compiled
//! `fancy_regex::Regex` plus the source and MRI option bits (SPEC.md B-41).
//!
//! `Regexp.compile` is the entry a `/.../ ` literal compiles to, so it and
//! `Regexp.new` are singleton methods that build the carrier directly. Each
//! successful match refreshes the `$~` / `$1..$9` / `$&` / `` $` `` / `$'`
//! globals, mirroring the curated regexp engine's always-on behaviour.

use crate::matchdata::{self, MatchState};
use crate::translate;
use beni::{format, DataType, FromValue, Module, Mrb, Object, Value};
use core::ffi::CStr;
use std::ffi::CString;

/// Compiled pattern plus the metadata `#source` / `#options` / `#casefold?`
/// report without an engine getter.
pub(crate) struct RegexpState {
    regex: fancy_regex::Regex,
    source: String,
    options: i64,
}

static REGEXP_TYPE: DataType<RegexpState> = DataType::new(c"Kobako::Regexp");

/// `$1`..`$9` global names, indexed by capture group minus one.
const NUMBERED: [&CStr; 9] = [
    c"$1", c"$2", c"$3", c"$4", c"$5", c"$6", c"$7", c"$8", c"$9",
];

/// Define the `Regexp` class, its option constants, and its methods.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let cls = mrb.define_class(c"Regexp", mrb.object_class())?;
    cls.set_instance_data_tt(mrb);

    cls.define_const(
        mrb,
        c"IGNORECASE",
        Value::from_int(mrb, translate::IGNORECASE as _),
    )?;
    cls.define_const(
        mrb,
        c"EXTENDED",
        Value::from_int(mrb, translate::EXTENDED as _),
    )?;
    cls.define_const(
        mrb,
        c"MULTILINE",
        Value::from_int(mrb, translate::MULTILINE as _),
    )?;

    cls.define_singleton_method(mrb, c"new", beni::method!(rx_compile, -1))?;
    cls.define_singleton_method(mrb, c"compile", beni::method!(rx_compile, -1))?;
    cls.define_singleton_method(mrb, c"escape", beni::method!(rx_escape, -1))?;
    cls.define_singleton_method(mrb, c"quote", beni::method!(rx_escape, -1))?;

    cls.define_method(mrb, c"match", beni::method!(rx_match, -1))?;
    cls.define_method(mrb, c"match?", beni::method!(rx_match_p, -1))?;
    cls.define_method(mrb, c"=~", beni::method!(rx_eqtilde, -1))?;
    cls.define_method(mrb, c"===", beni::method!(rx_eqq, -1))?;
    cls.define_method(mrb, c"source", beni::method!(rx_source, 0))?;
    cls.define_method(mrb, c"options", beni::method!(rx_options, 0))?;
    cls.define_method(mrb, c"casefold?", beni::method!(rx_casefold, 0))?;
    cls.define_method(mrb, c"inspect", beni::method!(rx_inspect, 0))?;
    cls.define_method(mrb, c"to_s", beni::method!(rx_to_s, 0))?;
    cls.define_method(mrb, c"==", beni::method!(rx_eq, -1))?;
    Ok(())
}

/// `Regexp.new` / `Regexp.compile` / literal compilation. The flags
/// argument is an Integer option mask, a letter String (`"im"`), or nil.
fn rx_compile(mrb: &Mrb, _self: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        unsafe { raise_argument_error(mrb, c"wrong number of arguments (given 0, expected 1..3)") };
    }
    let source = args[0].to_string(mrb);
    let options = parse_options(mrb, args.get(1).copied());
    let pattern = translate::build_pattern(&source, options);
    match fancy_regex::Regex::new(&pattern) {
        Ok(regex) => {
            let cls = mrb
                .class_get(c"Regexp")
                .expect("Regexp is defined at gem init");
            cls.data_wrap(
                mrb,
                RegexpState {
                    regex,
                    source,
                    options,
                },
                &REGEXP_TYPE,
            )
        }
        Err(error) => unsafe { raise_regexp_error(mrb, &source, &error.to_string()) },
    }
}

/// Resolve the optional flags argument to the MRI option mask.
fn parse_options(mrb: &Mrb, flags: Option<Value>) -> i64 {
    match flags {
        Some(value) if !value.is_nil() => match i32::from_value(value) {
            Some(mask) => i64::from(mask),
            None => translate::parse_flag_string(&value.to_string(mrb)),
        },
        _ => 0,
    }
}

fn rx_match(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Value::nil();
    }
    let subject = args[0].to_string(mrb);
    let pos = args
        .get(1)
        .and_then(|v| i32::from_value(*v))
        .unwrap_or(0)
        .max(0) as usize;
    do_match(mrb, self_, &subject, pos)
}

fn rx_match_p(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Value::false_();
    }
    let subject = args[0].to_string(mrb);
    let pos = args
        .get(1)
        .and_then(|v| i32::from_value(*v))
        .unwrap_or(0)
        .max(0) as usize;
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Value::false_();
    };
    match state.regex.find_from_pos(&subject, pos) {
        Ok(Some(_)) => Value::true_(),
        _ => Value::false_(),
    }
}

fn rx_eqtilde(mrb: &Mrb, self_: Value) -> Value {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Value::nil();
    }
    let subject = arg.to_string(mrb);
    let md = do_match(mrb, self_, &subject, 0);
    if md.is_nil() {
        Value::nil()
    } else {
        md.call(mrb, c"begin", &[Value::from_int(mrb, 0)])
    }
}

fn rx_eqq(mrb: &Mrb, self_: Value) -> Value {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Value::false_();
    }
    let subject = arg.to_string(mrb);
    if do_match(mrb, self_, &subject, 0).is_nil() {
        Value::false_()
    } else {
        Value::true_()
    }
}

fn rx_source(mrb: &Mrb, self_: Value) -> Value {
    match self_.data_get(mrb, &REGEXP_TYPE) {
        Some(state) => mrb.str_new(state.source.as_bytes()),
        None => Value::nil(),
    }
}

fn rx_options(mrb: &Mrb, self_: Value) -> Value {
    match self_.data_get(mrb, &REGEXP_TYPE) {
        Some(state) => Value::from_int(mrb, state.options as _),
        None => Value::from_int(mrb, 0),
    }
}

fn rx_casefold(mrb: &Mrb, self_: Value) -> Value {
    match self_.data_get(mrb, &REGEXP_TYPE) {
        Some(state) if state.options & translate::IGNORECASE != 0 => Value::true_(),
        _ => Value::false_(),
    }
}

fn rx_inspect(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Value::nil();
    };
    mrb.str_new(format!("/{}/{}", state.source, enabled_flags(state.options)).as_bytes())
}

fn rx_to_s(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Value::nil();
    };
    let (on, off) = on_off_flags(state.options);
    mrb.str_new(format!("(?{on}-{off}:{})", state.source).as_bytes())
}

fn rx_eq(mrb: &Mrb, self_: Value) -> Value {
    let arg = mrb.get_args::<format::O>();
    let (Some(this), Some(other)) = (
        self_.data_get(mrb, &REGEXP_TYPE),
        arg.data_get(mrb, &REGEXP_TYPE),
    ) else {
        return Value::false_();
    };
    if this.source == other.source && this.options == other.options {
        Value::true_()
    } else {
        Value::false_()
    }
}

fn rx_escape(mrb: &Mrb, _self: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        unsafe { raise_argument_error(mrb, c"wrong number of arguments (given 0, expected 1)") };
    }
    mrb.str_new(escape_str(&args[0].to_string(mrb)).as_bytes())
}

/// Run the pattern against `subject` from byte `pos`, building a
/// `MatchData` and refreshing the match globals on a hit, clearing them on
/// a miss, and raising `RegexpError` on an engine error.
fn do_match(mrb: &Mrb, regexp: Value, subject: &str, pos: usize) -> Value {
    let Some(state) = regexp.data_get(mrb, &REGEXP_TYPE) else {
        return Value::nil();
    };
    match state.regex.captures_from_pos(subject, pos) {
        Ok(Some(captures)) => {
            let count = state.regex.captures_len();
            let groups = (0..count)
                .map(|i| captures.get(i).map(|m| (m.start(), m.end())))
                .collect();
            let names = state
                .regex
                .capture_names()
                .enumerate()
                .filter_map(|(i, name)| name.map(|n| (n.to_string(), i)))
                .collect();
            finalize(mrb, regexp, subject, groups, names)
        }
        Ok(None) => {
            clear_globals(mrb);
            Value::nil()
        }
        Err(error) => unsafe { raise_regexp_error(mrb, subject, &error.to_string()) },
    }
}

/// Byte spans of one match: the whole match plus each numbered group.
pub(crate) struct MatchSpan {
    pub whole: (usize, usize),
    pub groups: Vec<Option<(usize, usize)>>,
}

/// Collect non-overlapping matches of `regexp` over `subject` as owned byte
/// spans, so the String methods can build results after the engine borrow
/// is released. A zero-width match advances by one character.
pub(crate) fn match_spans(mrb: &Mrb, regexp: Value, subject: &str) -> Vec<MatchSpan> {
    let Some(state) = regexp.data_get(mrb, &REGEXP_TYPE) else {
        return Vec::new();
    };
    let count = state.regex.captures_len();
    let mut spans = Vec::new();
    let mut pos = 0;
    while pos <= subject.len() {
        match state.regex.captures_from_pos(subject, pos) {
            Ok(Some(captures)) => {
                let Some(whole) = captures.get(0).map(|m| (m.start(), m.end())) else {
                    break;
                };
                let groups = (1..count)
                    .map(|i| captures.get(i).map(|m| (m.start(), m.end())))
                    .collect();
                spans.push(MatchSpan { whole, groups });
                pos = if whole.1 > whole.0 {
                    whole.1
                } else {
                    whole.1 + subject[whole.1..].chars().next().map_or(1, char::len_utf8)
                };
            }
            _ => break,
        }
    }
    spans
}

/// True when `value` is a `Regexp` carrier.
pub(crate) fn is_regexp(mrb: &Mrb, value: Value) -> bool {
    value.data_get(mrb, &REGEXP_TYPE).is_some()
}

/// Coerce a String method's pattern argument to a `Regexp`: a `Regexp`
/// passes through; anything else compiles as a literal (escaped) pattern.
pub(crate) fn coerce_regexp(mrb: &Mrb, arg: Value) -> Value {
    if is_regexp(mrb, arg) {
        return arg;
    }
    let cls = mrb
        .class_get(c"Regexp")
        .expect("Regexp is defined at gem init");
    let escaped = mrb.str_new(escape_str(&arg.to_string(mrb)).as_bytes());
    // SAFETY: `cls` is the live Regexp class; reifying it as a value is
    // GC-stable for the VM lifetime.
    unsafe { cls.to_value(mrb) }.call(mrb, c"new", &[escaped])
}

/// Build a `MatchData`, bind `$~`, and refresh `$&` / `` $` `` / `$'` /
/// `$1..$9` from a match's byte spans (`groups[0]` is the whole match);
/// returns the `MatchData`.
pub(crate) fn finalize(
    mrb: &Mrb,
    regexp: Value,
    subject: &str,
    groups: Vec<Option<(usize, usize)>>,
    names: Vec<(String, usize)>,
) -> Value {
    let md = matchdata::build(
        mrb,
        regexp,
        MatchState {
            subject: subject.to_owned(),
            groups: groups.clone(),
            names,
        },
    );
    mrb.gv_set(mrb.intern_cstr(c"$~"), md);
    let whole = groups.first().copied().flatten();
    set_global(
        mrb,
        c"$&",
        whole.map(|(s, e)| mrb.str_new(&subject.as_bytes()[s..e])),
    );
    if let Some((s, e)) = whole {
        set_global(mrb, c"$`", Some(mrb.str_new(&subject.as_bytes()[..s])));
        set_global(mrb, c"$'", Some(mrb.str_new(&subject.as_bytes()[e..])));
    }
    for (i, name) in NUMBERED.iter().enumerate() {
        let value = groups
            .get(i + 1)
            .copied()
            .flatten()
            .map(|(s, e)| mrb.str_new(&subject.as_bytes()[s..e]));
        set_global(mrb, name, value);
    }
    md
}

/// Refresh the match globals for a gsub/scan block from owned spans, so
/// `$1` is fresh on each iteration.
pub(crate) fn set_span_globals(mrb: &Mrb, regexp: Value, subject: &str, span: &MatchSpan) {
    let mut groups = Vec::with_capacity(span.groups.len() + 1);
    groups.push(Some(span.whole));
    groups.extend(span.groups.iter().copied());
    finalize(mrb, regexp, subject, groups, Vec::new());
}

/// Reset every match global to nil after a failed match.
fn clear_globals(mrb: &Mrb) {
    mrb.gv_set(mrb.intern_cstr(c"$~"), Value::nil());
    for name in [c"$&", c"$`", c"$'"].iter().chain(NUMBERED.iter()) {
        set_global(mrb, name, None);
    }
}

fn set_global(mrb: &Mrb, name: &CStr, value: Option<Value>) {
    mrb.gv_set(mrb.intern_cstr(name), value.unwrap_or_else(Value::nil));
}

/// Enabled option letters in MRI's `m`, `i`, `x` order — the form
/// `Regexp#inspect` appends after the pattern.
fn enabled_flags(options: i64) -> String {
    let mut flags = String::new();
    for (bit, letter) in [
        (translate::MULTILINE, 'm'),
        (translate::IGNORECASE, 'i'),
        (translate::EXTENDED, 'x'),
    ] {
        if options & bit != 0 {
            flags.push(letter);
        }
    }
    flags
}

/// Enabled and disabled option letters for `Regexp#to_s`'s `(?on-off:…)`.
fn on_off_flags(options: i64) -> (String, String) {
    let mut on = String::new();
    let mut off = String::new();
    for (bit, letter) in [
        (translate::MULTILINE, 'm'),
        (translate::IGNORECASE, 'i'),
        (translate::EXTENDED, 'x'),
    ] {
        if options & bit != 0 {
            on.push(letter);
        } else {
            off.push(letter);
        }
    }
    (on, off)
}

/// Backslash-escape the regexp metacharacters in `source`, mirroring
/// `Regexp.escape`.
fn escape_str(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for c in source.chars() {
        match c {
            '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            | '/' | '-' | '#' | ' ' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{0b}' => out.push_str("\\v"),
            _ => out.push(c),
        }
    }
    out
}

/// Raise `RegexpError` with a message naming the offending source. Diverges.
///
/// # Safety
/// Only callable from a bridge frame mruby may unwind from.
unsafe fn raise_regexp_error(mrb: &Mrb, source: &str, detail: &str) -> ! {
    let message = format!(
        "{source:?} is an invalid regular expression: {}",
        detail.lines().next().unwrap_or(detail)
    );
    let c_message = CString::new(message)
        .unwrap_or_else(|_| CString::new("invalid regular expression").expect("no NUL"));
    let cls = mrb
        .class_get(c"RegexpError")
        .expect("RegexpError is an mruby core class");
    // SAFETY: bridge frame — caller upholds the unwind contract.
    unsafe { cls.raise(mrb, &c_message) }
}

/// Raise `ArgumentError` with a static message. Diverges.
///
/// # Safety
/// Only callable from a bridge frame mruby may unwind from.
unsafe fn raise_argument_error(mrb: &Mrb, message: &CStr) -> ! {
    let cls = mrb
        .class_get(c"ArgumentError")
        .expect("ArgumentError is an mruby core class");
    // SAFETY: bridge frame — caller upholds the unwind contract.
    unsafe { cls.raise(mrb, message) }
}
