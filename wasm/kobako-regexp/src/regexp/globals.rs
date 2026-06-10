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

/// Build a `MatchData`, bind `$~`, and refresh the derived globals from a
/// match's byte spans (`groups[0]` is the whole match); returns the `MatchData`.
pub(super) fn finalize(
    mrb: &Mrb,
    regexp: Value,
    subject: String,
    groups: Vec<Option<(usize, usize)>>,
    names: Vec<(String, usize)>,
) -> Value {
    // Set the derived globals from borrows first, then hand the subject and
    // group spans to the `MatchData` by move — the match owns them outright, so
    // neither is cloned.
    set_derived(mrb, &subject, &groups);
    let md = matchdata::build(
        mrb,
        regexp,
        MatchState {
            subject,
            groups,
            names,
        },
    );
    mrb.gv_set(mrb.intern_cstr(c"$~"), md);
    md
}

/// Set `$&` / `` $` `` / `$'` / `$+` / `$1..$9` from a match's byte spans
/// (`groups[0]` is the whole match); does not touch `$~`. The derived globals
/// are views of `$~`, so both `finalize` and `Regexp.last_match=` refresh them
/// from this one routine.
fn set_derived(mrb: &Mrb, subject: &str, groups: &[Option<(usize, usize)>]) {
    let whole = groups.first().copied().flatten();
    set_global(
        mrb,
        c"$&",
        whole.map(|(s, e)| mrb.str_new(&subject.as_bytes()[s..e])),
    );
    set_global(
        mrb,
        c"$`",
        whole.map(|(s, _)| mrb.str_new(&subject.as_bytes()[..s])),
    );
    set_global(
        mrb,
        c"$'",
        whole.map(|(_, e)| mrb.str_new(&subject.as_bytes()[e..])),
    );
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
}

/// Set `$~` to `value` and refresh its derived views (`Regexp.last_match=`).
/// A `MatchData` refreshes `$&` / `` $` `` / `$'` / `$+` / `$1..$9` from its
/// own spans; `nil` or any non-`MatchData` leaves no captures to view, so the
/// derived globals clear.
pub(super) fn set_last_match(mrb: &Mrb, value: Value) {
    mrb.gv_set(mrb.intern_cstr(c"$~"), value);
    match matchdata::state_of(mrb, value) {
        Some(state) => set_derived(mrb, &state.subject, &state.groups),
        None => clear_derived(mrb),
    }
}

/// Refresh the match globals for a gsub/scan block from owned spans, so
/// `$1` is fresh on each iteration.
pub(crate) fn set_span_globals(mrb: &Mrb, regexp: Value, subject: &str, span: &MatchSpan) {
    let mut groups = Vec::with_capacity(span.groups.len() + 1);
    groups.push(Some(span.whole));
    groups.extend(span.groups.iter().copied());
    // A gsub/scan block reuses one subject across iterations, so each match's
    // `MatchData` takes its own owned copy.
    finalize(mrb, regexp, subject.to_owned(), groups, Vec::new());
}

/// Reset every match global to nil after a failed match.
pub(super) fn clear_globals(mrb: &Mrb) {
    mrb.gv_set(mrb.intern_cstr(c"$~"), Value::nil());
    clear_derived(mrb);
}

/// Reset the derived globals (`$&` / `` $` `` / `$'` / `$+` / `$1..$9`) to
/// nil, leaving `$~` untouched — the clear half shared by a failed match and a
/// `Regexp.last_match=` to a non-`MatchData`.
fn clear_derived(mrb: &Mrb) {
    for name in [c"$&", c"$`", c"$'", c"$+"].iter().chain(NUMBERED.iter()) {
        set_global(mrb, name, None);
    }
}

fn set_global(mrb: &Mrb, name: &CStr, value: Option<Value>) {
    mrb.gv_set(mrb.intern_cstr(name), value.unwrap_or_else(Value::nil));
}
