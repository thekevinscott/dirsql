//! Integration tests for file names that are not valid Unicode, through the
//! SDK: each keeps its own bytes in a path-table's path columns, and a
//! table's `on-file` command receives it as it is on disk. Real filesystem,
//! real SQLite, a real Python hook.

use std::ffi::OsString;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// A file name holding `unit` where a character should be: a byte that is
/// not UTF-8 on Unix, an unpaired surrogate on Windows.
fn not_unicode(unit: u16) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let byte = u8::try_from(unit & 0xff).unwrap();
        OsString::from_vec([b"ok".as_slice(), &[byte], b"name.json"].concat())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let wide: Vec<u16> = "ok"
            .encode_utf16()
            .chain([unit])
            .chain("name.json".encode_utf16())
            .collect();
        OsString::from_wide(&wide)
    }
}

fn hex(name: &OsString) -> String {
    name.as_encoded_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn fixture() -> (TempDir, [OsString; 2]) {
    let root = TempDir::new().unwrap();
    let names = [not_unicode(0xD800 | 0xff), not_unicode(0xD800 | 0xfe)];
    std::fs::write(root.path().join(&names[0]), "A").unwrap();
    std::fs::write(root.path().join(&names[1]), "BB").unwrap();
    (root, names)
}

fn texts(db: &DirSQL, sql: &str, column: &str) -> Vec<String> {
    db.query(sql)
        .unwrap()
        .iter()
        .map(|row| match &row[column] {
            Value::Text(s) => s.clone(),
            other => panic!("{column} was not text: {other:?}"),
        })
        .collect()
}

#[test]
fn a_path_table_keeps_each_name_distinct() {
    let (root, names) = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    assert_eq!(
        texts(&db, "SELECT hex(path) AS p FROM './*' ORDER BY size", "p"),
        [hex(&names[0]), hex(&names[1])]
    );
}

#[test]
fn reading_such_a_path_as_text_fails_naming_the_column() {
    let (root, _) = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    let error = db.query("SELECT path FROM './*'").unwrap_err();
    assert_eq!(
        error.to_string(),
        "column \"path\" holds text that is not valid UTF-8"
    );
}

#[test]
fn an_on_file_command_receives_each_name_as_it_is_on_disk() {
    let (root, names) = fixture();
    std::fs::write(
        root.path().join("hook.py"),
        "import json, os, sys\n\
         for p in sys.argv[1:]:\n    \
         print(json.dumps({\"found\": os.path.isfile(p), \
         \"name\": os.fsencode(os.path.basename(p)).hex().upper()}))\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("dirsql.toml"),
        format!(
            "[[table]]\nname = \"found\"\nddl = \"CREATE TABLE found (found INTEGER, name TEXT)\"\n\
             glob = \"*.json\"\non-file = \"{} hook.py\"\n",
            python()
        ),
    )
    .unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join("dirsql.toml"))
        .build()
        .unwrap();
    // A query's re-read rebuilds hook paths from the index's text keys,
    // which cannot hold these names; this asserts what the build hands
    // the command.
    db.set_fresh_reads(false);
    assert_eq!(
        texts(
            &db,
            "SELECT found || ':' || name AS f FROM found ORDER BY name DESC",
            "f"
        ),
        [
            format!("1:{}", hex(&names[0])),
            format!("1:{}", hex(&names[1]))
        ]
    );
}
