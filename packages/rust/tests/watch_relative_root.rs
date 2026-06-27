//! Regression test for #250: the file watcher must deliver `RowEvent`s when
//! `DirSQL` is constructed with a **relative** `root` (e.g. `DirSQL::new(".",
//! ...)`), not just an absolute one.
//!
//! Root cause: `start_watching()` handed the user-supplied (possibly relative)
//! `root` straight to `notify`, which has surprising behavior when watching
//! relative paths like `./`. The initial scan worked fine either way; only the
//! live watcher was broken. The CLI binary already canonicalized its root for
//! exactly this reason — the SDK did not. The fix watches a canonicalized
//! `watch_root` while keeping the user-supplied `root` for scanning,
//! `config()`, and `_path` output.
//!
//! `std::env::set_current_dir` mutates **process-global** state, so every test
//! in this file serializes through `CWD_LOCK` and restores the original cwd on
//! the way out (via the `CwdGuard` drop), even on panic.

use dirsql::{DirSQL, Table, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Serializes the process-global cwd across the tests in this file. A `OnceLock`
/// avoids pulling in a `lazy_static`/`once_cell` dependency just for the test.
fn cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Holds the cwd lock and the original working directory, restoring it on drop.
/// Recovers a poisoned lock: a prior test panicking while holding the guard
/// must not cascade-fail every later cwd test.
struct CwdGuard {
    _guard: MutexGuard<'static, ()>,
    original: PathBuf,
}

impl CwdGuard {
    fn enter(target: &std::path::Path) -> Self {
        let guard = cwd_lock().lock().unwrap_or_else(|p| p.into_inner());
        let original = std::env::current_dir().expect("read cwd");
        std::env::set_current_dir(target).expect("chdir into target");
        Self {
            _guard: guard,
            original,
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        // Best-effort restore; if this fails the process is already in a bad
        // state and later tests will surface it.
        let _ = std::env::set_current_dir(&self.original);
    }
}

fn items_table() -> Table {
    Table::new("CREATE TABLE items (name TEXT, _path TEXT)", "**/*.txt", |path| {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        vec![std::collections::HashMap::from([(
            "name".to_string(),
            Value::Text(content.trim().to_string()),
        )])]
    })
}

/// #250: a watcher built on a relative `root` (`"."`) must still emit an insert
/// `RowEvent` when a matching file is created after `start_watching()`.
///
/// Without the fix the watcher hands `"."` to `notify`, which never delivers
/// events, so this poll loop times out with zero events (RED). With the fix the
/// watcher canonicalizes the root, the create is observed, and an `Insert`
/// arrives (GREEN). The companion absolute-root assertion is intentionally
/// *not* a separate `#[test]` so it can't race for the cwd lock; it shares this
/// one to prove the two roots behave identically.
#[test]
fn watch_with_relative_root_emits_events() {
    let dir = tempfile::TempDir::new().unwrap();
    // Canonicalize the temp dir up front: on macOS `TempDir` lives under
    // `/var/...` which is a symlink to `/private/var/...`, and the post-fix
    // `_path` is computed by stripping the *canonical* watch root. Comparing
    // against a canonical base keeps the relative-path assertion portable.
    let canonical_dir = fs::canonicalize(dir.path()).unwrap();

    let _cwd = CwdGuard::enter(&canonical_dir);

    // Relative root: the exact shape from the bug report / docs examples.
    let db = DirSQL::new(".", vec![items_table()]).unwrap();
    db.start_watching().unwrap();

    // Give the watcher a moment to register before mutating the tree.
    std::thread::sleep(Duration::from_millis(250));
    fs::write(canonical_dir.join("apple.txt"), "apple").unwrap();

    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while events.is_empty() && Instant::now() < deadline {
        events.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }

    let insert = events
        .iter()
        .find(|e| matches!(e, dirsql::RowEvent::Insert { .. }));
    assert!(
        insert.is_some(),
        "relative-root watcher must emit an Insert event (#250); saw: {events:?}"
    );

    // The relative `_path` must be the root-relative file name, identical to
    // what an absolute root would have produced — proving the canonical
    // watch-root did not leak the absolute prefix into the event path.
    if let Some(dirsql::RowEvent::Insert { row, .. }) = insert {
        assert_eq!(
            row.get("_path"),
            Some(&Value::Text("apple.txt".to_string())),
            "_path must stay root-relative for a relative root"
        );
    }

    // And the row landed in the in-memory index.
    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(
        rows.iter().any(|r| matches!(
            r.get("name"),
            Some(Value::Text(name)) if name == "apple"
        )),
        "indexed rows should include the created file"
    );
}
