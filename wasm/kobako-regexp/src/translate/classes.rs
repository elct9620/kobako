//! What MRI's shorthand classes, word boundaries and POSIX brackets reach
//! under each character set option, written as the engine's syntax.
//!
//! The Unicode sets are Onigmo's own, measured against MRI over every code
//! point, so they differ from the engine's `\w` and POSIX classes.

use super::pieces::Charset;

const ASCII_ONLY: &str = "the Unicode build is needed for this class";

const ASCII_WORD: &str = "0-9A-Za-z_";
const ASCII_DIGIT: &str = "0-9";
const ASCII_SPACE: &str = r"\x20\t\n\x0b\x0c\r";

const UNICODE_WORD: &str = r"\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}";
/// Outside a class, MRI's Unicode `\w` also takes these Latin-1 numbers.
const UNICODE_WORD_LATIN1: &str =
    r"\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}\x{B2}\x{B3}\x{B9}\x{BC}-\x{BE}";
const UNICODE_DIGIT: &str = r"\p{Nd}";
const ASCII_HEX_DIGIT: &str = "0-9A-Fa-f";
const GRAPH_EXCLUDED: &str = r"\p{Zs}\p{Zl}\p{Zp}\p{Cc}\p{Cn}\p{Cs}";
const PRINT_EXCLUDED: &str = r"\p{Zl}\p{Zp}\p{Cc}\p{Cn}\p{Cs}";
const PUNCT: &str = r"\p{P}$+<=>\^`|~";
const UNICODE_SPACE: &str = r"\t\n\x0b\x0c\r\x{85}\p{Zs}\p{Zl}\p{Zp}";

/// One set as class items, and whether the items are the set's complement.
struct Set {
    items: &'static str,
    complement: bool,
}

impl Set {
    const fn of(items: &'static str) -> Self {
        Self {
            items,
            complement: false,
        }
    }

    const fn all_but(items: &'static str) -> Self {
        Self {
            items,
            complement: true,
        }
    }

    /// A class of its own, which a class may also nest.
    fn class(&self, negated: bool) -> String {
        let caret = if self.complement != negated { "^" } else { "" };
        format!("[{caret}{}]", self.items)
    }
}

/// The engine's text for `\w`, `\d`, `\s` and their negations, or `None` for
/// any other escape. A positive class inside a class is written as bare items.
pub(super) fn shorthand(
    letter: char,
    in_class: bool,
    charset: Charset,
) -> Result<Option<String>, &'static str> {
    let unicode = charset == Charset::Unicode;
    let items = match (letter.to_ascii_lowercase(), unicode) {
        ('w', false) => ASCII_WORD,
        ('d', false) => ASCII_DIGIT,
        ('s', false) => ASCII_SPACE,
        _ if unicode && !cfg!(feature = "unicode") && "wdsWDS".contains(letter) => {
            return Err(ASCII_ONLY)
        }
        ('w', true) if in_class => UNICODE_WORD,
        ('w', true) => UNICODE_WORD_LATIN1,
        ('d', true) => UNICODE_DIGIT,
        ('s', true) => UNICODE_SPACE,
        _ => return Ok(None),
    };
    let negated = letter.is_ascii_uppercase();
    Ok(Some(if in_class && !negated {
        items.to_string()
    } else {
        Set::of(items).class(negated)
    }))
}

/// `\b` or `\B` outside a class. Under `a` the boundary is ASCII's, which the
/// engine cannot switch to inline, so it is spelled with look-arounds.
pub(super) fn word_boundary(letter: char, charset: Charset) -> Result<String, &'static str> {
    if charset == Charset::Ascii {
        let word = format!("[{ASCII_WORD}]");
        return Ok(if letter == 'b' {
            format!("(?:(?<={word})(?!{word})|(?<!{word})(?={word}))")
        } else {
            format!("(?:(?<={word})(?={word})|(?<!{word})(?!{word}))")
        });
    }
    if !cfg!(feature = "unicode") {
        return Err("a word boundary needs the Unicode build");
    }
    Ok(format!(r"\{letter}"))
}

/// A property whose Onigmo set the engine names differently or not at all,
/// as a class; `None` leaves the property to the engine. MRI reads a name
/// without regard to case or underscores.
pub(super) fn property(name: &str, negated: bool) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '_' | ' ' | '-'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let set = match name.as_str() {
        "ascii" => Set::of(r"\x00-\x7F"),
        "xdigit" => Set::of(ASCII_HEX_DIGIT),
        _ if !cfg!(feature = "unicode") => return None,
        "graph" => Set::all_but(GRAPH_EXCLUDED),
        "print" => Set::all_but(PRINT_EXCLUDED),
        "xposixpunct" => Set::of(PUNCT),
        "unknown" | "zzzz" => Set::of(r"\p{Cn}\p{Co}"),
        _ => return None,
    };
    Some(set.class(negated))
}

/// A POSIX bracket as a class nested in the one holding it.
pub(super) fn posix_bracket(
    name: &str,
    negated: bool,
    charset: Charset,
) -> Result<String, &'static str> {
    if charset == Charset::Ascii || matches!(name, "ascii" | "xdigit") {
        let caret = if negated { "^" } else { "" };
        return Ok(format!("[[:{caret}{name}:]]"));
    }
    if !cfg!(feature = "unicode") {
        return Err(ASCII_ONLY);
    }
    let set = match name {
        "alnum" => Set::of(r"\p{Alphabetic}\p{Nd}"),
        "alpha" => Set::of(r"\p{Alphabetic}"),
        "blank" => Set::of(r"\p{Zs}\t"),
        "cntrl" => Set::of(r"\p{Cc}"),
        "digit" => Set::of(UNICODE_DIGIT),
        "graph" => Set::all_but(GRAPH_EXCLUDED),
        "lower" => Set::of(r"\p{Lowercase}"),
        "print" => Set::all_but(PRINT_EXCLUDED),
        "punct" => Set::of(PUNCT),
        "space" => Set::of(UNICODE_SPACE),
        "upper" => Set::of(r"\p{Uppercase}"),
        _ => Set::of(UNICODE_WORD),
    };
    Ok(set.class(negated))
}
