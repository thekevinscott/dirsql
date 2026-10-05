//! POSIX character classes (`[[:digit:]]`) for globset, which has no
//! `[:name:]` syntax: each bracket expression holding one is rewritten into an
//! equivalent bracket of plain ranges before the glob is compiled.

/// Rewrite every bracket expression in `pattern` that holds a `[:name:]`
/// class into plain ranges. Brackets without one, unclosed brackets and
/// backslash-escaped characters are left exactly as written.
pub(crate) fn expand_posix_classes(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let end = (i + 2).min(chars.len());
                out.extend(&chars[i..end]);
                i = end;
            }
            '[' => match bracket(&chars, i) {
                Some((end, Some(rewritten))) => {
                    out.push_str(&rewritten);
                    i = end;
                }
                Some((end, None)) => {
                    out.extend(&chars[i..end]);
                    i = end;
                }
                None => {
                    out.push('[');
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Parse the bracket expression opening at `start`. `None` when it never
/// closes; otherwise the index past its `]`, plus its rewrite when it holds a
/// class.
fn bracket(chars: &[char], start: usize) -> Option<(usize, Option<String>)> {
    let mut j = start + 1;
    let negated = matches!(chars.get(j), Some('!' | '^'));
    if negated {
        j += 1;
    }
    let body_start = j;
    let mut ranges: Vec<(char, char)> = Vec::new();
    let mut has_class = false;
    // bash matches nothing at all when a range ends in a class, negated or not.
    let mut range_into_class = false;
    loop {
        let c = *chars.get(j)?;
        if c == ']' && j > body_start {
            j += 1;
            break;
        }
        if let Some((name, next)) = class_at(chars, j) {
            ranges.extend_from_slice(class_ranges(&name));
            has_class = true;
            j = next;
            continue;
        }
        match (chars.get(j + 1), chars.get(j + 2)) {
            (Some('-'), Some(&end)) if end != ']' => {
                if let Some((_, next)) = class_at(chars, j + 2) {
                    has_class = true;
                    range_into_class = true;
                    j = next;
                    continue;
                }
                if c <= end {
                    ranges.push((c, end));
                }
                j += 3;
            }
            _ => {
                ranges.push((c, c));
                j += 1;
            }
        }
    }
    if !has_class {
        return Some((j, None));
    }
    let rewritten = if range_into_class {
        "[\0]".to_string()
    } else {
        render(negated, ranges)
    };
    Some((j, Some(rewritten)))
}

/// The class `[:name:]` starting at `j`, with the index past its `:]`. A `]`
/// before the `:]` means the `[` is an ordinary character, as in bash.
fn class_at(chars: &[char], j: usize) -> Option<(String, usize)> {
    if chars.get(j) != Some(&'[') || chars.get(j + 1) != Some(&':') {
        return None;
    }
    let mut k = j + 2;
    loop {
        match (chars.get(k)?, chars.get(k + 1)) {
            (':', Some(']')) => return Some((chars[j + 2..k].iter().collect(), k + 2)),
            (']', _) => return None,
            _ => k += 1,
        }
    }
}

/// The ASCII members of a class; an unknown name has none, as in bash.
fn class_ranges(name: &str) -> &'static [(char, char)] {
    match name {
        "alnum" => &[('0', '9'), ('A', 'Z'), ('a', 'z')],
        "alpha" => &[('A', 'Z'), ('a', 'z')],
        "ascii" => &[('\0', '\x7f')],
        "blank" => &[(' ', ' '), ('\t', '\t')],
        "cntrl" => &[('\0', '\x1f'), ('\x7f', '\x7f')],
        "digit" => &[('0', '9')],
        "graph" => &[('!', '~')],
        "lower" => &[('a', 'z')],
        "print" => &[(' ', '~')],
        "punct" => &[('!', '/'), (':', '@'), ('[', '`'), ('{', '~')],
        "space" => &[('\t', '\r'), (' ', ' ')],
        "upper" => &[('A', 'Z')],
        "word" => &[('0', '9'), ('A', 'Z'), ('a', 'z'), ('_', '_')],
        "xdigit" => &[('0', '9'), ('A', 'F'), ('a', 'f')],
        _ => &[],
    }
}

/// Characters globset reads as syntax at some position inside a bracket, so
/// they are pulled out of ranges and placed where they are literal.
const POSITIONAL: [char; 4] = [']', '-', '!', '^'];

/// A globset bracket matching exactly `ranges` (or everything else when
/// `negated`). A bracket never matches the separator, so `/` is dropped from a
/// positive set and added to a negated one. NUL, which no path holds, stands
/// in when nothing else may open the bracket.
fn render(negated: bool, ranges: Vec<(char, char)>) -> String {
    let mut set: Vec<(u32, u32)> = ranges
        .into_iter()
        .map(|(a, b)| (a as u32, b as u32))
        .collect();
    set = without(set, '/');
    if negated {
        set.push(('/' as u32, '/' as u32));
    }
    let present = |c: char| set.iter().any(|&(a, b)| a <= c as u32 && c as u32 <= b);
    let specials: Vec<char> = POSITIONAL.into_iter().filter(|&c| present(c)).collect();
    let mut plain = POSITIONAL.into_iter().fold(set.clone(), without);
    plain.sort_unstable();
    if plain.is_empty() && !specials.contains(&']') {
        plain.push((0, 0));
    }

    let mut out = String::from("[");
    if negated {
        out.push('!');
    }
    if specials.contains(&']') {
        out.push(']');
    }
    for (a, b) in plain {
        out.extend(char::from_u32(a));
        if a != b {
            out.push('-');
            out.extend(char::from_u32(b));
        }
    }
    for c in ['!', '^', '-'] {
        if specials.contains(&c) {
            out.push(c);
        }
    }
    out.push(']');
    out
}

/// `set` with `c` removed, splitting any range that spans it. `c` is never
/// NUL, so `c - 1` cannot underflow.
fn without(set: Vec<(u32, u32)>, c: char) -> Vec<(u32, u32)> {
    let c = c as u32;
    set.into_iter()
        .flat_map(|(a, b)| [(a, b.min(c - 1)), (a.max(c + 1), b)])
        .filter(|&(a, b)| a <= b)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_becomes_its_ranges() {
        assert_eq!(expand_posix_classes("[[:digit:]].md"), "[0-9].md");
        assert_eq!(expand_posix_classes("[[:upper:]]*"), "[A-Z]*");
    }

    #[test]
    fn every_named_class_expands() {
        let cases = [
            ("alnum", "[0-9A-Za-z]"),
            ("alpha", "[A-Za-z]"),
            ("ascii", "[]\0- \"-,.0-\\_-\x7f!^-]"),
            ("blank", "[\t ]"),
            ("cntrl", "[\0-\x1f\x7f]"),
            ("digit", "[0-9]"),
            ("graph", "[]\"-,.0-\\_-~!^-]"),
            ("lower", "[a-z]"),
            ("print", "[] \"-,.0-\\_-~!^-]"),
            ("punct", "[]\"-,.:-@[-\\_-`{-~!^-]"),
            ("space", "[\t-\r ]"),
            ("upper", "[A-Z]"),
            ("word", "[0-9A-Z_a-z]"),
            ("xdigit", "[0-9A-Fa-f]"),
        ];
        for (name, want) in cases {
            assert_eq!(
                expand_posix_classes(&format!("[[:{name}:]]")),
                want,
                "{name}"
            );
        }
    }

    #[test]
    fn a_negated_class_also_excludes_the_separator() {
        assert_eq!(expand_posix_classes("[![:digit:]]"), "[!/0-9]");
        assert_eq!(expand_posix_classes("[^[:digit:]]"), "[!/0-9]");
    }

    #[test]
    fn classes_and_characters_combine() {
        assert_eq!(expand_posix_classes("[a[:digit:]]"), "[0-9a]");
        assert_eq!(expand_posix_classes("[[:digit:][:upper:]]"), "[0-9A-Z]");
        assert_eq!(expand_posix_classes("[[:digit:]x-z]"), "[0-9x-z]");
    }

    #[test]
    fn a_dash_after_a_class_is_literal() {
        assert_eq!(expand_posix_classes("[[:digit:]-z]"), "[0-9z-]");
        assert_eq!(expand_posix_classes("[[:digit:]-]"), "[0-9-]");
    }

    #[test]
    fn a_leading_close_bracket_is_a_member() {
        assert_eq!(expand_posix_classes("[][:digit:]]"), "[]0-9]");
    }

    #[test]
    fn an_unknown_class_contributes_nothing() {
        assert_eq!(expand_posix_classes("[[:foo:]a]"), "[a]");
        assert_eq!(expand_posix_classes("[[:DIGIT:]]"), "[\0]");
        assert_eq!(expand_posix_classes("[![:foo:]]"), "[!/]");
    }

    #[test]
    fn a_lone_bang_is_kept_off_the_negation_position() {
        assert_eq!(expand_posix_classes("[[:foo:]!]"), "[\0!]");
        assert_eq!(expand_posix_classes("[[:foo:]^]"), "[\0^]");
    }

    #[test]
    fn a_range_ending_in_a_class_matches_nothing() {
        assert_eq!(expand_posix_classes("[x-[:digit:]]"), "[\0]");
        assert_eq!(expand_posix_classes("[!x-[:digit:]]"), "[\0]");
    }

    #[test]
    fn a_reversed_range_beside_a_class_is_empty() {
        assert_eq!(expand_posix_classes("[z-a[:digit:]]"), "[0-9]");
    }

    #[test]
    fn a_bracket_without_a_class_is_untouched() {
        for pattern in ["[a-c]*", "[!x].md", "[]]", "[[:a]", "{a,b}/[xy]"] {
            assert_eq!(expand_posix_classes(pattern), pattern);
        }
    }

    #[test]
    fn an_unclosed_bracket_is_untouched() {
        assert_eq!(expand_posix_classes("[[:digit:]"), "[[:digit:]");
        assert_eq!(expand_posix_classes("a["), "a[");
    }

    #[test]
    fn an_escaped_bracket_is_untouched() {
        assert_eq!(expand_posix_classes("\\[[:digit:]]"), "\\[[:digit:]]");
        assert_eq!(expand_posix_classes("a\\"), "a\\");
    }

    #[test]
    fn text_around_brackets_is_kept() {
        assert_eq!(
            expand_posix_classes("**/v[[:digit:]]/*.md"),
            "**/v[0-9]/*.md"
        );
    }
}
