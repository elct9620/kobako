//! Pattern and flag translation from Ruby regexp syntax to the
//! `fancy-regex` (regex-crate) dialect.
//!
//! These are pure string transforms with no mruby dependency, so they are
//! unit-tested directly on the host. They carry what MRI reads differently
//! from the engine: per-line anchors, `/m` as DOTALL, the classes each
//! character set option reaches, escapes, stacked quantifiers, folds to
//! several characters, and how named groups capture and are referred to.
//! What the engine cannot answer as MRI would is refused here rather than
//! answered differently.

mod classes;
mod fold;
mod pieces;
mod repeat;

use pieces::{Charset, Piece, Pieces};
use repeat::Quantity;

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
    let captures = if names.is_empty() {
        Pieces::new(source, extended, 0)
            .filter(|piece| matches!(piece, Piece::PlainGroup { .. }))
            .count()
    } else {
        names.iter().map(|(_, numbers)| numbers.len()).sum()
    };
    let has_calls = Pieces::new(source, extended, captures)
        .any(|piece| matches!(piece, Piece::NamedReference { kind: 'g', .. }));
    let mut rewriter = Rewriter::new(&names, has_calls, out);
    let mut pieces =
        Pieces::new(source, extended, captures).ignoring_case(options & IGNORECASE != 0);
    while let Some(piece) = pieces.next() {
        let folding = cfg!(feature = "unicode") && pieces.ignores_case();
        rewriter.rewrite(piece, folding)?;
    }
    rewriter.finish()
}

/// Each capture name with its group numbers. A named group makes every plain
/// group non-capturing, so only named groups count.
fn collect_names(source: &str, extended: bool) -> Vec<(&str, Vec<usize>)> {
    let mut names: Vec<(&str, Vec<usize>)> = Vec::new();
    let mut number = 0;
    for piece in Pieces::new(source, extended, 0) {
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
    look_around: bool,
}

/// What a written piece means to the repeatable unit a later quantifier reaches
/// back for.
enum Role {
    /// Comments and a class's inside leave the unit in place.
    Inert,
    /// A quantifier keeps the unit, and the state it leaves for the next one.
    Repeat,
    /// An alternation or an isolated option leaves nothing to repeat.
    Boundary,
    Unit,
    Opening {
        look_around: bool,
    },
    Closing,
}

impl Role {
    /// Text is a unit of its own outside a class, and part of the class
    /// inside one.
    fn of_text(in_class: bool) -> Self {
        if in_class {
            Self::Inert
        } else {
            Self::Unit
        }
    }
}

/// How far the quantifiers after the last unit have gone. MRI reads a
/// quantifier that cannot modify them as repeating the quantified unit whole,
/// where the engine refuses it.
#[derive(Clone, Copy, PartialEq)]
enum Repeat {
    None,
    /// `?`, `*` or `+`, which a following `?` or `+` modifies.
    Greedy,
    /// An interval, which a following `?` makes lazy unless it is exact.
    Interval {
        exact: bool,
    },
    Settled,
}

/// Writes the engine's pattern, keeping what a later piece reaches back for.
struct Rewriter<'n, 's> {
    names: &'n [(&'s str, Vec<usize>)],
    /// Whether the source calls a subexpression, so it may recurse.
    has_calls: bool,
    out: String,
    /// The source itself comes first, so this is never empty.
    groups: Vec<OpenGroup>,
    /// Where the last repeatable unit starts in `out`.
    unit_start: Option<usize>,
    /// The engine refuses to repeat a look-around itself.
    unit_is_look_around: bool,
    /// The look-around a skippable quantifier turned into an alternative,
    /// which a following `?` or `+` rewrites.
    optional_look_around: Option<String>,
    /// The last unit as written, and the quantifiers stacked on it so far.
    quantified_unit: String,
    quantities: Vec<Quantity>,
    repeat: Repeat,
    /// The case-insensitive literal characters read in a row, where they
    /// start in `out`, and where the last of them starts.
    literal_run: String,
    literal_run_start: usize,
    literal_run_last: usize,
    /// A case-insensitive class being read: where it starts in `out`, and
    /// the characters written inside it.
    folded_class: Option<(usize, Vec<char>)>,
}

impl<'n, 's> Rewriter<'n, 's> {
    fn new(names: &'n [(&'s str, Vec<usize>)], has_calls: bool, out: String) -> Self {
        Self {
            names,
            has_calls,
            out,
            groups: vec![OpenGroup {
                start: 0,
                owed_closes: 0,
                look_around: false,
            }],
            unit_start: None,
            unit_is_look_around: false,
            optional_look_around: None,
            quantified_unit: String::new(),
            quantities: Vec::new(),
            repeat: Repeat::None,
            literal_run: String::new(),
            literal_run_start: 0,
            literal_run_last: 0,
            folded_class: None,
        }
    }

    fn finish(mut self) -> Result<String, String> {
        self.close_literal_run(false)?;
        let owed = self.groups.iter().map(|group| group.owed_closes).sum();
        self.out.push_str(&")".repeat(owed));
        Ok(self.out)
    }

    /// `folding` says whether the piece reads case-insensitively, where a
    /// run of literal characters or a class may reach a multi-character fold.
    fn rewrite(&mut self, piece: Piece, folding: bool) -> Result<(), String> {
        match piece {
            Piece::Text(text) if folding && is_literal_char(text) => {
                if self.literal_run.is_empty() {
                    self.literal_run_start = self.out.len();
                }
                self.literal_run_last = self.out.len();
                self.literal_run.push_str(text);
            }
            Piece::Quantifier(_) | Piece::Interval { .. } => self.close_literal_run(true)?,
            _ => self.close_literal_run(false)?,
        }
        match piece {
            Piece::ClassOpen(_) if folding => {
                self.folded_class = Some((self.out.len(), Vec::new()))
            }
            Piece::ClassText(text) => {
                if let Some((_, named)) = self.folded_class.as_mut() {
                    named.extend(text.chars());
                }
            }
            _ => {}
        }
        let closes_class = matches!(piece, Piece::ClassClose);
        let start = self.out.len();
        let role = self.push_piece(piece)?;
        if closes_class {
            self.close_folded_class();
        }
        if !matches!(role, Role::Inert | Role::Repeat) {
            self.repeat = Repeat::None;
            self.unit_is_look_around = false;
            self.optional_look_around = None;
            self.quantities.clear();
        }
        match role {
            Role::Inert | Role::Repeat => {}
            Role::Boundary => self.unit_start = None,
            Role::Unit => self.unit_start = Some(start),
            Role::Opening { look_around } => {
                self.groups.push(OpenGroup {
                    start,
                    owed_closes: 0,
                    look_around,
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
                self.unit_is_look_around = group.as_ref().is_some_and(|group| group.look_around);
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
                charset,
            } => {
                self.push_escape(letter, true, charset)?;
                Role::Inert
            }
            Piece::Char { value, in_class } => {
                self.push_code_point(value);
                Role::of_text(in_class)
            }
            Piece::CodePoints { body, in_class } => {
                for hex in body.split_whitespace() {
                    let value = u32::from_str_radix(hex, 16)
                        .map_err(|_| "invalid Unicode escape".to_string())?;
                    self.push_code_point(value);
                }
                Role::of_text(in_class)
            }
            Piece::Property {
                text,
                name,
                negated,
                in_class,
            } => {
                match classes::property(name, negated) {
                    Some(class) => self.out.push_str(&class),
                    None => self.out.push_str(text),
                }
                Role::of_text(in_class)
            }
            Piece::PosixBracket {
                name,
                negated,
                charset,
            } => {
                self.out
                    .push_str(&classes::posix_bracket(name, negated, charset)?);
                Role::Inert
            }
            Piece::Quantifier(text) => {
                let modifies = matches!(
                    (self.repeat, text),
                    (Repeat::Greedy, "?" | "+") | (Repeat::Interval { exact: false }, "?")
                );
                if modifies {
                    self.modify_repeat(text);
                } else {
                    self.repeat_unit(match text {
                        "?" => Quantity::greedy(0, Some(1)),
                        "*" => Quantity::greedy(0, None),
                        _ => Quantity::greedy(1, None),
                    });
                    self.repeat = Repeat::Greedy;
                }
                Role::Repeat
            }
            Piece::Interval { lower, upper } => {
                let quantity = read_interval(lower, upper)?;
                self.repeat_unit(quantity);
                self.repeat = Repeat::Interval {
                    exact: upper.is_none(),
                };
                Role::Repeat
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
                Role::Opening { look_around: false }
            }
            Piece::Open(text) => {
                self.out.push_str(text);
                Role::Opening {
                    look_around: ["(?=", "(?!", "(?<=", "(?<!"]
                        .iter()
                        .any(|opener| text.starts_with(opener)),
                }
            }
            Piece::NamedGroup { text, .. } => {
                self.out.push_str(text);
                Role::Opening { look_around: false }
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
                Role::Opening { look_around: false }
            }
            Piece::NumberedCondition(text) => {
                if named {
                    return Err(NUMBERED_REFERENCE.to_string());
                }
                self.out.push_str(text);
                Role::Opening { look_around: false }
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
                Role::Opening { look_around: false }
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
                letter: letter @ ('b' | 'B'),
                in_class: false,
                charset,
            } => {
                self.out.push_str(&classes::word_boundary(letter, charset)?);
                Role::Unit
            }
            Piece::Escape {
                letter,
                in_class: false,
                charset,
            } => {
                self.push_escape(letter, false, charset)?;
                Role::Unit
            }
            Piece::NumberedReference(text) => {
                if named {
                    return Err(NUMBERED_REFERENCE.to_string());
                }
                self.out.push_str(text);
                Role::Unit
            }
            Piece::NamedReference {
                kind: 'k',
                name: leveled,
                ..
            } if split_level(leveled).is_some() => {
                let (name, level) = split_level(leveled).expect("the guard split it");
                self.push_leveled_reference(name, level)?;
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

    /// `\k<name+level>` names the group at a recursion level relative to the
    /// reference. With no subexpression call there is one level, so level zero
    /// is the group itself and any other level matches nothing. The engine
    /// has no levels, so a pattern that recurses, or a name several groups
    /// share, is refused.
    fn push_leveled_reference(&mut self, name: &str, level: i64) -> Result<(), String> {
        if self.has_calls || find_shared_numbers(self.names, name).is_some() {
            return Err("a backreference with a level is not supported here".to_string());
        }
        if !self.names.is_empty() && is_group_number(name) {
            return Err(NUMBERED_REFERENCE.to_string());
        }
        if level == 0 {
            self.out.push_str(&format!(r"\k<{name}>"));
        } else {
            self.out.push_str("(?!)");
        }
        Ok(())
    }

    fn push_escape(
        &mut self,
        letter: char,
        in_class: bool,
        charset: Charset,
    ) -> Result<(), String> {
        match classes::shorthand(letter, in_class, charset)? {
            Some(replacement) => self.out.push_str(&replacement),
            None => {
                self.out.push('\\');
                self.out.push(letter);
            }
        }
        Ok(())
    }

    fn push_code_point(&mut self, value: u32) {
        self.out.push_str(&format!(r"\x{{{value:X}}}"));
    }

    /// Writes a quantifier that repeats the unit before it, folded with the
    /// quantifiers already on it. The engine repeats no look-around, and
    /// repeating one changes nothing but whether it is tried, so one tried at
    /// least once stands alone and one that may be skipped becomes an
    /// alternative with nothing.
    fn repeat_unit(&mut self, quantity: Quantity) {
        self.optional_look_around = None;
        let Some(start) = self.unit_start else {
            self.out.push_str(&repeat::write("", &[quantity]));
            return;
        };
        if self.unit_is_look_around {
            if quantity.lower == 0 {
                let look_around = self.out.split_off(start);
                self.out.push_str(&format!("(?:{look_around}|)"));
                self.optional_look_around = Some(look_around);
                self.unit_is_look_around = false;
            }
            return;
        }
        if self.quantities.is_empty() {
            self.quantified_unit = self.out[start..].to_string();
        }
        self.quantities.push(quantity);
        self.write_quantified_unit(start);
    }

    /// A `?` or `+` that makes the quantifier before it lazy or possessive.
    fn modify_repeat(&mut self, modifier: &str) {
        self.repeat = Repeat::Settled;
        if self.unit_is_look_around {
            return;
        }
        let Some(start) = self.unit_start else {
            self.out.push_str(modifier);
            return;
        };
        if let Some(look_around) = self.optional_look_around.take() {
            self.out.truncate(start);
            let form = if modifier == "?" {
                "(?:|{})"
            } else {
                "(?>{}|)"
            };
            self.out.push_str(&form.replace("{}", &look_around));
            return;
        }
        if let Some(last) = self.quantities.last_mut() {
            if modifier == "?" {
                last.greedy = false;
            } else {
                last.possessive = true;
            }
            self.write_quantified_unit(start);
        }
    }

    /// Rewrites the run of literal characters just written so it also
    /// matches every spelling its folds join to. MRI splits the last
    /// character off a run a quantifier follows, so that one is spelled
    /// alone and stays the unit the quantifier repeats.
    fn close_literal_run(&mut self, split_last: bool) -> Result<(), String> {
        if self.literal_run.is_empty() {
            return Ok(());
        }
        let run = std::mem::take(&mut self.literal_run);
        let last_len = run.chars().next_back().map_or(0, char::len_utf8);
        let (head, last) = if split_last {
            run.split_at(run.len() - last_len)
        } else {
            (run.as_str(), "")
        };
        let head_spelled = fold::expand_run(head)?;
        let last_spelled = fold::expand_run(last)?;
        if head_spelled.is_none() && last_spelled.is_none() {
            return Ok(());
        }
        self.out.truncate(self.literal_run_start);
        self.out.push_str(head_spelled.as_deref().unwrap_or(head));
        let last_start = self.out.len();
        self.out.push_str(last_spelled.as_deref().unwrap_or(last));
        self.unit_start = Some(if split_last {
            last_start
        } else {
            self.literal_run_start
        });
        Ok(())
    }

    /// A class that writes a character folding to several, a range's ends
    /// included, adds that fold as an alternative. A negated class adds none,
    /// and neither does a character a range only spans.
    fn close_folded_class(&mut self) {
        let Some((start, written)) = self.folded_class.take() else {
            return;
        };
        if written.first() == Some(&'^') {
            return;
        }
        let folds = fold::class_folds(&written);
        if folds.is_empty() {
            return;
        }
        self.out.insert_str(start, "(?:");
        for folded in folds {
            self.out.push('|');
            self.out.push_str(&folded);
        }
        self.out.push(')');
    }

    fn write_quantified_unit(&mut self, start: usize) {
        self.out.truncate(start);
        self.out
            .push_str(&repeat::write(&self.quantified_unit, &self.quantities));
    }
}

/// A single character MRI reads as itself, and so as part of a string.
fn is_literal_char(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !matches!(c, '|' | '.' | '^' | '$'))
}

/// MRI's ceiling on an interval bound.
const MAX_REPEAT: u64 = 100_000;

/// An interval's bounds as MRI reads them: no lower bound is zero, `{n}` is
/// exact and `{n,}` has no upper bound.
fn read_interval(lower: &str, upper: Option<&str>) -> Result<Quantity, String> {
    let bound = |text: &str| -> Result<u64, String> {
        match text.parse::<u64>() {
            Ok(number) if number <= MAX_REPEAT => Ok(number),
            _ => Err("too big number for repeat range".to_string()),
        }
    };
    let lower_bound = if lower.is_empty() { 0 } else { bound(lower)? };
    let upper_bound = match upper {
        None => Some(lower_bound),
        Some("") => None,
        Some(upper) => Some(bound(upper)?),
    };
    if upper_bound.is_some_and(|upper| upper < lower_bound) {
        return Err("upper is smaller than lower in repeat range".to_string());
    }
    Ok(Quantity::greedy(lower_bound, upper_bound))
}

const NUMBERED_REFERENCE: &str = "numbered backref/call is not allowed. (use name)";

/// Always as `(?on-off:`, since MRI holds an option through the alternatives
/// that follow it, where the engine ends it at the next `|`. Ruby's `m` is the
/// engine's `s`; the character set letters are read while rewriting classes.
fn push_options(letters: &str, out: &mut String) {
    let (on, off) = letters.split_once('-').unwrap_or((letters, ""));
    let engine_letters = |letters: &str| -> String {
        letters
            .chars()
            .filter(|letter| !"-dau".contains(*letter))
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

/// `name+level` or `name-level` split apart, for a name that is not empty.
fn split_level(reference: &str) -> Option<(&str, i64)> {
    let sign = reference[1..].rfind(['+', '-'])? + 1;
    let (name, level) = reference.split_at(sign);
    let digits = &level[1..];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.parse::<i64>().ok()?;
    Some((
        name,
        if level.starts_with('-') {
            -magnitude
        } else {
            magnitude
        },
    ))
}

fn is_group_number(name: &str) -> bool {
    let digits = name.strip_prefix('-').unwrap_or(name);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
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
    fn nests_a_negated_shorthand_inside_a_class() {
        assert_eq!(translate(r"[\D]", 0), r"(?m)[[^0-9]]");
        assert_eq!(translate(r"[a\W]", 0), r"(?m)[a[^0-9A-Za-z_]]");
        assert_eq!(translate(r"[\S]", 0), r"(?m)[[^\x20\t\n\x0b\x0c\r]]");
    }

    #[test]
    fn reads_a_character_set_option_until_its_group_closes() {
        assert_eq!(
            translate(r"(?a:\w)\w", 0),
            r"(?m)(?:[0-9A-Za-z_])[0-9A-Za-z_]"
        );
        assert_eq!(translate(r"(?ia)a", 0), r"(?m)(?i:a)");
        assert_eq!(translate(r"[[:^ascii:]x]", 0), r"(?m)[[[:^ascii:]]x]");
        assert_eq!(translate(r"(?a)[[:alpha:]]", 0), r"(?m)(?:[[[:alpha:]]])");
        assert_eq!(translate(r"[:alpha:]", 0), r"(?m)[:alpha:]");
        assert!(build_pattern("[[:Alpha:]]", 0).is_err());
    }

    #[test]
    fn spells_an_ascii_word_boundary_with_look_arounds() {
        let word = "[0-9A-Za-z_]";
        assert_eq!(
            translate(r"(?a)\b", 0),
            format!("(?m)(?:(?:(?<={word})(?!{word})|(?<!{word})(?={word})))")
        );
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn reads_unicode_classes_under_the_unicode_option() {
        assert_eq!(translate(r"(?u)\d", 0), r"(?m)(?:[\p{Nd}])");
        assert_eq!(translate(r"(?u)[\d]", 0), r"(?m)(?:[\p{Nd}])");
        assert_eq!(translate(r"[[:^alpha:]]", 0), r"(?m)[[^\p{Alphabetic}]]");
        assert_eq!(
            translate(r"[[:graph:]]", 0),
            r"(?m)[[^\p{Zs}\p{Zl}\p{Zp}\p{Cc}\p{Cn}\p{Cs}]]"
        );
    }

    #[test]
    fn writes_an_ascii_property_as_a_class() {
        assert_eq!(translate(r"\p{XDigit}", 0), r"(?m)[0-9A-Fa-f]");
        assert_eq!(translate(r"[x\P{x_digit}]", 0), r"(?m)[x[^0-9A-Fa-f]]");
        assert_eq!(translate(r"\p{^ASCII}", 0), r"(?m)[^\x00-\x7F]");
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn writes_a_property_the_engine_reads_differently_as_onigmo_does() {
        assert_eq!(translate(r"\p{Unknown}", 0), r"(?m)[\p{Cn}\p{Co}]");
        assert_eq!(translate(r"\P{Alpha}", 0), r"(?m)\P{Alpha}");
    }

    #[cfg(not(feature = "unicode"))]
    #[test]
    fn refuses_a_class_that_reads_unicode_without_the_unicode_build() {
        assert!(build_pattern(r"(?u)\w", 0).is_err());
        assert!(build_pattern(r"[[:alpha:]]", 0).is_err());
        assert!(build_pattern(r"(?u)a", 0).is_ok());
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

    #[test]
    fn reads_a_backreference_level_without_recursion() {
        assert_eq!(translate(r"(?<n>a)\k<n+0>", 0), r"(?m)(?<n>a)\k<n>");
        assert_eq!(translate(r"(a)\k<-1+0>\k<1-1>", 0), r"(?m)(a)\k<-1>(?!)");
        assert!(build_pattern(r"(?<n>a)\k<n+0>\g<n>", 0).is_err());
        assert!(build_pattern(r"(?<n>a)(?<n>b)\k<n+0>", 0).is_err());
        assert_eq!(split_level("-1"), None);
        assert_eq!(split_level("n+"), None);
    }

    fn translate(source: &str, options: i64) -> String {
        build_pattern(source, options).expect("the pattern is not refused")
    }
}
