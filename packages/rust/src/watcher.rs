use crate::matcher::TableMatcher;
use crate::scanner::{reaches_dir, scan_dirs};
use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RenameMode};
use notify::{
    Config, ErrorKind, Event, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher as NotifyWatcher,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

const EMFILE: i32 = 24;

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Created(PathBuf),
    /// A path renamed into the tree, whose contents were complete before it
    /// arrived.
    MovedIn(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    /// The OS dropped events (an inotify queue overflow); the index may have
    /// diverged from disk and must be rebuilt from a walk.
    Rescan,
}

/// A tree to watch, and the skip rules the scan applies within it.
pub struct WatchScope {
    pub root: PathBuf,
    pub ignore: TableMatcher,
}

/// inotify and kqueue spend a kernel watch on every directory, so only those
/// the scan enters get one. FSEvents and ReadDirectoryChangesW cover a whole
/// tree with one watch, where a watch per directory would cost far more.
const PER_DIRECTORY: bool = !cfg!(any(target_os = "macos", target_os = "windows"));

/// Wraps notify::RecommendedWatcher and translates raw events into FileEvent values.
pub struct Watcher {
    _watcher: Arc<Mutex<RecommendedWatcher>>,
    rx: mpsc::Receiver<FileEvent>,
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
            if let FileEvent::Created(path) | FileEvent::MovedIn(path) = &fe
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

/// [`watch_tree`] from `dir`, carrying on past any directory that cannot be
/// watched: one gone again before its watch lands has nothing to report.
fn watch_tree_quietly(
    scopes: &[WatchScope],
    dir: &Path,
    walk: &dyn Fn(&WatchScope, &Path) -> Vec<PathBuf>,
    watch: &mut dyn FnMut(&Path) -> Result<(), notify::Error>,
) {
    let _ = watch_tree(scopes, dir, walk, &mut HashSet::new(), &mut |dir| {
        let _ = tolerate_missing(watch(dir));
        Ok(())
    });
}

/// Watch each directory [`dirs_to_watch`] finds from `dir` that is not in
/// `watched` yet, walking again until a walk finds nothing new. A directory
/// created below another after the walk but before that one's watch landed
/// is reported by no event; only a later walk finds it.
fn watch_tree(
    scopes: &[WatchScope],
    dir: &Path,
    walk: &dyn Fn(&WatchScope, &Path) -> Vec<PathBuf>,
    watched: &mut HashSet<PathBuf>,
    watch: &mut dyn FnMut(&Path) -> Result<(), notify::Error>,
) -> Result<(), notify::Error> {
    loop {
        let mut found_new = false;
        for found in dirs_to_watch(scopes, dir, walk) {
            if watched.insert(found.clone()) {
                found_new = true;
                watch(&found)?;
            }
        }
        if !found_new {
            return Ok(());
        }
    }
}

/// A directory, not a symlink to one: the scan does not follow links, so
/// nothing behind one is indexed.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn walk_scope(scope: &WatchScope, dir: &Path) -> Vec<PathBuf> {
    scan_dirs(&scope.root, dir, &scope.ignore)
}

/// A directory removed since the walk found it is no error: there is nothing
/// left to watch.
fn tolerate_missing(result: Result<(), notify::Error>) -> Result<(), notify::Error> {
    match result {
        Err(e) if is_not_found(&e) => Ok(()),
        other => other,
    }
}

fn is_not_found(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::PathNotFound => true,
        notify::ErrorKind::Io(io) => io.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

/// The directories to watch once `dir` exists: for each scope whose walk
/// enters `dir`, what `walk` finds from there, each directory once.
fn dirs_to_watch(
    scopes: &[WatchScope],
    dir: &Path,
    walk: &dyn Fn(&WatchScope, &Path) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut dirs = Vec::new();
    for scope in scopes {
        let Ok(rel) = dir.strip_prefix(&scope.root) else {
            continue;
        };
        if !reaches_dir(rel, &scope.ignore) {
            continue;
        }
        for found in walk(scope, dir) {
            if seen.insert(found.clone()) {
                dirs.push(found);
            }
        }
    }
    dirs
}

/// The message for a watcher that failed to start. On inotify, the opaque
/// limit errors name the exhausted limit and the sysctl that raises it.
pub(crate) fn describe_watch_error(err: &notify::Error, inotify: bool) -> String {
    let base = err.to_string();
    if !inotify {
        return base;
    }
    let (limit, sysctl) = match &err.kind {
        ErrorKind::Io(io) if io.raw_os_error() == Some(EMFILE) => {
            ("inotify instance limit", "fs.inotify.max_user_instances")
        }
        ErrorKind::MaxFilesWatch => ("inotify watch limit", "fs.inotify.max_user_watches"),
        _ => return base,
    };
    format!(
        "{base}; the {limit} is likely exhausted: raise it with `sudo sysctl {sysctl}=<higher value>`"
    )
}

/// inotify reports a file's creation when its writer opens it, before the
/// bytes land, so a file is read once the writer closes it or it is renamed
/// into place. Elsewhere no close is reported, so a create or write is read.
pub(crate) const READ_ON_CLOSE: bool = cfg!(target_os = "linux");

fn translate_event(event: &Event) -> Vec<FileEvent> {
    translate(event, READ_ON_CLOSE, &is_symlink)
}

/// Translate a notify Event into zero or more FileEvents.
fn translate(
    event: &Event,
    read_on_close: bool,
    is_symlink: &dyn Fn(&Path) -> bool,
) -> Vec<FileEvent> {
    let mut results = Vec::new();

    for path in &event.paths {
        let fe = match event.kind {
            EventKind::Create(CreateKind::Folder) => Some(FileEvent::Created(path.clone())),
            // A symlink is complete when it appears; no close follows it.
            EventKind::Create(_) if read_on_close && !is_symlink(path) => None,
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
            // moment ago, and for a directory one the watcher must walk, since
            // the OS reports nothing for the files already inside.
            EventKind::Modify(ModifyKind::Name(_)) => Some(FileEvent::MovedIn(path.clone())),
            EventKind::Modify(_) if read_on_close => None,
            EventKind::Modify(_) => Some(FileEvent::Modified(path.clone())),
            EventKind::Access(AccessKind::Close(AccessMode::Write)) if read_on_close => {
                Some(FileEvent::Modified(path.clone()))
            }
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

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

// Test fixtures: build `notify::Event`s and `notify::Error`s without the unit tests naming the
// `notify::event::*` inner-kind types (the unit-lint isolation rule).
#[cfg(test)]
fn create_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Create(notify::event::CreateKind::File),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn create_folder_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Create(notify::event::CreateKind::Folder),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn metadata_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Metadata(
            notify::event::MetadataKind::Any,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn close_write_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Access(notify::event::AccessKind::Close(
            notify::event::AccessMode::Write,
        )),
        paths,
        attrs: Default::default(),
    }
}

#[cfg(test)]
fn close_read_event(paths: Vec<PathBuf>) -> Event {
    Event {
        kind: EventKind::Access(notify::event::AccessKind::Close(
            notify::event::AccessMode::Read,
        )),
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
fn io_error(raw_os_error: i32) -> notify::Error {
    notify::Error::io(std::io::Error::from_raw_os_error(raw_os_error))
}

#[cfg(test)]
fn max_files_watch_error() -> notify::Error {
    notify::Error::new(ErrorKind::MaxFilesWatch)
}

#[cfg(test)]
fn path_not_found_error() -> notify::Error {
    notify::Error::path_not_found()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Effectful tests driving a real `notify` OS watcher live in
    // `tests/watcher.rs` (unit-lint isolation); only pure functions are
    // tested here.

    fn scope(root: &str, ignore: &[&str]) -> WatchScope {
        WatchScope {
            root: PathBuf::from(root),
            ignore: TableMatcher::new(&[], ignore).unwrap(),
        }
    }

    #[test]
    fn per_directory_on_linux() {
        if cfg!(target_os = "linux") {
            assert!(std::hint::black_box(PER_DIRECTORY));
        }
    }

    #[test]
    fn is_not_found_is_true_for_a_missing_path_either_way() {
        assert!(is_not_found(&path_not_found_error()));
        assert!(is_not_found(&io_error(2)));
    }

    #[test]
    fn is_not_found_is_false_for_other_errors() {
        assert!(!is_not_found(&io_error(13)));
        assert!(!is_not_found(&max_files_watch_error()));
    }

    #[test]
    fn tolerate_missing_passes_success_and_swallows_only_a_missing_path() {
        assert!(tolerate_missing(Ok(())).is_ok());
        assert!(tolerate_missing(Err(path_not_found_error())).is_ok());
        assert!(tolerate_missing(Err(io_error(2))).is_ok());
        assert!(tolerate_missing(Err(io_error(13))).is_err());
        assert!(tolerate_missing(Err(max_files_watch_error())).is_err());
    }

    #[test]
    fn is_real_dir_is_true_only_for_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("f");
        std::fs::write(&file, "").unwrap();
        assert!(is_real_dir(root.path()));
        assert!(!is_real_dir(&file));
        assert!(!is_real_dir(&root.path().join("missing")));
    }

    #[test]
    fn walk_scope_lists_the_directories_the_scope_enters() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("a").join("b")).unwrap();
        std::fs::create_dir(root.path().join("skip")).unwrap();
        let scope = WatchScope {
            root: root.path().to_path_buf(),
            ignore: TableMatcher::new(&[], &["skip/**"]).unwrap(),
        };
        let mut dirs = walk_scope(&scope, &root.path().join("a"));
        dirs.sort();
        assert_eq!(dirs, vec![root.path().join("a"), root.path().join("a/b")]);
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
            rename_from_event(vec![PathBuf::from("/nowhere/a")]),
            remove_event(vec![PathBuf::from("/nowhere/b")]),
        ]);
        assert_eq!(
            got,
            vec![
                FileEvent::Deleted(PathBuf::from("/nowhere/a")),
                FileEvent::Deleted(PathBuf::from("/nowhere/b")),
            ]
        );
    }

    #[test]
    fn forward_stops_at_a_new_directory_once_the_watcher_is_gone() {
        let root = tempfile::tempdir().unwrap();
        let got = forwarded(vec![
            create_folder_event(vec![root.path().to_path_buf()]),
            remove_event(vec![PathBuf::from("/nowhere/a")]),
        ]);
        assert!(got.is_empty());
    }

    fn unwalked(_: &WatchScope, _: &Path) -> Vec<PathBuf> {
        panic!("a directory the scan skips is never walked")
    }

    #[test]
    fn dirs_to_watch_walks_a_directory_the_scan_enters() {
        let scopes = [scope("/r", &["node_modules/**"])];
        let dirs = dirs_to_watch(&scopes, Path::new("/r/src"), &|_, dir| {
            vec![dir.to_path_buf(), dir.join("deep")]
        });
        assert_eq!(
            dirs,
            vec![PathBuf::from("/r/src"), PathBuf::from("/r/src/deep")]
        );
    }

    #[test]
    fn watch_tree_watches_a_directory_created_before_its_parent_was_watched() {
        let scopes = [scope("/r", &[])];
        let walks = std::cell::Cell::new(0);
        let walk = |_: &WatchScope, dir: &Path| {
            walks.set(walks.get() + 1);
            if walks.get() == 1 {
                vec![dir.to_path_buf()]
            } else {
                vec![dir.to_path_buf(), dir.join("late")]
            }
        };
        let mut seen = Vec::new();
        watch_tree(
            &scopes,
            Path::new("/r/new"),
            &walk,
            &mut HashSet::new(),
            &mut |dir| {
                seen.push(dir.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            seen,
            vec![PathBuf::from("/r/new"), PathBuf::from("/r/new/late")]
        );
    }

    #[test]
    fn watch_tree_quietly_watches_every_directory_past_a_failed_one() {
        let scopes = [scope("/r", &[])];
        let mut seen = Vec::new();
        watch_tree_quietly(
            &scopes,
            Path::new("/r"),
            &|_, dir| vec![dir.to_path_buf(), dir.join("a"), dir.join("b")],
            &mut |dir| {
                seen.push(dir.to_path_buf());
                Err(io_error(13))
            },
        );
        assert_eq!(
            seen,
            vec![
                PathBuf::from("/r"),
                PathBuf::from("/r/a"),
                PathBuf::from("/r/b")
            ]
        );
    }

    #[test]
    fn watch_tree_skips_a_directory_already_watched() {
        let scopes = [scope("/r", &[])];
        let mut watched = HashSet::from([PathBuf::from("/r")]);
        let mut seen = Vec::new();
        watch_tree(
            &scopes,
            Path::new("/r"),
            &|_, dir| vec![dir.to_path_buf(), dir.join("a")],
            &mut watched,
            &mut |dir| {
                seen.push(dir.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, vec![PathBuf::from("/r/a")]);
    }

    #[test]
    fn dirs_to_watch_skips_an_ignored_directory() {
        let scopes = [scope("/r", &["node_modules/**"])];
        assert!(dirs_to_watch(&scopes, Path::new("/r/node_modules"), &unwalked).is_empty());
        assert!(dirs_to_watch(&scopes, Path::new("/r/node_modules/pkg"), &unwalked).is_empty());
    }

    #[test]
    fn dirs_to_watch_skips_a_directory_outside_every_scope() {
        let scopes = [scope("/r", &[])];
        assert!(dirs_to_watch(&scopes, Path::new("/elsewhere"), &unwalked).is_empty());
    }

    #[test]
    fn dirs_to_watch_unites_overlapping_scopes() {
        let scopes = [scope("/r", &["x/**"]), scope("/r/x", &[])];
        let dirs = dirs_to_watch(&scopes, Path::new("/r/x/new"), &|_, dir| {
            vec![dir.to_path_buf()]
        });
        assert_eq!(dirs, vec![PathBuf::from("/r/x/new")]);
        let both = dirs_to_watch(
            &[scope("/r", &[]), scope("/r/x", &[])],
            Path::new("/r/x/new"),
            &|_, dir| vec![dir.to_path_buf()],
        );
        assert_eq!(both, vec![PathBuf::from("/r/x/new")]);
    }

    #[test]
    fn describe_watch_error_names_the_instance_limit_for_emfile_on_inotify() {
        let err = io_error(24);
        let msg = describe_watch_error(&err, true);
        assert!(msg.starts_with(&err.to_string()), "{msg}");
        assert!(msg.contains("inotify instance limit"), "{msg}");
        assert!(msg.contains("fs.inotify.max_user_instances"), "{msg}");
    }

    #[test]
    fn describe_watch_error_names_the_watch_limit_for_max_files_watch_on_inotify() {
        let err = max_files_watch_error();
        let msg = describe_watch_error(&err, true);
        assert!(msg.starts_with(&err.to_string()), "{msg}");
        assert!(msg.contains("inotify watch limit"), "{msg}");
        assert!(msg.contains("fs.inotify.max_user_watches"), "{msg}");
    }

    #[test]
    fn describe_watch_error_leaves_emfile_alone_off_inotify() {
        let err = io_error(24);
        assert_eq!(describe_watch_error(&err, false), err.to_string());
    }

    #[test]
    fn describe_watch_error_leaves_other_errors_alone() {
        let other_io = io_error(2);
        assert_eq!(describe_watch_error(&other_io, true), other_io.to_string());
        let not_found = path_not_found_error();
        assert_eq!(
            describe_watch_error(&not_found, true),
            not_found.to_string()
        );
    }

    fn elsewhere(event: &Event) -> Vec<FileEvent> {
        translate(event, false, &|_| false)
    }

    fn on_close(event: &Event) -> Vec<FileEvent> {
        translate(event, true, &|_| false)
    }

    fn one(event: fn(PathBuf) -> FileEvent, path: &str) -> Vec<FileEvent> {
        vec![event(PathBuf::from(path))]
    }

    #[test]
    fn translate_event_reads_on_close_only_on_linux() {
        let closed = close_write_event(vec![PathBuf::from("/tmp/a")]);
        assert_eq!(
            translate_event(&closed),
            translate(&closed, cfg!(target_os = "linux"), &|_| false)
        );
    }

    #[test]
    fn translate_on_close_reads_nothing_on_a_file_create() {
        assert!(on_close(&create_event(vec![PathBuf::from("/tmp/a")])).is_empty());
    }

    #[test]
    fn translate_on_close_reads_a_symlink_on_its_create() {
        let event = create_event(vec![PathBuf::from("/tmp/link")]);
        assert_eq!(
            translate(&event, true, &|path| path == Path::new("/tmp/link")),
            one(FileEvent::Created, "/tmp/link")
        );
    }

    #[test]
    fn translate_maps_a_directory_create_either_way() {
        let event = create_folder_event(vec![PathBuf::from("/tmp/d")]);
        assert_eq!(on_close(&event), one(FileEvent::Created, "/tmp/d"));
        assert_eq!(elsewhere(&event), one(FileEvent::Created, "/tmp/d"));
    }

    #[test]
    fn translate_on_close_reads_nothing_on_a_write_or_metadata_change() {
        assert!(on_close(&modify_event(vec![PathBuf::from("/tmp/a")])).is_empty());
        assert!(on_close(&metadata_event(vec![PathBuf::from("/tmp/a")])).is_empty());
    }

    #[test]
    fn translate_on_close_reads_a_file_closed_after_writing() {
        let event = close_write_event(vec![PathBuf::from("/tmp/a")]);
        assert_eq!(on_close(&event), one(FileEvent::Modified, "/tmp/a"));
        assert!(elsewhere(&event).is_empty());
    }

    #[test]
    fn translate_ignores_a_file_closed_after_reading() {
        let event = close_read_event(vec![PathBuf::from("/tmp/a")]);
        assert!(on_close(&event).is_empty());
    }

    #[test]
    fn translate_on_close_keeps_renames_and_removals() {
        let to = rename_to_event(vec![PathBuf::from("/tmp/in")]);
        let from = rename_from_event(vec![PathBuf::from("/tmp/out")]);
        let gone = remove_event(vec![PathBuf::from("/tmp/gone")]);
        assert_eq!(on_close(&to), one(FileEvent::MovedIn, "/tmp/in"));
        assert_eq!(on_close(&from), one(FileEvent::Deleted, "/tmp/out"));
        assert_eq!(on_close(&gone), one(FileEvent::Deleted, "/tmp/gone"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn translate_event_reads_a_created_path_only_when_it_is_a_symlink() {
        let link = PathBuf::from("/proc/self");
        let missing = PathBuf::from("/nowhere/a");
        assert_eq!(
            translate_event(&create_event(vec![link.clone()])),
            vec![FileEvent::Created(link)]
        );
        assert!(translate_event(&create_event(vec![missing])).is_empty());
    }

    #[test]
    fn translate_event_maps_create() {
        let results = elsewhere(&create_event(vec![PathBuf::from("/tmp/test.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Created(PathBuf::from("/tmp/test.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_remove() {
        let results = elsewhere(&remove_event(vec![PathBuf::from("/tmp/gone.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Deleted(PathBuf::from("/tmp/gone.txt"))]
        );
    }

    #[test]
    fn translate_event_turns_a_rescan_flag_into_a_rescan() {
        assert_eq!(elsewhere(&rescan_event()), vec![FileEvent::Rescan]);
    }

    #[test]
    fn translate_event_ignores_access_events() {
        let results = elsewhere(&access_event(vec![PathBuf::from("/tmp/read.txt")]));
        assert!(results.is_empty());
    }

    #[test]
    fn translate_event_maps_modify() {
        let results = elsewhere(&modify_event(vec![PathBuf::from("/tmp/changed.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Modified(PathBuf::from("/tmp/changed.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_rename_from_to_delete() {
        // inotify emits `Modify(Name(From))` when a file is renamed OUT of the
        // watched tree; it must be treated as a removal so its rows are deleted.
        let results = elsewhere(&rename_from_event(vec![PathBuf::from("/tmp/moved.txt")]));
        assert_eq!(
            results,
            vec![FileEvent::Deleted(PathBuf::from("/tmp/moved.txt"))]
        );
    }

    #[test]
    fn translate_event_maps_rename_to_as_moved_in() {
        // inotify emits `Modify(Name(To))` when a path is renamed INTO the
        // watched tree.
        let results = elsewhere(&rename_to_event(vec![PathBuf::from("/tmp/arrived")]));
        assert_eq!(
            results,
            vec![FileEvent::MovedIn(PathBuf::from("/tmp/arrived"))]
        );
    }

    #[test]
    fn translate_event_maps_rename_any_as_moved_in() {
        // kqueue / FSEvents cannot tell which side of a rename they saw; an
        // arrival of a path that turns out not to exist is a harmless no-op.
        let results = elsewhere(&rename_any_event(vec![PathBuf::from("/tmp/either")]));
        assert_eq!(
            results,
            vec![FileEvent::MovedIn(PathBuf::from("/tmp/either"))]
        );
    }

    #[test]
    fn translate_event_ignores_rename_both() {
        // `Both` restates a `From`/`To` pair inotify already delivered; acting
        // on it would index the destination twice.
        let results = elsewhere(&rename_both_event(vec![
            PathBuf::from("/tmp/old"),
            PathBuf::from("/tmp/new"),
        ]));
        assert!(results.is_empty());
    }

    #[test]
    fn translate_event_multiple_paths() {
        let results = elsewhere(&create_event(vec![
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
        let results = elsewhere(&event);
        assert!(results.is_empty());
    }

    #[test]
    fn translate_event_ignores_other_kind() {
        let event = Event {
            kind: EventKind::Other,
            paths: vec![PathBuf::from("/tmp/other.txt")],
            attrs: Default::default(),
        };
        let results = elsewhere(&event);
        assert!(results.is_empty());
    }
}
