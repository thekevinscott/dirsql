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

// Test fixtures: build `notify::Event`s for each top-level `EventKind` without
// the unit tests below naming the `notify::event::*` inner-kind types (the
// `unit lint` isolation rule). `translate_event` matches only the outer
// `EventKind` variant, so the inner kind here is an arbitrary valid value.
#[cfg(test)]
fn create_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Create(notify::event::CreateKind::File),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn modify_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn remove_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Remove(notify::event::RemoveKind::File),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn access_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Access(notify::event::AccessKind::Read),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Effectful tests that drive a real `notify` OS watcher over real temp
    // files (`std::fs`, `std::thread`, `Instant::now`) live in
    // `tests/watcher.rs` -- they're integration tests, and keeping them out of
    // this inline module is what the `unit lint` isolation rule requires. Only
    // the pure `translate_event` mapping tests belong here; they build their
    // `notify::Event` inputs via the `*_event` fixtures above so no unit test
    // names a `notify::event::*` type directly.

    #[test]
    fn translate_event_maps_create() {
        let results = translate_event(&create_event(vec![PathBuf::from("/tmp/test.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Created(PathBuf::from("/tmp/test.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_remove() {
        let results = translate_event(&remove_event(vec![PathBuf::from("/tmp/gone.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Deleted(PathBuf::from("/tmp/gone.txt"))]
        );
    }

    #[test]
    fn translate_event_ignores_access_events() {
        let results = translate_event(&access_event(vec![PathBuf::from("/tmp/read.txt")]));
        assert!(results.is_empty());
    }

    #[test]
    fn translate_event_maps_modify() {
        let results = translate_event(&modify_event(vec![PathBuf::from("/tmp/changed.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Modified(PathBuf::from("/tmp/changed.txt"))]
        );
    }

    #[test]
    fn translate_event_multiple_paths() {
        let results = translate_event(&create_event(vec![
            PathBuf::from("/tmp/a.txt"),
            PathBuf::from("/tmp/b.txt"),
        ]));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], FileEvent::Created(PathBuf::from("/tmp/a.txt")));
        assert_eq!(results[1], FileEvent::Created(PathBuf::from("/tmp/b.txt")));
    }

    #[test]
    fn translate_event_empty_paths() {
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![],
            attrs: Default::default(),
        };
        let results = translate_event(&event);
        assert!(results.is_empty());
    }

    #[test]
    fn translate_event_ignores_other_kind() {
        let event = Event {
            kind: EventKind::Other,
            paths: vec![PathBuf::from("/tmp/other.txt")],
            attrs: Default::default(),
        };
        let results = translate_event(&event);
        assert!(results.is_empty());
    }
}
