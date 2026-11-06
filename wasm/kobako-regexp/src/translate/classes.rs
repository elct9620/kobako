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
        "graph" => Set::all_but(r"\p{Zs}\p{Zl}\p{Zp}\p{Cc}\p{Cn}\p{Cs}"),
        "lower" => Set::of(r"\p{Lowercase}"),
        "print" => Set::all_but(r"\p{Zl}\p{Zp}\p{Cc}\p{Cn}\p{Cs}"),
        "punct" => Set::of(r"\p{P}$+<=>\^`|~"),
        "space" => Set::of(UNICODE_SPACE),
        "upper" => Set::of(r"\p{Uppercase}"),
        _ => Set::of(UNICODE_WORD),
    };
    Ok(set.class(negated))
}
