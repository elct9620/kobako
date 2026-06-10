//! The `$~` / `$&` / `` $` `` / `$'` / `$+` / `$1..$9` match globals,
//! refreshed on every successful match and cleared on a miss (SPEC.md B-41).

use crate::matchdata::{self, MatchState};
use beni::{Mrb, Value};
use core::ffi::CStr;

use super::replace::MatchSpan;

/// `$1`..`$9` global names, indexed by capture group minus one.
const NUMBERED: [&CStr; 9] = [
    c"$1", c"$2", c"$3", c"$4", c"$5", c"$6", c"$7", c"$8", c"$9",
];

/// Build a `MatchData`, bind `$~`, and refresh `$&` / `` $` `` / `$'` /
/// `$1..$9` from a match's byte spans (`groups[0]` is the whole match);
/// returns the `MatchData`.
pub(super) fn finalize(
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
pub(super) fn clear_globals(mrb: &Mrb) {
    mrb.gv_set(mrb.intern_cstr(c"$~"), Value::nil());
    for name in [c"$&", c"$`", c"$'", c"$+"].iter().chain(NUMBERED.iter()) {
        set_global(mrb, name, None);
    }
}

fn set_global(mrb: &Mrb, name: &CStr, value: Option<Value>) {
    mrb.gv_set(mrb.intern_cstr(name), value.unwrap_or_else(Value::nil));
}
