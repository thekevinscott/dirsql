//! The `dirsql` binary: a shim over [`dirsql::cli::run_cli`].
//!
//! Every packaging path runs the same code — `cargo install dirsql --features
//! cli` compiles this shim, and the pip/npm launchers reach `run_cli` through
//! their bindings. All argument parsing, dispatch and exit-code selection live
//! in the library so none of it can drift per entry point.
//!
//! Only compiled with `--features cli`.

use std::process::ExitCode;

fn main() -> ExitCode {
    // `run_cli` returns rather than exiting, so the process exit lives here.
    ExitCode::from(exit_status(dirsql::cli::run_cli(
        std::env::args().collect(),
    )))
}

/// Narrow `run_cli`'s `i32` to the `u8` a process exit status actually
/// carries. `run_cli` is `i32` for the binding boundary but only ever yields
/// 0..=255; anything outside that range would be truncated by the OS anyway,
/// so it is clamped to a generic failure rather than wrapped into a
/// misleading code (a hypothetical 256 must not surface as success).
fn exit_status(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::exit_status;

    #[test]
    fn passes_through_the_codes_run_cli_produces() {
        assert_eq!(exit_status(0), 0);
        assert_eq!(exit_status(1), 1);
        assert_eq!(exit_status(2), 2);
        assert_eq!(exit_status(255), 255);
    }

    #[test]
    fn clamps_out_of_range_codes_to_failure() {
        // Truncating would turn these into 0 — a failure reported as success.
        assert_eq!(exit_status(256), 1);
        assert_eq!(exit_status(-1), 1);
    }
}
