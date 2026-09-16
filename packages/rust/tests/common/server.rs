//! Spawn `dirsql server` as a child that cannot outlive its test.
//!
//! The guard reaps the server when the test unwinds. A SIGKILLed test process
//! never unwinds, so on Linux the child also asks the kernel to SIGKILL it
//! when its parent thread dies (`PR_SET_PDEATHSIG`).

use std::ffi::OsStr;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use assert_cmd::prelude::*;

/// A running `dirsql server`, killed and reaped on drop so a panicking test
/// never leaks a process that holds the inherited stderr pipe open.
pub struct ServerGuard(Child);

impl Deref for ServerGuard {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for ServerGuard {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn `dirsql server --port <port> --host localhost` in `cwd` with `extra`
/// args appended. stdout is piped; stderr is inherited so failures surface in
/// test output.
pub fn spawn_server<S: AsRef<OsStr>>(cwd: &Path, port: u16, extra: &[S]) -> ServerGuard {
    let mut cmd: Command = Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli");
    cmd.arg("server")
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .args(extra)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        #[expect(
            unsafe_code,
            reason = "pre_exec runs between fork and exec; prctl is async-signal-safe"
        )]
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    ServerGuard(cmd.spawn().expect("spawning dirsql failed"))
}
