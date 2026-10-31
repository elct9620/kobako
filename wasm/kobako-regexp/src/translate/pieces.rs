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
        Piece::Open(self.take(1))
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
        if first == ')' && self.class_depth == 0 {
            self.close_frame();
            self.take(1);
            return Some(Piece::Close);
        }
        if first == '#' && self.class_depth == 0 && self.frame().extended {
            let len = self
                .rest
                .find('\n')
                .map_or(self.rest.len(), |newline| newline + 1);
            return Some(Piece::Comment(self.take(len)));
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
            _ => first.len_utf8(),
        };
        Some(Piece::Text(self.take(text_len)))
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
