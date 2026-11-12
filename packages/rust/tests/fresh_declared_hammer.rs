//! A query issued the moment a write returns reads that write, every time.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, lines_of};

const ROUNDS: usize = 2000;

#[test]
fn every_create_is_read_by_the_next_query() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        let rel = format!("h/{i}.txt");
        tree.write(&rel, &format!("c{i}"));
        assert_eq!(lines_of(&db, &rel), [format!("c{i}")], "round {i}");
        tree.remove(&rel);
    }
}

#[test]
fn every_edit_is_read_by_the_next_query() {
    let tree = Tree::new();
    tree.write("e.txt", "start");
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("e.txt", &format!("e{i}"));
        assert_eq!(lines_of(&db, "e.txt"), [format!("e{i}")], "round {i}");
    }
}

#[test]
fn every_delete_is_read_by_the_next_query() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("d.txt", &format!("d{i}"));
        assert_eq!(lines_of(&db, "d.txt"), [format!("d{i}")], "round {i}");
        tree.remove("d.txt");
        assert!(lines_of(&db, "d.txt").is_empty(), "round {i}");
    }
}

#[test]
fn every_rename_into_and_out_of_the_glob_is_read_by_the_next_query() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("r.stage", &format!("r{i}"));
        tree.rename("r.stage", "r.txt");
        assert_eq!(lines_of(&db, "r.txt"), [format!("r{i}")], "round {i}");
        tree.rename("r.txt", "r.stage");
        assert!(lines_of(&db, "r.txt").is_empty(), "round {i}");
    }
}

#[test]
fn every_atomic_save_is_read_by_the_next_query() {
    let tree = Tree::new();
    tree.write("a.txt", "start");
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.save_atomically("a.txt", &format!("a{i}"));
        assert_eq!(lines_of(&db, "a.txt"), [format!("a{i}")], "round {i}");
    }
}

#[test]
fn every_same_size_edit_is_read_by_the_next_query() {
    let tree = Tree::new();
    tree.write("s.txt", "s------");
    let db = tree.open();
    for i in 0..ROUNDS {
        let content = format!("s{i:06}");
        tree.write("s.txt", &content);
        assert_eq!(lines_of(&db, "s.txt"), [content], "round {i}");
    }
}

#[test]
fn every_same_size_edit_with_its_times_restored_is_read_by_the_next_query() {
    let tree = Tree::new();
    tree.write("t.txt", "t------");
    let times = tree.times("t.txt");
    let db = tree.open();
    for i in 0..ROUNDS {
        let content = format!("t{i:06}");
        tree.write("t.txt", &content);
        tree.restore_times("t.txt", times);
        assert_eq!(lines_of(&db, "t.txt"), [content], "round {i}");
    }
}
