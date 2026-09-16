//! The shared server helper must not leak `dirsql server` children when the
//! test process dies without unwinding (OOM kill, cargo-mutants timeout,
//! Ctrl-C). Nothing is mocked: a real test binary holding a real server is
//! SIGKILLed.
//!
//! `server_dies_when_test_process_is_sigkilled` runs this same test binary as
//! a subprocess filtered to the ignored `hold_a_server` helper, which starts a
//! server through the shared helper, waits for its startup banner, prints the
//! server's pid, and sleeps. The driver reads that pid, SIGKILLs the helper,
//! and asserts the server is gone.

#![cfg(all(feature = "cli", target_os = "linux"))]

#[path = "common/server.rs"]
mod server;

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const PID_PREFIX: &str = "server-pid=";

#[test]
#[ignore = "subprocess helper for server_dies_when_test_process_is_sigkilled"]
fn hold_a_server() {
    let root = TempDir::new().unwrap();
    let port = TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = server::spawn_server::<&str>(root.path(), port, &[]);
    let mut banner = String::new();
    BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();
    assert!(
        banner.contains(&format!("localhost:{port}")),
        "unexpected banner: {banner:?}"
    );
    println!("{PID_PREFIX}{}", server.id());
    std::thread::sleep(Duration::from_secs(60));
    drop(server);
}

fn alive(pid: libc::pid_t) -> bool {
    #[expect(unsafe_code, reason = "no safe std API probes another process")]
    unsafe {
        libc::kill(pid, 0) == 0
    }
}

fn sigkill(pid: libc::pid_t) {
    #[expect(
        unsafe_code,
        reason = "no safe std API sends a signal to another process"
    )]
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

#[test]
fn server_dies_when_test_process_is_sigkilled() {
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hold_a_server", "--ignored", "--nocapture"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawning the helper test failed");
    let mut helper_stdout = BufReader::new(helper.stdout.take().unwrap());
    let server_pid: libc::pid_t = loop {
        let mut line = String::new();
        assert_ne!(
            helper_stdout.read_line(&mut line).unwrap(),
            0,
            "helper exited before printing the server pid"
        );
        if let Some(pid) = line.trim_end().strip_prefix(PID_PREFIX) {
            break pid.parse().unwrap();
        }
    };
    assert!(
        alive(server_pid),
        "server {server_pid} must be running before its parent dies"
    );

    sigkill(i32::try_from(helper.id()).unwrap());
    helper.wait().unwrap();
    drop(helper_stdout);

    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(server_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if alive(server_pid) {
        sigkill(server_pid);
        panic!("dirsql server {server_pid} outlived its SIGKILLed test process");
    }
}
