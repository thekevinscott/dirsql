use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
}

/// Wraps notify::RecommendedWatcher and translates raw events into FileEvent values.
pub struct Watcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<FileEvent>,
}

impl Watcher {
    /// Start watching a directory recursively. Events are buffered in an internal channel.
    pub fn new(path: &Path) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let events = translate_event(&event);
                    for fe in events {
                        // Ignore send errors (receiver dropped)
                        let _ = tx.send(fe);
                    }
                }
            },
            Config::default(),
        )?;

        watcher.watch(path, RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Receive the next event, blocking until one is available.
    pub fn recv(&self) -> Option<FileEvent> {
        self.rx.recv().ok()
    }

    /// Try to receive an event with a timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Drain all currently pending events without blocking.
    pub fn try_recv_all(&self) -> Vec<FileEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// Translate a notify Event into zero or more FileEvents.
fn translate_event(event: &Event) -> Vec<FileEvent> {
    let mut results = Vec::new();

    for path in &event.paths {
        let fe = match event.kind {
            EventKind::Create(_) => Some(FileEvent::Created(path.clone())),
            EventKind::Modify(_) => Some(FileEvent::Modified(path.clone())),
            EventKind::Remove(_) => Some(FileEvent::Deleted(path.clone())),
            _ => None,
        };
        if let Some(fe) = fe {
            results.push(fe);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn detects_file_creation() {
        let dir = TempDir::new().unwrap();
        let watcher = Watcher::new(dir.path()).unwrap();

        // Small delay to let watcher initialize
        thread::sleep(Duration::from_millis(100));

        let file_path = dir.path().join("new_file.txt");
        fs::write(&file_path, "hello").unwrap();

        // Wait for events with timeout
        let mut found_create = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(200)) {
                if matches!(event, FileEvent::Created(_)) {
                    found_create = true;
                    break;
                }
            }
        }
        assert!(found_create, "Expected a Created event");
    }

    #[test]
    fn detects_file_deletion() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        fs::write(&file_path, "doomed").unwrap();

        let watcher = Watcher::new(dir.path()).unwrap();
        thread::sleep(Duration::from_millis(100));

        fs::remove_file(&file_path).unwrap();

        let mut found_delete = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(200)) {
                if matches!(event, FileEvent::Deleted(_)) {
                    found_delete = true;
                    break;
                }
            }
        }
        assert!(found_delete, "Expected a Deleted event");
    }

    #[test]
    fn detects_file_modification() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("modify_me.txt");
        fs::write(&file_path, "original").unwrap();

        let watcher = Watcher::new(dir.path()).unwrap();
        thread::sleep(Duration::from_millis(100));

        fs::write(&file_path, "modified content").unwrap();

        // We should get either a Modified or Created event (some backends emit Create on overwrite)
        let mut found_event = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(200)) {
                if matches!(event, FileEvent::Modified(_) | FileEvent::Created(_)) {
                    found_event = true;
                    break;
                }
            }
        }
        assert!(found_event, "Expected a Modified or Created event");
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

    #[test]
    fn translate_event_maps_create() {
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/tmp/test.txt")],
            attrs: Default::default(),
        };
        let results = translate_event(&event);
        assert_eq!(
            results,
            vec![FileEvent::Created(PathBuf::from("/tmp/test.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_remove() {
        let event = Event {
            kind: EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![PathBuf::from("/tmp/gone.txt")],
            attrs: Default::default(),
        };
        let results = translate_event(&event);
        assert_eq!(
            results,
            vec![FileEvent::Deleted(PathBuf::from("/tmp/gone.txt"))]
        );
    }

    #[test]
    fn translate_event_ignores_access_events() {
        let event = Event {
            kind: EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/tmp/read.txt")],
            attrs: Default::default(),
        };
        let results = translate_event(&event);
        assert!(results.is_empty());
    }
}
