//! Index-root derivation is owned by the runner, not the config file (#540).
//!
//! One uniform rule: the index root is the explicit `.root(...)` when given,
//! else the **process cwd**. The config file's parent directory plays no role
//! in root derivation. These tests exercise that rule against the SDK builder.
//!
//! `std::env::set_current_dir` mutates **process-global** state, so the cwd
//! test serializes through `CWD_LOCK` and restores the original cwd on the way
//! out (via the `CwdGuard` drop), even on panic.

use dirsql::{DirSQL, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

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
        let _ = std::env::set_current_dir(&self.original);
    }
}

/// A `.config(path)` with no explicit `.root()` still roots the index at the
/// **process cwd**, but a config table anchors at the config file's directory.
/// The cwd holds a decoy that must NOT be indexed; the config's parent holds
/// the file that must be.
#[test]
fn config_tables_anchor_at_the_config_directory_not_the_process_cwd() {
    let cwd_dir = tempfile::TempDir::new().unwrap();
    let cwd_dir = fs::canonicalize(cwd_dir.path()).unwrap();
    fs::write(cwd_dir.join("decoy.txt"), "x").unwrap();

    let cfg_dir = tempfile::TempDir::new().unwrap();
    fs::write(cfg_dir.path().join("in_config_dir.txt"), "x").unwrap();
    let cfg_path = cfg_dir.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.txt"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "{\"path\":\"%s\"}\n" "$rel"; done' sh {root}'''
"#,
    )
    .unwrap();

    let _cwd = CwdGuard::enter(&cwd_dir);
    let db = DirSQL::builder().config(&cfg_path).build().unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();

    assert_eq!(rows.len(), 1, "should index the config's directory");
    assert_eq!(rows[0]["path"], Value::Text("in_config_dir.txt".into()));
}

/// An explicit `.root(...)` still wins over the process cwd for the index root.
#[test]
fn explicit_root_wins_over_cwd() {
    let cwd_dir = tempfile::TempDir::new().unwrap();
    let cwd_dir = fs::canonicalize(cwd_dir.path()).unwrap();
    fs::write(cwd_dir.join("in_cwd.txt"), "x").unwrap();

    let root_dir = tempfile::TempDir::new().unwrap();
    fs::write(root_dir.path().join("in_root.txt"), "x").unwrap();
    let cfg_path = root_dir.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.txt"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "{\"path\":\"%s\"}\n" "$rel"; done' sh {root}'''
"#,
    )
    .unwrap();

    let _cwd = CwdGuard::enter(&cwd_dir);
    let db = DirSQL::builder()
        .root(root_dir.path())
        .config(&cfg_path)
        .build()
        .unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();

    assert_eq!(rows.len(), 1, "explicit root must win over cwd");
    assert_eq!(rows[0]["path"], Value::Text("in_root.txt".into()));
}
