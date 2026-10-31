//! Pattern and flag translation from Ruby regexp syntax to the
//! `fancy-regex` (regex-crate) dialect.
//!
//! These are pure string transforms with no mruby dependency, so they are
//! unit-tested directly on the host. Three Ruby↔regex-crate mismatches drive
//! the work: Ruby's `^` / `$` always match per line (regex needs the `m`
//! flag), Ruby `/m` means DOTALL (regex spells that `s`), and Ruby's
//! `\d` / `\w` / `\s` are ASCII while the regex crate's are Unicode — so the
//! shorthand classes are rewritten to explicit ASCII ranges.

/// MRI `Regexp` option bits. The Rust gem exposes these rather than
/// the engine's internal mask, so `Regexp#options` reads back as MRI does.
pub(crate) const IGNORECASE: i64 = 1;
pub(crate) const EXTENDED: i64 = 2;
pub(crate) const MULTILINE: i64 = 4;

/// The option bits a flag string names, or `None` when it carries a letter
/// MRI does not name.
pub(crate) fn parse_flag_string(flags: &str) -> Option<i64> {
    flags.chars().try_fold(0, |options, letter| match letter {
        'i' => Some(options | IGNORECASE),
        'm' => Some(options | MULTILINE),
        'x' => Some(options | EXTENDED),
        _ => None,
    })
}

/// The engine's multiline flag is always set so `^` / `$` match per line as
/// in Ruby; Ruby's `/m` maps to the engine's `s` (DOTALL) flag.
///
/// The answer is the engine's pattern, or MRI's reason for refusing the source.
pub(crate) fn build_pattern(source: &str, options: i64) -> Result<String, String> {
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
    let extended = options & EXTENDED != 0;
    let names = declared_names(source, extended);
    for piece in Pieces::new(source, extended) {
        rewrite_piece(piece, &names, &mut out)?;
    }
    Ok(out)
}

/// Each capture name with its group numbers. A named group makes every plain
/// group non-capturing, so only named groups count.
fn declared_names(source: &str, extended: bool) -> Vec<(&str, Vec<usize>)> {
    let mut names: Vec<(&str, Vec<usize>)> = Vec::new();
    let mut number = 0;
    for piece in Pieces::new(source, extended) {
        let Piece::NamedGroup { name, .. } = piece else {
            continue;
        };
        number += 1;
        match names.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, numbers)) => numbers.push(number),
            None => names.push((name, vec![number])),
        }
    }
    names
}

fn rewrite_piece(
    piece: Piece,
    names: &[(&str, Vec<usize>)],
    out: &mut String,
) -> Result<(), String> {
    let named = !names.is_empty();
    match piece {
        Piece::Text(text) | Piece::NamedGroup { text, .. } => out.push_str(text),
        Piece::PlainGroup => out.push_str(if named { "(?:" } else { "(" }),
        Piece::Escape { letter, in_class } => match ascii_class(letter, in_class) {
            Some(replacement) => out.push_str(replacement),
            None => {
                out.push('\\');
                out.push(letter);
            }
        },
        Piece::NumberedReference(text) | Piece::NumberedCondition(text) => {
            if named {
                return Err(NUMBERED_REFERENCE.to_string());
            }
            out.push_str(text);
        }
        Piece::NamedReference { text, kind, name } => {
            if named && is_group_number(name) {
                return Err(NUMBERED_REFERENCE.to_string());
            }
            match (kind, shared_numbers(names, name)) {
                ('g', Some(_)) => return Err(format!("multiplex definition name <{name}> call")),
                (_, Some(numbers)) => push_each_group(numbers, out),
                _ => out.push_str(text),
            }
        }
        Piece::PythonGroup => return Err("undefined group option".to_string()),
    }
    Ok(())
}

const NUMBERED_REFERENCE: &str = "numbered backref/call is not allowed. (use name)";

fn shared_numbers<'n>(names: &'n [(&str, Vec<usize>)], name: &str) -> Option<&'n [usize]> {
    names
        .iter()
        .find(|(existing, numbers)| *existing == name && numbers.len() > 1)
        .map(|(_, numbers)| numbers.as_slice())
}

/// MRI tries a shared name's groups from the last, settling on the first that
/// matches: an atomic alternation, since it never returns to try another.
fn push_each_group(numbers: &[usize], out: &mut String) {
    out.push_str("(?>");
    for (position, number) in numbers.iter().rev().enumerate() {
        if position > 0 {
            out.push('|');
        }
        out.push('\\');
        out.push_str(&number.to_string());
    }
    out.push(')');
}

fn is_group_number(name: &str) -> bool {
    let digits = name.strip_prefix('-').unwrap_or(name);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// One meaningful stretch of a pattern's source.
enum Piece<'a> {
    Text(&'a str),
    Escape {
        letter: char,
        in_class: bool,
    },
    PlainGroup,
    NamedGroup {
        text: &'a str,
        name: &'a str,
    },
    NumberedReference(&'a str),
    NamedReference {
        text: &'a str,
        kind: char,
        name: &'a str,
    },
    NumberedCondition(&'a str),
    PythonGroup,
}

/// Reads a source the way MRI does, so an escape, a character class or a
/// comment never reads as a group or a reference.
struct Pieces<'a> {
    rest: &'a str,
    class_depth: usize,
    extended: bool,
}

impl<'a> Pieces<'a> {
    fn new(source: &'a str, extended: bool) -> Self {
        Self {
            rest: source,
            class_depth: 0,
            extended,
        }
    }

    fn take(&mut self, len: usize) -> &'a str {
        let (taken, rest) = self.rest.split_at(len);
        self.rest = rest;
        taken
    }

    fn escape(&mut self) -> Piece<'a> {
        let after = &self.rest[1..];
        let Some(letter) = after.chars().next() else {
            return Piece::Text(self.take(1));
        };
        let in_class = self.class_depth > 0;
        if !in_class {
            if matches!(letter, 'k' | 'g') {
                if let Some((name, len)) = delimited(&after[1..]) {
                    let text = self.take(2 + len);
                    return Piece::NamedReference {
                        text,
                        kind: letter,
                        name,
                    };
                }
            }
            if ('1'..='9').contains(&letter) {
                let digits = after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len());
                return Piece::NumberedReference(self.take(1 + digits));
            }
        }
        self.take(1 + letter.len_utf8());
        Piece::Escape { letter, in_class }
    }

    fn group(&mut self) -> Piece<'a> {
        let Some(option) = self.rest[1..].strip_prefix('?') else {
            self.take(1);
            return Piece::PlainGroup;
        };
        if option.starts_with('#') {
            return Piece::Text(self.take(comment_len(self.rest)));
        }
        if option.starts_with('P') {
            self.take(1);
            return Piece::PythonGroup;
        }
        if option.starts_with('(') && option[1..].starts_with(|c: char| c.is_ascii_digit()) {
            return Piece::NumberedCondition(self.take(3));
        }
        if !option.starts_with("<=") && !option.starts_with("<!") {
            if let Some((name, len)) = delimited(option) {
                let text = self.take(2 + len);
                return Piece::NamedGroup { text, name };
            }
        }
        Piece::Text(self.take(1))
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        let first = self.rest.chars().next()?;
        if first == '\\' {
            return Some(self.escape());
        }
        let text_len = match first {
            '[' => {
                self.class_depth += 1;
                1
            }
            ']' if self.class_depth > 0 => {
                self.class_depth -= 1;
                1
            }
            '(' if self.class_depth == 0 => return Some(self.group()),
            '#' if self.extended && self.class_depth == 0 => self
                .rest
                .find('\n')
                .map_or(self.rest.len(), |newline| newline + 1),
            _ => first.len_utf8(),
        };
        Some(Piece::Text(self.take(text_len)))
    }
}

/// The name in `<name>` or `'name'`, with the length the delimiters span.
fn delimited(text: &str) -> Option<(&str, usize)> {
    let close = match text.chars().next()? {
        '<' => '>',
        '\'' => '\'',
        _ => return None,
    };
    let len = text[1..].find(close)?;
    Some((&text[1..=len], len + 2))
}

/// A `(?#…)` comment, whose `)` can be escaped; one left open runs to the end.
fn comment_len(text: &str) -> usize {
    let mut escaped = false;
    for (index, c) in text.char_indices() {
        match c {
            '\\' => escaped = !escaped,
            ')' if !escaped => return index + 1,
            _ => escaped = false,
        }
    }
    text.len()
}

/// Negated forms only apply outside a character class, where a class
/// wrapper is meaningful.
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

    // @behavior RX-168
    #[test]
    fn pattern_always_enables_multiline_for_ruby_anchors() {
        assert_eq!(translated("abc", 0), "(?m)abc");
    }

    // @behavior RX-169
    #[test]
    fn pattern_maps_options_to_inline_flags() {
        assert_eq!(translated("abc", IGNORECASE), "(?mi)abc");
        assert_eq!(translated("abc", EXTENDED), "(?mx)abc");
        // Ruby /m is DOTALL, which the regex crate spells `s`.
        assert_eq!(translated("abc", MULTILINE), "(?ms)abc");
        assert_eq!(
            translated("abc", IGNORECASE | EXTENDED | MULTILINE),
            "(?mixs)abc"
        );
    }

    // @behavior RX-170
    #[test]
    fn rewrites_shorthand_classes_outside_a_class() {
        assert_eq!(translated(r"\d+", 0), r"(?m)[0-9]+");
        assert_eq!(translated(r"\w", 0), r"(?m)[0-9A-Za-z_]");
        assert_eq!(translated(r"\D", 0), r"(?m)[^0-9]");
        assert_eq!(translated(r"\s", 0), r"(?m)[\x20\t\n\x0b\x0c\r]");
    }

    // @behavior RX-171
    #[test]
    fn rewrites_shorthand_inside_a_character_class() {
        assert_eq!(translated(r"[\d]", 0), r"(?m)[0-9]");
        assert_eq!(translated(r"[a\dz]", 0), r"(?m)[a0-9z]");
        assert_eq!(translated(r"[^\d]", 0), r"(?m)[^0-9]");
    }

    // @behavior RX-172
    #[test]
    fn keeps_negated_shorthand_inside_a_class_as_unicode() {
        // `[\D]` / `[\W]` / `[\S]` keep the engine's
        // Unicode category semantics rather than the ASCII rewrite.
        assert_eq!(translated(r"[\D]", 0), r"(?m)[\D]");
        assert_eq!(translated(r"[\W]", 0), r"(?m)[\W]");
        assert_eq!(translated(r"[\S]", 0), r"(?m)[\S]");
    }

    // @behavior RX-173
    #[test]
    fn leaves_an_escaped_backslash_as_a_literal() {
        // `\\d` is a literal backslash then `d`, not a shorthand class.
        assert_eq!(translated(r"\\d", 0), r"(?m)\\d");
    }

    // @behavior RX-174
    #[test]
    fn leaves_non_shorthand_escapes_untouched() {
        assert_eq!(translated(r"\bword\b", 0), r"(?m)\bword\b");
        assert_eq!(translated(r"a\.b", 0), r"(?m)a\.b");
    }

    // @behavior RX-273
    #[test]
    fn counts_only_what_really_opens_a_group() {
        let names = |source, options| {
            declared_names(source, options & EXTENDED != 0)
                .into_iter()
                .map(|(name, numbers)| (name.to_string(), numbers))
                .collect::<Vec<_>>()
        };
        let n_only = vec![("n".to_string(), vec![1])];

        assert_eq!(names(r"(?<n>a)(?#(?<m>b))", 0), n_only);
        assert_eq!(names(r"(?<n>a)[(?<m>b)]", 0), n_only);
        assert_eq!(names(r"(?<n>a)\(?<m>b", 0), n_only);
        assert_eq!(names("(?<n>a) # (?<m>b)\n", EXTENDED), n_only);
        assert_eq!(names(r"(?<=a)(?<!b)(?<n>a)", 0), n_only);
    }

    // @behavior RX-274
    #[test]
    fn rewrites_a_backreference_by_a_shared_name_from_its_last_group() {
        assert_eq!(
            translated(r"(?<n>a)(b)(?<m>c)(?<n>d)\k<n>\k'n'\k<m>", 0),
            r"(?m)(?<n>a)(?:b)(?<m>c)(?<n>d)(?>\3|\1)(?>\3|\1)\k<m>"
        );
    }

    fn translated(source: &str, options: i64) -> String {
        build_pattern(source, options).expect("the pattern is not refused")
    }
}
