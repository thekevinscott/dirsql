//! The one encoding a path-table's arguments travel through.
//!
//! `dirsql` mints a path-table by writing a `CREATE VIRTUAL TABLE` statement
//! whose module arguments are SQL literals ([`quote_literal`]), and SQLite
//! hands those arguments to the module verbatim, quotes included, for
//! [`unquote`] to decode. The two are inverses, and they live together so they
//! stay that way: a value that survives one but not the other reaches the
//! module as something the user never wrote.

use std::borrow::Cow;

/// Wrap `s` as a single-quoted SQL string literal.
pub fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Wrap `s` as a double-quoted SQL identifier.
pub fn quote_identifier(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Decode one quoted `CREATE VIRTUAL TABLE` argument: strip the surrounding
/// quotes and collapse the doubling that escaped them. Borrowed when there is
/// nothing to collapse, which is the ordinary case.
pub fn unquote(arg: &str) -> Cow<'_, str> {
    let trimmed = arg.trim();
    for quote in ['\'', '"'] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            let inner = &trimmed[1..trimmed.len() - 1];
            let doubled = [quote, quote].iter().collect::<String>();
            return if inner.contains(&doubled) {
                Cow::Owned(inner.replace(&doubled, &quote.to_string()))
            } else {
                Cow::Borrowed(inner)
            };
        }
    }
    Cow::Borrowed(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_literal_doubles_embedded_quotes() {
        assert_eq!(quote_literal("it's"), "'it''s'");
    }

    #[test]
    fn quote_identifier_doubles_embedded_quotes() {
        assert_eq!(quote_identifier("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn unquote_strips_single_quotes() {
        assert_eq!(unquote("'./docs'"), "./docs");
    }

    #[test]
    fn unquote_strips_double_quotes() {
        assert_eq!(unquote("\"./docs\""), "./docs");
    }

    #[test]
    fn unquote_trims_surrounding_whitespace() {
        assert_eq!(unquote("  './docs'  "), "./docs");
    }

    #[test]
    fn unquote_leaves_unquoted_arguments_alone() {
        assert_eq!(unquote("./docs"), "./docs");
    }

    #[test]
    fn unquote_leaves_a_lone_quote_alone() {
        assert_eq!(unquote("'"), "'");
    }

    #[test]
    fn unquote_leaves_mismatched_quotes_alone() {
        assert_eq!(unquote("'./docs\""), "'./docs\"");
    }

    #[test]
    fn unquote_collapses_the_doubling_that_escaped_a_quote() {
        assert_eq!(unquote("'sh -c ''echo hi'''"), "sh -c 'echo hi'");
    }

    #[test]
    fn unquote_collapses_only_the_quote_character_it_stripped() {
        // A double-quote inside a single-quoted literal was never doubled, so
        // collapsing it would corrupt the value in the other direction.
        assert_eq!(unquote("'say \"\"hi\"\"'"), "say \"\"hi\"\"");
    }

    #[test]
    fn unquote_borrows_when_there_is_nothing_to_collapse() {
        assert!(matches!(unquote("'plain'"), Cow::Borrowed("plain")));
    }

    #[test]
    fn unquote_inverts_quote_literal() {
        for value in [
            "plain",
            "it's",
            "sh -c 'echo hi'",
            "''",
            "a''b",
            "say \"hi\"",
            "",
        ] {
            assert_eq!(
                unquote(&quote_literal(value)),
                value,
                "round trip must be lossless for {value:?}"
            );
        }
    }

    #[test]
    fn unquote_inverts_quote_identifier() {
        for value in ["plain", "a\"b", "./docs/*.md"] {
            assert_eq!(unquote(&quote_identifier(value)), value);
        }
    }
}
