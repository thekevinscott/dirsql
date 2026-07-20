//! A path-table whose columns come from a parser command's row objects.

use std::ffi::c_int;

use rusqlite::vtab::{
    Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind, Values,
    read_only_module,
};
use rusqlite::{Connection, Result, ffi};

use crate::Value;

/// SQL module name a parsed path-table is created with.
pub const MODULE_NAME: &str = "dirsql_parsed";

/// Register the parsed path-table module on `conn`.
pub fn load_module(conn: &Connection) -> Result<()> {
    let aux: Option<()> = None;
    conn.create_module(MODULE_NAME, read_only_module::<ParsedTab>(), aux)
}

#[repr(C)]
struct ParsedTab {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
}

unsafe impl<'vtab> VTab<'vtab> for ParsedTab {
    type Aux = ();
    type Cursor = ParsedTabCursor;

    fn connect(
        _db: &mut VTabConnection,
        _aux: Option<&()>,
        _args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
        };
        Ok(("CREATE TABLE x(placeholder TEXT)".to_string(), vtab))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        info.set_estimated_cost(1000.);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<ParsedTabCursor> {
        Ok(ParsedTabCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            index: 0,
        })
    }
}

impl<'vtab> CreateVTab<'vtab> for ParsedTab {
    const KIND: VTabKind = VTabKind::Default;
}

#[repr(C)]
struct ParsedTabCursor {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab_cursor,
    index: usize,
}

unsafe impl VTabCursor for ParsedTabCursor {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _args: &Values<'_>,
    ) -> Result<()> {
        self.index = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.index += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        true
    }

    fn column(&self, ctx: &mut Context, _i: c_int) -> Result<()> {
        ctx.set_result(&Value::Null)
    }

    fn rowid(&self) -> Result<i64> {
        Ok(0)
    }
}
