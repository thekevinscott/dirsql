//! Every write reaches the index, every time, for a table whose rows come
//! from a spawned hook.
//!
//! Its own binary: a hook spawned while another test's write is open holds
//! that file until exec, delaying the close event past the write's return.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use fresh::{Tree, await_recs};

const ROUNDS: usize = 2000;

#[test]
fn every_edit_to_a_table_hook_file_reaches_the_index() {
    let tree = Tree::new();
    tree.write("rec.json", "{\"k\":\"rec\",\"v\":\"start\"}\n");
    let db = tree.open();
    for i in 0..ROUNDS {
        tree.write("rec.json", &format!("{{\"k\":\"rec\",\"v\":\"{i}\"}}\n"));
        await_recs(&db, "rec", &[i.to_string()]);
    }
}
