//! Speed and memory limits for the tests harness-md writes into tests/.
//! Cargo's test runner has no hooks, so a test wraps its own body in [`measured`].
//! `MEASURE=speed` or `MEASURE=memory` turns the limits on, as separate passes.

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

const PAIRS: usize = 5;

fn gnu_time(peak: &Path) -> Vec<String> {
    ["/usr/bin/time", "-f", "%M", "-o", &peak.to_string_lossy()].map(String::from).to_vec()
}

fn peak_mb(peak: &Path) -> f64 {
    let text = std::fs::read_to_string(peak).unwrap();
    text.split_whitespace().last().unwrap().parse::<f64>().unwrap() / 1024.0
}

fn rss_mb() -> f64 {
    memory_stats::memory_stats().unwrap().physical_mem as f64 / 1024.0 / 1024.0
}

fn run_baseline(prefix: &[String], baseline: &str, cwd: &Path) {
    let command: Vec<&str> = prefix.iter().map(String::as_str).chain(["sh", "-c", baseline]).collect();
    let status = Command::new(command[0]).args(&command[1..]).current_dir(cwd).stdout(Stdio::null()).status().unwrap();
    assert!(status.success(), "baseline failed: {baseline}");
}

fn over(what: &str, got: f64, limit: &Value, unit: &str) -> Option<String> {
    let limit = limit.as_f64()?;
    (got > limit).then(|| format!("{what} {got:.1}{unit}, limit {limit}{unit}"))
}

/// Runs `test` with the launcher it should use. `cli` is `None` for an SDK test.
pub fn measured(ms: &Value, mb: &Value, baseline: &str, cwd: &Path, cli: Option<&[String]>, test: impl Fn(&[String])) {
    let launcher = cli.unwrap_or_default();
    let mode = std::env::var("MEASURE").unwrap_or_default();
    if mode != "speed" && mode != "memory" {
        return test(launcher);
    }
    let peak = std::env::temp_dir().join(format!("measure-{}", std::process::id()));
    let (limits, unit, verb) = if mode == "speed" { (ms, " ms", "took") } else { (mb, " MB", "used") };
    let mut failures = Vec::new();
    let mut pairs = Vec::new();

    for _ in 0..PAIRS {
        let pair = if mode == "speed" {
            let start = Instant::now();
            test(launcher);
            let took = start.elapsed().as_secs_f64() * 1000.0;
            let start = Instant::now();
            run_baseline(&[], baseline, cwd);
            (took, start.elapsed().as_secs_f64() * 1000.0)
        } else {
            let used = if cli.is_some() {
                // A CLI test's memory is its child's peak, so the launcher runs under GNU time.
                test(&[gnu_time(&peak), launcher.to_vec()].concat());
                peak_mb(&peak)
            } else {
                let before = rss_mb();
                test(launcher);
                rss_mb() - before
            };
            run_baseline(&gnu_time(&peak), baseline, cwd);
            (used, peak_mb(&peak))
        };
        failures.extend(over(verb, pair.0, &limits["max"], unit));
        pairs.push(pair);
    }

    let got = pairs.iter().map(|pair| pair.0).sum::<f64>() / PAIRS as f64;
    let base = pairs.iter().map(|pair| pair.1).sum::<f64>() / PAIRS as f64;
    println!("mean of {PAIRS} pairs: test {got:.1}{unit}; baseline {base:.1}{unit}; {:.1}x", got / base);
    failures.extend(over(&format!("mean {verb}"), got, &limits["mean"], unit));
    failures.extend(over("mean was", got / base, &limits["times_baseline"], "x baseline"));
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
