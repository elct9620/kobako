//! The guest `MatchData` class — a CDATA carrier over an owned snapshot
//! of one match.
//!
//! The snapshot keeps the subject string plus each group's byte range, so
//! every accessor answers from owned Rust data without borrowing the
//! engine's `Captures` (which borrows the subject). Offsets and slices are
//! byte-based, mirroring the curated regexp engine. The originating
//! `Regexp` is held as the `@regexp` ivar so the mruby GC keeps it alive.

use crate::errors::index_error;
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

/// Borrow the match snapshot a `MatchData` value carries, if it is one, so
/// `Regexp.last_match=` can refresh the derived match globals from an assigned
/// match.
pub(crate) fn state_of(mrb: &Mrb, value: Value) -> Option<&MatchState> {
    value.data_get(mrb, &MATCH_TYPE)
}

/// Wrap `state` as a fresh `MatchData`, recording `regexp` as the
/// `@regexp` ivar so the GC keeps the originating pattern reachable.
pub(crate) fn build(mrb: &Mrb, regexp: Value, state: MatchState) -> Value {
    let cls = mrb
        .class_get(c"MatchData")
        .expect("MatchData is defined at gem init");
    let md = cls
        .data_wrap(mrb, state, &MATCH_TYPE)
        .expect("MatchData is data-marked at gem init");
    // Fresh MatchData, never frozen — storing `@regexp` cannot raise.
    let _ = md.iv_set(mrb, mrb.intern_cstr(c"@regexp"), regexp);
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
    cls.define_method(
        mrb,
        c"initialize_copy",
        beni::method!(md_initialize_copy, -1),
    )?;
    Ok(())
}

/// `initialize_copy` — restore the owned match snapshot into the bare copy
/// mruby's `dup` / `clone` allocate. Only the CDATA payload
/// needs cloning; the `@regexp` ivar rides along on mruby's own ivar copy.
fn md_initialize_copy(mrb: &Mrb, self_: Value) -> Value {
    let other = mrb.get_args::<format::O>();
    if let Some(state) = other.data_get(mrb, &MATCH_TYPE) {
        self_.data_reinit(
            mrb,
            MatchState {
                subject: state.subject.clone(),
                groups: state.groups.clone(),
                names: state.names.clone(),
            },
            &MATCH_TYPE,
        );
    }
    self_
}

/// `MatchData.new` is forbidden — a `MatchData` only ever arises from a
/// match, never direct construction. Raising `NoMethodError` follows the
/// raising-bridge pattern for non-constructible types.
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
        Some((begin, end)) => mrb
            .str_new(&state.subject.as_bytes()[begin..end])
            .as_value(),
        None => Value::nil(),
    }
}

/// The numeric group index a single `MatchData#[]` argument resolves to: an
/// Integer is used as-is (a negative index counts from the end via `Array#[]`),
/// a Symbol or String names a capture (an undefined name raises IndexError).
/// Any other argument (e.g. a Range) yields `None` so the caller delegates the
/// whole argument list to `Array#[]` on the group list.
fn numeric_index(mrb: &Mrb, state: &MatchState, arg: Value) -> Result<Option<i32>, Error> {
    if let Some(n) = i32::from_value(arg) {
        return Ok(Some(n));
    }
    if arg.is_symbol() || arg.is_string() {
        let name = arg.to_string(mrb);
        return state
            .names
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, i)| Some(*i as i32))
            .ok_or_else(|| index_error(mrb, &format!("undefined group name reference: {name}")));
    }
    Ok(None)
}

/// `MatchData#[]`: a single Integer or capture name selects one group; a
/// start+length or a Range slices the group list, mirroring `Array#[]` over
/// `#to_a` (the whole match followed by the captures).
fn md_aref(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let array = to_a(mrb, state)?.as_value();
    if let [arg] = args.as_slice() {
        if let Some(index) = numeric_index(mrb, state, *arg)? {
            return array.funcall(mrb, c"[]", &[index.into_value(mrb)]);
        }
    }
    array.funcall(mrb, c"[]", &args)
}

/// The byte span the begin/end/offset argument names, or `None` when the
/// group is valid but did not participate (reported as `nil`). An index past
/// the group count, a negative index, or an undefined capture name raises
/// IndexError, mirroring the curated engine's bounds check.
fn group_at(mrb: &Mrb, state: &MatchState, arg: Value) -> Result<Option<(usize, usize)>, Error> {
    let index = numeric_index(mrb, state, arg)?.unwrap_or(0);
    if index < 0 || index as usize >= state.groups.len() {
        return Err(index_error(mrb, &format!("index {index} out of matches")));
    }
    Ok(state.groups[index as usize])
}

fn md_begin(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    Ok(match group_at(mrb, state, mrb.get_args::<format::O>())? {
        Some((begin, _)) => (begin as i32).into_value(mrb),
        None => Value::nil(),
    })
}

fn md_end(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    Ok(match group_at(mrb, state, mrb.get_args::<format::O>())? {
        Some((_, end)) => (end as i32).into_value(mrb),
        None => Value::nil(),
    })
}

fn md_offset(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    let pair = mrb.ary_new();
    match group_at(mrb, state, mrb.get_args::<format::O>())? {
        Some((begin, end)) => {
            pair.push(mrb, (begin as i32).into_value(mrb))?;
            pair.push(mrb, (end as i32).into_value(mrb))?;
        }
        None => {
            pair.push(mrb, Value::nil())?;
            pair.push(mrb, Value::nil())?;
        }
    }
    Ok(pair.as_value())
}

fn md_captures(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    let captures = mrb.ary_new();
    for index in 1..state.groups.len() {
        captures.push(mrb, group_str(mrb, state, index))?;
    }
    Ok(captures.as_value())
}

fn md_named_captures(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    let symbolize = symbolize_names_requested(mrb)?;
    let map = mrb.hash_new();
    for (name, index) in &state.names {
        let key = mrb.str_new(name.as_bytes()).as_value();
        let key = if symbolize {
            key.funcall(mrb, c"to_sym", &[])?
        } else {
            key
        };
        map.set(mrb, key, group_str(mrb, state, *index))?;
    }
    Ok(map.as_value())
}

/// Read the optional `symbolize_names:` keyword. mruby passes it as a trailing
/// option Hash; a truthy value (Ruby semantics: anything but nil/false) turns
/// the keys into Symbols, as in MRI.
fn symbolize_names_requested(mrb: &Mrb) -> Result<bool, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let Some(options) = args.last().copied().filter(|arg| arg.is_hash()) else {
        return Ok(false);
    };
    let key = mrb
        .str_new(b"symbolize_names")
        .as_value()
        .funcall(mrb, c"to_sym", &[])?;
    Ok(options.funcall(mrb, c"[]", &[key])?.to_bool())
}

fn md_names(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    let names = mrb.ary_new();
    for (name, _) in &state.names {
        names.push(mrb, mrb.str_new(name.as_bytes()).as_value())?;
    }
    Ok(names.as_value())
}

fn md_size(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    (state.groups.len() as i32).into_value(mrb)
}

fn md_pre_match(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    match state.groups.first().copied().flatten() {
        Some((begin, _)) => mrb.str_new(&state.subject.as_bytes()[..begin]).as_value(),
        None => mrb.str_new(b"").as_value(),
    }
}

fn md_post_match(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    match state.groups.first().copied().flatten() {
        Some((_, end)) => mrb.str_new(&state.subject.as_bytes()[end..]).as_value(),
        None => mrb.str_new(b"").as_value(),
    }
}

fn md_string(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    mrb.str_new(state.subject.as_bytes()).as_value()
}

fn md_regexp(mrb: &Mrb, self_: Value) -> Value {
    self_.iv_get(mrb, mrb.intern_cstr(c"@regexp"))
}

fn md_to_a(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let Some(state) = self_.data_get(mrb, &MATCH_TYPE) else {
        return Ok(Value::nil());
    };
    Ok(to_a(mrb, state)?.as_value())
}

/// The whole match followed by each capture, as an Array of Strings (a group
/// that did not participate is `nil`). Shared by `#to_a` and the slicing form
/// of `#[]`.
fn to_a(mrb: &Mrb, state: &MatchState) -> Result<beni::Array, Error> {
    let all = mrb.ary_new();
    for index in 0..state.groups.len() {
        all.push(mrb, group_str(mrb, state, index))?;
    }
    Ok(all)
}

fn md_to_s(mrb: &Mrb, self_: Value) -> Value {
    let state = state_or_nil!(mrb, self_);
    group_str(mrb, state, 0)
}
