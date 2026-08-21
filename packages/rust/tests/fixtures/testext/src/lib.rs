//! Minimal SQLite loadable extension for dirsql's integration tests.
//!
//! Registers a deterministic scalar function, `dirsql_testext_answer()`,
//! returning 42, and a virtual table module, `dirsql_testext_vtab`, serving a
//! single row of that same answer. A test loads this extension through
//! dirsql's public config surface and calls the function to prove the
//! extension was actually loaded onto the connection.
//!
//! The module is what a *built-in* one (FTS5) cannot stand in for: a table
//! created with it is only droppable on a connection that has this extension
//! loaded, so a cache holding one exercises the sweep's real dependency on
//! the module set.

use std::ffi::c_int;
use std::os::raw::c_char;

use rusqlite::ffi;
use rusqlite::functions::FunctionFlags;
use rusqlite::vtab::{
    read_only_module, Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind,
    Values,
};
use rusqlite::{Connection, Result};

/// The answer both the scalar function and the virtual table serve.
const ANSWER: i64 = 42;

/// SQL module name the extension's virtual table is created with.
const MODULE_NAME: &str = "dirsql_testext_vtab";

/// SQLite loadable-extension entry point.
///
/// # Safety
/// Called by SQLite with a valid database handle and API-routines pointer
/// during `sqlite3_load_extension`.
#[no_mangle]
pub unsafe extern "C" fn sqlite3_extension_init(
    db: *mut ffi::sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut ffi::sqlite3_api_routines,
) -> c_int {
    Connection::extension_init2(db, pz_err_msg, p_api, extension_init)
}

fn extension_init(db: Connection) -> Result<bool> {
    db.create_scalar_function(
        "dirsql_testext_answer",
        0,
        FunctionFlags::SQLITE_DETERMINISTIC,
        |_ctx| Ok(ANSWER),
    )?;
    let aux: Option<()> = None;
    db.create_module(MODULE_NAME, read_only_module::<AnswerTab>(), aux)?;
    // `false` => not a persistent extension; unloaded with the connection.
    Ok(false)
}

#[repr(C)]
struct AnswerTab {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
}

unsafe impl<'vtab> VTab<'vtab> for AnswerTab {
    type Aux = ();
    type Cursor = AnswerCursor;

    fn connect(
        _db: &mut VTabConnection,
        _aux: Option<&()>,
        _args: &[&[u8]],
    ) -> Result<(String, Self)> {
        Ok((
            "CREATE TABLE x(n INTEGER)".to_owned(),
            Self {
                base: ffi::sqlite3_vtab::default(),
            },
        ))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        info.set_estimated_cost(1.);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<AnswerCursor> {
        Ok(AnswerCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            row: 0,
        })
    }
}

impl<'vtab> CreateVTab<'vtab> for AnswerTab {
    const KIND: VTabKind = VTabKind::Default;
}

#[repr(C)]
struct AnswerCursor {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab_cursor,
    row: i64,
}

unsafe impl VTabCursor for AnswerCursor {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _args: &Values<'_>,
    ) -> Result<()> {
        self.row = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.row += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        self.row >= 1
    }

    fn column(&self, ctx: &mut Context, _i: c_int) -> Result<()> {
        ctx.set_result(&ANSWER)
    }

    fn rowid(&self) -> Result<i64> {
        Ok(self.row)
    }
}
