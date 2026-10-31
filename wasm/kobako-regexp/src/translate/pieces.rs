//! Reads a pattern's source the way MRI does, so an escape, a character class
//! or a comment never reads as a group or a reference.

/// One meaningful stretch of a pattern's source.
pub(super) enum Piece<'a> {
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

pub(super) struct Pieces<'a> {
    rest: &'a str,
    class_depth: usize,
    extended: bool,
}

impl<'a> Pieces<'a> {
    pub(super) fn new(source: &'a str, extended: bool) -> Self {
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
            if let Some((name, len)) = read_delimited(option) {
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
fn read_delimited(text: &str) -> Option<(&str, usize)> {
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
