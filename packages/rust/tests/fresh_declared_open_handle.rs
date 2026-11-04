//! A write through a handle that is still open is read by the next query,
//! whether or not the writer flushed, even when the file had settled and its
//! last read was trusted.

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, lines_of};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::thread::sleep;
use std::time::Duration;

const ROUNDS: usize = 300;

/// Past the settle window, so every file's last read is trusted on its stat
/// alone when the write through the open handle lands.
const SETTLED: Duration = Duration::from_millis(2500);

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

fn hammer(
    resize: bool,
    flush: bool,
    before: impl Fn(usize) -> String,
    after: impl Fn(usize) -> String,
) {
    let tree = Tree::new();
    let rel = |i: usize| format!("o/{i}.txt");
    for i in 0..ROUNDS {
        tree.write(&rel(i), &before(i));
    }
    let db = tree.open();
    let mut files: Vec<File> = (0..ROUNDS)
        .map(|i| {
            OpenOptions::new()
                .write(true)
                .open(tree.path(&rel(i)))
                .unwrap()
        })
        .collect();
    sleep(SETTLED);
    for i in 0..ROUNDS {
        assert_eq!(lines_of(&db, &rel(i)), [before(i)], "file {i} before");
    }
    for (i, file) in files.iter_mut().enumerate() {
        let expected = after(i);
        overwrite(file, &expected, resize, flush);
        assert_eq!(lines_of(&db, &rel(i)), [expected], "file {i}");
    }
    drop(files);
}

fn same_size_before(i: usize) -> String {
    format!("a{i:08}")
}

fn same_size_after(i: usize) -> String {
    format!("b{i:08}")
}

fn short(i: usize) -> String {
    format!("{i}")
}

fn long(i: usize) -> String {
    format!("{i}-{}", "z".repeat(1 + i % 37))
}

#[test]
fn a_same_size_rewrite_through_an_open_handle_is_read_by_the_next_query() {
    hammer(false, false, same_size_before, same_size_after);
}

#[test]
fn a_flushed_same_size_rewrite_through_an_open_handle_is_read_by_the_next_query() {
    hammer(false, true, same_size_before, same_size_after);
}

#[test]
fn a_resize_through_an_open_handle_is_read_by_the_next_query() {
    hammer(true, false, short, long);
}

#[test]
fn a_flushed_resize_through_an_open_handle_is_read_by_the_next_query() {
    hammer(true, true, short, long);
}
