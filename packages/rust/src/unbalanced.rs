//! Bash reads a `{`, `}` or `[` with no partner as a literal character;
//! globset refuses it. Spell each such character as a one-member class.

use crate::brace::class_end;

/// `glob` with every unmatched `{`, `}` and `[` escaped for globset. A
/// complete `[...]` class and a balanced `{...}` group are left as written.
pub(crate) fn escape_unbalanced(glob: &str) -> String {
    let chars: Vec<char> = glob.chars().collect();
    let mut literal = vec![false; chars.len()];
    let mut open = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => match class_end(&chars, i) {
                Some(end) => i = end,
                None => literal[i] = true,
            },
            '{' => open.push(i),
            '}' if open.pop().is_none() => literal[i] = true,
            _ => {}
        }
        i += 1;
    }
    for unclosed in open {
        literal[unclosed] = true;
    }
    let mut out = String::with_capacity(glob.len());
    for (c, lit) in chars.iter().zip(literal) {
        if lit {
            out.extend(['[', *c, ']']);
        } else {
            out.push(*c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unclosed_brace_is_escaped() {
        assert_eq!(escape_unbalanced("{a.md"), "[{]a.md");
        assert_eq!(escape_unbalanced("{a,b.md"), "[{]a,b.md");
    }

    #[test]
    fn an_unopened_brace_is_escaped() {
        assert_eq!(escape_unbalanced("a}.md"), "a[}].md");
        assert_eq!(escape_unbalanced("{a,b}}.md"), "{a,b}[}].md");
    }

    #[test]
    fn an_unclosed_bracket_is_escaped() {
        assert_eq!(escape_unbalanced("[a.md"), "[[]a.md");
        assert_eq!(escape_unbalanced("a[]b"), "a[[]]b");
        assert_eq!(escape_unbalanced("[!a"), "[[]!a");
    }

    #[test]
    fn a_stray_closing_bracket_is_left_alone() {
        assert_eq!(escape_unbalanced("a].md"), "a].md");
    }

    #[test]
    fn balanced_groups_and_classes_are_untouched() {
        for glob in ["{a,b}.md", "[ab].md", "[{]x[}]", "{a,{b,c}}", "[]a].md"] {
            assert_eq!(escape_unbalanced(glob), glob);
        }
    }

    #[test]
    fn braces_inside_a_class_do_not_count() {
        assert_eq!(escape_unbalanced("[{]a}"), "[{]a[}]");
    }

    #[test]
    fn an_escaped_character_does_not_count() {
        assert_eq!(escape_unbalanced(r"\{a"), r"\{a");
        assert_eq!(escape_unbalanced(r"\[a"), r"\[a");
    }
}
