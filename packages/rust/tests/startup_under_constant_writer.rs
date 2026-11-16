//! Startup finishes while another process keeps writing files faster than
//! `on_file` reads them.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn startup_finishes_while_files_are_written_faster_than_they_are_read() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let items = root.join("items");
    fs::create_dir(&items).unwrap();
    for n in 0..50 {
        fs::write(items.join(format!("seed{n}.txt")), "x").unwrap();
    }
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (items, stop) = (items.clone(), Arc::clone(&stop));
        thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::SeqCst) {
                fs::write(items.join(format!("{n}.txt")), "x").unwrap();
                n += 1;
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let (tx, rx) = mpsc::channel();
    {
        let root = root.clone();
        thread::spawn(move || {
            let table = Table::new(
                "item",
                "CREATE TABLE item (name TEXT)",
                "items/*.txt",
                |path| {
                    thread::sleep(Duration::from_millis(3));
                    vec![HashMap::from([(
                        "name".to_string(),
                        Value::Text(path.to_string()),
                    )])] as Vec<Row>
                },
            );
            let built = DirSQL::new(&root, vec![table]).map(|_| ());
            let _ = tx.send(built.map_err(|e| e.to_string()));
        });
    }

    let outcome = rx.recv_timeout(Duration::from_secs(30));
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap();

    match outcome {
        Ok(built) => built.unwrap(),
        Err(_) => panic!("startup did not finish within 30s while the writer ran"),
    }
}
