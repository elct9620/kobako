//! The guest `Regexp` class — a CDATA carrier over a compiled
//! `fancy_regex::Regex` plus the source and MRI option bits (SPEC.md B-41).
//!
//! `Regexp.compile` is the entry a `/.../ ` literal compiles to, so it and
//! `Regexp.new` are singleton methods that build the carrier directly. Each
//! successful match refreshes the `$~` / `$1..$9` / `$&` / `` $` `` / `$'`
//! globals, mirroring the curated regexp engine's always-on behaviour.

use crate::matchdata::{self, MatchState};
use crate::translate;
use beni::{format, DataType, Error, FromValue, Module, Mrb, Object, Proc, Value};
use core::ffi::CStr;

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
    // RegexpError is the guest exception a bad pattern or a blown
    // backtracking limit raises; the gem owns it as a StandardError subclass.
    mrb.define_class(c"RegexpError", mrb.class_get(c"StandardError")?)?;

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
    cls.define_singleton_method(mrb, c"last_match", beni::method!(rx_last_match, 0))?;
    cls.define_singleton_method(mrb, c"last_match=", beni::method!(rx_set_last_match, -1))?;

    cls.define_method(mrb, c"match", beni::method!(rx_match, -1))?;
    cls.define_method(mrb, c"match?", beni::method!(rx_match_p, -1))?;
    cls.define_method(mrb, c"=~", beni::method!(rx_eqtilde, -1))?;
    cls.define_method(mrb, c"===", beni::method!(rx_eqq, -1))?;
    cls.define_method(mrb, c"source", beni::method!(rx_source, 0))?;
    cls.define_method(mrb, c"options", beni::method!(rx_options, 0))?;
    cls.define_method(mrb, c"casefold?", beni::method!(rx_casefold, 0))?;
    cls.define_method(mrb, c"named_captures", beni::method!(rx_named_captures, 0))?;
    cls.define_method(mrb, c"names", beni::method!(rx_names, 0))?;
    cls.define_method(mrb, c"inspect", beni::method!(rx_inspect, 0))?;
    cls.define_method(mrb, c"to_s", beni::method!(rx_to_s, 0))?;
    cls.define_method(mrb, c"==", beni::method!(rx_eq, -1))?;
    cls.define_method(
        mrb,
        c"initialize_copy",
        beni::method!(rx_initialize_copy, -1),
    )?;
    Ok(())
}

/// `initialize_copy` — the body mruby's `dup` / `clone` run on the freshly
/// allocated bare copy (SPEC.md B-41). The compiled pattern is `Clone`, so
/// install a clone of `other`'s state instead of recompiling; without this the
/// copy would carry no payload and every accessor would fail.
fn rx_initialize_copy(mrb: &Mrb, self_: Value) -> Value {
    let other = mrb.get_args::<format::O>();
    if let Some(state) = other.data_get(mrb, &REGEXP_TYPE) {
        self_.data_reinit(
            mrb,
            RegexpState {
                regex: state.regex.clone(),
                source: state.source.clone(),
                options: state.options,
            },
            &REGEXP_TYPE,
        );
    }
    self_
}

/// Fancy-mode backtracking ceiling. A pattern that exceeds it fails with
/// `RegexpError` instead of burning the invocation's wall-clock budget;
/// non-fancy patterns delegate to the linear engine and never backtrack.
/// The host fuel / epoch / memory caps (docs/behavior.md B-01) remain the
/// ultimate compute bound.
const BACKTRACK_LIMIT: usize = 1_000_000;

/// `Regexp.new` / `Regexp.compile` / literal compilation. The flags
/// argument is an Integer option mask, a letter String (`"im"`), or nil.
fn rx_compile(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Err(argument_error(
            mrb,
            "wrong number of arguments (given 0, expected 1..3)",
        ));
    }
    let source = args[0].to_string(mrb);
    let options = parse_options(mrb, args.get(1).copied());
    compile(mrb, source, options)
}

/// Build a `Regexp` carrier from an owned `source` and MRI `options`,
/// raising `RegexpError` on an invalid pattern. The canonical construction
/// path shared by `Regexp.new` / `Regexp.compile` and the String-method
/// coercion of a non-`Regexp` pattern, so both build through the engine
/// directly instead of re-dispatching to Ruby `Regexp.new`.
fn compile(mrb: &Mrb, source: String, options: i64) -> Result<Value, Error> {
    let pattern = translate::build_pattern(&source, options);
    match fancy_regex::RegexBuilder::new(&pattern)
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
    {
        Ok(regex) => {
            let cls = mrb
                .class_get(c"Regexp")
                .expect("Regexp is defined at gem init");
            Ok(cls.data_wrap(
                mrb,
                RegexpState {
                    regex,
                    source,
                    options,
                },
                &REGEXP_TYPE,
            ))
        }
        Err(error) => Err(regexp_error(mrb, &source, &error.to_string())),
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

/// Resolve a `pos` argument to a byte offset, MRI-style: a negative `pos`
/// counts back from the end of `subject`. A position outside `0..=len` yields
/// `None`, so the caller reports no match; a valid offset is snapped down to a
/// UTF-8 char boundary so the engine never receives a mid-codepoint offset.
fn resolve_pos(subject: &str, pos: i64) -> Option<usize> {
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

/// Read the optional `pos` argument (the second positional) as a byte offset
/// in `subject`, or `None` when it is out of range.
fn match_pos(subject: &str, args: &[Value]) -> Option<usize> {
    let raw = args.get(1).and_then(|v| i32::from_value(*v)).unwrap_or(0);
    resolve_pos(subject, i64::from(raw))
}

fn rx_match(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    if args.is_empty() {
        return Ok(Value::nil());
    }
    let subject = args[0].to_string(mrb);
    let Some(pos) = match_pos(&subject, &args) else {
        return Ok(Value::nil());
    };
    let md = do_match(mrb, self_, &subject, pos)?;
    yield_match(mrb, md, block)
}

/// On a hit, yield the `MatchData` to a given block and return its result
/// (mirroring `Regexp#match`'s block form); on a miss, or with no block,
/// return the `MatchData`/`nil` directly. The block is never called on a miss.
pub(crate) fn yield_match(mrb: &Mrb, md: Value, block: Value) -> Result<Value, Error> {
    match Proc::from_value(block) {
        Some(b) if !md.is_nil() => b.call(mrb, &[md]),
        _ => Ok(md),
    }
}

fn rx_match_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Ok(Value::false_());
    }
    let subject = args[0].to_string(mrb);
    let Some(pos) = match_pos(&subject, &args) else {
        return Ok(Value::false_());
    };
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Ok(Value::false_());
    };
    match state.regex.find_from_pos(&subject, pos) {
        Ok(Some(_)) => Ok(Value::true_()),
        Ok(None) => Ok(Value::false_()),
        Err(error) => Err(regexp_error(mrb, &state.source, &error.to_string())),
    }
}

fn rx_eqtilde(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Ok(Value::nil());
    }
    let subject = arg.to_string(mrb);
    let md = do_match(mrb, self_, &subject, 0)?;
    if md.is_nil() {
        Ok(Value::nil())
    } else {
        Ok(md.call(mrb, c"begin", &[Value::from_int(mrb, 0)]))
    }
}

fn rx_eqq(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Ok(Value::false_());
    }
    let subject = arg.to_string(mrb);
    if do_match(mrb, self_, &subject, 0)?.is_nil() {
        Ok(Value::false_())
    } else {
        Ok(Value::true_())
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

/// `Regexp#named_captures` — a Hash mapping each capture name to the list of
/// group numbers carrying it (`{name => [index]}`), mirroring the C gem. Names
/// are listed in declaration order; a same-named group appends its index.
fn rx_named_captures(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Value::nil();
    };
    let map = mrb.hash_new();
    for (name, indexes) in named_groups(state) {
        let array = mrb.ary_new();
        for index in indexes {
            array.push(mrb, Value::from_int(mrb, index as _));
        }
        map.set(mrb, mrb.str_new(name.as_bytes()), array.as_value());
    }
    map.as_value()
}

/// `Regexp#names` — the capture names in declaration order (the keys of
/// `#named_captures`).
fn rx_names(mrb: &Mrb, self_: Value) -> Value {
    let Some(state) = self_.data_get(mrb, &REGEXP_TYPE) else {
        return Value::nil();
    };
    let names = mrb.ary_new();
    for (name, _) in named_groups(state) {
        names.push(mrb, mrb.str_new(name.as_bytes()));
    }
    names.as_value()
}

/// Capture names paired with their group numbers, in declaration order; a
/// name shared by several groups collects every index.
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

/// `Regexp.last_match` — the most recent match's `MatchData`, read straight
/// from `$~`. MRI keeps the two in lock-step and the gem refreshes `$~` on
/// every match, so no separate state is needed.
fn rx_last_match(mrb: &Mrb, _self: Value) -> Value {
    mrb.gv_get(mrb.intern_cstr(c"$~"))
}

/// `Regexp.last_match=` — overwrite `$~`, letting a caller save and restore
/// the match state around an inner match (`String#slice!` relies on this).
fn rx_set_last_match(mrb: &Mrb, _self: Value) -> Value {
    let value = mrb.get_args::<format::O>();
    mrb.gv_set(mrb.intern_cstr(c"$~"), value);
    value
}

fn rx_escape(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Err(argument_error(
            mrb,
            "wrong number of arguments (given 0, expected 1)",
        ));
    }
    Ok(mrb.str_new(escape_str(&args[0].to_string(mrb)).as_bytes()))
}

/// Run the pattern against `subject` from byte `pos`, building a
/// `MatchData` and refreshing the match globals on a hit, clearing them on
/// a miss, and raising `RegexpError` on an engine error.
fn do_match(mrb: &Mrb, regexp: Value, subject: &str, pos: usize) -> Result<Value, Error> {
    let Some(state) = regexp.data_get(mrb, &REGEXP_TYPE) else {
        return Ok(Value::nil());
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
            Ok(finalize(mrb, regexp, subject, groups, names))
        }
        Ok(None) => {
            clear_globals(mrb);
            Ok(Value::nil())
        }
        Err(error) => Err(regexp_error(mrb, subject, &error.to_string())),
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
pub(crate) fn match_spans(
    mrb: &Mrb,
    regexp: Value,
    subject: &str,
) -> Result<Vec<MatchSpan>, Error> {
    let Some(state) = regexp.data_get(mrb, &REGEXP_TYPE) else {
        return Ok(Vec::new());
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
            Ok(None) => break,
            Err(error) => return Err(regexp_error(mrb, &state.source, &error.to_string())),
        }
    }
    Ok(spans)
}

/// Expand the backreferences in a gsub/sub replacement string against one
/// match's spans, mirroring the curated regexp engine: `\0`..`\9` insert a
/// numbered group (`\0` the whole match; an out-of-range number inserts
/// nothing), `\k<name>` inserts a named group (an undefined name raises
/// `IndexError`), `\\` is a literal backslash, and any other `\x` stays the
/// two literal characters. A trailing backslash is literal.
pub(crate) fn expand_replacement(
    mrb: &Mrb,
    regexp: Value,
    subject: &str,
    span: &MatchSpan,
    replacement: &str,
) -> Result<String, Error> {
    let state = regexp.data_get(mrb, &REGEXP_TYPE);
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            None => out.push('\\'),
            Some('\\') => out.push('\\'),
            Some('k') => {
                let name = read_group_name(mrb, &mut chars, replacement)?;
                let index = state.and_then(|s| name_to_index(s, &name)).ok_or_else(|| {
                    index_error(mrb, &format!("undefined group name reference: {name}"))
                })?;
                push_group(&mut out, subject, span, index);
            }
            Some(digit) if digit.is_ascii_digit() => {
                push_group(
                    &mut out,
                    subject,
                    span,
                    digit.to_digit(10).unwrap_or(0) as usize,
                );
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    Ok(out)
}

/// Read the `<name>` body following a `\k` in a replacement string, consuming
/// up to and including the closing `>`. A `\k` not followed by `<…>` is an
/// invalid replace expression.
fn read_group_name(
    mrb: &Mrb,
    chars: &mut core::str::Chars,
    replacement: &str,
) -> Result<String, Error> {
    if chars.next() != Some('<') {
        return Err(replace_expression_error(mrb, replacement));
    }
    let mut name = String::new();
    loop {
        match chars.next() {
            Some('>') => return Ok(name),
            Some(c) => name.push(c),
            None => return Err(replace_expression_error(mrb, replacement)),
        }
    }
}

/// Append group `index`'s matched substring to `out` (`0` is the whole match);
/// a group that did not participate or sits past the pattern appends nothing.
fn push_group(out: &mut String, subject: &str, span: &MatchSpan, index: usize) {
    let range = if index == 0 {
        Some(span.whole)
    } else {
        span.groups.get(index - 1).copied().flatten()
    };
    if let Some((start, end)) = range {
        out.push_str(&subject[start..end]);
    }
}

/// The group number a capture name resolves to, or `None` when the pattern
/// has no such name.
fn name_to_index(state: &RegexpState, name: &str) -> Option<usize> {
    state.regex.capture_names().position(|n| n == Some(name))
}

/// True when `value` is a `Regexp` carrier.
pub(crate) fn is_regexp(mrb: &Mrb, value: Value) -> bool {
    value.data_get(mrb, &REGEXP_TYPE).is_some()
}

/// Coerce a String method's pattern argument to a `Regexp`: a `Regexp`
/// passes through; anything else compiles as a literal (escaped) pattern.
pub(crate) fn coerce_regexp(mrb: &Mrb, arg: Value) -> Result<Value, Error> {
    if is_regexp(mrb, arg) {
        return Ok(arg);
    }
    compile(mrb, escape_str(&arg.to_string(mrb)), 0)
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
    // $+ is the last capture group that participated (MRI semantics): the
    // highest-numbered non-nil group, nil when the pattern has no groups.
    let last_group = groups.iter().skip(1).rev().find_map(|group| *group);
    set_global(
        mrb,
        c"$+",
        last_group.map(|(s, e)| mrb.str_new(&subject.as_bytes()[s..e])),
    );
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
    for name in [c"$&", c"$`", c"$'", c"$+"].iter().chain(NUMBERED.iter()) {
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

/// Build a `RegexpError` naming the offending `source`. The class is defined
/// at gem init, so the lookup cannot miss.
fn regexp_error(mrb: &Mrb, source: &str, detail: &str) -> Error {
    let message = format!(
        "{source:?} is an invalid regular expression: {}",
        detail.lines().next().unwrap_or(detail)
    );
    let cls = mrb
        .class_get(c"RegexpError")
        .expect("RegexpError is defined at gem init");
    Error::Exception(cls.exc_new(mrb, &message))
}

/// Build an `ArgumentError` carrying a static `message`.
fn argument_error(mrb: &Mrb, message: &str) -> Error {
    let cls = mrb
        .class_get(c"ArgumentError")
        .expect("ArgumentError is an mruby core class");
    Error::Exception(cls.exc_new(mrb, message))
}

/// Build an `IndexError` carrying `message` — raised for an undefined named
/// backreference in a replacement string.
fn index_error(mrb: &Mrb, message: &str) -> Error {
    let cls = mrb
        .class_get(c"IndexError")
        .expect("IndexError is an mruby core class");
    Error::Exception(cls.exc_new(mrb, message))
}

/// Build a `RegexpError` naming a malformed replacement expression (a `\k`
/// not followed by `<name>`).
fn replace_expression_error(mrb: &Mrb, replacement: &str) -> Error {
    let message = format!("invalid replace expression: {replacement:?}");
    let cls = mrb
        .class_get(c"RegexpError")
        .expect("RegexpError is defined at gem init");
    Error::Exception(cls.exc_new(mrb, &message))
}
