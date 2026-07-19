use std::ffi::c_int;

use rusqlite::vtab::{
    read_only_module, Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind,
    Values,
};
use rusqlite::{ffi, Connection, Result};

/// SQL module name a path-table is created with:
/// `CREATE VIRTUAL TABLE t USING dirsql_path('<root>', '<glob>')`.
pub const MODULE_NAME: &str = "dirsql_path";

/// The seven stat columns, in declaration order.
pub const STAT_COLUMNS: [&str; 7] = [
    "path", "basename", "dir", "ext", "size", "mtime", "ctime",
];

/// Schema a path-table declares to SQLite.
pub fn declared_schema() -> String {
    let cols = STAT_COLUMNS.join(", ");
    format!("CREATE TABLE x({cols}, content)")
}

/// Register the path-table module on `conn`.
pub fn load_module(conn: &Connection) -> Result<()> {
    let aux: Option<()> = None;
    conn.create_module(MODULE_NAME, read_only_module::<PathTab>(), aux)
}

#[repr(C)]
struct PathTab {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
}

unsafe impl<'vtab> VTab<'vtab> for PathTab {
    type Aux = ();
    type Cursor = PathTabCursor;

    fn connect(
        _db: &mut VTabConnection,
        _aux: Option<&()>,
        _args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
        };
        Ok((declared_schema(), vtab))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        info.set_estimated_cost(1000.);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<PathTabCursor> {
        Ok(PathTabCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            row_id: 0,
        })
    }
}

impl<'vtab> CreateVTab<'vtab> for PathTab {
    const KIND: VTabKind = VTabKind::Default;
}

#[repr(C)]
struct PathTabCursor {
    /// Base class. Must be first: `rust_open` hands this pointer straight to
    /// SQLite as a `sqlite3_vtab_cursor`, so anything ahead of it gets
    /// overwritten.
    base: ffi::sqlite3_vtab_cursor,
    row_id: i64,
}

unsafe impl VTabCursor for PathTabCursor {
    fn filter(&mut self, _idx_num: c_int, _idx_str: Option<&str>, _args: &Values<'_>) -> Result<()> {
        self.row_id = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.row_id += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        true
    }

    fn column(&self, _ctx: &mut Context, _i: c_int) -> Result<()> {
        Ok(())
    }

    fn rowid(&self) -> Result<i64> {
        Ok(self.row_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vtab behavior over a real directory is covered by `tests/vtab.rs`
    // (unit-lint isolation); only the pure schema helpers are tested here.

    #[test]
    fn stat_columns_are_the_seven_documented_names() {
        assert_eq!(
            STAT_COLUMNS,
            ["path", "basename", "dir", "ext", "size", "mtime", "ctime"]
        );
    }

    #[test]
    fn declared_schema_lists_every_stat_column() {
        let schema = declared_schema();
        for col in STAT_COLUMNS {
            assert!(schema.contains(col), "{col} missing from {schema}");
        }
    }

    #[test]
    fn declared_schema_declares_content_last() {
        let schema = declared_schema();
        let content_at = schema.find("content").unwrap();
        let last_stat_at = schema.find("ctime").unwrap();
        assert!(
            content_at > last_stat_at,
            "content must follow the stat columns: {schema}"
        );
    }

    #[test]
    fn module_name_is_stable() {
        assert_eq!(MODULE_NAME, "dirsql_path");
    }
}
