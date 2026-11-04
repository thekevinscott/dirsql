//! The quoted names a statement reads as tables.
//!
//! SQLite folds the case of every identifier, so `'./Up'` and `'./up'` are one
//! table to it and two directories on disk. Keeping them apart takes knowing
//! which quoted tokens stand where a table goes. That is all this module reads
//! out of a statement; it does not parse SQL.

use std::ops::Range;

/// A quoted token in table position: its byte span in the statement and the
/// name it spells, unquoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableRef {
    pub span: Range<usize>,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Quoted(String),
    Word(String),
    Open,
    Close,
    Comma,
    Dot,
    End,
    Other,
}

/// The clause keywords that end a `FROM` list at their own nesting depth.
const ENDS_FROM: &[&str] = &[
    "WHERE",
    "GROUP",
    "HAVING",
    "ORDER",
    "LIMIT",
    "WINDOW",
    "SELECT",
    "VALUES",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "RETURNING",
];

/// Every quoted token of `sql` that names a table: one following `FROM`,
/// `JOIN`, or a comma of a `FROM` list, or one qualifying a column (`"t".c`).
pub(crate) fn table_refs(sql: &str) -> Vec<TableRef> {
    let tokens = tokens(sql);
    let mut in_from = vec![false];
    let mut expect_table = false;
    let mut refs = Vec::new();
    for (i, (span, token)) in tokens.iter().enumerate() {
        let table_here = std::mem::take(&mut expect_table);
        let from = in_from
            .last_mut()
            .expect("the outermost depth is never popped");
        match token {
            Token::Quoted(name) => {
                let qualifies = matches!(tokens.get(i + 1), Some((_, Token::Dot)));
                if table_here || qualifies {
                    refs.push(TableRef {
                        span: span.clone(),
                        name: name.clone(),
                    });
                }
            }
            Token::Word(word) if word.eq_ignore_ascii_case("FROM") => {
                *from = true;
                expect_table = true;
            }
            Token::Word(word) if word.eq_ignore_ascii_case("JOIN") => expect_table = true,
            Token::Word(word) if ENDS_FROM.iter().any(|k| word.eq_ignore_ascii_case(k)) => {
                *from = false;
            }
            Token::Comma => expect_table = *from,
            Token::Open => in_from.push(false),
            Token::Close if in_from.len() > 1 => {
                in_from.pop();
            }
            Token::End => in_from = vec![false],
            _ => {}
        }
    }
    refs
}

/// The name a case variant of `name` is minted under: the case spelled out,
/// so no two spellings share it under SQLite's folding, behind a control
/// character no path a user types starts with.
pub(crate) fn case_alias(name: &str) -> String {
    let mut alias = String::from('\u{1}');
    for c in name.chars() {
        match c {
            '^' => alias.push_str("^^"),
            c if c.is_ascii_uppercase() => {
                alias.push('^');
                alias.push(c.to_ascii_lowercase());
            }
            c => alias.push(c),
        }
    }
    alias
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

/// SQLite's tokens, as far as [`table_refs`] tells them apart, with
/// whitespace and comments dropped.
fn tokens(sql: &str) -> Vec<(Range<usize>, Token)> {
    let mut chars = sql.char_indices().peekable();
    let mut out = Vec::new();
    while let Some((start, c)) = chars.next() {
        let token = match c {
            c if c.is_whitespace() => continue,
            '-' if chars.next_if(|&(_, n)| n == '-').is_some() => {
                skip_while(&mut chars, |c| c != '\n');
                continue;
            }
            '/' if chars.next_if(|&(_, n)| n == '*').is_some() => {
                let mut star = false;
                for (_, c) in chars.by_ref() {
                    if star && c == '/' {
                        break;
                    }
                    star = c == '*';
                }
                continue;
            }
            '\'' | '"' | '`' => Token::Quoted(quoted(&mut chars, c)),
            '[' => {
                let mut name = String::new();
                for (_, c) in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    name.push(c);
                }
                Token::Quoted(name)
            }
            'x' | 'X' if chars.next_if(|&(_, n)| n == '\'').is_some() => {
                quoted(&mut chars, '\'');
                Token::Other
            }
            '(' => Token::Open,
            ')' => Token::Close,
            ',' => Token::Comma,
            '.' => Token::Dot,
            ';' => Token::End,
            c if c.is_ascii_digit() || matches!(c, '?' | ':' | '@' | '$') => {
                skip_while(&mut chars, is_word_char);
                Token::Other
            }
            c if is_word_char(c) => {
                skip_while(&mut chars, is_word_char);
                Token::Word(sql[start..offset(&mut chars, sql)].to_string())
            }
            _ => Token::Other,
        };
        out.push((start..offset(&mut chars, sql), token));
    }
    out
}

/// The byte offset of the next character, or the end of `sql`.
fn offset(chars: &mut Chars<'_>, sql: &str) -> usize {
    chars.peek().map_or(sql.len(), |&(i, _)| i)
}

fn skip_while(chars: &mut Chars<'_>, keep: impl Fn(char) -> bool) {
    while chars.next_if(|&(_, c)| keep(c)).is_some() {}
}

/// The text of a `quote`-delimited token whose opening quote `chars` has
/// just passed, with its doubled quotes collapsed, consuming the closing one.
fn quoted(chars: &mut Chars<'_>, quote: char) -> String {
    let mut name = String::new();
    while let Some((_, c)) = chars.next() {
        if c != quote {
            name.push(c);
        } else if chars.next_if(|&(_, n)| n == quote).is_some() {
            name.push(quote);
        } else {
            break;
        }
    }
    name
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || !c.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(sql: &str) -> Vec<String> {
        table_refs(sql).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn a_quoted_name_after_from_is_a_table() {
        assert_eq!(names("SELECT path FROM './up'"), vec!["./up"]);
    }

    #[test]
    fn its_span_covers_the_quotes() {
        let sql = "SELECT path FROM './up' WHERE 1";
        let refs = table_refs(sql);
        assert_eq!(&sql[refs[0].span.clone()], "'./up'");
    }

    #[test]
    fn every_quote_style_names_a_table() {
        assert_eq!(
            names(r#"SELECT 1 FROM 'a', "b", `c`, [d]"#),
            vec!["a", "b", "c", "d"]
        );
    }

    #[test]
    fn doubled_quotes_collapse() {
        assert_eq!(names("SELECT 1 FROM './it''s'"), vec!["./it's"]);
        assert_eq!(names(r#"SELECT 1 FROM "a""b""#), vec![r#"a"b"#]);
    }

    #[test]
    fn a_join_names_a_table() {
        assert_eq!(
            names("SELECT 1 FROM './a' l LEFT JOIN './b' r ON l.path = r.path"),
            vec!["./a", "./b"]
        );
    }

    #[test]
    fn a_comma_after_an_on_clause_still_joins_a_table() {
        assert_eq!(
            names("SELECT 1 FROM './a' l JOIN './b' r ON f(l.x, 'v'), './c'"),
            vec!["./a", "./b", "./c"]
        );
    }

    #[test]
    fn a_literal_in_the_select_list_is_not_a_table() {
        assert_eq!(names("SELECT 'x', './a' FROM './b'"), vec!["./b"]);
    }

    #[test]
    fn a_literal_in_a_where_clause_is_not_a_table() {
        assert_eq!(
            names("SELECT 1 FROM './b' WHERE path IN ('./a', './c')"),
            vec!["./b"]
        );
        assert_eq!(names("SELECT 1 FROM './b' WHERE x = 'a', 'c'"), vec!["./b"]);
    }

    #[test]
    fn a_function_argument_in_from_is_not_a_table() {
        assert_eq!(
            names("SELECT 1 FROM json_each('./a')"),
            vec![] as Vec<String>
        );
    }

    #[test]
    fn a_from_list_resumes_after_a_subquery() {
        assert_eq!(
            names("SELECT 1 FROM (SELECT 1 FROM './a', './b'), './c' WHERE x, 'd'"),
            vec!["./a", "./b", "./c"]
        );
    }

    #[test]
    fn a_compound_select_starts_afresh() {
        assert_eq!(
            names("SELECT 1 FROM './a' UNION ALL SELECT 'x', 'y' FROM './b'"),
            vec!["./a", "./b"]
        );
    }

    #[test]
    fn a_new_statement_starts_afresh() {
        assert_eq!(names("SELECT 1 FROM './a'; SELECT 'x', 'y'"), vec!["./a"]);
    }

    #[test]
    fn an_unbalanced_close_keeps_the_outer_depth() {
        assert_eq!(names("SELECT 1) FROM './a'"), vec!["./a"]);
    }

    #[test]
    fn a_qualifier_names_a_table() {
        assert_eq!(
            names(r#"SELECT "./a".path, './b'.size FROM t"#),
            vec!["./a", "./b"]
        );
    }

    #[test]
    fn comments_hide_their_text() {
        assert_eq!(
            names("SELECT 1 -- FROM './a'\nFROM /* JOIN './b' */ './c'"),
            vec!["./c"]
        );
        assert_eq!(names("SELECT 1 FROM './a' /* unterminated"), vec!["./a"]);
    }

    #[test]
    fn a_trailing_line_comment_ends_the_statement() {
        assert_eq!(names("SELECT 1 FROM './a' -- done"), vec!["./a"]);
    }

    #[test]
    fn a_blob_literal_is_not_a_table() {
        assert_eq!(names("SELECT 1 FROM x'00', './a'"), vec!["./a"]);
    }

    #[test]
    fn a_parameter_named_like_a_keyword_is_not_one() {
        assert_eq!(names("SELECT :from, 'x' FROM './a'"), vec!["./a"]);
    }

    #[test]
    fn a_number_with_a_point_is_not_a_qualifier() {
        assert_eq!(names("SELECT 1.5, 'x' FROM './a'"), vec!["./a"]);
    }

    #[test]
    fn an_unterminated_quote_runs_to_the_end() {
        assert_eq!(names("SELECT 1 FROM './a"), vec!["./a"]);
        assert_eq!(names("SELECT 1 FROM [./a"), vec!["./a"]);
    }

    #[test]
    fn non_ascii_text_is_one_word() {
        assert_eq!(names("SELECT 1 FROM './é' é, './b'"), vec!["./é", "./b"]);
        assert_eq!(names("SELECT 1 FROM './a' ™ './b'"), vec!["./a"]);
    }

    #[test]
    fn case_alias_spells_out_the_case() {
        assert_eq!(case_alias("./Up"), "\u{1}./^up");
        assert_eq!(case_alias("./a^B"), "\u{1}./a^^^b");
    }

    #[test]
    fn case_aliases_of_case_variants_never_fold_together() {
        let aliases = ["./up", "./Up", "./UP", "./uP"].map(case_alias);
        for (i, a) in aliases.iter().enumerate() {
            for b in &aliases[i + 1..] {
                assert!(!a.eq_ignore_ascii_case(b), "{a:?} and {b:?} fold together");
            }
        }
    }
}
