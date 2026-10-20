//! Foreign keys checked once per unit of work. Tables fill in no fixed order,
//! so a unit -- the build, or one batch of watch events -- defers every
//! constraint to its COMMIT, and a COMMIT that finds an orphan rolls the unit
//! back and names the rows that broke it.

use rusqlite::Connection;
use rusqlite::types::ValueRef;

use crate::db::DbError;
use crate::sql_literal::{quote_identifier, quote_literal};

/// How many offending rows a violation message lists before summarizing.
const LISTED: usize = 10;

/// A unit of work that left rows referencing nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyViolation {
    /// The table holding the first offending row.
    pub table: String,
    /// The file that produced the first offending row, when one owns it.
    pub file: Option<String>,
    /// Every offending row, named.
    pub message: String,
}

/// Defer every constraint in the open transaction to its COMMIT.
pub(crate) fn defer(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA defer_foreign_keys = ON")
}

/// Open a transaction whose constraints are checked at its COMMIT.
pub(crate) fn begin(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN")?;
    defer(conn)
}

/// Commit the open transaction. A COMMIT SQLite refuses rolls the
/// transaction back; one refused for an orphaned row reports it as a
/// [`DbError::ForeignKey`].
pub(crate) fn commit(conn: &Connection) -> Result<(), DbError> {
    let Err(err) = conn.execute_batch("COMMIT") else {
        return Ok(());
    };
    if conn.is_autocommit() {
        return Err(DbError::Sqlite(err));
    }
    let violation = describe(conn).ok().flatten();
    conn.execute_batch("ROLLBACK")?;
    Err(violation.map_or(DbError::Sqlite(err), DbError::ForeignKey))
}

/// Run `unit` so that it either applies whole or not at all, inside or
/// outside an open transaction.
pub(crate) fn atomically<T, E: From<rusqlite::Error>>(
    conn: &Connection,
    unit: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    conn.execute_batch("SAVEPOINT dirsql_unit")?;
    match unit() {
        Ok(value) => {
            conn.execute_batch("RELEASE dirsql_unit")?;
            Ok(value)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK TO dirsql_unit; RELEASE dirsql_unit")?;
            Err(e)
        }
    }
}

/// The rows violating a foreign key right now, or `None` when there are none.
fn describe(conn: &Connection) -> rusqlite::Result<Option<ForeignKeyViolation>> {
    let mut stmt =
        conn.prepare("SELECT \"table\", rowid, parent, fkid FROM pragma_foreign_key_check")?;
    let found = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let Some((table, rowid, _, _)) = found.first() else {
        return Ok(None);
    };
    let table = table.clone();
    let file = match rowid {
        Some(rowid) => owner(conn, &table, *rowid)?,
        None => None,
    };
    let mut rows = Vec::new();
    for (child, rowid, parent, fkid) in found.iter().take(LISTED) {
        rows.push(name_row(conn, child, *rowid, parent, *fkid)?);
    }
    let mut message = format!("foreign key violation: {}", rows.join("; "));
    if found.len() > LISTED {
        message.push_str(&format!(" (and {} more)", found.len() - LISTED));
    }
    Ok(Some(ForeignKeyViolation {
        table,
        file,
        message,
    }))
}

/// One offending row: its table, the file that produced it, and the values
/// that match nothing in `parent`.
fn name_row(
    conn: &Connection,
    child: &str,
    rowid: Option<i64>,
    parent: &str,
    fkid: i64,
) -> rusqlite::Result<String> {
    let columns = conn
        .prepare("SELECT \"from\" FROM pragma_foreign_key_list(?1) WHERE id = ?2 ORDER BY seq")?
        .query_map(rusqlite::params![child, fkid], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut named = format!("`{child}` row");
    let Some(rowid) = rowid else {
        return Ok(format!("{named} matches no row in `{parent}`"));
    };
    if let Some(file) = owner(conn, child, rowid)? {
        named.push_str(&format!(" from `{file}`"));
    }
    let select = columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let values = conn.query_row(
        &format!(
            "SELECT {select} FROM {} WHERE rowid = ?1",
            quote_identifier(child)
        ),
        [rowid],
        |row| {
            (0..columns.len())
                .map(|i| row.get_ref(i).map(render))
                .collect::<rusqlite::Result<Vec<_>>>()
        },
    )?;
    let pairs = columns
        .iter()
        .zip(values)
        .map(|(column, value)| format!("{column} = {value}"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "{named} has {pairs}, which matches no row in `{parent}`"
    ))
}

/// The file whose rows include `rowid` of `table`. A per-table hook's rows
/// are owned by the empty path, which names no file.
fn owner(conn: &Connection, table: &str, rowid: i64) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT file_path FROM _dirsql_internal_rows WHERE table_name = ?1 AND rowid_ref = ?2 \
         UNION ALL \
         SELECT file_path FROM _dirsql_internal_ranges \
         WHERE table_name = ?1 AND ?2 BETWEEN first_rowid AND last_rowid \
         LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![table, rowid])?;
    let file: Option<String> = rows.next()?.map(|row| row.get(0)).transpose()?;
    Ok(file.filter(|file| !file.is_empty()))
}

fn render(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => quote_literal(&String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => format!("<{} byte blob>", b.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(child_ddl: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE _dirsql_internal_rows (table_name TEXT, file_path TEXT, row_index INTEGER, rowid_ref INTEGER);
             CREATE TABLE _dirsql_internal_ranges (table_name TEXT, file_path TEXT, first_rowid INTEGER, last_rowid INTEGER);",
        )
        .unwrap();
        conn.execute_batch("CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        conn.execute_batch(child_ddl).unwrap();
        conn
    }

    const CHILD: &str = "CREATE TABLE child (name TEXT, pid INTEGER REFERENCES parent(id))";

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn violation(err: DbError) -> ForeignKeyViolation {
        match err {
            DbError::ForeignKey(v) => v,
            other => panic!("expected a foreign key violation, got {other:?}"),
        }
    }

    #[test]
    fn a_unit_filled_child_first_commits() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO child VALUES ('kid', 1); INSERT INTO parent VALUES (1, 'a')",
        )
        .unwrap();
        commit(&conn).unwrap();
        assert!(conn.is_autocommit());
        assert_eq!(count(&conn, "child"), 1);
    }

    #[test]
    fn an_orphan_rolls_the_unit_back_and_names_the_row_and_its_file() {
        let conn = family(CHILD);
        conn.execute_batch("INSERT INTO parent VALUES (1, 'a')")
            .unwrap();
        begin(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO parent VALUES (2, 'b');
             INSERT INTO child VALUES ('kid', 9);
             INSERT INTO _dirsql_internal_ranges VALUES ('child', 'kids/a.txt', 1, 1);",
        )
        .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.table, "child");
        assert_eq!(v.file.as_deref(), Some("kids/a.txt"));
        assert_eq!(
            v.message,
            "foreign key violation: `child` row from `kids/a.txt` has pid = 9, \
             which matches no row in `parent`"
        );
        assert!(conn.is_autocommit());
        assert_eq!(count(&conn, "parent"), 1);
        assert_eq!(count(&conn, "child"), 0);
    }

    #[test]
    fn a_row_owned_one_at_a_time_names_its_file() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO child VALUES ('kid', 9);
             INSERT INTO _dirsql_internal_rows VALUES ('child', 'kids/b.txt', 0, 1);",
        )
        .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.file.as_deref(), Some("kids/b.txt"));
    }

    #[test]
    fn a_row_a_per_table_hook_owns_is_named_without_a_file() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO child VALUES ('kid', 9);
             INSERT INTO _dirsql_internal_ranges VALUES ('child', '', 1, 1);",
        )
        .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.file, None);
        assert!(
            v.message
                .starts_with("foreign key violation: `child` row has"),
            "{}",
            v.message
        );
    }

    #[test]
    fn a_row_no_file_owns_is_named_without_one() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        conn.execute_batch("INSERT INTO child VALUES ('kid', 9)")
            .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.file, None);
        assert_eq!(
            v.message,
            "foreign key violation: `child` row has pid = 9, which matches no row in `parent`"
        );
    }

    #[test]
    fn a_row_without_a_rowid_is_named_by_its_table() {
        let conn = family(
            "CREATE TABLE child (name TEXT PRIMARY KEY, pid INTEGER REFERENCES parent(id)) \
             WITHOUT ROWID",
        );
        begin(&conn).unwrap();
        conn.execute_batch("INSERT INTO child VALUES ('kid', 9)")
            .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.file, None);
        assert_eq!(
            v.message,
            "foreign key violation: `child` row matches no row in `parent`"
        );
    }

    #[test]
    fn many_orphans_are_listed_up_to_a_limit() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        for pid in 0..12 {
            conn.execute("INSERT INTO child VALUES ('kid', ?1)", [100 + pid])
                .unwrap();
        }
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.message.matches("matches no row").count(), LISTED);
        assert!(v.message.ends_with(" (and 2 more)"), "{}", v.message);
    }

    #[test]
    fn orphans_up_to_the_limit_are_all_listed() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        for pid in 100..100 + LISTED {
            conn.execute("INSERT INTO child VALUES ('kid', ?1)", [pid])
                .unwrap();
        }
        let v = violation(commit(&conn).unwrap_err());
        assert_eq!(v.message.matches("matches no row").count(), LISTED);
        assert!(!v.message.contains("more)"), "{}", v.message);
    }

    #[test]
    fn a_composite_key_names_every_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE p (a TEXT, b REAL, c BLOB, PRIMARY KEY (a, b, c));
             CREATE TABLE ch (a TEXT, b REAL, c BLOB, d INTEGER,
                FOREIGN KEY (a, b, c) REFERENCES p (a, b, c));",
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TABLE _dirsql_internal_rows (table_name TEXT, file_path TEXT, row_index INTEGER, rowid_ref INTEGER);
             CREATE TABLE _dirsql_internal_ranges (table_name TEXT, file_path TEXT, first_rowid INTEGER, last_rowid INTEGER);",
        )
        .unwrap();
        begin(&conn).unwrap();
        conn.execute_batch("INSERT INTO ch VALUES ('o''k', 1.5, x'0102', NULL)")
            .unwrap();
        let v = violation(commit(&conn).unwrap_err());
        assert!(
            v.message.contains("a = 'o''k', b = 1.5, c = <2 byte blob>"),
            "{}",
            v.message
        );
    }

    #[test]
    fn a_null_value_renders_as_null() {
        assert_eq!(render(ValueRef::Null), "NULL");
    }

    #[test]
    fn a_commit_without_a_transaction_reports_sqlite() {
        let conn = family(CHILD);
        assert!(matches!(commit(&conn), Err(DbError::Sqlite(_))));
    }

    #[test]
    fn atomically_applies_a_unit_that_succeeds() {
        let conn = family(CHILD);
        let out: rusqlite::Result<i32> = atomically(&conn, || {
            conn.execute_batch("INSERT INTO parent VALUES (1, 'a')")?;
            Ok(7)
        });
        assert_eq!(out.unwrap(), 7);
        assert_eq!(count(&conn, "parent"), 1);
    }

    #[test]
    fn atomically_drops_a_unit_that_fails_inside_a_transaction() {
        let conn = family(CHILD);
        begin(&conn).unwrap();
        conn.execute_batch("INSERT INTO parent VALUES (1, 'a')")
            .unwrap();
        let out: rusqlite::Result<()> = atomically(&conn, || {
            conn.execute_batch("INSERT INTO parent VALUES (2, 'b')")?;
            conn.execute_batch("INSERT INTO nowhere VALUES (1)")
        });
        assert!(out.is_err());
        assert!(!conn.is_autocommit());
        commit(&conn).unwrap();
        assert_eq!(count(&conn, "parent"), 1);
    }
}
