//! A glob's literal text read as bash reads it: a `\` quotes the character
//! after it.

/// `text` with each backslash escape replaced by the character it quotes. A
/// trailing `\` quotes nothing and is kept.
pub(crate) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut escaped = false;
    for c in text.chars() {
        if escaped || c != '\\' {
            out.push(c);
            escaped = false;
        } else {
            escaped = true;
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_escaped_character_is_the_character() {
        assert_eq!(unescape(r"do\cs"), "docs");
        assert_eq!(unescape(r"my\ dir"), "my dir");
        assert_eq!(unescape(r"\.env"), ".env");
    }

    #[test]
    fn an_escaped_backslash_is_one_backslash() {
        assert_eq!(unescape(r"a\\b"), r"a\b");
        assert_eq!(unescape(r"a\\\.b"), r"a\.b");
    }

    #[test]
    fn a_trailing_backslash_is_kept() {
        assert_eq!(unescape(r"a\"), r"a\");
    }

    #[test]
    fn text_without_a_backslash_is_unchanged() {
        assert_eq!(unescape("docs/api.md"), "docs/api.md");
        assert_eq!(unescape(""), "");
    }
}
