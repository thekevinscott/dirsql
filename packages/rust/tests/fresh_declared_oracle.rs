//! After any sequence of file operations, a long-lived instance reads exactly
//! what an instance built fresh over the same tree reads.

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Rng, Tree, assert_matches_fresh_scan};
use std::fs;
use std::time::Duration;

const DIRS: &[&str] = &["", "a/", "a/b/", "c/"];
const NAMES: &[&str] = &["n0", "n1", "n2", "n3"];
const EXTS: &[&str] = &["txt", "json", "stage"];
const IGNORES: &[&str] = &["n0.txt\n", "a/\n", "*.json\n", "c/n1.*\n"];

/// Every file holds JSON lines, so a rename across globs leaves both tables
/// readable.
fn content(rng: &mut Rng, rel: &str) -> String {
    let mut text = String::new();
    for line in 0..=rng.below(3) {
        text.push_str(&format!(
            "{{\"k\":\"{rel}-{line}\",\"v\":\"{:04}\"}}\n",
            rng.below(10_000)
        ));
    }
    text
}

fn random_file(rng: &mut Rng) -> String {
    format!("{}{}.{}", rng.pick(DIRS), rng.pick(NAMES), rng.pick(EXTS))
}

fn existing_files(tree: &Tree) -> Vec<String> {
    let mut files = Vec::new();
    for dir in DIRS {
        for name in NAMES {
            for ext in EXTS {
                let rel = format!("{dir}{name}.{ext}");
                if tree.path(&rel).is_file() {
                    files.push(rel);
                }
            }
        }
    }
    files
}

/// Apply one random operation and say what it was.
fn step(tree: &Tree, rng: &mut Rng) -> String {
    let existing = existing_files(tree);
    match rng.below(10) {
        0 | 1 => {
            let rel = random_file(rng);
            let text = content(rng, &rel);
            tree.write(&rel, &text);
            format!("write {rel}")
        }
        2 if !existing.is_empty() => {
            let rel = rng.pick(&existing).clone();
            let old = fs::read_to_string(tree.path(&rel)).unwrap();
            let times = tree.times(&rel);
            let flipped: String = old
                .chars()
                .map(|c| match c.to_digit(10) {
                    Some(d) => char::from_digit((d + 1) % 10, 10).unwrap(),
                    None => c,
                })
                .collect();
            tree.write(&rel, &flipped);
            tree.restore_times(&rel, times);
            format!("same-size edit of {rel} with its times restored")
        }
        3 => {
            let rel = random_file(rng);
            let text = content(rng, &rel);
            tree.save_atomically(&rel, &text);
            format!("atomic save of {rel}")
        }
        4 if !existing.is_empty() => {
            let rel = rng.pick(&existing).clone();
            tree.remove(&rel);
            format!("delete {rel}")
        }
        5 if !existing.is_empty() => {
            let from = rng.pick(&existing).clone();
            let to = random_file(rng);
            if from == to {
                return "nothing".to_string();
            }
            tree.rename(&from, &to);
            format!("rename {from} to {to}")
        }
        6 => {
            let dir = format!("{}new{}", rng.pick(DIRS), rng.below(3));
            for name in &NAMES[..2] {
                let rel = format!("{dir}/{name}.{}", rng.pick(EXTS));
                let text = content(rng, &rel);
                tree.write(&rel, &text);
            }
            format!("populate new directory {dir}")
        }
        7 => {
            let dir = rng.pick(&DIRS[1..]).trim_end_matches('/').to_string();
            if tree.path(&dir).is_dir() {
                fs::remove_dir_all(tree.path(&dir)).unwrap();
            }
            format!("remove directory {dir}")
        }
        8 => {
            if rng.below(3) == 0 {
                let _ = fs::remove_file(tree.path(".gitignore"));
                "remove .gitignore".to_string()
            } else {
                let rule = rng.pick(IGNORES);
                tree.write(".gitignore", rule);
                format!("set .gitignore to {rule:?}")
            }
        }
        _ => {
            let rel = random_file(rng);
            let text = content(rng, &rel);
            tree.write(&rel, &text);
            format!("write {rel}")
        }
    }
}

fn seeded_tree(rng: &mut Rng) -> Tree {
    let tree = Tree::new();
    fs::create_dir(tree.path(".git")).unwrap();
    for _ in 0..8 {
        let rel = random_file(rng);
        let text = content(rng, &rel);
        tree.write(&rel, &text);
    }
    tree
}

#[test]
fn random_operations_read_what_a_fresh_scan_reads() {
    for seed in 1..=4 {
        let mut rng = Rng::new(seed);
        let tree = seeded_tree(&mut rng);
        let db = tree.open();
        for op in 0..150 {
            let done = step(&tree, &mut rng);
            assert_matches_fresh_scan(&db, &tree, &format!("seed {seed} op {op}: {done}"));
        }
    }
}

#[test]
fn a_long_lived_instance_reads_what_a_fresh_scan_reads_across_idle_gaps_and_bursts() {
    let mut rng = Rng::new(99);
    let tree = seeded_tree(&mut rng);
    let db = tree.open();
    for cycle in 0..12 {
        std::thread::sleep(Duration::from_millis(250 * (cycle % 4)));
        for op in 0..25 {
            let done = step(&tree, &mut rng);
            assert_matches_fresh_scan(&db, &tree, &format!("cycle {cycle} op {op}: {done}"));
        }
    }
}
