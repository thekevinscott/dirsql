//! Each way a change could slip past the watcher, read once the index has
//! caught up.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, await_fresh_scan, await_lines, lines_of, snapshot};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn a_same_size_edit_within_one_mtime_tick_is_read() {
    let tree = Tree::new();
    tree.write("tick.txt", "before");
    let db = tree.open();
    await_lines(&db, "tick.txt", &["before"]);
    let mut same_tick = 0;
    for i in 0..2000 {
        let (_, mtime) = tree.times("tick.txt");
        let content = format!("x{i:05}");
        tree.write("tick.txt", &content);
        if tree.times("tick.txt").1 == mtime {
            same_tick += 1;
        }
        await_lines(&db, "tick.txt", &[content]);
    }
    eprintln!("{same_tick} of 2000 edits kept the previous mtime");
}

#[test]
fn an_edit_whose_times_are_put_back_is_read() {
    let tree = Tree::new();
    tree.write("touched.txt", "before");
    let times = tree.times("touched.txt");
    let db = tree.open();
    await_lines(&db, "touched.txt", &["before"]);
    tree.write("touched.txt", "after!");
    tree.restore_times("touched.txt", times);
    assert_eq!(tree.times("touched.txt").1, times.1);
    await_lines(&db, "touched.txt", &["after!"]);
}

#[test]
fn an_atomic_save_over_the_original_is_read() {
    let tree = Tree::new();
    tree.write("doc.txt", "one");
    let db = tree.open();
    await_lines(&db, "doc.txt", &["one"]);
    tree.save_atomically("doc.txt", "two");
    await_lines(&db, "doc.txt", &["two"]);
}

#[test]
fn a_file_deleted_and_recreated_under_the_same_name_is_read() {
    let tree = Tree::new();
    tree.write("again.txt", "first");
    let db = tree.open();
    await_lines(&db, "again.txt", &["first"]);
    tree.remove("again.txt");
    tree.write("again.txt", "secnd");
    await_lines(&db, "again.txt", &["secnd"]);
    tree.remove("again.txt");
    await_lines::<&str>(&db, "again.txt", &[]);
    tree.write("again.txt", "third");
    await_lines(&db, "again.txt", &["third"]);
}

#[test]
fn a_file_renamed_into_the_glob_is_read() {
    let tree = Tree::new();
    tree.write("incoming.stage", "arrived");
    let db = tree.open();
    await_lines::<&str>(&db, "incoming.txt", &[]);
    tree.rename("incoming.stage", "incoming.txt");
    await_lines(&db, "incoming.txt", &["arrived"]);
}

#[test]
fn a_file_renamed_out_of_the_glob_is_dropped() {
    let tree = Tree::new();
    tree.write("outgoing.txt", "leaving");
    let db = tree.open();
    await_lines(&db, "outgoing.txt", &["leaving"]);
    tree.rename("outgoing.txt", "outgoing.stage");
    await_lines::<&str>(&db, "outgoing.txt", &[]);
}

#[test]
fn a_new_subdirectory_is_read() {
    let tree = Tree::new();
    let db = tree.open();
    tree.write("fresh/deep/one.txt", "one");
    tree.write("fresh/two.txt", "two");
    await_lines(&db, "fresh/deep/one.txt", &["one"]);
    await_lines(&db, "fresh/two.txt", &["two"]);
    await_fresh_scan(&db, &tree, "a new subdirectory");
}

#[test]
fn a_deleted_subdirectory_is_dropped() {
    let tree = Tree::new();
    tree.write("old/deep/one.txt", "one");
    tree.write("old/two.txt", "two");
    let db = tree.open();
    await_lines(&db, "old/two.txt", &["two"]);
    fs::remove_dir_all(tree.path("old")).unwrap();
    await_lines::<&str>(&db, "old/deep/one.txt", &[]);
    await_lines::<&str>(&db, "old/two.txt", &[]);
    await_fresh_scan(&db, &tree, "a deleted subdirectory");
}

fn version(k: usize) -> String {
    (0..50).map(|_| format!("v{k:06}\n")).collect()
}

/// A file always holds one whole version on disk, so a query that mixes two
/// versions, or reads part of one, tore it.
#[test]
fn a_file_rewritten_while_queries_run_never_reads_torn_and_ends_exact() {
    let tree = Arc::new(Tree::new());
    tree.write("busy.txt", &version(0));
    let db = tree.open();
    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let tree = Arc::clone(&tree);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            for k in 1..=500 {
                tree.save_atomically("busy.txt", &version(k));
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    while !done.load(Ordering::SeqCst) {
        let lines = lines_of(&db, "busy.txt");
        assert_eq!(lines.len(), 50, "a partial version: {lines:?}");
        assert!(
            lines.iter().all(|line| line == &lines[0]),
            "mixed versions: {lines:?}"
        );
    }
    writer.join().unwrap();
    await_lines(&db, "busy.txt", &vec!["v000500".to_string(); 50]);
    await_fresh_scan(&db, &tree, "the concurrent writer finished");
}

#[test]
fn a_file_written_in_place_while_queries_run_ends_exact() {
    let tree = Arc::new(Tree::new());
    tree.write("inplace.txt", &version(0));
    let db = tree.open();
    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let tree = Arc::clone(&tree);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            for k in 1..=500 {
                tree.write("inplace.txt", &version(k));
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    while !done.load(Ordering::SeqCst) {
        snapshot(&db);
    }
    writer.join().unwrap();
    await_lines(&db, "inplace.txt", &vec!["v000500".to_string(); 50]);
    await_fresh_scan(&db, &tree, "the in-place writer finished");
}
