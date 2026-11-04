//! A file the scan finds as a regular file but that is something else by the
//! time its content is read yields NULL content, like a file deleted in that
//! window: never a hung query, an endless read, or a device's bytes. Real temp
//! trees, real SQLite, a raw connection so the scan and the read run as two
//! statements with the swap in between.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use dirsql::vtab::{StatementScope, load_module};
use rusqlite::Connection;
use tempfile::TempDir;

/// Long enough for any read of a one-byte file; a blocked open never ends.
const READ_BOUND: Duration = Duration::from_secs(30);

/// What `kind` replaces the file at `path` with.
fn swap(path: &Path, kind: &str) {
    fs::remove_file(path).unwrap();
    match kind {
        "directory" => fs::create_dir(path).unwrap(),
        #[cfg(unix)]
        "fifo" => rustix::fs::mknodat(
            rustix::fs::CWD,
            path,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o644),
            0,
        )
        .unwrap(),
        #[cfg(unix)]
        "device" => std::os::unix::fs::symlink("/dev/null", path).unwrap(),
        #[cfg(windows)]
        "device" => std::os::windows::fs::symlink_file(r"\\.\NUL", path).unwrap(),
        other => panic!("no swap for {other}"),
    }
}

/// The non-regular entries this platform can put at a path.
fn kinds() -> Vec<&'static str> {
    let mut kinds = vec!["directory", "device"];
    if cfg!(unix) {
        kinds.push("fifo");
    }
    kinds
}

/// `b.txt`'s content after the scan saw it as a regular file and it was then
/// replaced by `kind`, or `None` when the read did not return.
fn content_after_swap(kind: &'static str) -> Option<Option<String>> {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.txt"), "a").unwrap();
    let b: PathBuf = root.path().join("b.txt");
    fs::write(&b, "b").unwrap();
    let (tx, rx) = mpsc::channel();
    let dir = root.path().to_path_buf();
    thread::spawn(move || {
        let conn = Connection::open_in_memory().unwrap();
        load_module(&conn, StatementScope::new()).unwrap();
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE t USING dirsql_path('{}', '*.txt', '', 'gitignore')",
            dir.display()
        ))
        .unwrap();
        let scanned: i64 = conn
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(scanned, 2, "the scan sees both regular files");
        swap(&b, kind);
        let content: Option<String> = conn
            .query_row("SELECT content FROM t WHERE path = 'b.txt'", [], |r| {
                r.get(0)
            })
            .unwrap();
        tx.send(content).unwrap();
    });
    rx.recv_timeout(READ_BOUND).ok()
}

#[test]
fn a_file_swapped_for_a_non_regular_entry_after_the_scan_reads_as_null() {
    let read: Vec<_> = kinds()
        .into_iter()
        .map(|kind| (kind, content_after_swap(kind)))
        .collect();
    let expected: Vec<_> = kinds().into_iter().map(|kind| (kind, Some(None))).collect();
    assert_eq!(
        read, expected,
        "b.txt was swapped after the scan; its content must read as NULL promptly \
         (None = the read never returned)"
    );
}
