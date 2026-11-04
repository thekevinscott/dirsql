//! A write through a handle that is still open is read by the next query,
//! whether or not the writer flushed.

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, lines_of};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

const ROUNDS: usize = 500;

fn open_writer(tree: &Tree, rel: &str) -> File {
    OpenOptions::new().write(true).open(tree.path(rel)).unwrap()
}

/// A same-size rewrite never truncates, so the length never moves.
fn overwrite(file: &mut File, content: &str, resize: bool, flush: bool) {
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    if resize {
        file.set_len(content.len() as u64).unwrap();
    }
    if flush {
        file.sync_all().unwrap();
    }
}

fn hammer(resize: bool, flush: bool, content: impl Fn(usize) -> String) {
    let tree = Tree::new();
    tree.write("o.txt", &content(usize::MAX));
    let db = tree.open();
    let mut file = open_writer(&tree, "o.txt");
    for i in 0..ROUNDS {
        let expected = content(i);
        overwrite(&mut file, &expected, resize, flush);
        assert_eq!(lines_of(&db, "o.txt"), [expected], "round {i}");
    }
    drop(file);
}

fn same_size(i: usize) -> String {
    format!("s{:08}", i % 100_000_000)
}

fn size_change(i: usize) -> String {
    "z".repeat(1 + i % 37)
}

#[test]
fn a_same_size_rewrite_through_an_open_handle_is_read_by_the_next_query() {
    hammer(false, false, same_size);
}

#[test]
fn a_flushed_same_size_rewrite_through_an_open_handle_is_read_by_the_next_query() {
    hammer(false, true, same_size);
}

#[test]
fn a_resize_through_an_open_handle_is_read_by_the_next_query() {
    hammer(true, false, size_change);
}

#[test]
fn a_flushed_resize_through_an_open_handle_is_read_by_the_next_query() {
    hammer(true, true, size_change);
}
