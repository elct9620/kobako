//! Textual rendering of a pattern: the flag letters and source escaping
//! behind `Regexp#inspect` / `#to_s` / `Regexp.escape`.

use crate::translate;

/// MRI's `m`, `i`, `x` order, as `Regexp#inspect` appends them.
pub(super) fn enabled_flags(options: i64) -> String {
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

pub(super) fn on_off_flags(options: i64) -> (String, String) {
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

/// Mirrors the lift MRI's `Regexp#to_s` applies: only an inline-flag group
/// spanning the whole source lifts, never a named group such as
/// `(?<name>…)`.
pub(super) fn lift_inline_group(source: &str) -> Option<(i64, i64, &str)> {
    let bytes = source.as_bytes();
    if !source.starts_with("(?") {
        return None;
    }
    let mut i = 2;
    let mut enabled = 0;
    let mut disabled = 0;
    let mut in_disable = false;
    loop {
        match bytes.get(i)? {
            b':' => {
                i += 1;
                break;
            }
            b'-' if !in_disable => {
                in_disable = true;
                i += 1;
            }
            b'i' | b'm' | b'x' => {
                let bit = match bytes[i] {
                    b'i' => translate::IGNORECASE,
                    b'm' => translate::MULTILINE,
                    _ => translate::EXTENDED,
                };
                if in_disable {
                    disabled |= bit;
                } else {
                    enabled |= bit;
                }
                i += 1;
            }
            _ => return None,
        }
    }
    let body_start = i;
    let mut depth = 1;
    let mut in_class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => depth += 1,
            b')' if !in_class => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if depth == 0 && i == bytes.len() - 1 {
        Some((enabled, disabled, &source[body_start..i]))
    } else {
        None
    }
}

/// Matches MRI: whitespace controls pass through literally while other
/// controls render as `\xHH`.
pub(super) fn inspect_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for c in source.chars() {
        match c {
            '/' => out.push_str("\\/"),
            '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' => out.push(c),
            c if c.is_control() => out.push_str(&format!("\\x{:02X}", u32::from(c))),
            _ => out.push(c),
        }
    }
    out
}

pub(super) fn escape_str(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for c in source.chars() {
        match c {
            '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            | '-' | '#' | ' ' => {
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
