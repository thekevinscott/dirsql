//! Every write reaches the index, every time.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, await_lines};

const ROUNDS: usize = 2000;

#[test]
fn every_create_reaches_the_index() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        let rel = format!("h/{i}.txt");
        tree.write(&rel, &format!("c{i}"));
        await_lines(&db, &rel, &[format!("c{i}")]);
        tree.remove(&rel);
    }
}

#[test]
fn every_edit_reaches_the_index() {
    let tree = Tree::new();
    tree.write("e.txt", "start");
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("e.txt", &format!("e{i}"));
        await_lines(&db, "e.txt", &[format!("e{i}")]);
    }
}

#[test]
fn every_delete_reaches_the_index() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("d.txt", &format!("d{i}"));
        await_lines(&db, "d.txt", &[format!("d{i}")]);
        tree.remove("d.txt");
        await_lines::<&str>(&db, "d.txt", &[]);
    }
}

#[test]
fn every_rename_into_and_out_of_the_glob_reaches_the_index() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("r.stage", &format!("r{i}"));
        tree.rename("r.stage", "r.txt");
        await_lines(&db, "r.txt", &[format!("r{i}")]);
        tree.rename("r.txt", "r.stage");
        await_lines::<&str>(&db, "r.txt", &[]);
    }
}

#[test]
fn every_atomic_save_reaches_the_index() {
    let tree = Tree::new();
    tree.write("a.txt", "start");
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.save_atomically("a.txt", &format!("a{i}"));
        await_lines(&db, "a.txt", &[format!("a{i}")]);
    }
}

#[test]
fn every_same_size_edit_reaches_the_index() {
    let tree = Tree::new();
    tree.write("s.txt", "s------");
    let db = tree.open();
    for i in 0..ROUNDS {
        let content = format!("s{i:06}");
        tree.write("s.txt", &content);
        await_lines(&db, "s.txt", &[content]);
    }
}

#[test]
fn every_same_size_edit_with_its_times_restored_reaches_the_index() {
    let tree = Tree::new();
    tree.write("t.txt", "t------");
    let times = tree.times("t.txt");
    let db = tree.open();
    for i in 0..ROUNDS {
        let content = format!("t{i:06}");
        tree.write("t.txt", &content);
        tree.restore_times("t.txt", times);
        await_lines(&db, "t.txt", &[content]);
    }
}
