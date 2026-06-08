//! Pattern and flag translation from Ruby / Onigmo regexp syntax to the
//! `fancy-regex` (regex-crate) dialect.
//!
//! These are pure string transforms with no mruby dependency, so they are
//! unit-tested directly on the host. Two Ruby↔regex-crate mismatches drive
//! the work: Ruby's `^` / `$` always match per line (regex needs the `m`
//! flag), Ruby `/m` means DOTALL (regex spells that `s`), and Ruby's
//! `\d` / `\w` / `\s` are ASCII while the regex crate's are Unicode — so the
//! shorthand classes are rewritten to explicit ASCII ranges.

/// MRI `Regexp` option bits. The Rust gem exposes these rather than
/// Onigmo's internal mask, so `Regexp#options` reads back as MRI does.
pub const IGNORECASE: i64 = 1;
pub const EXTENDED: i64 = 2;
pub const MULTILINE: i64 = 4;

/// Parse the letter flags a regexp literal carries (`"imx"`) into the MRI
/// option bitmask. Unknown letters are ignored.
pub fn parse_flag_string(flags: &str) -> i64 {
    let mut options = 0;
    for letter in flags.chars() {
        match letter {
            'i' => options |= IGNORECASE,
            'm' => options |= MULTILINE,
            'x' => options |= EXTENDED,
            _ => {}
        }
    }
    options
}

/// Build a `fancy-regex` pattern string from a Ruby `source` and MRI
/// `options`. The multiline (`m`) flag is always set so `^` / `$` match per
/// line as in Ruby; Ruby's `/m` maps to the regex `s` (DOTALL) flag.
pub fn build_pattern(source: &str, options: i64) -> String {
    let mut out = String::with_capacity(source.len() + 8);
    out.push_str("(?m");
    if options & IGNORECASE != 0 {
        out.push('i');
    }
    if options & EXTENDED != 0 {
        out.push('x');
    }
    if options & MULTILINE != 0 {
        out.push('s');
    }
    out.push(')');
    rewrite_ascii_classes(source, &mut out);
    out
}

/// Rewrite Ruby's ASCII `\d` / `\w` / `\s` (and the `\D` / `\W` / `\S`
/// negations) into explicit ASCII ranges, appending to `out`. Escaped
/// backslashes (`\\`) are consumed as a pair so the following letter is not
/// misread as a shorthand class, and the inside-vs-outside-character-class
/// form is tracked so `[\d]` becomes `[0-9]` rather than `[[0-9]]`.
fn rewrite_ascii_classes(source: &str, out: &mut String) {
    let mut chars = source.chars();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(letter) => match ascii_class(letter, in_class) {
                    Some(replacement) => out.push_str(replacement),
                    None => {
                        out.push('\\');
                        out.push(letter);
                    }
                },
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push('[');
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            _ => out.push(c),
        }
    }
}

/// The explicit ASCII range a shorthand class letter expands to, or `None`
/// when the letter is not a rewritten shorthand. Negated forms only apply
/// outside a character class, where a class wrapper is meaningful.
fn ascii_class(letter: char, in_class: bool) -> Option<&'static str> {
    match letter {
        'd' if in_class => Some("0-9"),
        'd' => Some("[0-9]"),
        'w' if in_class => Some("0-9A-Za-z_"),
        'w' => Some("[0-9A-Za-z_]"),
        's' if in_class => Some(r"\x20\t\n\x0b\x0c\r"),
        's' => Some(r"[\x20\t\n\x0b\x0c\r]"),
        'D' if !in_class => Some("[^0-9]"),
        'W' if !in_class => Some("[^0-9A-Za-z_]"),
        'S' if !in_class => Some(r"[^\x20\t\n\x0b\x0c\r]"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_string_parses_known_letters() {
        assert_eq!(parse_flag_string(""), 0);
        assert_eq!(parse_flag_string("i"), IGNORECASE);
        assert_eq!(parse_flag_string("im"), IGNORECASE | MULTILINE);
        assert_eq!(parse_flag_string("imx"), IGNORECASE | MULTILINE | EXTENDED);
        assert_eq!(parse_flag_string("z"), 0);
    }

    #[test]
    fn pattern_always_enables_multiline_for_ruby_anchors() {
        assert_eq!(build_pattern("abc", 0), "(?m)abc");
    }

    #[test]
    fn pattern_maps_options_to_inline_flags() {
        assert_eq!(build_pattern("abc", IGNORECASE), "(?mi)abc");
        assert_eq!(build_pattern("abc", EXTENDED), "(?mx)abc");
        // Ruby /m is DOTALL, which the regex crate spells `s`.
        assert_eq!(build_pattern("abc", MULTILINE), "(?ms)abc");
        assert_eq!(
            build_pattern("abc", IGNORECASE | EXTENDED | MULTILINE),
            "(?mixs)abc"
        );
    }

    #[test]
    fn rewrites_shorthand_classes_outside_a_class() {
        assert_eq!(build_pattern(r"\d+", 0), r"(?m)[0-9]+");
        assert_eq!(build_pattern(r"\w", 0), r"(?m)[0-9A-Za-z_]");
        assert_eq!(build_pattern(r"\D", 0), r"(?m)[^0-9]");
        assert_eq!(build_pattern(r"\s", 0), r"(?m)[\x20\t\n\x0b\x0c\r]");
    }

    #[test]
    fn rewrites_shorthand_inside_a_character_class() {
        assert_eq!(build_pattern(r"[\d]", 0), r"(?m)[0-9]");
        assert_eq!(build_pattern(r"[a\dz]", 0), r"(?m)[a0-9z]");
        assert_eq!(build_pattern(r"[^\d]", 0), r"(?m)[^0-9]");
    }

    #[test]
    fn leaves_an_escaped_backslash_as_a_literal() {
        // `\\d` is a literal backslash then `d`, not a shorthand class.
        assert_eq!(build_pattern(r"\\d", 0), r"(?m)\\d");
    }

    #[test]
    fn leaves_non_shorthand_escapes_untouched() {
        assert_eq!(build_pattern(r"\bword\b", 0), r"(?m)\bword\b");
        assert_eq!(build_pattern(r"a\.b", 0), r"(?m)a\.b");
    }
}
