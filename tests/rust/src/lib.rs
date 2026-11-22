//! Speed and memory measuring for the tests harness-md writes into tests/.
//! Cargo's test runner has no hooks, so a test wraps its own body in [`measured`].
//! tests/measure.py sets MEASURE to a results file and runs cargo once per repeat.

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

fn gnu_time(peak: &Path) -> Vec<String> {
    ["/usr/bin/time", "-f", "%M", "-o", &peak.to_string_lossy()].map(String::from).to_vec()
}

fn peak_mb(peak: &Path) -> f64 {
    let text = std::fs::read_to_string(peak).unwrap();
    text.split_whitespace().last().unwrap().parse::<f64>().unwrap() / 1024.0
}

fn status_mb(field: &str) -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|line| line.starts_with(field)).unwrap();
    line.split_whitespace().nth(1).unwrap().parse::<f64>().unwrap() / 1024.0
}

/// Runs `test` with the launcher it should use. `cli` is `None` for an SDK test.
#[track_caller]
pub fn measured(ms: &Value, mb: &Value, baseline: &str, cwd: &Path, cli: Option<&[String]>, test: impl Fn(&[String])) {
    let launcher = cli.unwrap_or_default();
    let Ok(results) = std::env::var("MEASURE") else {
        return test(launcher);
    };
    let file = std::panic::Location::caller().file();
    let peak = std::env::temp_dir().join(format!("measure-{}", std::process::id()));

    // A CLI test's memory is its child's peak, so the launcher runs under GNU time.
    let timed = [gnu_time(&peak), launcher.to_vec()].concat();
    // Resets this process's peak, so the peak read after the test belongs to the test.
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let before = status_mb("VmRSS");
    let start = Instant::now();
    test(if cli.is_some() { &timed } else { launcher });
    let took = start.elapsed().as_secs_f64() * 1000.0;
    let used = if cli.is_some() { peak_mb(&peak) } else { status_mb("VmHWM") - before };

    let start = Instant::now();
    let command = [gnu_time(&peak), vec!["sh".into(), "-c".into(), baseline.into()]].concat();
    let status = Command::new(&command[0]).args(&command[1..]).current_dir(cwd).stdout(Stdio::null()).status().unwrap();
    assert!(status.success(), "baseline failed: {baseline}");
    let base_ms = start.elapsed().as_secs_f64() * 1000.0;

    let line = json!({
        "file": file, "ms": took, "mb": used, "base_ms": base_ms, "base_mb": peak_mb(&peak),
        "limits": {"ms": ms, "mb": mb},
    });
    let mut results = std::fs::OpenOptions::new().append(true).create(true).open(results).unwrap();
    writeln!(results, "{line}").unwrap();
}
