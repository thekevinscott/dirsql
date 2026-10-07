//! Bash brace expansion for path globs.
//!
//! A pattern expands the way bash expands a word before globbing it: a brace
//! group holding a top-level comma is an alternation, one holding a `..`
//! sequence is a range, and any other brace is literal text. The expansions
//! come back as globset patterns with every remaining brace escaped, so
//! globset never sees an alternation of its own.

/// Expand `pattern`'s brace groups as bash does, one globset pattern per word.
pub(crate) fn expand(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    expand_word(&chars)
        .iter()
        .map(|word| escape_braces(word))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    Close,
    Comma,
}

/// The characters of `text` from `from` on that no backslash escapes, with
/// their indices; the backslashes themselves are dropped too.
fn unescaped(text: &[char], from: usize) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut escaped = false;
    text.iter()
        .copied()
        .enumerate()
        .skip(from)
        .filter(move |&(_, c)| {
            let keep = !escaped && c != '\\';
            escaped = !escaped && c == '\\';
            keep
        })
}

/// Index of the first unescaped `stop` at nesting level zero, scanning from
/// `from`; `text.len()` when there is none. A `}` only closes once a comma or
/// a `..` has been seen, which is why `{q}` never closes.
fn gobble(text: &[char], from: usize, stop: Stop) -> usize {
    let target = match stop {
        Stop::Close => '}',
        Stop::Comma => ',',
    };
    let mut level = 0usize;
    let mut closable = stop != Stop::Close;
    for (i, c) in unescaped(text, from) {
        if c == target && level == 0 && closable {
            return i;
        }
        if c == '{' {
            level += 1;
        } else if c == '}' && level > 0 {
            level -= 1;
        } else if level == 0
            && (c == ',' || (text[i..].starts_with(&['.', '.']) && text.get(i + 2) != Some(&'}')))
        {
            closable = true;
        }
    }
    text.len()
}

/// Bash never opens a group at a `{` that starts a word-like token and is
/// followed by whitespace or `}`, so `{}` stays intact for `find -exec`.
fn is_shell_brace(text: &[char], i: usize) -> bool {
    let after_space = i == 0 || text[i - 1].is_ascii_whitespace();
    let before_space_or_close = text
        .get(i + 1)
        .is_some_and(|c| c.is_ascii_whitespace() || *c == '}');
    after_space && before_space_or_close
}

fn expand_word(text: &[char]) -> Vec<String> {
    let group = unescaped(text, 0)
        .filter(|&(i, c)| c == '{' && !is_shell_brace(text, i))
        .find_map(|(open, _)| {
            let close = gobble(text, open + 1, Stop::Close);
            (close < text.len()).then_some((open, close))
        });
    let Some((open, close)) = group else {
        return vec![text.iter().collect()];
    };
    let preamble: String = text[..open].iter().collect();
    let amble = &text[open + 1..close];
    let middle = if unescaped(amble, 0).any(|(_, c)| c == ',') {
        expand_alternatives(amble)
    } else {
        let amble: String = amble.iter().collect();
        sequence(&amble).unwrap_or_else(|| vec![format!("{{{amble}}}")])
    };
    let rest = expand_word(&text[close + 1..]);
    middle
        .iter()
        .flat_map(|m| {
            rest.iter()
                .map(|r| format!("{preamble}{m}{r}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn expand_alternatives(amble: &[char]) -> Vec<String> {
    let end = gobble(amble, 0, Stop::Comma);
    let mut words = expand_word(&amble[..end]);
    if let Some(tail) = amble.get(end + 1..) {
        words.extend(expand_alternatives(tail));
    }
    words
}

/// The `..` range bash expands `{lo..hi}` / `{lo..hi..step}` to, or `None`
/// when the text is not a range bash accepts.
fn sequence(amble: &str) -> Option<Vec<String>> {
    let (lhs, rhs) = amble.split_once("..")?;
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    let digits = number_prefix_len(rhs);
    let (end, rhs_len) = if let Ok(n) = rhs[..digits].parse() {
        (Term::Int(n), digits)
    } else {
        let c = rhs.as_bytes()[0];
        if !c.is_ascii_alphabetic() || !(rhs.len() == 1 || rhs.as_bytes()[1] == b'.') {
            return None;
        }
        (Term::Char(c), 1)
    };
    let step = match &rhs[rhs_len..] {
        "" => 1,
        tail => tail
            .strip_prefix("..")
            .filter(|s| !s.is_empty())?
            .parse()
            .ok()?,
    };
    let start = if let Ok(n) = lhs.parse::<i64>() {
        Term::Int(n)
    } else if lhs.len() == 1 && lhs.as_bytes()[0].is_ascii_alphabetic() {
        Term::Char(lhs.as_bytes()[0])
    } else {
        return None;
    };
    match (start, end) {
        (Term::Int(lo), Term::Int(hi)) => {
            let width = pad_width(lhs, &rhs[..rhs_len]);
            Some(
                steps(lo, hi, step)?
                    .map(|n| format!("{n:0width$}"))
                    .collect(),
            )
        }
        (Term::Char(lo), Term::Char(hi)) => Some(
            steps(i64::from(lo), i64::from(hi), step)?
                .filter_map(|n| u8::try_from(n).ok())
                .map(|b| char::from(b).to_string())
                .collect(),
        ),
        _ => None,
    }
}

enum Term {
    Int(i64),
    Char(u8),
}

fn number_prefix_len(s: &str) -> usize {
    let sign = usize::from(matches!(s.as_bytes()[0], b'+' | b'-'));
    sign + s[sign..].bytes().take_while(u8::is_ascii_digit).count()
}

/// Bash zero-pads every term to the widest end written with a leading zero.
fn pad_width(lhs: &str, rhs: &str) -> usize {
    let padded =
        |s: &str| (s.len() > 1 && s.starts_with('0')) || (s.len() > 2 && s.starts_with("-0"));
    if padded(lhs) || padded(rhs) {
        lhs.len().max(rhs.len())
    } else {
        0
    }
}

/// `lo` to `hi` inclusive in steps of `|step|` (0 counts as 1), walking down
/// when `hi < lo`. `None` past the element count bash refuses to generate.
fn steps(lo: i64, hi: i64, step: i64) -> Option<impl Iterator<Item = i128>> {
    let stride = i128::from(step).abs().max(1);
    let distance = i128::from(hi) - i128::from(lo);
    if distance.abs() / stride > i128::from(i32::MAX) - 3 {
        return None;
    }
    let count = distance.abs() / stride + 1;
    let signed = stride * distance.signum();
    Some((0..count).map(move |k| i128::from(lo) + k * signed))
}

/// Escape the braces bash left as literal text, so globset matches them
/// rather than reading an alternation. A complete `[...]` class is copied as
/// written; an unclosed `[` is a literal character.
fn escape_braces(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len());
    let mut resume = 0;
    for (i, &c) in chars.iter().enumerate() {
        if i < resume {
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('\\', Some(&next)) => {
                match next {
                    '{' => out.push_str("[{]"),
                    '}' => out.push_str("[}]"),
                    ',' => out.push(','),
                    _ => out.extend(['\\', next]),
                }
                resume = i + 2;
            }
            ('[', _) => match class_end(&chars, i) {
                Some(end) => {
                    out.extend(&chars[i..=end]);
                    resume = end + 1;
                }
                None => out.push_str("[[]"),
            },
            ('{', _) => out.push_str("[{]"),
            ('}', _) => out.push_str("[}]"),
            _ => out.push(c),
        }
    }
    out
}

/// Index of the `]` closing the class globset opens at `open`, where a `]`
/// first in the class (after any `!` / `^`) is a member, not the close.
fn class_end(chars: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if matches!(chars.get(i), Some('!' | '^')) {
        i += 1;
    }
    if chars.get(i) == Some(&']') {
        i += 1;
    }
    (i..chars.len()).find(|&j| chars[j] == ']')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(pattern: &str) -> Vec<String> {
        expand_word(&pattern.chars().collect::<Vec<_>>())
    }

    #[test]
    fn a_group_without_a_comma_or_sequence_is_literal() {
        for pattern in [
            "x{q}y", "x{}y", "x{a,b", "xa,b}y", "x{}}y", "x{{}}y", "{},a}",
        ] {
            assert_eq!(words(pattern), vec![pattern], "{pattern}");
        }
    }

    #[test]
    fn a_comma_group_alternates_in_order() {
        assert_eq!(words("x{a,b}y"), vec!["xay", "xby"]);
        assert_eq!(words("{a,b}{c,d}"), vec!["ac", "ad", "bc", "bd"]);
    }

    #[test]
    fn empty_alternatives_are_kept() {
        assert_eq!(words("x{,a}y"), vec!["xy", "xay"]);
        assert_eq!(words("x{a,}y"), vec!["xay", "xy"]);
        assert_eq!(words("x{,}y"), vec!["xy", "xy"]);
    }

    #[test]
    fn nested_groups_expand_inside_out_as_bash_does() {
        assert_eq!(words("x{a,{b,c}}y"), vec!["xay", "xby", "xcy"]);
        assert_eq!(words("x{a{b,c}}y"), vec!["x{ab}y", "x{ac}y"]);
        assert_eq!(words("x{{a,b}}y"), vec!["x{a}y", "x{b}y"]);
        assert_eq!(words("x{a,{b}}y"), vec!["xay", "x{b}y"]);
        assert_eq!(words("x{{},a}y"), vec!["x{}y", "xay"]);
        assert_eq!(words("x{}a,b}y"), vec!["x}ay", "xby"]);
        assert_eq!(words("x{..{a,b}}y"), vec!["x..ay", "x..by"]);
    }

    #[test]
    fn stray_braces_beside_a_group_are_literal() {
        assert_eq!(words("x{{a,b}y"), vec!["x{ay", "x{by"]);
        assert_eq!(words("x{a,b}}y"), vec!["xa}y", "xb}y"]);
        assert_eq!(words("x{a,}}y"), vec!["xa}y", "x}y"]);
        assert_eq!(words("xx{a..}y,b}"), vec!["xxa..}y", "xxb"]);
    }

    #[test]
    fn an_escaped_brace_or_comma_does_not_count() {
        assert_eq!(words(r"x\{a,b}y"), vec![r"x\{a,b}y"]);
        assert_eq!(words(r"x{a\,b}y"), vec![r"x{a\,b}y"]);
        assert_eq!(words(r"x{a\,b,c}y"), vec![r"xa\,by", "xcy"]);
        assert_eq!(words(r"x{\}a,b}y"), vec![r"x\}ay", "xby"]);
    }

    #[test]
    fn a_brace_before_whitespace_or_a_close_after_whitespace_never_opens() {
        assert_eq!(words("x {},a}y"), vec!["x {},a}y"]);
        assert_eq!(words("{ a,b}"), vec!["{ a,b}"]);
        assert_eq!(words("x{ a,b}"), vec!["x a", "xb"]);
    }

    #[test]
    fn integer_sequences_count_either_way() {
        assert_eq!(words("x{1..3}y"), vec!["x1y", "x2y", "x3y"]);
        assert_eq!(words("x{3..1}"), vec!["x3", "x2", "x1"]);
        assert_eq!(words("{-2..1}"), vec!["-2", "-1", "0", "1"]);
        assert_eq!(words("{+1..3}"), vec!["1", "2", "3"]);
        assert_eq!(words("{1..-1}"), vec!["1", "0", "-1"]);
        assert_eq!(words("{1..1}"), vec!["1"]);
    }

    #[test]
    fn a_step_is_taken_by_magnitude() {
        assert_eq!(words("{1..10..3}"), vec!["1", "4", "7", "10"]);
        assert_eq!(words("{10..1..3}"), vec!["10", "7", "4", "1"]);
        assert_eq!(words("{1..3..-1}"), vec!["1", "2", "3"]);
        assert_eq!(words("{1..3..0}"), vec!["1", "2", "3"]);
    }

    #[test]
    fn a_leading_zero_pads_every_term() {
        assert_eq!(words("{08..10}"), vec!["08", "09", "10"]);
        assert_eq!(words("{1..03}"), vec!["01", "02", "03"]);
        assert_eq!(words("{001..2}"), vec!["001", "002"]);
        assert_eq!(words("{-01..1}"), vec!["-01", "000", "001"]);
        assert_eq!(words("{0..10..5}"), vec!["0", "5", "10"]);
        assert_eq!(words("{-0..10..5}"), vec!["0", "5", "10"]);
    }

    #[test]
    fn letter_sequences_walk_the_alphabet() {
        assert_eq!(words("{a..c}"), vec!["a", "b", "c"]);
        assert_eq!(words("{e..a..2}"), vec!["e", "c", "a"]);
        assert_eq!(words("{Y..[}"), vec!["{Y..[}"]);
    }

    #[test]
    fn a_malformed_sequence_is_literal() {
        for pattern in [
            "{1..a}",
            "{ab..c}",
            "{1...3}",
            "{a..}",
            "{..a}",
            "{1..3..}",
            "{ab..cd}",
            "{1..3...}",
            "{1..3.}",
            "{1..3..x}",
            "{é..f}",
            "{9223372036854775807..9223372036854775808}",
        ] {
            assert_eq!(words(pattern), vec![pattern], "{pattern}");
        }
    }

    #[test]
    fn a_malformed_sequence_leaves_later_groups_expanding() {
        assert_eq!(words("x{1..z}{a,b}"), vec!["x{1..z}a", "x{1..z}b"]);
        assert_eq!(words("x{q}{1..2}"), vec!["x{q}1", "x{q}2"]);
    }

    #[test]
    fn a_comma_beside_dots_makes_an_alternation_not_a_range() {
        assert_eq!(words("{a..c,d}"), vec!["a..c", "d"]);
        assert_eq!(words("{1..3,}"), vec!["1..3", ""]);
    }

    #[test]
    fn a_range_bash_refuses_to_generate_stays_literal() {
        assert!(steps(0, i64::from(i32::MAX), 1).is_none());
        assert!(steps(0, i64::from(i32::MAX) - 2, 1).is_none());
        let widest = steps(0, i64::from(i32::MAX) - 3, 1).unwrap();
        assert_eq!(widest.size_hint().0, i32::MAX as usize - 2);
        assert!(steps(0, i64::from(i32::MAX), 2).is_some());
    }

    #[test]
    fn a_range_missing_either_end_is_not_a_sequence() {
        assert!(sequence("a..").is_none());
        assert!(sequence("..a").is_none());
    }

    #[test]
    fn literal_braces_are_escaped_for_globset() {
        assert_eq!(escape_braces("x{q}y"), "x[{]q[}]y");
        assert_eq!(escape_braces(r"x\{a,b\}"), "x[{]a,b[}]");
        assert_eq!(escape_braces(r"a\,b"), "a,b");
        assert_eq!(escape_braces(r"a\*b\"), r"a\*b\");
    }

    #[test]
    fn a_class_is_copied_as_written() {
        assert_eq!(escape_braces("[{}]x"), "[{}]x");
        assert_eq!(escape_braces("[]{]{"), "[]{][{]");
        assert_eq!(escape_braces("[!]{]{"), "[!]{][{]");
        assert_eq!(escape_braces("[^}]}"), "[^}][}]");
    }

    #[test]
    fn an_unclosed_bracket_is_literal() {
        assert_eq!(escape_braces("x[{q}"), "x[[][{]q[}]");
        assert_eq!(escape_braces("x[]{"), "x[[]][{]");
        assert_eq!(escape_braces("[!a"), "[[]!a");
        assert_eq!(escape_braces("a].md"), "a].md");
    }

    #[test]
    fn expand_leaves_an_unpartnered_brace_literal() {
        assert_eq!(expand("{a.md"), vec!["[{]a.md"]);
        assert_eq!(expand("a}.md"), vec!["a[}].md"]);
        assert_eq!(expand("{a,b}}.md"), vec!["a[}].md", "b[}].md"]);
    }

    #[test]
    fn expand_escapes_every_word() {
        assert_eq!(expand("{a,{b}}.md"), vec!["a.md", "[{]b[}].md"]);
        assert_eq!(expand("{q}"), vec!["[{]q[}]"]);
        assert_eq!(expand("plain/*.md"), vec!["plain/*.md"]);
    }
}
