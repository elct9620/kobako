//! The paths a `Dir.glob` pattern names, matched as CRuby's `dir.c` matches
//! them on Linux: braces expand first, each segment between separators is
//! literal, a wildcard, or `**/`, and the walk visits a directory's entries in
//! order, descending into each before the next.

use std::ffi::{CStr, OsStr};
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "wasi")]
use std::os::wasi::ffi::OsStrExt;

/// The `File::Constants` matching flags, with CRuby's values on Linux. A glob
/// reads only `FNM_NOESCAPE` and `FNM_DOTMATCH`; Linux matches case
/// sensitively and a glob always matches by path and expands braces, so the
/// others change nothing.
pub(super) const FNM: [(&CStr, i64); 7] = [
    (c"FNM_NOESCAPE", FNM_NOESCAPE),
    (c"FNM_PATHNAME", 2),
    (c"FNM_DOTMATCH", FNM_DOTMATCH),
    (c"FNM_CASEFOLD", 8),
    (c"FNM_EXTGLOB", 16),
    (c"FNM_SYSCASE", 0),
    (c"FNM_SHORTNAME", 0),
];

const FNM_NOESCAPE: i64 = 1;
const FNM_DOTMATCH: i64 = 4;

/// How a glob matches and lists.
#[derive(Clone, Copy)]
pub(super) struct Options {
    escape: bool,
    dotmatch: bool,
    sort: bool,
}

impl Options {
    pub(super) fn new(flags: i64, sort: bool) -> Self {
        Options {
            escape: flags & FNM_NOESCAPE == 0,
            dotmatch: flags & FNM_DOTMATCH != 0,
            sort,
        }
    }
}

/// The paths `pattern` names, read from `base` when the pattern is
/// relative and `base` is given, as `Dir.glob` answers them.
pub(super) fn glob(pattern: &[u8], base: Option<&[u8]>, options: Options) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    for expanded in brace_expand(pattern, options.escape) {
        Walk {
            nodes: compile(&expanded, options.escape),
            base,
            options,
            found: &mut found,
        }
        .start(&expanded);
    }
    found
}

/// Each pattern `pattern`'s braces spell, in order, as `ruby_brace_expand`
/// spells them; an unclosed brace stays a literal character.
fn brace_expand(pattern: &[u8], escape: bool) -> Vec<Vec<u8>> {
    let (mut lbrace, mut rbrace, mut nest, mut i) = (None, None, 0usize, 0);
    while i < pattern.len() {
        match pattern[i] {
            b'{' => {
                if nest == 0 && lbrace.is_none() {
                    lbrace = Some(i);
                }
                nest += 1;
            }
            b'}' if lbrace.is_some() => {
                nest -= 1;
                if nest == 0 {
                    rbrace = Some(i);
                    break;
                }
            }
            b'\\' if escape => i += 1,
            _ => {}
        }
        i += 1;
    }
    let (Some(lbrace), Some(rbrace)) = (lbrace, rbrace) else {
        return vec![pattern.to_vec()];
    };
    let (prefix, suffix) = (&pattern[..lbrace], &pattern[rbrace + 1..]);
    let mut expanded = Vec::new();
    let mut start = lbrace + 1;
    let (mut nest, mut i) = (0usize, start);
    while i <= rbrace {
        match pattern[i] {
            b',' | b'}' if nest == 0 => {
                let alternative = [prefix, &pattern[start..i], suffix].concat();
                expanded.extend(brace_expand(&alternative, escape));
                start = i + 1;
            }
            b'{' => nest += 1,
            b'}' => nest -= 1,
            b'\\' if escape => i += 1,
            _ => {}
        }
        i += 1;
    }
    expanded
}

/// One step of a compiled pattern, as `glob_make_pattern` classifies it.
enum Node {
    /// A segment without wildcards, its escapes removed.
    Plain(Vec<u8>),
    /// A segment with `*`, `?`, or `[`.
    Magic(Vec<u8>),
    /// `**/`: no directory or any number of them, links not followed.
    Recursive,
    /// The pattern ends here and names what the path names.
    End,
    /// The pattern ends in a separator and names only a directory.
    DirEnd,
}

/// A compiled pattern: each node beside the separators that follow it.
type Nodes = Vec<(Node, Vec<u8>)>;

/// The leading separators of `pattern`, which root its walk.
fn root_of(pattern: &[u8]) -> &[u8] {
    let len = pattern.iter().take_while(|&&byte| byte == b'/').count();
    &pattern[..len]
}

fn compile(pattern: &[u8], escape: bool) -> Nodes {
    let mut nodes = Vec::new();
    let mut rest = &pattern[root_of(pattern).len()..];
    while !rest.is_empty() {
        let len = rest
            .iter()
            .position(|&byte| byte == b'/')
            .unwrap_or(rest.len());
        let (segment, after) = rest.split_at(len);
        let seps = after.iter().take_while(|&&byte| byte == b'/').count();
        let (separators, next) = after.split_at(seps);
        let node = if segment == b"**" && seps > 0 {
            Node::Recursive
        } else if has_magic(segment, escape) {
            Node::Magic(segment.to_vec())
        } else {
            Node::Plain(unescape(segment, escape))
        };
        let repeated =
            matches!(node, Node::Recursive) && matches!(nodes.last(), Some((Node::Recursive, _)));
        if !repeated {
            nodes.push((node, separators.to_vec()));
        }
        rest = next;
        if rest.is_empty() && seps > 0 {
            nodes.push((Node::DirEnd, Vec::new()));
            return nodes;
        }
    }
    nodes.push((Node::End, Vec::new()));
    nodes
}

fn has_magic(segment: &[u8], escape: bool) -> bool {
    let mut i = 0;
    while i < segment.len() {
        match segment[i] {
            b'*' | b'?' | b'[' => return true,
            b'\\' if escape => i += 1,
            _ => {}
        }
        i += 1;
    }
    false
}

fn unescape(segment: &[u8], escape: bool) -> Vec<u8> {
    if !escape {
        return segment.to_vec();
    }
    let mut plain = Vec::with_capacity(segment.len());
    let mut bytes = segment.iter();
    while let Some(&byte) = bytes.next() {
        match byte {
            b'\\' => plain.push(*bytes.next().unwrap_or(&b'\\')),
            _ => plain.push(byte),
        }
    }
    plain
}

/// Whether `name` matches the segment `pattern`, as `fnmatch_helper` matches
/// one path component: a leading `.` only by a leading `.` without
/// `FNM_DOTMATCH`.
fn fnmatch(pattern: &[u8], name: &[u8], options: Options) -> bool {
    let leading = match pattern {
        [b'\\', escaped, ..] if options.escape => Some(*escaped),
        _ => pattern.first().copied(),
    };
    if !options.dotmatch && name.first() == Some(&b'.') && leading != Some(b'.') {
        return false;
    }
    let (mut p, mut s) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if p < pattern.len() {
            let advanced = match pattern[p] {
                b'*' => {
                    while pattern.get(p) == Some(&b'*') {
                        p += 1;
                    }
                    star = Some((p, s));
                    continue;
                }
                b'?' => (s < name.len()).then_some(p + 1),
                b'[' => name
                    .get(s)
                    .and_then(|&byte| bracket(pattern, p + 1, byte, options)),
                b'\\' if options.escape && p + 1 < pattern.len() => {
                    (name.get(s) == Some(&pattern[p + 1])).then_some(p + 2)
                }
                byte => (name.get(s) == Some(&byte)).then_some(p + 1),
            };
            if let Some(next) = advanced {
                p = next;
                s += 1;
                continue;
            }
        } else if s == name.len() {
            return true;
        }
        match star {
            Some((after, from)) if from < name.len() => {
                star = Some((after, from + 1));
                p = after;
                s = from + 1;
            }
            _ => return false,
        }
    }
}

/// Where the bracket expression opening before `start` ends, when it holds
/// `byte`; `None` when it does not, or never closes.
fn bracket(pattern: &[u8], start: usize, byte: u8, options: Options) -> Option<usize> {
    let mut i = start;
    let negated = matches!(pattern.get(i), Some(b'!' | b'^'));
    if negated {
        i += 1;
    }
    let mut held = false;
    let take = |i: &mut usize| -> u8 {
        if options.escape && pattern[*i] == b'\\' && *i + 1 < pattern.len() {
            *i += 1;
        }
        let taken = pattern[*i];
        *i += 1;
        taken
    };
    while i < pattern.len() && pattern[i] != b']' {
        let low = take(&mut i);
        let mut high = low;
        if pattern.get(i) == Some(&b'-') && pattern.get(i + 1).is_some_and(|&next| next != b']') {
            i += 1;
            high = take(&mut i);
        }
        held |= (low..=high).contains(&byte);
    }
    (i < pattern.len() && held != negated).then_some(i + 1)
}

/// A node a path still has to meet, beside the separators that join the
/// next name onto the path.
type Pending = (usize, Vec<u8>);

struct Walk<'a> {
    nodes: Nodes,
    base: Option<&'a [u8]>,
    options: Options,
    found: &'a mut Vec<Vec<u8>>,
}

impl Walk<'_> {
    fn start(&mut self, pattern: &[u8]) {
        self.visit(root_of(pattern).to_vec(), vec![(0, Vec::new())], false);
    }

    /// Where `path` stands on the host: as written when absolute, beneath
    /// `base` when one is given, and the working directory's otherwise.
    fn host_path(&self, path: &[u8]) -> Vec<u8> {
        match self.base {
            Some(base) if path.first() != Some(&b'/') => match path {
                [] => base.to_vec(),
                _ => [base, b"/", path].concat(),
            },
            _ if path.is_empty() => b".".to_vec(),
            _ => path.to_vec(),
        }
    }

    fn exists(&self, path: &[u8]) -> bool {
        fs::symlink_metadata(OsStr::from_bytes(&self.host_path(path))).is_ok()
    }

    fn is_dir(&self, path: &[u8]) -> bool {
        fs::metadata(OsStr::from_bytes(&self.host_path(path))).is_ok_and(|found| found.is_dir())
    }

    /// The entries of the directory `path` names, `.` among them and `..`
    /// never, each beside whether it is a directory and not a link; none
    /// where it reads as no directory, as a glob ignores what it cannot list.
    fn entries(&self, path: &[u8]) -> Vec<(Vec<u8>, bool)> {
        let Ok(listing) = fs::read_dir(OsStr::from_bytes(&self.host_path(path))) else {
            return Vec::new();
        };
        let mut entries = vec![(b".".to_vec(), false)];
        entries.extend(listing.flatten().map(|entry| {
            let real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            (entry.file_name().as_bytes().to_vec(), real_dir)
        }));
        if self.options.sort {
            entries.sort();
        }
        entries
    }

    /// Meet each of `pending` at `path`, as `glob_helper` does: answer the
    /// ends it reaches, then the entries its segments match, each walked in
    /// turn. `known` says whether `path` is known to exist.
    fn visit(&mut self, path: Vec<u8>, pending: Vec<Pending>, known: bool) {
        let mut here = Vec::new();
        for (node, separators) in pending {
            here.push((node, separators.clone()));
            if matches!(self.nodes[node].0, Node::Recursive) {
                here.push((node + 1, separators));
            }
        }
        let (mut listed, mut plain) = (false, false);
        for (node, separators) in &here {
            match self.nodes[*node].0 {
                Node::End if !path.is_empty() && (known || self.exists(&path)) => {
                    self.found.push(path.clone());
                }
                Node::DirEnd if !path.is_empty() && self.is_dir(&path) => {
                    self.found.push([&path, separators.as_slice()].concat());
                }
                Node::Magic(_) | Node::Recursive => listed = true,
                Node::Plain(_) => plain = true,
                _ => {}
            }
        }
        if listed {
            for (name, real_dir) in self.entries(&path) {
                self.descend(&path, &here, &name, real_dir);
            }
        } else if plain {
            for (node, separators) in &here {
                if let Node::Plain(name) = &self.nodes[*node].0 {
                    let child = [&path, separators.as_slice(), name].concat();
                    let next = vec![(node + 1, self.nodes[*node].1.clone())];
                    self.visit(child, next, false);
                }
            }
        }
    }

    /// Walk the entry `name` of `path` with every node of `here` it meets.
    fn descend(&mut self, path: &[u8], here: &[Pending], name: &[u8], real_dir: bool) {
        let mut children: Vec<(Vec<u8>, Vec<Pending>)> = Vec::new();
        for (node, separators) in here {
            let (kind, after) = &self.nodes[*node];
            let next = match kind {
                Node::Recursive
                    if real_dir
                        && name != b"."
                        && (self.options.dotmatch || name.first() != Some(&b'.')) =>
                {
                    *node
                }
                Node::Magic(segment) if fnmatch(segment, name, self.options) => node + 1,
                Node::Plain(literal) if literal == name => node + 1,
                _ => continue,
            };
            let child = [path, separators.as_slice(), name].concat();
            let pending = (next, after.clone());
            match children.iter_mut().find(|(seen, _)| *seen == child) {
                Some((_, nodes)) => nodes.push(pending),
                None => children.push((child, vec![pending])),
            }
        }
        for (child, pending) in children {
            self.visit(child, pending, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, name: &str) -> bool {
        fnmatch(pattern.as_bytes(), name.as_bytes(), Options::new(0, true))
    }

    #[test]
    fn a_segment_matches_as_cruby_fnmatch_does() {
        assert_eq!(
            [
                matches("*.rb", "a.rb"),
                matches("*.rb", ".a.rb"),
                matches(".*", ".a"),
                matches("?.rb", "ab.rb"),
                matches("[a-c].rb", "b.rb"),
                matches("[!a].rb", "a.rb"),
                matches("[^a].rb", "b.rb"),
                matches("\\*", "*"),
                matches("[a", "a"),
                matches("*b*", "abc"),
            ],
            [true, false, true, false, true, false, true, true, false, true],
            "each segment must match as CRuby's File.fnmatch with FNM_PATHNAME matches one name"
        );
    }

    #[test]
    fn braces_expand_in_order_and_an_unclosed_one_stays_literal() {
        let spelled = |pattern: &str| -> Vec<String> {
            brace_expand(pattern.as_bytes(), true)
                .into_iter()
                .map(|bytes| String::from_utf8(bytes).unwrap())
                .collect()
        };
        assert_eq!(
            (spelled("{a,{b,Z}}.*"), spelled("{a.rb"), spelled("{}"), spelled("x\\{a,b}")),
            (
                vec!["a.*".to_string(), "b.*".to_string(), "Z.*".to_string()],
                vec!["{a.rb".to_string()],
                vec![String::new()],
                vec!["x\\{a,b}".to_string()]
            ),
            "braces must expand as CRuby's ruby_brace_expand: nested in order, unclosed and escaped literal"
        );
    }
}
