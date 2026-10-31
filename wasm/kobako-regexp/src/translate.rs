//! Pattern and flag translation from Ruby regexp syntax to the
//! `fancy-regex` (regex-crate) dialect.
//!
//! These are pure string transforms with no mruby dependency, so they are
//! unit-tested directly on the host. They carry what MRI reads differently
//! from the engine: per-line anchors, `/m` as DOTALL, ASCII shorthand classes,
//! and how named groups capture and are referred to. What the engine cannot
//! answer as MRI would is refused here rather than answered differently.

mod pieces;

use pieces::{Piece, Pieces};

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
    let names = collect_names(source, extended);
    let mut rewriter = Rewriter::new(&names, out);
    for piece in Pieces::new(source, extended) {
        rewriter.rewrite(piece)?;
    }
    Ok(rewriter.finish())
}

/// Each capture name with its group numbers. A named group makes every plain
/// group non-capturing, so only named groups count.
fn collect_names(source: &str, extended: bool) -> Vec<(&str, Vec<usize>)> {
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

/// An open group: where it starts, and the `)` its isolated options each owe
/// when it closes.
struct OpenGroup {
    start: usize,
    owed_closes: usize,
}

/// What a written piece means to the repeatable unit a later quantifier reaches
/// back for.
enum Role {
    /// Comments, a class's inside, and a quantifier leave the unit in place.
    Inert,
    Interval,
    /// An alternation or an isolated option leaves nothing to repeat.
    Boundary,
    Unit,
    Opening,
    Closing,
}

/// Writes the engine's pattern, keeping what a later piece reaches back for.
struct Rewriter<'n, 's> {
    names: &'n [(&'s str, Vec<usize>)],
    out: String,
    /// The source itself comes first, so this is never empty.
    groups: Vec<OpenGroup>,
    /// Where the last repeatable unit starts in `out`.
    unit_start: Option<usize>,
    after_interval: bool,
}

impl<'n, 's> Rewriter<'n, 's> {
    fn new(names: &'n [(&'s str, Vec<usize>)], out: String) -> Self {
        Self {
            names,
            out,
            groups: vec![OpenGroup {
                start: 0,
                owed_closes: 0,
            }],
            unit_start: None,
            after_interval: false,
        }
    }

    fn finish(mut self) -> String {
        let owed = self.groups.iter().map(|group| group.owed_closes).sum();
        self.out.push_str(&")".repeat(owed));
        self.out
    }

    fn rewrite(&mut self, piece: Piece) -> Result<(), String> {
        let start = self.out.len();
        let role = self.push_piece(piece)?;
        if !matches!(role, Role::Inert) {
            self.after_interval = matches!(role, Role::Interval);
        }
        match role {
            Role::Inert | Role::Interval => {}
            Role::Boundary => self.unit_start = None,
            Role::Unit => self.unit_start = Some(start),
            Role::Opening => {
                self.groups.push(OpenGroup {
                    start,
                    owed_closes: 0,
                });
                self.unit_start = None;
            }
            Role::Closing => {
                let group = if self.groups.len() > 1 {
                    self.groups.pop()
                } else {
                    None
                };
                let owed = group.as_ref().map_or(0, |group| group.owed_closes);
                self.out.push_str(&")".repeat(owed + 1));
                self.unit_start = group.map(|group| group.start);
            }
        }
        Ok(())
    }

    fn push_piece(&mut self, piece: Piece) -> Result<Role, String> {
        let named = !self.names.is_empty();
        Ok(match piece {
            Piece::Comment(text) => {
                self.out.push_str(text);
                if text.starts_with('#') && !text.ends_with('\n') {
                    self.out.push('\n');
                }
                Role::Inert
            }
            Piece::ClassText(text) => {
                self.out.push_str(text);
                Role::Inert
            }
            Piece::ClassClose => {
                self.out.push(']');
                Role::Inert
            }
            Piece::Escape {
                letter,
                in_class: true,
            } => {
                self.push_escape(letter, true);
                Role::Inert
            }
            Piece::Quantifier(text) => {
                if self.after_interval && text != "?" {
                    self.wrap_unit();
                }
                self.out.push_str(text);
                self.after_interval = false;
                Role::Inert
            }
            Piece::Interval { lower, upper } => {
                if is_reversed(lower, upper) {
                    return Err("upper is smaller than lower in repeat range".to_string());
                }
                if self.after_interval {
                    self.wrap_unit();
                }
                self.push_interval(lower, upper);
                Role::Interval
            }
            Piece::Text("|") => {
                self.out.push('|');
                Role::Boundary
            }
            Piece::Close => Role::Closing,
            Piece::Options {
                letters,
                scoped: false,
            } => {
                push_options(letters, &mut self.out);
                if let Some(group) = self.groups.last_mut() {
                    group.owed_closes += 1;
                }
                Role::Boundary
            }
            Piece::Options {
                letters,
                scoped: true,
            } => {
                push_options(letters, &mut self.out);
                Role::Opening
            }
            Piece::Open(text) | Piece::NamedGroup { text, .. } => {
                self.out.push_str(text);
                Role::Opening
            }
            // Beside a named group it captures nothing, so a negative look-behind
            // may hold it.
            Piece::PlainGroup {
                in_negative_look_behind,
            } => {
                if in_negative_look_behind && !named {
                    return Err("invalid pattern in look-behind".to_string());
                }
                self.out.push_str(if named { "(?:" } else { "(" });
                Role::Opening
            }
            Piece::NumberedCondition(text) => {
                if named {
                    return Err(NUMBERED_REFERENCE.to_string());
                }
                self.out.push_str(text);
                Role::Opening
            }
            // The engine has no condition by name, and MRI asks a shared name's
            // first group alone.
            Piece::NamedCondition { text, name } => {
                match self.names.iter().find(|(existing, _)| *existing == name) {
                    Some((_, numbers)) => {
                        self.out.push_str("(?(");
                        self.out.push_str(&numbers[0].to_string());
                        self.out.push(')');
                    }
                    None => self.out.push_str(text),
                }
                Role::Opening
            }
            Piece::Text(text) | Piece::ClassOpen(text) => {
                self.out.push_str(text);
                Role::Unit
            }
            Piece::LiteralBrace => {
                self.out.push_str(r"\{");
                Role::Unit
            }
            Piece::Escape {
                letter: 'b' | 'B',
                in_class: false,
            } if !cfg!(feature = "unicode") => {
                return Err("a word boundary needs the Unicode build".to_string());
            }
            Piece::Escape {
                letter,
                in_class: false,
            } => {
                self.push_escape(letter, false);
                Role::Unit
            }
            Piece::NumberedReference(text) => {
                if named {
                    return Err(NUMBERED_REFERENCE.to_string());
                }
                self.out.push_str(text);
                Role::Unit
            }
            Piece::NamedReference { text, kind, name } => {
                if named && is_group_number(name) {
                    return Err(NUMBERED_REFERENCE.to_string());
                }
                match (kind, find_shared_numbers(self.names, name)) {
                    ('g', Some(_)) => {
                        return Err(format!("multiplex definition name <{name}> call"));
                    }
                    (_, Some(numbers)) => push_each_group(numbers, &mut self.out),
                    _ => self.out.push_str(text),
                }
                Role::Unit
            }
            Piece::Refused(reason) => return Err(reason.to_string()),
        })
    }

    fn push_escape(&mut self, letter: char, in_class: bool) {
        match ascii_class(letter, in_class) {
            Some(replacement) => self.out.push_str(replacement),
            None => {
                self.out.push('\\');
                self.out.push(letter);
            }
        }
    }

    /// The engine reads `{,n}` as literal text, where MRI reads no lower bound
    /// as zero.
    fn push_interval(&mut self, lower: &str, upper: Option<&str>) {
        self.out.push('{');
        self.out
            .push_str(if lower.is_empty() { "0" } else { lower });
        if let Some(upper) = upper {
            self.out.push(',');
            self.out.push_str(upper);
        }
        self.out.push('}');
    }

    /// MRI reads a quantifier after an interval as repeating it, where the
    /// engine reads the pair as a modifier or refuses it.
    fn wrap_unit(&mut self) {
        if let Some(start) = self.unit_start {
            self.out.insert_str(start, "(?:");
            self.out.push(')');
        }
    }
}

const NUMBERED_REFERENCE: &str = "numbered backref/call is not allowed. (use name)";

fn is_reversed(lower: &str, upper: Option<&str>) -> bool {
    let bound = |text: &str| text.parse::<u64>().ok();
    matches!(
        (bound(lower), upper.and_then(bound)),
        (Some(lower), Some(upper)) if lower > upper
    )
}

/// Always as `(?on-off:`, since MRI holds an option through the alternatives
/// that follow it, where the engine ends it at the next `|`. Ruby's `m` is the
/// engine's `s`.
fn push_options(letters: &str, out: &mut String) {
    let (on, off) = letters.split_once('-').unwrap_or((letters, ""));
    let engine_letters = |letters: &str| -> String {
        letters
            .chars()
            .filter(|letter| *letter != '-')
            .map(|letter| if letter == 'm' { 's' } else { letter })
            .collect()
    };
    out.push_str("(?");
    out.push_str(&engine_letters(on));
    let off = engine_letters(off);
    if !off.is_empty() {
        out.push('-');
        out.push_str(&off);
    }
    out.push(':');
}

fn find_shared_numbers<'n>(names: &'n [(&str, Vec<usize>)], name: &str) -> Option<&'n [usize]> {
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
        assert_eq!(translate("abc", 0), "(?m)abc");
    }

    // @behavior RX-169
    #[test]
    fn pattern_maps_options_to_inline_flags() {
        assert_eq!(translate("abc", IGNORECASE), "(?mi)abc");
        assert_eq!(translate("abc", EXTENDED), "(?mx)abc");
        // Ruby /m is DOTALL, which the regex crate spells `s`.
        assert_eq!(translate("abc", MULTILINE), "(?ms)abc");
        assert_eq!(
            translate("abc", IGNORECASE | EXTENDED | MULTILINE),
            "(?mixs)abc"
        );
    }

    // @behavior RX-170
    #[test]
    fn rewrites_shorthand_classes_outside_a_class() {
        assert_eq!(translate(r"\d+", 0), r"(?m)[0-9]+");
        assert_eq!(translate(r"\w", 0), r"(?m)[0-9A-Za-z_]");
        assert_eq!(translate(r"\D", 0), r"(?m)[^0-9]");
        assert_eq!(translate(r"\s", 0), r"(?m)[\x20\t\n\x0b\x0c\r]");
    }

    // @behavior RX-171
    #[test]
    fn rewrites_shorthand_inside_a_character_class() {
        assert_eq!(translate(r"[\d]", 0), r"(?m)[0-9]");
        assert_eq!(translate(r"[a\dz]", 0), r"(?m)[a0-9z]");
        assert_eq!(translate(r"[^\d]", 0), r"(?m)[^0-9]");
    }

    // @behavior RX-172
    #[test]
    fn keeps_negated_shorthand_inside_a_class_as_unicode() {
        // `[\D]` / `[\W]` / `[\S]` keep the engine's
        // Unicode category semantics rather than the ASCII rewrite.
        assert_eq!(translate(r"[\D]", 0), r"(?m)[\D]");
        assert_eq!(translate(r"[\W]", 0), r"(?m)[\W]");
        assert_eq!(translate(r"[\S]", 0), r"(?m)[\S]");
    }

    // @behavior RX-173
    #[test]
    fn leaves_an_escaped_backslash_as_a_literal() {
        // `\\d` is a literal backslash then `d`, not a shorthand class.
        assert_eq!(translate(r"\\d", 0), r"(?m)\\d");
    }

    // @behavior RX-174
    #[test]
    fn leaves_non_shorthand_escapes_untouched() {
        assert_eq!(translate(r"\Aword\z", 0), r"(?m)\Aword\z");
        assert_eq!(translate(r"a\.b", 0), r"(?m)a\.b");
        assert_eq!(translate(r"[\b]", 0), r"(?m)[\b]");
    }

    // @behavior RX-273
    #[test]
    fn counts_only_what_really_opens_a_group() {
        let names = |source, options| {
            collect_names(source, options & EXTENDED != 0)
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
        assert_eq!(names("(?x)(?<n>a) # (?<m>b)\n", 0), n_only);
        assert_eq!(names("(?x:(?<n>a) # (?<m>b)\n)", 0), n_only);
        assert_eq!(names("(?<n>a)(?x:#\n)#(?<n>b)", 0).len(), 1);
        assert_eq!(
            names("(?x)(?<n>a)(?-x)#(?<m>b)", 0),
            vec![("n".to_string(), vec![1]), ("m".to_string(), vec![2])]
        );
    }

    // @behavior RX-274
    #[test]
    fn rewrites_a_backreference_by_a_shared_name_from_its_last_group() {
        assert_eq!(
            translate(r"(?<n>a)(b)(?<m>c)(?<n>d)\k<n>\k'n'\k<m>", 0),
            r"(?m)(?<n>a)(?:b)(?<m>c)(?<n>d)(?>\3|\1)(?>\3|\1)\k<m>"
        );
    }

    fn translate(source: &str, options: i64) -> String {
        build_pattern(source, options).expect("the pattern is not refused")
    }
}
