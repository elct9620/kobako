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
    NamedCondition {
        text: &'a str,
        name: &'a str,
    },
    PythonGroup,
}

/// What holds until the group a frame belongs to closes.
#[derive(Clone, Copy)]
struct Frame {
    extended: bool,
}

pub(super) struct Pieces<'a> {
    rest: &'a str,
    class_depth: usize,
    frames: Vec<Frame>,
}

impl<'a> Pieces<'a> {
    pub(super) fn new(source: &'a str, extended: bool) -> Self {
        Self {
            rest: source,
            class_depth: 0,
            frames: vec![Frame { extended }],
        }
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
        if !in_class {
            if matches!(letter, 'k' | 'g') {
                if let Some((name, len)) = read_delimited(&after[1..]) {
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
            self.open_frame();
            return Piece::PlainGroup;
        };
        if option.starts_with('#') {
            return Piece::Comment(self.take(comment_len(self.rest)));
        }
        if option.starts_with('P') {
            self.take(1);
            return Piece::PythonGroup;
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
                self.open_frame();
                return Piece::NamedGroup { text, name };
            }
        }
        if let Some(letters) = read_option_letters(option) {
            return self.apply_options(letters);
        }
        self.open_frame();
        let len = ["<=", "<!", "=", "!", ">", "~"]
            .iter()
            .find(|opener| option.starts_with(**opener))
            .map_or(2, |opener| 2 + opener.len());
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
        let mut setting = true;
        for letter in letters.chars() {
            match letter {
                '-' => setting = false,
                'x' => self.frame().extended = setting,
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
