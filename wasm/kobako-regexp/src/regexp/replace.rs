//! Owned match spans and gsub/sub replacement expansion — the engine-borrow
//! bridge the String integration builds results from.

use crate::errors::{index_error, regexp_error, replace_expression_error};
use beni::{Error, Mrb, Value};

use super::{state_of, RegexpState};

/// Byte spans of one match: the whole match plus each numbered group.
pub(crate) struct MatchSpan {
    pub whole: (usize, usize),
    pub groups: Vec<Option<(usize, usize)>>,
}

/// Owned spans, so the String methods can build results after the engine
/// borrow is released. A zero-width match advances by one character.
pub(crate) fn match_spans(
    mrb: &Mrb,
    regexp: Value,
    subject: &str,
) -> Result<Vec<MatchSpan>, Error> {
    let Some(state) = state_of(mrb, regexp) else {
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

/// Mirrors the curated regexp engine: an out-of-range group number inserts
/// nothing, an undefined `\k<name>` raises `IndexError`, and an unknown
/// `\x` or a trailing backslash stays literal.
pub(crate) fn expand_replacement(
    mrb: &Mrb,
    regexp: Value,
    subject: &str,
    span: &MatchSpan,
    replacement: &str,
) -> Result<String, Error> {
    let state = state_of(mrb, regexp);
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

/// A `\k` not followed by `<…>` is an invalid replace expression.
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

fn name_to_index(state: &RegexpState, name: &str) -> Option<usize> {
    state.regex.capture_names().position(|n| n == Some(name))
}
