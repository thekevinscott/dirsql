//! Minimal SQLite loadable extension for dirsql's integration tests.
//!
//! Registers a single deterministic scalar function,
//! `dirsql_testext_answer()`, returning 42. A test loads this extension
//! through dirsql's public config surface and calls the function to prove the
//! extension was actually loaded onto the connection.

use std::os::raw::{c_char, c_int};

use rusqlite::ffi;
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, Result};

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
        |_ctx| Ok(42_i64),
    )?;
    // `false` => not a persistent extension; unloaded with the connection.
    Ok(false)
}
