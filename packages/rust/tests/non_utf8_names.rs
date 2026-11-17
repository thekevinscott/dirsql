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

fn found_table(root: &std::path::Path) -> DirSQL {
    std::fs::write(
        root.join("hook.py"),
        "import json, os, sys\n\
         for p in sys.argv[1:]:\n    \
         print(json.dumps({\"found\": os.path.isfile(p), \
         \"name\": os.fsencode(os.path.basename(p)).hex().upper()}))\n",
    )
    .unwrap();
    std::fs::write(
        root.join("dirsql.toml"),
        format!(
            "[[table]]\nname = \"found\"\nddl = \"CREATE TABLE found (found INTEGER, name TEXT)\"\n\
             glob = \"*.json\"\non-file = \"{} hook.py\"\n",
            python()
        ),
    )
    .unwrap();
    DirSQL::builder()
        .root(root)
        .config(root.join("dirsql.toml"))
        .build()
        .unwrap()
}

fn found(db: &DirSQL) -> Vec<String> {
    texts(
        db,
        "SELECT found || ':' || name AS f FROM found ORDER BY name DESC",
        "f",
    )
}

#[cfg(unix)]
fn await_found(db: &DirSQL, expected: &[String]) {
    let mut last = found(db);
    for _ in 0..1000 {
        if last == expected {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        last = found(db);
    }
    assert_eq!(last, expected);
}

#[test]
fn an_on_file_command_receives_each_name_as_it_is_on_disk() {
    let (root, names) = fixture();
    let db = found_table(root.path());
    assert_eq!(
        found(&db),
        [
            format!("1:{}", hex(&names[0])),
            format!("1:{}", hex(&names[1]))
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_watched_change_reruns_the_command_over_each_name_as_it_is_on_disk() {
    let (root, names) = fixture();
    let db = found_table(root.path());
    std::fs::write(root.path().join("later.json"), "C").unwrap();
    await_found(
        &db,
        &[
            format!("1:{}", hex(&names[0])),
            format!("1:{}", hex(&names[1])),
            format!("1:{}", hex(&OsString::from("later.json"))),
        ],
    );
}

#[cfg(unix)]
#[test]
fn files_created_under_a_watch_keep_each_name_distinct() {
    let root = TempDir::new().unwrap();
    let db = found_table(root.path());
    let names = [not_unicode(0xff), not_unicode(0xfe)];
    std::fs::write(root.path().join(&names[0]), "A").unwrap();
    std::fs::write(root.path().join(&names[1]), "BB").unwrap();
    await_found(
        &db,
        &[
            format!("1:{}", hex(&names[0])),
            format!("1:{}", hex(&names[1])),
        ],
    );
}
