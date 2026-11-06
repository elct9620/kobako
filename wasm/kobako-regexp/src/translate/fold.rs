//! Case-insensitive matching across a multi-character fold. MRI folds `ß`
//! to `ss` and `ﬃ` to `ffi`, so `/ss/i` matches `ß` and `/ß/i` matches
//! `SS`; the engine folds one character to one character only. A run of
//! literal text is rewritten to every spelling whose folds join to the
//! run's fold, and a class adds the folds of the characters it names.

/// Each character CRuby 3.4 (Unicode 15.0) folds to more than one.
const MULTI: [(char, &str); 104] = [
    ('\u{DF}', "\u{73}\u{73}"),
    ('\u{130}', "\u{69}\u{307}"),
    ('\u{149}', "\u{2BC}\u{6E}"),
    ('\u{1F0}', "\u{6A}\u{30C}"),
    ('\u{390}', "\u{3B9}\u{308}\u{301}"),
    ('\u{3B0}', "\u{3C5}\u{308}\u{301}"),
    ('\u{587}', "\u{565}\u{582}"),
    ('\u{1E96}', "\u{68}\u{331}"),
    ('\u{1E97}', "\u{74}\u{308}"),
    ('\u{1E98}', "\u{77}\u{30A}"),
    ('\u{1E99}', "\u{79}\u{30A}"),
    ('\u{1E9A}', "\u{61}\u{2BE}"),
    ('\u{1E9E}', "\u{73}\u{73}"),
    ('\u{1F50}', "\u{3C5}\u{313}"),
    ('\u{1F52}', "\u{3C5}\u{313}\u{300}"),
    ('\u{1F54}', "\u{3C5}\u{313}\u{301}"),
    ('\u{1F56}', "\u{3C5}\u{313}\u{342}"),
    ('\u{1F80}', "\u{1F00}\u{3B9}"),
    ('\u{1F81}', "\u{1F01}\u{3B9}"),
    ('\u{1F82}', "\u{1F02}\u{3B9}"),
    ('\u{1F83}', "\u{1F03}\u{3B9}"),
    ('\u{1F84}', "\u{1F04}\u{3B9}"),
    ('\u{1F85}', "\u{1F05}\u{3B9}"),
    ('\u{1F86}', "\u{1F06}\u{3B9}"),
    ('\u{1F87}', "\u{1F07}\u{3B9}"),
    ('\u{1F88}', "\u{1F00}\u{3B9}"),
    ('\u{1F89}', "\u{1F01}\u{3B9}"),
    ('\u{1F8A}', "\u{1F02}\u{3B9}"),
    ('\u{1F8B}', "\u{1F03}\u{3B9}"),
    ('\u{1F8C}', "\u{1F04}\u{3B9}"),
    ('\u{1F8D}', "\u{1F05}\u{3B9}"),
    ('\u{1F8E}', "\u{1F06}\u{3B9}"),
    ('\u{1F8F}', "\u{1F07}\u{3B9}"),
    ('\u{1F90}', "\u{1F20}\u{3B9}"),
    ('\u{1F91}', "\u{1F21}\u{3B9}"),
    ('\u{1F92}', "\u{1F22}\u{3B9}"),
    ('\u{1F93}', "\u{1F23}\u{3B9}"),
    ('\u{1F94}', "\u{1F24}\u{3B9}"),
    ('\u{1F95}', "\u{1F25}\u{3B9}"),
    ('\u{1F96}', "\u{1F26}\u{3B9}"),
    ('\u{1F97}', "\u{1F27}\u{3B9}"),
    ('\u{1F98}', "\u{1F20}\u{3B9}"),
    ('\u{1F99}', "\u{1F21}\u{3B9}"),
    ('\u{1F9A}', "\u{1F22}\u{3B9}"),
    ('\u{1F9B}', "\u{1F23}\u{3B9}"),
    ('\u{1F9C}', "\u{1F24}\u{3B9}"),
    ('\u{1F9D}', "\u{1F25}\u{3B9}"),
    ('\u{1F9E}', "\u{1F26}\u{3B9}"),
    ('\u{1F9F}', "\u{1F27}\u{3B9}"),
    ('\u{1FA0}', "\u{1F60}\u{3B9}"),
    ('\u{1FA1}', "\u{1F61}\u{3B9}"),
    ('\u{1FA2}', "\u{1F62}\u{3B9}"),
    ('\u{1FA3}', "\u{1F63}\u{3B9}"),
    ('\u{1FA4}', "\u{1F64}\u{3B9}"),
    ('\u{1FA5}', "\u{1F65}\u{3B9}"),
    ('\u{1FA6}', "\u{1F66}\u{3B9}"),
    ('\u{1FA7}', "\u{1F67}\u{3B9}"),
    ('\u{1FA8}', "\u{1F60}\u{3B9}"),
    ('\u{1FA9}', "\u{1F61}\u{3B9}"),
    ('\u{1FAA}', "\u{1F62}\u{3B9}"),
    ('\u{1FAB}', "\u{1F63}\u{3B9}"),
    ('\u{1FAC}', "\u{1F64}\u{3B9}"),
    ('\u{1FAD}', "\u{1F65}\u{3B9}"),
    ('\u{1FAE}', "\u{1F66}\u{3B9}"),
    ('\u{1FAF}', "\u{1F67}\u{3B9}"),
    ('\u{1FB2}', "\u{1F70}\u{3B9}"),
    ('\u{1FB3}', "\u{3B1}\u{3B9}"),
    ('\u{1FB4}', "\u{3AC}\u{3B9}"),
    ('\u{1FB6}', "\u{3B1}\u{342}"),
    ('\u{1FB7}', "\u{3B1}\u{342}\u{3B9}"),
    ('\u{1FBC}', "\u{3B1}\u{3B9}"),
    ('\u{1FC2}', "\u{1F74}\u{3B9}"),
    ('\u{1FC3}', "\u{3B7}\u{3B9}"),
    ('\u{1FC4}', "\u{3AE}\u{3B9}"),
    ('\u{1FC6}', "\u{3B7}\u{342}"),
    ('\u{1FC7}', "\u{3B7}\u{342}\u{3B9}"),
    ('\u{1FCC}', "\u{3B7}\u{3B9}"),
    ('\u{1FD2}', "\u{3B9}\u{308}\u{300}"),
    ('\u{1FD3}', "\u{3B9}\u{308}\u{301}"),
    ('\u{1FD6}', "\u{3B9}\u{342}"),
    ('\u{1FD7}', "\u{3B9}\u{308}\u{342}"),
    ('\u{1FE2}', "\u{3C5}\u{308}\u{300}"),
    ('\u{1FE3}', "\u{3C5}\u{308}\u{301}"),
    ('\u{1FE4}', "\u{3C1}\u{313}"),
    ('\u{1FE6}', "\u{3C5}\u{342}"),
    ('\u{1FE7}', "\u{3C5}\u{308}\u{342}"),
    ('\u{1FF2}', "\u{1F7C}\u{3B9}"),
    ('\u{1FF3}', "\u{3C9}\u{3B9}"),
    ('\u{1FF4}', "\u{3CE}\u{3B9}"),
    ('\u{1FF6}', "\u{3C9}\u{342}"),
    ('\u{1FF7}', "\u{3C9}\u{342}\u{3B9}"),
    ('\u{1FFC}', "\u{3C9}\u{3B9}"),
    ('\u{FB00}', "\u{66}\u{66}"),
    ('\u{FB01}', "\u{66}\u{69}"),
    ('\u{FB02}', "\u{66}\u{6C}"),
    ('\u{FB03}', "\u{66}\u{66}\u{69}"),
    ('\u{FB04}', "\u{66}\u{66}\u{6C}"),
    ('\u{FB05}', "\u{73}\u{74}"),
    ('\u{FB06}', "\u{73}\u{74}"),
    ('\u{FB13}', "\u{574}\u{576}"),
    ('\u{FB14}', "\u{574}\u{565}"),
    ('\u{FB15}', "\u{574}\u{56B}"),
    ('\u{FB16}', "\u{57E}\u{576}"),
    ('\u{FB17}', "\u{574}\u{56D}"),
];

/// The characters CRuby folds onto one character a multi-character fold
/// holds, so a pattern spelled in another case still finds the fold.
const SIMPLE: [(char, char); 59] = [
    ('\u{41}', '\u{61}'),
    ('\u{46}', '\u{66}'),
    ('\u{48}', '\u{68}'),
    ('\u{49}', '\u{69}'),
    ('\u{4A}', '\u{6A}'),
    ('\u{4C}', '\u{6C}'),
    ('\u{4E}', '\u{6E}'),
    ('\u{53}', '\u{73}'),
    ('\u{54}', '\u{74}'),
    ('\u{57}', '\u{77}'),
    ('\u{59}', '\u{79}'),
    ('\u{17F}', '\u{73}'),
    ('\u{345}', '\u{3B9}'),
    ('\u{386}', '\u{3AC}'),
    ('\u{389}', '\u{3AE}'),
    ('\u{38F}', '\u{3CE}'),
    ('\u{391}', '\u{3B1}'),
    ('\u{397}', '\u{3B7}'),
    ('\u{399}', '\u{3B9}'),
    ('\u{3A1}', '\u{3C1}'),
    ('\u{3A5}', '\u{3C5}'),
    ('\u{3A9}', '\u{3C9}'),
    ('\u{3F1}', '\u{3C1}'),
    ('\u{535}', '\u{565}'),
    ('\u{53B}', '\u{56B}'),
    ('\u{53D}', '\u{56D}'),
    ('\u{544}', '\u{574}'),
    ('\u{546}', '\u{576}'),
    ('\u{54E}', '\u{57E}'),
    ('\u{552}', '\u{582}'),
    ('\u{1F08}', '\u{1F00}'),
    ('\u{1F09}', '\u{1F01}'),
    ('\u{1F0A}', '\u{1F02}'),
    ('\u{1F0B}', '\u{1F03}'),
    ('\u{1F0C}', '\u{1F04}'),
    ('\u{1F0D}', '\u{1F05}'),
    ('\u{1F0E}', '\u{1F06}'),
    ('\u{1F0F}', '\u{1F07}'),
    ('\u{1F28}', '\u{1F20}'),
    ('\u{1F29}', '\u{1F21}'),
    ('\u{1F2A}', '\u{1F22}'),
    ('\u{1F2B}', '\u{1F23}'),
    ('\u{1F2C}', '\u{1F24}'),
    ('\u{1F2D}', '\u{1F25}'),
    ('\u{1F2E}', '\u{1F26}'),
    ('\u{1F2F}', '\u{1F27}'),
    ('\u{1F68}', '\u{1F60}'),
    ('\u{1F69}', '\u{1F61}'),
    ('\u{1F6A}', '\u{1F62}'),
    ('\u{1F6B}', '\u{1F63}'),
    ('\u{1F6C}', '\u{1F64}'),
    ('\u{1F6D}', '\u{1F65}'),
    ('\u{1F6E}', '\u{1F66}'),
    ('\u{1F6F}', '\u{1F67}'),
    ('\u{1FBA}', '\u{1F70}'),
    ('\u{1FBE}', '\u{3B9}'),
    ('\u{1FCA}', '\u{1F74}'),
    ('\u{1FFA}', '\u{1F7C}'),
    ('\u{2126}', '\u{3C9}'),
];

/// The most text one run may expand to; a run of many overlapping folds
/// spells too many ways to write out.
const MAX_SPELLINGS_LEN: usize = 4096;

/// Each character that folds to several, with what it folds to.
pub(super) fn multi_folds() -> impl Iterator<Item = (char, &'static str)> {
    MULTI.iter().copied()
}

fn multi_fold(c: char) -> Option<&'static str> {
    MULTI
        .iter()
        .find(|(source, _)| *source == c)
        .map(|(_, folded)| *folded)
}

fn simple_fold(c: char) -> char {
    SIMPLE
        .iter()
        .find(|(source, _)| *source == c)
        .map_or(c, |(_, target)| *target)
}

/// How many pattern characters from `at` together fold to what one
/// character folds to, with that fold.
fn joined_folds(run: &[char], at: usize) -> Vec<(usize, &'static str)> {
    let mut joined: Vec<(usize, &'static str)> = Vec::new();
    for (_, target) in MULTI {
        let len = target.chars().count();
        let spelled = run
            .get(at..at + len)
            .is_some_and(|chars| chars.iter().map(|c| simple_fold(*c)).eq(target.chars()));
        if spelled && !joined.contains(&(len, target)) {
            joined.push((len, target));
        }
    }
    joined
}

fn push_literal(out: &mut String, c: char) {
    if c.is_alphanumeric() {
        out.push(c);
    } else {
        out.push_str(&format!(r"\x{{{:X}}}", u32::from(c)));
    }
}

fn push_fold(out: &mut String, folded: &str) {
    folded.chars().for_each(|c| push_literal(out, c));
}

/// The characters whose fold is `folded`, as a class.
fn sources_of(folded: &str) -> String {
    let mut class = String::from("[");
    for (source, target) in MULTI {
        if target == folded {
            push_literal(&mut class, source);
        }
    }
    class.push(']');
    class
}

/// One pattern character as MRI matches it case-insensitively: itself, or
/// what it folds to when that is several characters.
fn push_alone(out: &mut String, c: char) {
    match multi_fold(c) {
        Some(folded) => {
            out.push_str("(?:");
            push_literal(out, c);
            out.push('|');
            push_fold(out, folded);
            out.push(')');
        }
        None => push_literal(out, c),
    }
}

/// The engine's text for a run of literal characters read
/// case-insensitively, or `None` when no multi-character fold reaches it.
/// MRI spells the run character by character: each one alone, or a few
/// together where they spell what one character folds to.
pub(super) fn expand_run(run: &str) -> Result<Option<String>, &'static str> {
    let chars: Vec<char> = run.chars().collect();
    let reaches = (0..chars.len())
        .any(|at| multi_fold(chars[at]).is_some() || !joined_folds(&chars, at).is_empty());
    if !reaches {
        return Ok(None);
    }
    let mut out = String::new();
    write_spellings(&chars, 0, &mut out)?;
    Ok(Some(out))
}

fn write_spellings(run: &[char], at: usize, out: &mut String) -> Result<(), &'static str> {
    if out.len() > MAX_SPELLINGS_LEN {
        return Err("too many case-insensitive spellings");
    }
    if at == run.len() {
        return Ok(());
    }
    let joined = joined_folds(run, at);
    if joined.is_empty() {
        push_alone(out, run[at]);
        return write_spellings(run, at + 1, out);
    }
    out.push_str("(?:");
    push_alone(out, run[at]);
    write_spellings(run, at + 1, out)?;
    for (len, folded) in joined {
        out.push('|');
        out.push_str(&sources_of(folded));
        write_spellings(run, at + len, out)?;
    }
    out.push(')');
    Ok(())
}

/// The alternatives a case-insensitive class adds: the fold of each
/// character it names that folds to more than one.
pub(super) fn class_folds(named: &[char]) -> Vec<String> {
    let mut folds: Vec<String> = Vec::new();
    for &c in named {
        if let Some(folded) = multi_fold(c) {
            let mut text = String::new();
            push_fold(&mut text, folded);
            if !folds.contains(&text) {
                folds.push(text);
            }
        }
    }
    folds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_a_run_every_way_its_folds_join() {
        assert_eq!(
            expand_run("ss"),
            Ok(Some("(?:ss|[\u{DF}\u{1E9E}])".to_string()))
        );
        assert_eq!(expand_run("\u{DF}"), Ok(Some("(?:\u{DF}|ss)".to_string())));
        assert_eq!(
            expand_run("Ss"),
            Ok(Some("(?:Ss|[\u{DF}\u{1E9E}])".to_string()))
        );
        assert_eq!(
            expand_run("s\u{DF}"),
            Ok(Some("s(?:\u{DF}|ss)".to_string()))
        );
        assert_eq!(expand_run("abc"), Ok(None));
    }

    #[test]
    fn refuses_a_run_with_too_many_spellings() {
        assert!(expand_run(&"s".repeat(40)).is_err());
    }

    #[test]
    fn adds_the_folds_a_class_names() {
        assert_eq!(
            class_folds(&['x', '\u{DF}', '\u{1E9E}']),
            vec!["ss".to_string()]
        );
    }
}
