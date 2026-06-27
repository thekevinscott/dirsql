//! Integration tests for the filesystem watcher.
//!
//! These drive a **real** `notify` OS watcher over a real temp directory with
//! real file mutations, threads, and wall-clock polling -- they verify the
//! end-to-end "does the OS actually report this change" behavior through
//! `Watcher`'s public API. That makes them integration tests, not unit tests:
//! faking `notify` would only assert against the fake. They were moved here
//! out of `watcher.rs`'s inline `#[cfg(test)]` module so that module stays
//! purely unit (the `testing-conventions` `unit lint` isolation rule forbids
//! effectful std -- `std::fs`, `std::thread`, `Instant::now` -- in unit tests).
//! The pure `translate_event` mapping tests remain inline next to the private
//! function they exercise.

use std::time::{Duration, Instant};
use std::{fs, thread};

use dirsql::watcher::{FileEvent, Watcher};
use tempfile::TempDir;

/// Drain events from `watcher` into a Vec, returning early once `done`
/// reports satisfaction or the `budget` elapses. Returns everything seen
/// so far. Used by the `detects_*` tests so the polling loop has no
/// data-dependent `break` body whose coverage region races with real-OS
/// filesystem-event timing.
fn collect_events_until(
    watcher: &Watcher,
    budget: Duration,
    done: impl Fn(&[FileEvent]) -> bool,
) -> Vec<FileEvent> {
    let deadline = Instant::now() + budget;
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        if let Some(event) = watcher.recv_timeout(Duration::from_millis(200)) {
            seen.push(event);
        }
    }
    seen
}

#[test]
fn detects_file_creation() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();

    // Small delay to let watcher initialize
    thread::sleep(Duration::from_millis(100));

    let file_path = dir.path().join("new_file.txt");
    fs::write(&file_path, "hello").unwrap();

    // Collect every event seen up to the deadline, then assert. Draining
    // into a Vec (instead of breaking out of the loop on the first match)
    // keeps the loop body free of a data-dependent `break` whose region
    // races with real-OS event timing under coverage.
    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(|e| matches!(e, FileEvent::Created(_)))
    });
    assert!(
        events.iter().any(|e| matches!(e, FileEvent::Created(_))),
        "Expected a Created event, saw: {events:?}"
    );
}

#[test]
fn detects_file_deletion() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("to_delete.txt");
    fs::write(&file_path, "doomed").unwrap();

    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    fs::remove_file(&file_path).unwrap();

    // See `detects_file_creation` for why we collect rather than break.
    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(|e| matches!(e, FileEvent::Deleted(_)))
    });
    assert!(
        events.iter().any(|e| matches!(e, FileEvent::Deleted(_))),
        "Expected a Deleted event, saw: {events:?}"
    );
}

#[test]
fn detects_file_modification() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("modify_me.txt");
    fs::write(&file_path, "original").unwrap();

    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    fs::write(&file_path, "modified content").unwrap();

    // We should get either a Modified or Created event (some backends emit
    // Create on overwrite). See `detects_file_creation` for the collect
    // rationale.
    let matches_event = |e: &FileEvent| matches!(e, FileEvent::Modified(_) | FileEvent::Created(_));
    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(matches_event)
    });
    assert!(
        events.iter().any(matches_event),
        "Expected a Modified or Created event, saw: {events:?}"
    );
}

#[test]
fn recv_blocks_until_event_arrives() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));
    fs::write(dir.path().join("recv_test.txt"), "hi").unwrap();
    // recv() blocks; the event should arrive quickly
    let event = watcher.recv();
    assert!(event.is_some());
}

#[test]
fn try_recv_all_drains_pending_events() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Create several files
    for i in 0..3 {
        fs::write(dir.path().join(format!("file_{i}.txt")), "data").unwrap();
    }

    // Wait a bit for events to arrive
    thread::sleep(Duration::from_millis(500));

    let events = watcher.try_recv_all();
    assert!(
        !events.is_empty(),
        "Expected at least one event from batch file creation"
    );
}
