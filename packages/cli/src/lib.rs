//! `dirsql-cli` library crate. The binary at `src/main.rs` is the primary
//! consumer; tests import from this library to exercise the server directly.

pub mod args;
pub mod engine;
pub mod error;
pub mod server;
