//! Reads a pattern's source the way MRI does, so an escape, a character class
//! or a comment never reads as a group or a reference.

/// One meaningful stretch of a pattern's source.
pub(super) enum Piece<'a> {
    Text(&'a str),
    /// A comment, which no later text may fall into.
    Comment(&'a str),
    /// Any group opening with no rule of its own.
    Open(&'a str),
    Close,
    ClassOpen(&'a str),
    ClassText(&'a str),
    ClassClose,
    Quantifier(&'a str),
    /// `{n}`, `{n,}`, `{,m}` or `{n,m}`; `upper` is `None` without a comma.
    Interval {
        lower: &'a str,
        upper: Option<&'a str>,
    },
    /// A `{` that opens no interval.
    LiteralBrace,
    /// `(?i-m)`, holding until its enclosing group closes, or `(?i-m:`.
    Options {
        letters: &'a str,
        scoped: bool,
    },
    Escape {
        letter: char,
        in_class: bool,
        charset: Charset,
    },
    /// A character an octal or control escape names.
    Char {
        value: u32,
        in_class: bool,
    },
    /// The hex code points of a `\u{…}` that names more than one.
    CodePoints {
        body: &'a str,
        in_class: bool,
    },
    /// `\p{name}`, `\p{^name}` or `\P{name}`.
    Property {
        text: &'a str,
        name: &'a str,
        negated: bool,
        in_class: bool,
    },
    /// `[:name:]` or `[:^name:]` inside a character class.
    PosixBracket {
        name: &'a str,
        negated: bool,
        charset: Charset,
    },
    PlainGroup {
        in_negative_look_behind: bool,
    },
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
    NamedCondition {
        text: &'a str,
        name: &'a str,
    },
    /// A spelling MRI refuses, with its reason.
    Refused(&'static str),
}

/// The character set option (`d`, `a` or `u`) that decides what `\w`, `\d`,
/// `\s`, `\b` and a POSIX bracket reach.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Charset {
    /// `\w`, `\d` and `\s` stay ASCII; `\b` and POSIX brackets read Unicode.
    Default,
    Ascii,
    Unicode,
}

/// The enclosing look-behind a piece sits in.
#[derive(Clone, Copy, PartialEq)]
enum LookBehind {
    Outside,
    Positive,
    Negative,
}

const LOOK_BEHIND_REFUSAL: &str = "invalid pattern in look-behind";
const UNDEFINED_OPTION: &str = "undefined group option";

/// What holds until the group a frame belongs to closes.
#[derive(Clone, Copy)]
struct Frame {
    extended: bool,
    ignore_case: bool,
    charset: Charset,
    look_behind: LookBehind,
}

pub(super) struct Pieces<'a> {
    rest: &'a str,
    class_depth: usize,
    frames: Vec<Frame>,
    /// How many groups the whole source captures, which decides whether
    /// `\10` is a backreference or an octal escape.
    capture_count: usize,
}

impl<'a> Pieces<'a> {
    pub(super) fn new(source: &'a str, extended: bool, capture_count: usize) -> Self {
        Self {
            rest: source,
            class_depth: 0,
            capture_count,
            frames: vec![Frame {
                extended,
                ignore_case: false,
                charset: Charset::Default,
                look_behind: LookBehind::Outside,
            }],
        }
    }

    /// Reads the source as case-insensitive from its start.
    pub(super) fn ignoring_case(mut self, ignore_case: bool) -> Self {
        self.frame().ignore_case = ignore_case;
        self
    }

    /// Whether the piece last read is matched case-insensitively.
    pub(super) fn ignores_case(&self) -> bool {
        self.frames.last().is_some_and(|frame| frame.ignore_case)
    }

    fn take(&mut self, len: usize) -> &'a str {
        let (taken, rest) = self.rest.split_at(len);
        self.rest = rest;
        taken
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames
            .last_mut()
            .expect("the outermost frame is never closed")
    }

    fn open_frame(&mut self) {
        let inherited = *self.frame();
        self.frames.push(inherited);
    }

    /// A stray `)` is the engine's to refuse, so the outermost frame stays.
    fn close_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    fn escape(&mut self) -> Piece<'a> {
        let after = &self.rest[1..];
        let Some(letter) = after.chars().next() else {
            return Piece::Text(self.take(1));
        };
        let in_class = self.class_depth > 0;
        if letter == 'x' && after[1..].starts_with('{') {
            self.take(2);
            return Piece::Refused("invalid hex escape");
        }
        if matches!(letter, 'p' | 'P') && after[1..].starts_with('{') {
            if let Some(close) = after.find('}') {
                let text = self.take(close + 2);
                let body = &text[3..text.len() - 1];
                let (caret, name) = match body.strip_prefix('^') {
                    Some(name) => (true, name),
                    None => (false, body),
                };
                return Piece::Property {
                    text,
                    name,
                    negated: caret != (letter == 'P'),
                    in_class,
                };
            }
        }
        // The brace belongs to the escape, and opens no interval.
        if letter == 'u' && after[1..].starts_with('{') {
            if let Some(close) = after.find('}') {
                let text = self.take(close + 2);
                let body = text[3..text.len() - 1].trim();
                if body.contains(char::is_whitespace) {
                    return Piece::CodePoints { body, in_class };
                }
                return if in_class {
                    Piece::ClassText(text)
                } else {
                    Piece::Text(text)
                };
            }
        }
        if matches!(letter, 'c' | 'C' | 'M') {
            return self.control(letter, in_class);
        }
        if letter.is_ascii_digit() {
            if let Some(piece) = self.octal(in_class) {
                return piece;
            }
        } else if !letter.is_ascii()
            || letter.is_ascii_alphabetic() && !escape_has_meaning(letter, in_class, after)
        {
            self.take(1);
            let text = self.take(letter.len_utf8());
            return if in_class {
                Piece::ClassText(text)
            } else {
                Piece::Text(text)
            };
        }
        if !in_class {
            let in_look_behind = self.frame().look_behind != LookBehind::Outside;
            if matches!(letter, 'k' | 'g') {
                if let Some((name, len)) = read_delimited(&after[1..]) {
                    let text = self.take(2 + len);
                    if letter == 'k' && in_look_behind {
                        return Piece::Refused(LOOK_BEHIND_REFUSAL);
                    }
                    return Piece::NamedReference {
                        text,
                        kind: letter,
                        name,
                    };
                }
            }
            if letter.is_ascii_digit() {
                let digits = after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len());
                let text = self.take(1 + digits);
                if in_look_behind {
                    return Piece::Refused(LOOK_BEHIND_REFUSAL);
                }
                return Piece::NumberedReference(text);
            }
        }
        self.take(1 + letter.len_utf8());
        Piece::Escape {
            letter,
            in_class,
            charset: self.frame().charset,
        }
    }

    /// `\cX`, `\C-X` or `\M-…`, with the backslash unread. A control escape
    /// keeps the low five bits of an ASCII character; a meta escape names a
    /// byte no UTF-8 pattern can hold.
    fn control(&mut self, letter: char, in_class: bool) -> Piece<'a> {
        let prefix = match letter {
            'c' => 2,
            'C' if self.rest[2..].starts_with('-') => 3,
            'C' => {
                self.take(2);
                return Piece::Refused("too short control escape");
            }
            _ => {
                self.take(2);
                return Piece::Refused("too short meta escape");
            }
        };
        let target = &self.rest[prefix..];
        let (byte, len) = match target.as_bytes() {
            [b'\\', escaped, ..] => match simple_escape(*escaped) {
                Some(byte) => (byte, 2),
                None => {
                    self.take(prefix);
                    return Piece::Refused("too short control escape");
                }
            },
            [byte, ..] if byte.is_ascii() => (*byte, 1),
            _ => {
                self.take(prefix);
                return Piece::Refused("too short control escape");
            }
        };
        self.take(prefix + len);
        Piece::Char {
            value: u32::from(byte & 0x1f),
            in_class,
        }
    }

    /// A digit escape that is no backreference: up to three octal digits, or
    /// a lone `8` or `9`, which reads as itself. `None` leaves a backreference.
    /// Outside a class a number is a backreference while it names a group, or
    /// while it is a single digit.
    fn octal(&mut self, in_class: bool) -> Option<Piece<'a>> {
        let after = &self.rest[1..];
        let decimal_len = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        let number = after[..decimal_len].parse::<usize>().unwrap_or(usize::MAX);
        let is_reference =
            !in_class && !after.starts_with('0') && (number <= 9 || number <= self.capture_count);
        if is_reference {
            return None;
        }
        let octal_len = after[..decimal_len.min(3)]
            .find(|c: char| !('0'..='7').contains(&c))
            .unwrap_or(decimal_len.min(3));
        if octal_len == 0 {
            self.take(1);
            let digit = self.take(1);
            return Some(if in_class {
                Piece::ClassText(digit)
            } else {
                Piece::Text(digit)
            });
        }
        let value = u32::from_str_radix(&after[..octal_len], 8).unwrap_or(u32::MAX);
        self.take(1 + octal_len);
        Some(match value {
            0..=0x7f => Piece::Char { value, in_class },
            0x80..=0xff => Piece::Refused("invalid multibyte escape"),
            _ => Piece::Refused("invalid escape code"),
        })
    }

    fn group(&mut self) -> Piece<'a> {
        let Some(option) = self.rest[1..].strip_prefix('?') else {
            self.take(1);
            self.open_frame();
            return Piece::PlainGroup {
                in_negative_look_behind: self.frame().look_behind == LookBehind::Negative,
            };
        };
        if option.starts_with('#') {
            return Piece::Comment(self.take(comment_len(self.rest)));
        }
        if option.starts_with('P') {
            self.take(1);
            return Piece::Refused(UNDEFINED_OPTION);
        }
        // The engine's absence operator answers differently from Onigmo's.
        if option.starts_with('~') {
            self.take(1);
            return Piece::Refused("the absence operator is not supported");
        }
        if option.starts_with('(') && option[1..].starts_with(|c: char| c.is_ascii_digit()) {
            // The condition's own `)` is read with it, so it closes no frame.
            let len = option.find(')').map_or(self.rest.len(), |close| close + 3);
            self.open_frame();
            return Piece::NumberedCondition(self.take(len));
        }
        if let Some((name, len)) = option.strip_prefix('(').and_then(read_delimited) {
            if option[1 + len..].starts_with(')') {
                self.open_frame();
                let text = self.take(4 + len);
                return Piece::NamedCondition { text, name };
            }
        }
        if !option.starts_with("<=") && !option.starts_with("<!") {
            if let Some((name, len)) = read_delimited(option) {
                let text = self.take(2 + len);
                let in_negative = self.frame().look_behind == LookBehind::Negative;
                self.open_frame();
                if in_negative {
                    return Piece::Refused(LOOK_BEHIND_REFUSAL);
                }
                if name.starts_with(|c: char| c.is_ascii_digit()) {
                    return Piece::Refused("invalid group name");
                }
                return Piece::NamedGroup { text, name };
            }
        }
        if let Some(letters) = read_option_letters(option) {
            return self.apply_options(letters);
        }
        let outer = self.frame().look_behind;
        self.open_frame();
        let len = ["<=", "<!", "=", "!", ">"]
            .iter()
            .find(|opener| option.starts_with(**opener))
            .map_or(2, |opener| 2 + opener.len());
        let opener = &option[..len - 2];
        match opener {
            "<=" if outer == LookBehind::Outside => self.frame().look_behind = LookBehind::Positive,
            "<!" => self.frame().look_behind = LookBehind::Negative,
            "=" | "!" | ">" if outer != LookBehind::Outside => {
                self.take(len);
                return Piece::Refused(LOOK_BEHIND_REFUSAL);
            }
            _ => {}
        }
        Piece::Open(self.take(len))
    }

    fn brace(&mut self) -> Piece<'a> {
        let Some((lower, upper, len)) = read_interval(self.rest) else {
            self.take(1);
            return Piece::LiteralBrace;
        };
        self.take(len);
        Piece::Interval { lower, upper }
    }

    fn class_piece(&mut self, first: char) -> Piece<'a> {
        if let Some((body, len)) = read_posix_bracket(self.rest) {
            self.take(len);
            let (negated, name) = match body.strip_prefix('^') {
                Some(name) => (true, name),
                None => (false, body),
            };
            if !POSIX_BRACKETS.contains(&name) {
                return Piece::Refused("invalid POSIX bracket type");
            }
            return Piece::PosixBracket {
                name,
                negated,
                charset: self.frame().charset,
            };
        }
        match first {
            '[' => self.class_depth += 1,
            ']' => self.class_depth -= 1,
            _ => {}
        }
        if self.class_depth == 0 {
            self.take(1);
            return Piece::ClassClose;
        }
        Piece::ClassText(self.take(first.len_utf8()))
    }

    /// `(?x:` holds until its own group closes; `(?x)` holds until the
    /// enclosing group does.
    fn apply_options(&mut self, letters: &'a str) -> Piece<'a> {
        let scoped = self.rest.as_bytes()[2 + letters.len()] == b':';
        if scoped {
            self.open_frame();
        }
        if !are_known_options(letters) {
            self.take(3 + letters.len());
            return Piece::Refused(UNDEFINED_OPTION);
        }
        let mut setting = true;
        for letter in letters.chars() {
            match letter {
                '-' => setting = false,
                'x' => self.frame().extended = setting,
                'i' => self.frame().ignore_case = setting,
                'd' => self.frame().charset = Charset::Default,
                'a' => self.frame().charset = Charset::Ascii,
                'u' => self.frame().charset = Charset::Unicode,
                _ => {}
            }
        }
        self.take(3 + letters.len());
        Piece::Options { letters, scoped }
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        let first = self.rest.chars().next()?;
        if first == '\\' {
            return Some(self.escape());
        }
        if self.class_depth > 0 {
            return Some(self.class_piece(first));
        }
        let extended = self.frame().extended;
        Some(match first {
            '[' => {
                self.class_depth = 1;
                Piece::ClassOpen(self.take(1))
            }
            '(' => self.group(),
            ')' => {
                self.close_frame();
                self.take(1);
                Piece::Close
            }
            '{' => self.brace(),
            '?' | '*' | '+' => Piece::Quantifier(self.take(1)),
            '#' if extended => {
                let len = self
                    .rest
                    .find('\n')
                    .map_or(self.rest.len(), |newline| newline + 1);
                Piece::Comment(self.take(len))
            }
            c if extended && c.is_ascii_whitespace() => Piece::Comment(self.take(1)),
            c => Piece::Text(self.take(c.len_utf8())),
        })
    }
}

/// Whether MRI gives `\letter` a meaning of its own; any other letter reads
/// as itself. `\k` and `\g` mean something only before a delimited name, and
/// `\p` and `\P` only before a brace.
fn escape_has_meaning(letter: char, in_class: bool, after: &str) -> bool {
    match letter {
        'k' | 'g' => !in_class && read_delimited(&after[1..]).is_some(),
        'p' | 'P' => after[1..].starts_with('{'),
        _ if in_class => "abdefhnrstuvwxDHSW".contains(letter),
        _ => "abdefhnrstuvwxzABDGHKRSWXZ".contains(letter),
    }
}

/// The byte a single-letter escape after `\c` stands for.
fn simple_escape(letter: u8) -> Option<u8> {
    Some(match letter {
        b'\\' => b'\\',
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'f' => 0x0c,
        b'v' => 0x0b,
        b'a' => 0x07,
        b'e' => 0x1b,
        _ => return None,
    })
}

const POSIX_BRACKETS: [&str; 14] = [
    "alnum", "alpha", "ascii", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
    "space", "upper", "word", "xdigit",
];

/// The body of a `[:…:]` that opens `text`, with the length it spans. A body
/// of anything but letters, after an optional `^`, is no bracket.
fn read_posix_bracket(text: &str) -> Option<(&str, usize)> {
    let rest = text.strip_prefix("[:")?;
    let close = rest.find(":]")?;
    let body = &rest[..close];
    let name = body.strip_prefix('^').unwrap_or(body);
    name.bytes()
        .all(|byte| byte.is_ascii_alphabetic())
        .then_some((body, close + 4))
}

/// The name in `<name>` or `'name'`, with the length the delimiters span.
fn read_delimited(text: &str) -> Option<(&str, usize)> {
    let close = match text.chars().next()? {
        '<' => '>',
        '\'' => '\'',
        _ => return None,
    };
    let len = text[1..].find(close)?;
    Some((&text[1..=len], len + 2))
}

/// MRI's `(?imxdau-imx)`: three of its letters can only be turned on.
fn are_known_options(letters: &str) -> bool {
    let (on, off) = letters.split_once('-').unwrap_or((letters, ""));
    on.chars().all(|letter| "imxdau".contains(letter))
        && off.chars().all(|letter| "imx-".contains(letter))
}

/// An interval's bounds and the length it spans, for text opening with `{`.
/// Without a bound on either side, it is no interval.
fn read_interval(text: &str) -> Option<(&str, Option<&str>, usize)> {
    let close = text.find('}')?;
    let body = &text[1..close];
    let (lower, upper) = match body.split_once(',') {
        Some((lower, upper)) => (lower, Some(upper)),
        None => (body, None),
    };
    let digits = |bound: &str| bound.bytes().all(|byte| byte.is_ascii_digit());
    let bounded = !lower.is_empty() || upper.is_some_and(|upper| !upper.is_empty());
    (bounded && digits(lower) && upper.is_none_or(digits)).then_some((lower, upper, close + 1))
}

/// The letters of an option group: `(?i-m)` or `(?i-m:`, with the `(?` read.
fn read_option_letters(option: &str) -> Option<&str> {
    let len = option.find(|c: char| !c.is_ascii_alphanumeric() && c != '-')?;
    let close = option.as_bytes()[len];
    (close == b':' || (close == b')' && len > 0)).then(|| &option[..len])
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
