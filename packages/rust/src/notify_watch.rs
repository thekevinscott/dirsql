use crate::matcher::TableMatcher;
use crate::watcher::{
    FileEvent, WatchScope, tolerate_missing, walk_scope, watch_tree, watch_tree_quietly,
};
use notify::event::{ModifyKind, RenameMode};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher,
};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

/// inotify and kqueue spend a kernel watch on every directory, so only those
/// the scan enters get one. FSEvents and ReadDirectoryChangesW cover a whole
/// tree with one watch, where a watch per directory would cost far more.
const PER_DIRECTORY: bool = !cfg!(any(target_os = "macos", target_os = "windows"));

/// Wraps notify::RecommendedWatcher and translates raw events into FileEvent values.
pub struct Watcher {
    _watcher: Arc<Mutex<RecommendedWatcher>>,
    rx: Mutex<mpsc::Receiver<FileEvent>>,
    /// Events [`wait`](Self::wait) took off the channel, served first.
    waited: Mutex<VecDeque<FileEvent>>,
}

impl Watcher {
    /// Start watching a directory recursively. Events are buffered in an internal channel.
    pub fn new(path: &Path) -> Result<Self, notify::Error> {
        Self::over(&[path.to_path_buf()])
    }

    /// Start watching every directory in `paths` and below, merging their
    /// events into one channel.
    pub fn over(paths: &[PathBuf]) -> Result<Self, notify::Error> {
        let everything = TableMatcher::new(&[], &[]).expect("no patterns to compile");
        Self::scoped(
            paths
                .iter()
                .map(|root| WatchScope {
                    root: root.clone(),
                    ignore: everything.clone(),
                })
                .collect(),
        )
    }

    /// Watch each scope's root and the directories below it the scan would
    /// enter, merging their events into one channel. Where [`PER_DIRECTORY`]
    /// holds, each directory gets its own non-recursive watch, so a skipped
    /// subtree costs none, and a directory created later is walked and
    /// watched before its creation is reported.
    pub fn scoped(scopes: Vec<WatchScope>) -> Result<Self, notify::Error> {
        let (raw_tx, raw_rx) = mpsc::channel::<Event>();
        let (tx, rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    // Ignore send errors (receiver dropped)
                    let _ = raw_tx.send(event);
                }
            },
            Config::default(),
        )?;

        let mode = if PER_DIRECTORY {
            RecursiveMode::NonRecursive
        } else {
            RecursiveMode::Recursive
        };
        let mut watched = HashSet::new();
        for scope in &scopes {
            watcher.watch(&scope.root, mode)?;
            watched.insert(scope.root.clone());
        }
        // A recursive watch already covers every directory below its root.
        let scopes = if PER_DIRECTORY { scopes } else { Vec::new() };
        for scope in &scopes {
            watch_tree(
                &scopes,
                &scope.root,
                &walk_scope,
                &mut watched,
                &mut |dir| tolerate_missing(watcher.watch(dir, RecursiveMode::NonRecursive)),
            )?;
        }

        let watcher = Arc::new(Mutex::new(watcher));
        let handle = Arc::downgrade(&watcher);
        thread::spawn(move || forward(&raw_rx, &tx, &handle, &scopes));

        Ok(Self {
            _watcher: watcher,
            rx: Mutex::new(rx),
            waited: Mutex::new(VecDeque::new()),
        })
    }

    /// Receive the next event, blocking until one is available.
    pub fn recv(&self) -> Option<FileEvent> {
        if let Some(event) = lock(&self.waited).pop_front() {
            return Some(event);
        }
        lock(&self.rx).recv().ok()
    }

    /// Try to receive an event with a timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileEvent> {
        if let Some(event) = lock(&self.waited).pop_front() {
            return Some(event);
        }
        lock(&self.rx).recv_timeout(timeout).ok()
    }

    /// Every event queued so far, in order, without blocking. One a
    /// [`wait`](Self::wait) in progress is receiving is left to it.
    pub fn drain(&self) -> Vec<FileEvent> {
        let mut events: Vec<FileEvent> = lock(&self.waited).drain(..).collect();
        if let Ok(rx) = self.rx.try_lock() {
            events.extend(rx.try_iter());
        }
        events
    }

    /// Drain all currently pending events without blocking.
    pub fn try_recv_all(&self) -> Vec<FileEvent> {
        self.drain()
    }

    /// Block until an event is queued or `timeout` runs out, taking none.
    pub fn wait(&self, timeout: Duration) {
        if lock(&self.waited).is_empty() {
            if let Ok(event) = lock(&self.rx).recv_timeout(timeout) {
                lock(&self.waited).push_back(event);
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Translate each raw event and pass it on, first watching any directory it
/// creates, so nothing written inside the directory after its creation is
/// reported goes unseen. Ends once the [`Watcher`] or its reader is dropped.
fn forward(
    raw_rx: &mpsc::Receiver<Event>,
    tx: &mpsc::Sender<FileEvent>,
    watcher: &Weak<Mutex<RecommendedWatcher>>,
    scopes: &[WatchScope],
) {
    for event in raw_rx {
        for fe in translate_event(&event) {
            if let FileEvent::Created(path) = &fe
                && is_real_dir(path)
            {
                let Some(watcher) = watcher.upgrade() else {
                    return;
                };
                let mut watcher = watcher.lock().expect("watching a directory does not panic");
                watch_tree_quietly(scopes, path, &walk_scope, &mut |dir| {
                    watcher.watch(dir, RecursiveMode::NonRecursive)
                });
            }
            if fe == FileEvent::Rescan && PER_DIRECTORY {
                let Some(watcher) = watcher.upgrade() else {
                    return;
                };
                let mut watcher = watcher.lock().expect("watching a directory does not panic");
                // Directories created while events were being dropped have no
                // watch yet.
                for scope in scopes {
                    watch_tree_quietly(scopes, &scope.root, &walk_scope, &mut |dir| {
                        watcher.watch(dir, RecursiveMode::NonRecursive)
                    });
                }
            }
            if tx.send(fe).is_err() {
                return;
            }
        }
    }
}

/// A directory, not a symlink to one: the scan does not follow links, so
/// nothing behind one is indexed.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Translate a notify Event into zero or more FileEvents.
fn translate_event(event: &Event) -> Vec<FileEvent> {
    let mut results = Vec::new();

    for path in &event.paths {
        let fe = match event.kind {
            EventKind::Create(_) => Some(FileEvent::Created(path.clone())),
            // A rename OUT of the tree (inotify `IN_MOVED_FROM`) leaves no file
            // behind, so it is a removal — otherwise the moved-away file's rows
            // would persist.
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                Some(FileEvent::Deleted(path.clone()))
            }
            // inotify follows a matched `From`/`To` pair with a `Both` naming
            // the same two paths; both sides were already translated above.
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => None,
            // A rename INTO the tree puts a path here that did not exist a
            // moment ago — a creation, and for a directory one the watcher must
            // walk, since the OS reports nothing for the files already inside.
            EventKind::Modify(ModifyKind::Name(_)) => Some(FileEvent::Created(path.clone())),
            EventKind::Modify(_) => Some(FileEvent::Modified(path.clone())),
            EventKind::Remove(_) => Some(FileEvent::Deleted(path.clone())),
            _ => None,
        };
        if let Some(fe) = fe {
            results.push(fe);
        }
    }

    if event.need_rescan() {
        results.push(FileEvent::Rescan);
    }

    results
}

// Test fixtures: build `notify::Event`s and `notify::Error`s without the unit tests naming the
// `notify::event::*` inner-kind types (the unit-lint isolation rule).
// `translate_event` matches only the outer variant, so the inner kind is an
// arbitrary valid value.
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
fn rename_from_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::From,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn rename_to_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::To,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn rename_any_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Any,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn rename_both_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )),
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
fn rescan_event() -> Event {
    Event {
        kind: EventKind::Other,
        paths: Vec::new(),
        attrs: Default::default(),
    }
    .set_flag(notify::event::Flag::Rescan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_real_dir_is_true_only_for_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("f");
        std::fs::write(&file, "").unwrap();
        assert!(is_real_dir(root.path()));
        assert!(!is_real_dir(&file));
        assert!(!is_real_dir(&root.path().join("missing")));
    }

    fn forwarded(events: Vec<Event>) -> Vec<FileEvent> {
        let (raw_tx, raw_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        for event in events {
            raw_tx.send(event).unwrap();
        }
        drop(raw_tx);
        forward(&raw_rx, &tx, &Weak::new(), &[]);
        drop(tx);
        rx.iter().collect()
    }

    #[test]
    fn forward_passes_translated_events_on_in_order() {
        let got = forwarded(vec![
            modify_event(vec![PathBuf::from("/nowhere/a")]),
            remove_event(vec![PathBuf::from("/nowhere/b")]),
        ]);
        assert_eq!(
            got,
            vec![
                FileEvent::Modified(PathBuf::from("/nowhere/a")),
                FileEvent::Deleted(PathBuf::from("/nowhere/b")),
            ]
        );
    }

    #[test]
    fn forward_stops_at_a_new_directory_once_the_watcher_is_gone() {
        let root = tempfile::tempdir().unwrap();
        let got = forwarded(vec![
            create_event(vec![root.path().to_path_buf()]),
            modify_event(vec![PathBuf::from("/nowhere/a")]),
        ]);
        assert!(got.is_empty());
    }

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
    fn translate_event_turns_a_rescan_flag_into_a_rescan() {
        assert_eq!(translate_event(&rescan_event()), vec![FileEvent::Rescan]);
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
    fn translate_event_maps_rename_from_to_delete() {
        // inotify emits `Modify(Name(From))` when a file is renamed OUT of the
        // watched tree; it must be treated as a removal so its rows are deleted.
        let results = translate_event(&rename_from_event(vec![PathBuf::from("/tmp/moved.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Deleted(PathBuf::from("/tmp/moved.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_rename_to_as_create() {
        // inotify emits `Modify(Name(To))` when a path is renamed INTO the
        // watched tree; it is a creation so a moved-in directory gets walked.
        let results = translate_event(&rename_to_event(vec![PathBuf::from("/tmp/arrived")]));
        assert_eq!(
            results,
            vec![FileEvent::Created(PathBuf::from("/tmp/arrived"))]
        );
    }

    #[test]
    fn translate_event_maps_rename_any_as_create() {
        // kqueue / FSEvents cannot tell which side of a rename they saw; a
        // creation of a path that turns out not to exist is a harmless no-op.
        let results = translate_event(&rename_any_event(vec![PathBuf::from("/tmp/either")]));
        assert_eq!(
            results,
            vec![FileEvent::Created(PathBuf::from("/tmp/either"))]
        );
    }

    #[test]
    fn translate_event_ignores_rename_both() {
        // `Both` restates a `From`/`To` pair inotify already delivered; acting
        // on it would index the destination twice.
        let results = translate_event(&rename_both_event(vec![
            PathBuf::from("/tmp/old"),
            PathBuf::from("/tmp/new"),
        ]));
        assert!(results.is_empty());
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
