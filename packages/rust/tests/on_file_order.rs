//! Config tables' `on-file` commands run in declaration order: one table's
//! command finishes before the next table's starts, whatever order the walk
//! finds their files in. Across repeated configs, the order is the configs'.
//!
//! Unix-only: the fixtures shell out to `sh`.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use dirsql::DirSQL;
use dirsql::watcher::FileEvent;
use tempfile::TempDir;

/// Each run logs its start, lingers long enough for an overlapping run to
/// start too, then logs its end.
const RECORD: &str = "#!/bin/sh\nname=$1\nprintf 'start %s\\n' \"$name\" >> \"$LOG\"\nsleep 0.3\nprintf 'end %s\\n' \"$name\" >> \"$LOG\"\n";

/// A config whose tables are declared in `names` order, each over
/// `data/<name>/*.txt`, logging to `log`.
fn write_config(dir: &Path, names: &[&str], log: &Path) -> PathBuf {
    fs::write(dir.join("record.sh"), RECORD).unwrap();
    let mut toml = String::new();
    for name in names {
        let data = dir.join("data").join(name);
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("one.txt"), "x\n").unwrap();
        toml.push_str(&format!(
            "[[table]]\nname = \"{name}\"\nddl = \"CREATE TABLE {name} (v TEXT)\"\nglob = \"data/{name}/*.txt\"\non-file = \"env LOG='{}' sh record.sh {name}\"\n\n",
            log.display()
        ));
    }
    let path = dir.join(".dirsql.toml");
    fs::write(&path, toml).unwrap();
    path
}

fn runs(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn in_order(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|name| [format!("start {name}"), format!("end {name}")])
        .collect()
}

#[test]
fn startup_runs_commands_in_declaration_order_not_walk_order() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("order.log");
    let config = write_config(dir.path(), &["zeta", "alpha"], &log);

    let db = DirSQL::builder()
        .root(dir.path())
        .config(&config)
        .build()
        .unwrap();
    assert!(db.scan_failures().is_empty(), "{:?}", db.scan_failures());

    assert_eq!(runs(&log), in_order(&["zeta", "alpha"]));
}

#[test]
fn startup_with_a_cache_runs_commands_in_declaration_order() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("order.log");
    let config = write_config(dir.path(), &["zeta", "alpha"], &log);
    let cache = TempDir::new().unwrap();

    DirSQL::builder()
        .root(dir.path())
        .config(&config)
        .persist(Some(cache.path().join("cache.db")))
        .build()
        .unwrap();

    assert_eq!(runs(&log), in_order(&["zeta", "alpha"]));
}

#[test]
fn repeated_configs_run_commands_in_argv_order() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let log_dir = TempDir::new().unwrap();
    let log = log_dir.path().join("order.log");
    let first_config = write_config(first.path(), &["zeta"], &log);
    let second_config = write_config(second.path(), &["alpha"], &log);

    DirSQL::builder()
        .root(first.path())
        .config(&first_config)
        .config(&second_config)
        .build()
        .unwrap();

    assert_eq!(runs(&log), in_order(&["zeta", "alpha"]));
}

#[test]
fn a_watch_refresh_over_several_tables_runs_commands_in_declaration_order() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("order.log");
    let config = write_config(dir.path(), &["zeta", "alpha"], &log);
    let db = DirSQL::builder()
        .root(dir.path())
        .config(&config)
        .build()
        .unwrap();
    fs::write(&log, "").unwrap();

    let alpha_file = dir.path().join("data/alpha/one.txt");
    let zeta_file = dir.path().join("data/zeta/one.txt");
    fs::write(&alpha_file, "y\n").unwrap();
    fs::write(&zeta_file, "y\n").unwrap();
    db.apply_file_events(vec![
        FileEvent::Modified(alpha_file),
        FileEvent::Modified(zeta_file),
    ]);

    assert_eq!(runs(&log), in_order(&["zeta", "alpha"]));
}
