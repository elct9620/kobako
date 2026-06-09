//! String ⇄ Regexp integration (SPEC.md B-41) — the `String` methods that
//! take a regexp pattern, routed through the engine.
//!
//! `=~` / `match` / `match?` / `gsub` / `sub` / `scan` are defined outright
//! (a String pattern compiles through `Regexp`). `[]` / `slice` / `index` /
//! `split` keep their non-regexp behaviour by aliasing the core method and
//! delegating to it whenever the argument is not a `Regexp`.

use crate::regexp;
use beni::{format, Error, FromValue, Module, Mrb, Proc, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let cls = mrb.class_get(c"String")?;
    // Preserve each core method under a private name before overriding it,
    // so the non-Regexp dispatch path can delegate back to the original.
    cls.alias_method(mrb, c"__kobako_aref", c"[]")?;
    cls.alias_method(mrb, c"__kobako_aset", c"[]=")?;
    cls.alias_method(mrb, c"__kobako_index", c"index")?;
    cls.alias_method(mrb, c"__kobako_split", c"split")?;

    cls.define_method(mrb, c"=~", beni::method!(str_eqtilde, -1))?;
    cls.define_method(mrb, c"match", beni::method!(str_match, -1))?;
    cls.define_method(mrb, c"match?", beni::method!(str_match_p, -1))?;
    cls.define_method(mrb, c"scan", beni::method!(str_scan, -1))?;
    cls.define_method(mrb, c"gsub", beni::method!(str_gsub, -1))?;
    cls.define_method(mrb, c"sub", beni::method!(str_sub, -1))?;
    cls.define_method(mrb, c"split", beni::method!(str_split, -1))?;
    cls.define_method(mrb, c"index", beni::method!(str_index, -1))?;
    cls.define_method(mrb, c"[]", beni::method!(str_aref, -1))?;
    cls.define_method(mrb, c"slice", beni::method!(str_aref, -1))?;
    cls.define_method(mrb, c"[]=", beni::method!(str_aset, -1))?;
    cls.define_method(mrb, c"slice!", beni::method!(str_slice_bang, -1))?;
    Ok(())
}

fn str_eqtilde(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Ok(Value::nil());
    }
    Ok(regexp::coerce_regexp(mrb, arg)?.call(mrb, c"=~", &[self_]))
}

fn str_match(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Ok(Value::nil());
    }
    let re = regexp::coerce_regexp(mrb, args[0])?;
    let forwarded: Vec<Value> = core::iter::once(self_)
        .chain(args[1..].iter().copied())
        .collect();
    Ok(re.call(mrb, c"match", &forwarded))
}

fn str_match_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Ok(Value::false_());
    }
    let re = regexp::coerce_regexp(mrb, args[0])?;
    let forwarded: Vec<Value> = core::iter::once(self_)
        .chain(args[1..].iter().copied())
        .collect();
    Ok(re.call(mrb, c"match?", &forwarded))
}

fn str_scan(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    let result = mrb.ary_new();
    if args.is_empty() {
        return Ok(result.as_value());
    }
    let re = regexp::coerce_regexp(mrb, args[0])?;
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject)?;
    let block = Proc::from_value(block);
    for span in &spans {
        let item = scan_item(mrb, &subject, span);
        match block {
            Some(b) => {
                let _ = b.call(mrb, &[item]);
            }
            None => result.push(mrb, item),
        }
    }
    if block.is_some() {
        Ok(self_)
    } else {
        Ok(result.as_value())
    }
}

/// One `scan` element: the whole match for a group-less pattern, otherwise
/// an array of the group substrings.
fn scan_item(mrb: &Mrb, subject: &str, span: &regexp::MatchSpan) -> Value {
    if span.groups.is_empty() {
        return mrb.str_new(&subject.as_bytes()[span.whole.0..span.whole.1]);
    }
    let tuple = mrb.ary_new();
    for group in &span.groups {
        tuple.push(mrb, span_str(mrb, subject, *group));
    }
    tuple.as_value()
}

fn str_gsub(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    if args.is_empty() {
        return Ok(self_);
    }
    let re = regexp::coerce_regexp(mrb, args[0])?;
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject)?;
    let block = Proc::from_value(block);
    let replacement = args.get(1).copied();
    let mut out = String::with_capacity(subject.len());
    let mut last = 0;
    for span in &spans {
        let (start, end) = span.whole;
        out.push_str(&subject[last..start]);
        out.push_str(&substitution(mrb, re, &subject, span, block, replacement));
        last = end;
    }
    out.push_str(&subject[last..]);
    Ok(mrb.str_new(out.as_bytes()))
}

fn str_sub(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    if args.is_empty() {
        return Ok(self_);
    }
    let re = regexp::coerce_regexp(mrb, args[0])?;
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject)?;
    let block = Proc::from_value(block);
    let replacement = args.get(1).copied();
    let Some(span) = spans.first() else {
        return Ok(mrb.str_new(subject.as_bytes()));
    };
    let (start, end) = span.whole;
    let mut out = String::with_capacity(subject.len());
    out.push_str(&subject[..start]);
    out.push_str(&substitution(mrb, re, &subject, span, block, replacement));
    out.push_str(&subject[end..]);
    Ok(mrb.str_new(out.as_bytes()))
}

/// The replacement text for one match: a block's result (with `$1..$9`
/// refreshed first) or the literal replacement String.
fn substitution(
    mrb: &Mrb,
    re: Value,
    subject: &str,
    span: &regexp::MatchSpan,
    block: Option<Proc>,
    replacement: Option<Value>,
) -> String {
    let (start, end) = span.whole;
    if let Some(b) = block {
        regexp::set_span_globals(mrb, re, subject, span);
        let matched = mrb.str_new(&subject.as_bytes()[start..end]);
        b.call(mrb, &[matched])
            .map(|v| v.to_string(mrb))
            .unwrap_or_default()
    } else if let Some(rep) = replacement {
        rep.to_string(mrb)
    } else {
        String::new()
    }
}

/// `String#split` on a `Regexp`: the text between matches, with each match's
/// capture groups interleaved (a non-participating group is `nil`). A
/// positive +limit+ caps the field count (the remainder stays unsplit as the
/// last field); an omitted or `0` limit drops trailing empty fields; a
/// negative limit keeps them. A non-`Regexp` argument delegates to the core
/// method, which handles its own limit.
fn str_split(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return Ok(self_.call(mrb, c"__kobako_split", &args));
    }
    let subject = self_.to_string(mrb);
    let limit = args.get(1).and_then(|v| i32::from_value(*v)).unwrap_or(0);
    let spans = regexp::match_spans(mrb, args[0], &subject)?;

    let mut fields: Vec<Option<(usize, usize)>> = Vec::new();
    let mut last = 0;
    for (splits, span) in spans.iter().enumerate() {
        if limit > 0 && splits >= (limit - 1) as usize {
            break;
        }
        fields.push(Some((last, span.whole.0)));
        fields.extend(span.groups.iter().copied());
        last = span.whole.1;
    }
    fields.push(Some((last, subject.len())));
    if limit == 0 {
        while matches!(fields.last().copied(), Some(Some((s, e))) if s == e) {
            fields.pop();
        }
    }

    let result = mrb.ary_new();
    for field in fields {
        match field {
            Some((start, end)) => result.push(mrb, mrb.str_new(&subject.as_bytes()[start..end])),
            None => result.push(mrb, Value::nil()),
        }
    }
    Ok(result.as_value())
}

/// `String#index(re, pos)`: the byte index of the first match at or after
/// `pos` (a negative `pos` counts from the end), or `nil`. A non-`Regexp`
/// argument delegates to the core method, which handles its own `pos`.
fn str_index(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return self_.call(mrb, c"__kobako_index", &args);
    }
    let subject = self_.to_string(mrb);
    let size = subject.len() as i64;
    let pos = args.get(1).and_then(|v| i32::from_value(*v)).unwrap_or(0) as i64;
    let start = if pos < 0 { size + pos } else { pos };
    if start < 0 || start > size {
        return Value::nil();
    }
    let start = start as usize;
    let tail = mrb.str_new(&subject.as_bytes()[start..]);
    match i32::from_value(args[0].call(mrb, c"=~", &[tail])) {
        Some(offset) => Value::from_int(mrb, (offset as i64 + start as i64) as _),
        None => Value::nil(),
    }
}

fn str_aref(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return self_.call(mrb, c"__kobako_aref", &args);
    }
    let md = args[0].call(mrb, c"match", &[self_]);
    if md.is_nil() {
        return Value::nil();
    }
    let group = args
        .get(1)
        .copied()
        .unwrap_or_else(|| Value::from_int(mrb, 0));
    md.call(mrb, c"[]", &[group])
}

/// `String#[]=` on a `Regexp`: match the pattern and overwrite the matched
/// region in place — the whole match for the 2-arg form, capture group `n`
/// for the 3-arg form — then return the receiver. A non-`Regexp` first
/// argument delegates to the core method. A non-matching pattern raises
/// `IndexError`, as `str[regexp] = x` does in MRI.
fn str_aset(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return Ok(self_.call(mrb, c"__kobako_aset", &args));
    }
    let (group, replacement) = match args.len() {
        2 => (Value::from_int(mrb, 0), args[1]),
        3 => (args[1], args[2]),
        n => {
            return Err(argument_error(
                mrb,
                &format!("wrong number of arguments ({n} for 2..3)"),
            ))
        }
    };
    let md = args[0].call(mrb, c"match", &[self_]);
    if md.is_nil() {
        return Err(index_error(mrb, "regexp not matched"));
    }
    let begin = md.call(mrb, c"begin", &[group]);
    let end = md.call(mrb, c"end", &[group]);
    let (Some(b), Some(e)) = (i32::from_value(begin), i32::from_value(end)) else {
        return Err(index_error(mrb, "regexp not matched"));
    };
    let len = Value::from_int(mrb, (e - b) as _);
    self_.call(mrb, c"__kobako_aset", &[begin, len, replacement]);
    Ok(self_)
}

/// `String#slice!` — slice the matched (or indexed) portion out in place and
/// return it. The C gem implements every form here, as the core has no
/// `slice!`: a `Regexp` form saves and restores `$~` around the inner delete
/// so the visible match stays the slice's own; an Integer / Range / String
/// form deletes through the core `[]=`. Returns `nil`, leaving the string
/// untouched, when nothing matched.
fn str_slice_bang(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let Some(&nth) = args.first() else {
        return Err(argument_error(
            mrb,
            "wrong number of arguments (given 0, expected 1..2)",
        ));
    };
    let result = self_.call(mrb, c"slice", &args);
    let regexp_form = regexp::is_regexp(mrb, nth);
    let saved = regexp_form.then(|| mrb.gv_get(mrb.intern_cstr(c"$~")));
    if !result.is_nil() && slice_bang_should_delete(mrb, self_, &args, regexp_form) {
        let empty = mrb.str_new(b"");
        match args.get(1) {
            Some(&len) => self_.call(mrb, c"[]=", &[nth, len, empty]),
            None => self_.call(mrb, c"[]=", &[nth, empty]),
        };
    }
    if let Some(last_match) = saved {
        mrb.gv_set(mrb.intern_cstr(c"$~"), last_match);
    }
    Ok(result)
}

/// Whether `slice!`'s in-place delete runs: always for the 1-arg or `Regexp`
/// forms; the non-`Regexp` 2-arg form skips a start index sitting at the end
/// of the string (the C gem's `nth != self.size` guard against an empty tail).
fn slice_bang_should_delete(mrb: &Mrb, self_: Value, args: &[Value], regexp_form: bool) -> bool {
    if regexp_form || args.len() < 2 {
        return true;
    }
    let size = self_.call(mrb, c"size", &[]);
    i32::from_value(args[0]) != i32::from_value(size)
}

/// String value of a byte-range group, or `nil` for an absent group.
fn span_str(mrb: &Mrb, subject: &str, group: Option<(usize, usize)>) -> Value {
    match group {
        Some((start, end)) => mrb.str_new(&subject.as_bytes()[start..end]),
        None => Value::nil(),
    }
}

/// Build an `IndexError` carrying `message`.
fn index_error(mrb: &Mrb, message: &str) -> Error {
    let cls = mrb
        .class_get(c"IndexError")
        .expect("IndexError is an mruby core class");
    Error::Exception(cls.exc_new(mrb, message))
}

/// Build an `ArgumentError` carrying `message`.
fn argument_error(mrb: &Mrb, message: &str) -> Error {
    let cls = mrb
        .class_get(c"ArgumentError")
        .expect("ArgumentError is an mruby core class");
    Error::Exception(cls.exc_new(mrb, message))
}
