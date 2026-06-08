//! String ⇄ Regexp integration (SPEC.md B-41) — the `String` methods that
//! take a regexp pattern, routed through the engine.
//!
//! `=~` / `match` / `match?` / `gsub` / `sub` / `scan` are defined outright
//! (a String pattern compiles through `Regexp`). `[]` / `slice` / `index` /
//! `split` keep their non-regexp behaviour by aliasing the core method and
//! delegating to it whenever the argument is not a `Regexp`.

use crate::regexp;
use beni::{format, FromValue, Module, Mrb, Proc, Value};

pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let cls = mrb.class_get(c"String")?;
    // SAFETY: `cls` is the live String class; reifying it as a value is
    // GC-stable for the VM lifetime.
    let cls_val = unsafe { cls.to_value(mrb) };
    alias(mrb, cls_val, c"__kobako_aref", c"[]");
    alias(mrb, cls_val, c"__kobako_index", c"index");
    alias(mrb, cls_val, c"__kobako_split", c"split");

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
    Ok(())
}

/// `alias_method(new, old)` on the class via funcall (it is private, which
/// funcall bypasses); names ride as Strings, which `alias_method` accepts.
fn alias(mrb: &Mrb, cls_val: Value, new_name: &core::ffi::CStr, old_name: &core::ffi::CStr) {
    cls_val.call(
        mrb,
        c"alias_method",
        &[
            mrb.str_new(new_name.to_bytes()),
            mrb.str_new(old_name.to_bytes()),
        ],
    );
}

fn str_eqtilde(mrb: &Mrb, self_: Value) -> Value {
    let arg = mrb.get_args::<format::O>();
    if arg.is_nil() {
        return Value::nil();
    }
    regexp::coerce_regexp(mrb, arg).call(mrb, c"=~", &[self_])
}

fn str_match(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Value::nil();
    }
    let re = regexp::coerce_regexp(mrb, args[0]);
    let forwarded: Vec<Value> = core::iter::once(self_)
        .chain(args[1..].iter().copied())
        .collect();
    re.call(mrb, c"match", &forwarded)
}

fn str_match_p(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if args.is_empty() {
        return Value::false_();
    }
    let re = regexp::coerce_regexp(mrb, args[0]);
    let forwarded: Vec<Value> = core::iter::once(self_)
        .chain(args[1..].iter().copied())
        .collect();
    re.call(mrb, c"match?", &forwarded)
}

fn str_scan(mrb: &Mrb, self_: Value) -> Value {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    let result = mrb.ary_new();
    if args.is_empty() {
        return result.as_value();
    }
    let re = regexp::coerce_regexp(mrb, args[0]);
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject);
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
        self_
    } else {
        result.as_value()
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

fn str_gsub(mrb: &Mrb, self_: Value) -> Value {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    if args.is_empty() {
        return self_;
    }
    let re = regexp::coerce_regexp(mrb, args[0]);
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject);
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
    mrb.str_new(out.as_bytes())
}

fn str_sub(mrb: &Mrb, self_: Value) -> Value {
    let (args, block) = mrb.get_args::<format::RestBlock>();
    let args = args.to_vec();
    if args.is_empty() {
        return self_;
    }
    let re = regexp::coerce_regexp(mrb, args[0]);
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, re, &subject);
    let block = Proc::from_value(block);
    let replacement = args.get(1).copied();
    let Some(span) = spans.first() else {
        return mrb.str_new(subject.as_bytes());
    };
    let (start, end) = span.whole;
    let mut out = String::with_capacity(subject.len());
    out.push_str(&subject[..start]);
    out.push_str(&substitution(mrb, re, &subject, span, block, replacement));
    out.push_str(&subject[end..]);
    mrb.str_new(out.as_bytes())
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

fn str_split(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return self_.call(mrb, c"__kobako_split", &args);
    }
    let subject = self_.to_string(mrb);
    let spans = regexp::match_spans(mrb, args[0], &subject);
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut last = 0;
    for span in &spans {
        segments.push((last, span.whole.0));
        last = span.whole.1;
    }
    segments.push((last, subject.len()));
    while segments.last().is_some_and(|(s, e)| s == e) {
        segments.pop();
    }
    let result = mrb.ary_new();
    for (start, end) in segments {
        result.push(mrb, mrb.str_new(&subject.as_bytes()[start..end]));
    }
    result.as_value()
}

fn str_index(mrb: &Mrb, self_: Value) -> Value {
    let args: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if !args.first().is_some_and(|a| regexp::is_regexp(mrb, *a)) {
        return self_.call(mrb, c"__kobako_index", &args);
    }
    // `str.index(re)` is the byte index of the first match, or nil.
    args[0].call(mrb, c"=~", &[self_])
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

/// String value of a byte-range group, or `nil` for an absent group.
fn span_str(mrb: &Mrb, subject: &str, group: Option<(usize, usize)>) -> Value {
    match group {
        Some((start, end)) => mrb.str_new(&subject.as_bytes()[start..end]),
        None => Value::nil(),
    }
}
