//! The guest `MatchData` class — a CDATA carrier over an owned snapshot
//! of one match (SPEC.md B-41).
//!
//! The snapshot keeps the subject string plus each group's byte range, so
//! every accessor answers from owned Rust data without borrowing the
//! engine's `Captures` (which borrows the subject). Offsets and slices are
//! byte-based, mirroring the curated regexp engine. The originating
//! `Regexp` is held as the `@regexp` ivar so the mruby GC keeps it alive.

use beni::{format, DataType, Error, FromValue, IntoValue, Module, Mrb, Object, Value};

/// Owned snapshot of one successful match.
pub(crate) struct MatchState {
    /// The string the pattern matched against.
    pub subject: String,
    /// Byte `(start, end)` of each group; index 0 is the whole match,
    /// `None` for a group that did not participate.
    pub groups: Vec<Option<(usize, usize)>>,
    /// Named captures as `(name, group index)` in declaration order.
    pub names: Vec<(String, usize)>,
}

static MATCH_TYPE: DataType<MatchState> = DataType::new(c"Kobako::MatchData");

/// Wrap `state` as a fresh `MatchData`, recording `regexp` as the
/// `@regexp` ivar so the GC keeps the originating pattern reachable.
pub(crate) fn build(mrb: &Mrb, regexp: Value, state: MatchState) -> Value {
    let cls = mrb
        .class_get(c"MatchData")
        .expect("MatchData is defined at gem init");
    let md = cls.data_wrap(mrb, state, &MATCH_TYPE);
    md.iv_set(mrb, mrb.intern_cstr(c"@regexp"), regexp);
    md
}

/// Define the `MatchData` class and its accessors on `mrb`.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let cls = mrb.define_class(c"MatchData", mrb.object_class())?;
    cls.set_instance_data_tt(mrb);
    cls.define_singleton_method(mrb, c"new", beni::method!(md_new_forbidden, -1))?;
    cls.define_method(mrb, c"[]", beni::method!(md_aref, -1))?;
    cls.define_method(mrb, c"begin", beni::method!(md_begin, -1))?;
    cls.define_method(mrb, c"end", beni::method!(md_end, -1))?;
    cls.define_method(mrb, c"offset", beni::method!(md_offset, -1))?;
    cls.define_method(mrb, c"captures", beni::method!(md_captures, 0))?;
    cls.define_method(mrb, c"named_captures", beni::method!(md_named_captures, -1))?;
    cls.define_method(mrb, c"names", beni::method!(md_names, 0))?;
    cls.define_method(mrb, c"size", beni::method!(md_size, 0))?;
    cls.define_method(mrb, c"length", beni::method!(md_size, 0))?;
    cls.define_method(mrb, c"pre_match", beni::method!(md_pre_match, 0))?;
    cls.define_method(mrb, c"post_match", beni::method!(md_post_match, 0))?;
    cls.define_method(mrb, c"string", beni::method!(md_string, 0))?;
    cls.define_method(mrb, c"regexp", beni::method!(md_regexp, 0))?;
    cls.define_method(mrb, c"to_a", beni::method!(md_to_a, 0))?;
    cls.define_method(mrb, c"to_s", beni::method!(md_to_s, 0))?;
    Ok(())
}

/// `MatchData.new` is forbidden — a `MatchData` only ever arises from a
/// match, never direct construction. The C gem undefined the constructor;
/// raising `NoMethodError` matches that observable behaviour while following
/// the gem's raising-bridge pattern for non-constructible types.
fn md_new_forbidden(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let cls = mrb
        .class_get(c"NoMethodError")
        .expect("NoMethodError is an mruby core class");
    Err(Error::Exception(
        cls.exc_new(mrb, "undefined method 'new' for MatchData"),
    ))
}

/// Borrow the snapshot, or return `nil` from the calling bridge when the
/// receiver is not a `MatchData` carrier (never happens in practice).
macro_rules! state_or_nil {
    ($mrb:expr, $self_:expr) => {
        match $self_.data_get($mrb, &MATCH_TYPE) {
            Some(state) => state,
            None => return Value::nil(),
        }
    };
}

/// Build a String value from a group's byte range, or `nil` when the group
/// did not participate.
fn group_str(mrb: &Mrb, state: &MatchState, index: usize) -> Value {
    match state.groups.get(index).copied().flatten() {
        Some((begin, end)) => mrb.str_new(&state.subject.as_bytes()[begin..end]),
        None => Value::nil(),
    }
}

/// Resolve a `[]` argument to a group index: an Integer is the index
/// directly, a Symbol or String names a capture.
fn resolve_index(mrb: &Mrb, state: &MatchState, arg: Value) -> Option<usize> {
    if let Some(n) = i32::from_value(arg) {
        return usize::try_from(n).ok();
    }
    let name = arg.to_string(mrb);
    state
        .names
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, i)| *i)
}

fn md_aref(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let arg = mrb.get_args::<format::O>();
    match resolve_index(mrb, state, arg) {
        Some(index) => group_str(mrb, state, index),
        None => Value::nil(),
    }
}

fn md_begin(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let n = i32::from_value(mrb.get_args::<format::O>()).unwrap_or(0);
    match state.groups.get(n as usize).copied().flatten() {
        Some((begin, _)) => (begin as i32).into_value(mrb),
        None => Value::nil(),
    }
}

fn md_end(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let n = i32::from_value(mrb.get_args::<format::O>()).unwrap_or(0);
    match state.groups.get(n as usize).copied().flatten() {
        Some((_, end)) => (end as i32).into_value(mrb),
        None => Value::nil(),
    }
}

fn md_offset(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let n = i32::from_value(mrb.get_args::<format::O>()).unwrap_or(0);
    let pair = mrb.ary_new();
    match state.groups.get(n as usize).copied().flatten() {
        Some((begin, end)) => {
            pair.push(mrb, (begin as i32).into_value(mrb));
            pair.push(mrb, (end as i32).into_value(mrb));
        }
        None => {
            pair.push(mrb, Value::nil());
            pair.push(mrb, Value::nil());
        }
    }
    pair.as_value()
}

fn md_captures(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let captures = mrb.ary_new();
    for index in 1..state.groups.len() {
        captures.push(mrb, group_str(mrb, state, index));
    }
    captures.as_value()
}

fn md_named_captures(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let symbolize = symbolize_names_requested(mrb);
    let map = mrb.hash_new();
    for (name, index) in &state.names {
        let key = mrb.str_new(name.as_bytes());
        let key = if symbolize {
            key.call(mrb, c"to_sym", &[])
        } else {
            key
        };
        map.set(mrb, key, group_str(mrb, state, *index));
    }
    map.as_value()
}

/// Read the optional `symbolize_names:` keyword. mruby passes it as a trailing
/// option Hash; a truthy value (Ruby semantics: anything but nil/false) turns
/// the keys into Symbols, mirroring the C gem and MRI.
fn symbolize_names_requested(mrb: &Mrb) -> bool {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let Some(options) = args.last().copied().filter(|arg| arg.is_hash()) else {
        return false;
    };
    let key = mrb.str_new(b"symbolize_names").call(mrb, c"to_sym", &[]);
    options.call(mrb, c"[]", &[key]).to_bool()
}

fn md_names(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let names = mrb.ary_new();
    for (name, _) in &state.names {
        names.push(mrb, mrb.str_new(name.as_bytes()));
    }
    names.as_value()
}

fn md_size(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    (state.groups.len() as i32).into_value(mrb)
}

fn md_pre_match(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    match state.groups.first().copied().flatten() {
        Some((begin, _)) => mrb.str_new(&state.subject.as_bytes()[..begin]),
        None => mrb.str_new(b""),
    }
}

fn md_post_match(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    match state.groups.first().copied().flatten() {
        Some((_, end)) => mrb.str_new(&state.subject.as_bytes()[end..]),
        None => mrb.str_new(b""),
    }
}

fn md_string(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    mrb.str_new(state.subject.as_bytes())
}

fn md_regexp(mrb: &Mrb, self_: Value) -> Value {
    self_.iv_get(mrb, mrb.intern_cstr(c"@regexp"))
}

fn md_to_a(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    let all = mrb.ary_new();
    for index in 0..state.groups.len() {
        all.push(mrb, group_str(mrb, state, index));
    }
    all.as_value()
}

fn md_to_s(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    group_str(mrb, state, 0)
}
