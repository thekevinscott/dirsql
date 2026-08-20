//! Rendering result rows as a table, for rows that are read rather than piped.
//!
//! The JSON array every other surface prints is right for `dirsql "…" | jq`
//! and wrong for a human: `SELECT * FROM './'` over a few thousand files puts
//! a few thousand JSON objects on one line. A table is the same rows laid out
//! to be read.
//!
//! Deliberately modest, in the `sqlite3` `.mode column` style: aligned
//! columns, a rule under the header, a row count. **Long cells are truncated
//! rather than wrapped**, which is what keeps one `content` column from
//! swallowing the table; laying the table out to the terminal's width, and
//! paging a long result, are both out of scope.

use std::collections::BTreeSet;

use serde_json::Value;

/// Printed instead of an empty table. A query that matched nothing and a
/// query whose output was swallowed look identical otherwise.
const NO_ROWS: &str = "(no rows)";

/// How a SQL `NULL` is shown. Spelled out because a blank cell cannot be told
/// apart from an empty string.
const NULL: &str = "NULL";

/// Longest a cell may be before it is cut short. A `content` column holds
/// whole files; without a cap it decides the width of every row.
const MAX_CELL: usize = 60;

/// Marks a cell the cap cut short.
const ELLIPSIS: char = '…';

/// Spaces between columns.
const GUTTER: &str = "  ";

/// Render `rows` -- the JSON array the shared pipeline produces -- as a table.
pub(super) fn render(rows: &[Value]) -> String {
    let columns = columns(rows);
    if columns.is_empty() {
        return format!("{NO_ROWS}\n");
    }

    let grid: Vec<Vec<String>> = rows.iter().map(|row| cells(row, &columns)).collect();
    let widths = widths(&columns, &grid);

    let mut out = String::new();
    out.push_str(&line(&columns, &widths));
    out.push_str(&rule(&widths));
    for row in &grid {
        out.push_str(&line(row, &widths));
    }
    out.push_str(&format!("\n{}\n", count(rows.len())));
    out
}

/// Every column any row carries. Rows are JSON objects, so the keys arrive
/// sorted and stay in step with the JSON rendering of the same result.
fn columns(rows: &[Value]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for row in rows {
        if let Value::Object(map) = row {
            names.extend(map.keys().cloned());
        }
    }
    names.into_iter().collect()
}

/// One row's cells, in column order. A row missing a column renders it as
/// `NULL`, which is what a missing value is.
fn cells(row: &Value, columns: &[String]) -> Vec<String> {
    columns
        .iter()
        .map(|column| cell(row.get(column).unwrap_or(&Value::Null)))
        .collect()
}

/// One value as text: flattened onto a single line, then cut short if it
/// would dominate the table.
fn cell(value: &Value) -> String {
    let text = match value {
        Value::Null => NULL.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    truncate(&flatten(&text))
}

/// Escape the control characters that would break the layout.
///
/// A `content` cell holds a whole file, newlines and all. Left as they are,
/// one row spans as many lines as its longest value and no column lines up
/// with any other -- the table stops being a table. `--format json` is the
/// way to get the bytes back unaltered.
fn flatten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => out.push_str(&format!("\\u{{{:x}}}", other as u32)),
            other => out.push(other),
        }
    }
    out
}

/// Cut `text` to [`MAX_CELL`] characters, marking that something was dropped.
/// Counts characters rather than bytes so a multi-byte value is not split
/// mid-character.
fn truncate(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(MAX_CELL).collect();
    match chars.next() {
        Some(_) => format!("{head}{ELLIPSIS}"),
        None => head,
    }
}

/// How wide each column has to be to hold its header and every cell under it.
fn widths(columns: &[String], grid: &[Vec<String>]) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            grid.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain(std::iter::once(column.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect()
}

/// One rendered row. The last column is not padded, so nothing carries
/// trailing whitespace.
fn line(cells: &[String], widths: &[usize]) -> String {
    let padded: Vec<String> = cells
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(index, (cell, width))| match index == cells.len() - 1 {
            true => cell.clone(),
            false => pad(cell, *width),
        })
        .collect();
    format!("{}\n", padded.join(GUTTER))
}

/// The rule under the header.
fn rule(widths: &[usize]) -> String {
    let dashes: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    format!("{}\n", dashes.join(GUTTER))
}

fn pad(cell: &str, width: usize) -> String {
    let spaces = width.saturating_sub(cell.chars().count());
    format!("{cell}{}", " ".repeat(spaces))
}

/// The footer. Reading rows with eyes means wanting the count without
/// counting them.
fn count(rows: usize) -> String {
    match rows {
        1 => "1 row".to_string(),
        other => format!("{other} rows"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a row object from its columns.
    ///
    /// `Value` builds an object from any iterator of pairs, which keeps this
    /// to the module's own imports -- a unit test reaching for
    /// `serde_json::json!` would be reaching outside the unit.
    fn row<const N: usize>(columns: [(&str, Value); N]) -> Value {
        Value::from_iter(columns.map(|(name, value)| (name.to_string(), value)))
    }

    fn rendered<const N: usize>(rows: [Value; N]) -> String {
        render(&rows)
    }

    #[test]
    fn an_empty_result_says_so() {
        // Nothing at all is indistinguishable from output that went missing.
        assert_eq!(render(&[]), "(no rows)\n");
    }

    #[test]
    fn rows_with_no_columns_say_so_too() {
        // A `SELECT` of nothing is still nothing to lay out.
        assert_eq!(rendered([row([]), row([])]), "(no rows)\n");
    }

    #[test]
    fn a_single_row_renders_header_rule_and_value() {
        assert_eq!(
            rendered([row([("n", Value::from(1))])]),
            "n\n-\n1\n\n1 row\n",
            "header, rule, value, then the count"
        );
    }

    #[test]
    fn columns_are_padded_to_their_widest_cell() {
        let table = rendered([
            row([("basename", Value::from("a.md")), ("size", Value::from(6))]),
            row([
                ("basename", Value::from("bb.md")),
                ("size", Value::from(10)),
            ]),
        ]);

        assert_eq!(
            table,
            "basename  size\n--------  ----\na.md      6\nbb.md     10\n\n2 rows\n"
        );
    }

    #[test]
    fn a_header_wider_than_its_values_sets_the_width() {
        let table = rendered([row([("basename", Value::from("a"))])]);

        assert_eq!(table, "basename\n--------\na\n\n1 row\n");
    }

    #[test]
    fn the_last_column_carries_no_trailing_padding() {
        // Padding the final column leaves invisible whitespace on every line.
        let table = rendered([
            row([("a", Value::from("xx")), ("b", Value::from("y"))]),
            row([("a", Value::from("x")), ("b", Value::from("yy"))]),
        ]);

        for line in table.lines() {
            assert_eq!(line, line.trim_end(), "{line:?} has trailing whitespace");
        }
    }

    #[test]
    fn every_column_any_row_carries_is_shown() {
        // A ragged result must not silently drop the columns only some rows
        // have.
        let table = rendered([row([("a", Value::from(1))]), row([("b", Value::from(2))])]);

        assert!(table.starts_with("a     b\n"), "{table:?}");
    }

    #[test]
    fn a_row_missing_a_column_renders_it_as_null() {
        let table = rendered([
            row([("a", Value::from(1))]),
            row([("a", Value::from(2)), ("b", Value::from(3))]),
        ]);

        assert!(table.contains("1  NULL"), "{table:?}");
    }

    #[test]
    fn a_null_is_named_rather_than_blank() {
        // A blank cell cannot be told apart from an empty string.
        let table = rendered([row([("a", Value::Null)])]);

        assert!(table.contains(NULL), "{table:?}");
    }

    #[test]
    fn an_empty_string_stays_empty() {
        // The other half of the same distinction.
        let table = rendered([row([("a", Value::from(""))])]);

        assert!(!table.contains(NULL), "{table:?}");
    }

    #[test]
    fn a_string_renders_unquoted() {
        // JSON quotes are noise once the shape is a table.
        let table = rendered([row([("a", Value::from("hello"))])]);

        assert!(table.contains("hello"), "{table:?}");
        assert!(!table.contains('"'), "{table:?}");
    }

    #[test]
    fn numbers_and_booleans_render_as_themselves() {
        let table = rendered([row([
            ("i", Value::from(42)),
            ("f", Value::from(1.5)),
            ("b", Value::from(true)),
        ])]);

        assert!(table.contains("42"), "{table:?}");
        assert!(table.contains("1.5"), "{table:?}");
        assert!(table.contains("true"), "{table:?}");
    }

    #[test]
    fn a_nested_value_falls_back_to_its_json() {
        // Nothing in a result should render as a debug placeholder.
        let table = rendered([row([("a", Value::from(vec![1, 2]))])]);

        assert!(table.contains("[1,2]"), "{table:?}");
    }

    #[test]
    fn a_long_cell_is_cut_short() {
        // One `content` column holding a whole file would otherwise set the
        // width of every row.
        let long = "x".repeat(MAX_CELL + 10);
        let table = rendered([row([("content", Value::from(long))])]);

        assert!(table.contains(ELLIPSIS), "{table:?}");
        for line in table.lines() {
            assert!(
                line.chars().count() <= MAX_CELL + 1,
                "{line:?} is wider than the cap"
            );
        }
    }

    #[test]
    fn a_cell_exactly_at_the_cap_is_left_alone() {
        // Off by one here marks an untouched value as truncated.
        let exact = "x".repeat(MAX_CELL);
        let table = rendered([row([("a", Value::from(exact))])]);

        assert!(!table.contains(ELLIPSIS), "{table:?}");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // Cutting at a byte offset would split a multi-byte character and
        // produce invalid UTF-8 in the middle of the table.
        let wide = "é".repeat(MAX_CELL + 5);

        let cut = truncate(&wide);

        assert_eq!(cut.chars().count(), MAX_CELL + 1, "cap plus the ellipsis");
    }

    #[test]
    fn a_newline_in_a_cell_is_escaped_rather_than_wrapping() {
        // A `content` cell holds a whole file. Left alone, its newlines make
        // one row span several lines and nothing lines up.
        let table = rendered([row([("a", Value::from("one\ntwo")), ("b", Value::from(1))])]);

        assert_eq!(
            table.lines().count(),
            5,
            "header, rule, one row, blank, count: {table:?}"
        );
        assert!(table.contains(r"one\ntwo"), "{table:?}");
    }

    #[test]
    fn carriage_returns_and_tabs_are_escaped_too() {
        // A bare `\r` would rewind the cursor to the start of the line and
        // overwrite the row that was already drawn.
        let table = rendered([row([("a", Value::from("x\ry\tz"))])]);

        assert!(table.contains(r"x\ry\tz"), "{table:?}");
    }

    #[test]
    fn other_control_characters_are_escaped_by_code_point() {
        // An escape byte in a value would otherwise be handed to the terminal
        // as an instruction.
        assert_eq!(flatten("a\u{1b}b"), r"a\u{1b}b");
    }

    #[test]
    fn ordinary_text_passes_through_flattening_unchanged() {
        assert_eq!(flatten("héllo wörld"), "héllo wörld");
    }

    #[test]
    fn one_row_is_singular() {
        // "1 rows" is the kind of detail that makes output look unfinished.
        assert_eq!(count(1), "1 row");
    }

    #[test]
    fn every_other_count_is_plural() {
        assert_eq!(count(0), "0 rows");
        assert_eq!(count(2), "2 rows");
    }
}
