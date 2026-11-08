use crate::matcher::TableMatcher;
use crate::scanner::{reaches_dir, scan_dirs};
use notify::event::{Flag, ModifyKind, RenameMode};
use notify::{
    Config, ErrorKind, Event, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher as NotifyWatcher,
};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const EMFILE: i32 = 24;

/// The kernel's default `fs.inotify.max_queued_events`. A channel holding
/// this many undrained events takes no more; the backlog ends in a
/// [`FileEvent::Rescan`] instead.
const QUEUE_BOUND: usize = 16384;

/// How long the channel may stay silent while a sync cookie is awaited.
const COOKIE_TIMEOUT: Duration = Duration::from_secs(10);

/// Callback errors kept for a stalled barrier's message.
const FAILURES_KEPT: usize = 16;

static COOKIE_DIRS: AtomicU64 = AtomicU64::new(0);

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Created(PathBuf),
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
    failures: Arc<Mutex<Vec<String>>>,
    cookies: Option<Cookies>,
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
        let (raw_tx, raw_rx) = mpsc::sync_channel::<Event>(QUEUE_BOUND);
        let (tx, rx) = mpsc::sync_channel(QUEUE_BOUND);
        let overflowed = Arc::new(AtomicBool::new(false));
        let failures = Arc::new(Mutex::new(Vec::new()));

        let mut watcher = RecommendedWatcher::new(
            {
                let overflowed = Arc::clone(&overflowed);
                let failures = Arc::clone(&failures);
                move |res| relay(res, &raw_tx, &overflowed, &failures)
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
        // Only inotify promises one in-order queue for the whole instance,
        // which is what makes a cookie a barrier.
        let cookies = if cfg!(target_os = "linux") {
            let cookies = Cookies::create().map_err(notify::Error::io)?;
            watcher.watch(&cookies.dir, RecursiveMode::NonRecursive)?;
            Some(cookies)
        } else {
            None
        };

        let watcher = Arc::new(Mutex::new(watcher));
        let handle = Arc::downgrade(&watcher);
        thread::spawn(move || forward(&raw_rx, &tx, &handle, &scopes, &overflowed));

        Ok(Self {
            _watcher: watcher,
            rx,
            failures,
            cookies,
        })
    }

    /// Receive the next event, blocking until one is available.
    pub fn recv(&self) -> Option<FileEvent> {
        loop {
            let event = self.rx.recv().ok()?;
            if !self.is_cookie(&event) {
                return Some(event);
            }
        }
    }

    /// Try to receive an event with a timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self.rx.recv_timeout(left).ok()?;
            if !self.is_cookie(&event) {
                return Some(event);
            }
        }
    }

    /// The events already copied off the OS queue, without waiting for more.
    pub fn queued(&self) -> Vec<FileEvent> {
        self.rx.try_iter().filter(|e| !self.is_cookie(e)).collect()
    }

    /// Every event the OS queued before this call. Where the watcher has a
    /// sync cookie, that includes those not yet copied off the OS queue;
    /// elsewhere, only those already copied.
    pub fn try_recv_all(&self) -> Result<Vec<FileEvent>, notify::Error> {
        self.flush()
    }

    /// Touch a fresh cookie and read the channel until its creation arrives,
    /// returning everything read before it. Without cookies, drains what the
    /// channel holds.
    pub fn flush(&self) -> Result<Vec<FileEvent>, notify::Error> {
        let Some(cookies) = &self.cookies else {
            return Ok(self.rx.try_iter().collect());
        };
        read_to_cookie(cookies, &mut || self.rx.recv_timeout(COOKIE_TIMEOUT).ok()).map_err(
            |missing| match missing {
                Missing::Touch(e) => notify::Error::io(e),
                Missing::Cookie(path) => notify::Error::generic(&stalled(
                    &path,
                    &self
                        .failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                )),
            },
        )
    }

    fn is_cookie(&self, event: &FileEvent) -> bool {
        self.cookies.as_ref().is_some_and(|c| c.holds(event))
    }
}

/// A directory of the watcher's own under the OS temp dir, watched on the
/// same instance as the tree, where cookies are touched.
struct Cookies {
    dir: PathBuf,
    next: Cell<u64>,
}

impl Cookies {
    fn create() -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        loop {
            let dir = base.join(format!(
                "dirsql-cookies-{}-{}",
                std::process::id(),
                COOKIE_DIRS.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    return Ok(Self {
                        dir: dir.canonicalize()?,
                        next: Cell::new(0),
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for Cookies {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Where sync cookies are touched and recognised.
trait CookieJar {
    fn touch(&self) -> std::io::Result<PathBuf>;
    fn discard(&self, cookie: &Path);
    fn holds(&self, event: &FileEvent) -> bool;
}

impl CookieJar for Cookies {
    fn touch(&self) -> std::io::Result<PathBuf> {
        let n = self.next.get();
        self.next.set(n + 1);
        let cookie = self.dir.join(n.to_string());
        std::fs::File::create(&cookie)?;
        Ok(cookie)
    }

    fn discard(&self, cookie: &Path) {
        let _ = std::fs::remove_file(cookie);
    }

    fn holds(&self, event: &FileEvent) -> bool {
        match event {
            FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => {
                p.starts_with(&self.dir)
            }
            FileEvent::Rescan => false,
        }
    }
}

/// Why a barrier ended without its cookie.
#[derive(Debug)]
enum Missing {
    Touch(std::io::Error),
    Cookie(PathBuf),
}

/// Touch a cookie, then take events from `next` until the cookie's creation
/// arrives, keeping every non-cookie event before it. An overflow may have
/// dropped the cookie, so a [`FileEvent::Rescan`] swaps in a fresh one.
fn read_to_cookie(
    jar: &dyn CookieJar,
    next: &mut dyn FnMut() -> Option<FileEvent>,
) -> Result<Vec<FileEvent>, Missing> {
    let mut cookie = jar.touch().map_err(Missing::Touch)?;
    let mut events = Vec::new();
    loop {
        let Some(event) = next() else {
            return Err(Missing::Cookie(cookie));
        };
        if event == FileEvent::Created(cookie.clone()) {
            jar.discard(&cookie);
            return Ok(events);
        }
        if jar.holds(&event) {
            continue;
        }
        if event == FileEvent::Rescan {
            jar.discard(&cookie);
            cookie = jar.touch().map_err(Missing::Touch)?;
        }
        events.push(event);
    }
}

/// The message for a barrier whose cookie never arrived.
fn stalled(cookie: &Path, failures: &[String]) -> String {
    let mut msg = format!(
        "the file watcher's sync cookie {} did not arrive within {}s",
        cookie.display(),
        COOKIE_TIMEOUT.as_secs()
    );
    if !failures.is_empty() {
        msg.push_str("; the watcher reported: ");
        msg.push_str(&failures.join("; "));
    }
    msg
}

/// notify's callback. Never blocks: notify's thread also answers the
/// forwarder's watch requests, so blocking it on a full channel the
/// forwarder drains would deadlock both. An event that does not fit is
/// dropped and a rescan owed in its place, as the kernel does on overflow.
fn relay(
    res: Result<Event, notify::Error>,
    raw_tx: &mpsc::SyncSender<Event>,
    overflowed: &AtomicBool,
    failures: &Mutex<Vec<String>>,
) {
    let event = match res {
        Ok(event) => event,
        Err(e) => {
            let mut failures = failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if failures.len() < FAILURES_KEPT {
                failures.push(e.to_string());
            }
            return;
        }
    };
    if overflowed.swap(false, Ordering::SeqCst) && raw_tx.try_send(overflow_event()).is_err() {
        overflowed.store(true, Ordering::SeqCst);
        return;
    }
    if let Err(mpsc::TrySendError::Full(_)) = raw_tx.try_send(event) {
        overflowed.store(true, Ordering::SeqCst);
        // Wakes a forwarder that emptied the channel before the flag was
        // set, so the owed rescan is not left waiting for the next event.
        let _ = raw_tx.try_send(Event::new(EventKind::Other));
    }
}

/// What notify reports for an inotify queue overflow.
fn overflow_event() -> Event {
    Event::new(EventKind::Other).set_flag(Flag::Rescan)
}

/// The next raw event, or the rescan owed for dropped ones once every event
/// queued ahead of the drop has been taken. `None` once the sender is gone.
fn next_raw(raw_rx: &mpsc::Receiver<Event>, overflowed: &AtomicBool) -> Option<Event> {
    match raw_rx.try_recv() {
        Ok(event) => Some(event),
        Err(mpsc::TryRecvError::Empty) if overflowed.swap(false, Ordering::SeqCst) => {
            Some(overflow_event())
        }
        Err(mpsc::TryRecvError::Empty) => raw_rx.recv().ok(),
        Err(mpsc::TryRecvError::Disconnected) => None,
    }
}

/// Translate each raw event and pass it on, first watching any directory it
/// creates, so nothing written inside the directory after its creation is
/// reported goes unseen. Ends once the [`Watcher`] or its reader is dropped.
fn forward(
    raw_rx: &mpsc::Receiver<Event>,
    tx: &mpsc::SyncSender<FileEvent>,
    watcher: &Weak<Mutex<RecommendedWatcher>>,
    scopes: &[WatchScope],
    overflowed: &AtomicBool,
) {
    while let Some(event) = next_raw(raw_rx, overflowed) {
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
        let (raw_tx, raw_rx) = mpsc::sync_channel(QUEUE_BOUND);
        let (tx, rx) = mpsc::sync_channel(QUEUE_BOUND);
        for event in events {
            raw_tx.send(event).unwrap();
        }
        drop(raw_tx);
        forward(&raw_rx, &tx, &Weak::new(), &[], &AtomicBool::new(false));
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
    fn forward_stops_once_its_reader_is_gone() {
        let (raw_tx, raw_rx) = mpsc::sync_channel(QUEUE_BOUND);
        let (tx, rx) = mpsc::sync_channel(QUEUE_BOUND);
        drop(rx);
        raw_tx
            .send(modify_event(vec![PathBuf::from("/nowhere/a")]))
            .unwrap();
        let overflowed = AtomicBool::new(false);
        forward(&raw_rx, &tx, &Weak::new(), &[], &overflowed);
        raw_tx
            .send(modify_event(vec![PathBuf::from("/nowhere/b")]))
            .unwrap();
        assert_eq!(raw_rx.try_iter().count(), 1, "the forwarder took one event");
    }

    #[test]
    fn next_raw_takes_queued_events_before_an_owed_rescan() {
        let (raw_tx, raw_rx) = mpsc::sync_channel(QUEUE_BOUND);
        let overflowed = AtomicBool::new(true);
        raw_tx
            .send(modify_event(vec![PathBuf::from("/a")]))
            .unwrap();
        assert_eq!(
            next_raw(&raw_rx, &overflowed).map(|e| e.need_rescan()),
            Some(false)
        );
        assert_eq!(
            next_raw(&raw_rx, &overflowed).map(|e| e.need_rescan()),
            Some(true)
        );
        assert!(!overflowed.load(Ordering::SeqCst));
        drop(raw_tx);
        assert!(next_raw(&raw_rx, &overflowed).is_none());
    }

    #[test]
    fn next_raw_waits_for_an_event_when_nothing_is_owed() {
        let (raw_tx, raw_rx) = mpsc::sync_channel(QUEUE_BOUND);
        raw_tx
            .send(modify_event(vec![PathBuf::from("/a")]))
            .unwrap();
        drop(raw_tx);
        let overflowed = AtomicBool::new(false);
        assert!(next_raw(&raw_rx, &overflowed).is_some());
        assert!(next_raw(&raw_rx, &overflowed).is_none());
        assert!(!overflowed.load(Ordering::SeqCst));
    }

    fn relayed(
        bound: usize,
        events: Vec<Result<Event, notify::Error>>,
    ) -> (Vec<Event>, bool, Vec<String>) {
        let (raw_tx, raw_rx) = mpsc::sync_channel(bound);
        let overflowed = AtomicBool::new(false);
        let failures = Mutex::new(Vec::new());
        for event in events {
            relay(event, &raw_tx, &overflowed, &failures);
        }
        (
            raw_rx.try_iter().collect(),
            overflowed.load(Ordering::SeqCst),
            failures.into_inner().unwrap(),
        )
    }

    #[test]
    fn relay_passes_events_on_in_order() {
        let (got, overflowed, failures) = relayed(
            4,
            vec![
                Ok(modify_event(vec![PathBuf::from("/a")])),
                Ok(remove_event(vec![PathBuf::from("/b")])),
            ],
        );
        assert_eq!(
            got.iter().flat_map(translate_event).collect::<Vec<_>>(),
            vec![
                FileEvent::Modified(PathBuf::from("/a")),
                FileEvent::Deleted(PathBuf::from("/b"))
            ]
        );
        assert!(!overflowed);
        assert!(failures.is_empty());
    }

    #[test]
    fn relay_owes_a_rescan_for_an_event_that_does_not_fit() {
        let (got, overflowed, _) = relayed(
            1,
            vec![
                Ok(modify_event(vec![PathBuf::from("/a")])),
                Ok(modify_event(vec![PathBuf::from("/b")])),
                Ok(modify_event(vec![PathBuf::from("/c")])),
            ],
        );
        assert_eq!(
            got.iter().flat_map(translate_event).collect::<Vec<_>>(),
            vec![FileEvent::Modified(PathBuf::from("/a"))]
        );
        assert!(overflowed);
    }

    #[test]
    fn relay_sends_the_owed_rescan_ahead_of_the_next_event() {
        let (raw_tx, raw_rx) = mpsc::sync_channel(2);
        let overflowed = AtomicBool::new(true);
        let failures = Mutex::new(Vec::new());
        relay(
            Ok(modify_event(vec![PathBuf::from("/a")])),
            &raw_tx,
            &overflowed,
            &failures,
        );
        assert_eq!(
            raw_rx
                .try_iter()
                .flat_map(|e| translate_event(&e))
                .collect::<Vec<_>>(),
            vec![FileEvent::Rescan, FileEvent::Modified(PathBuf::from("/a"))]
        );
        assert!(!overflowed.load(Ordering::SeqCst));
    }

    #[test]
    fn relay_wakes_the_forwarder_when_an_event_is_dropped() {
        let (raw_tx, raw_rx) = mpsc::sync_channel(1);
        let overflowed = AtomicBool::new(false);
        let failures = Mutex::new(Vec::new());
        raw_tx
            .send(modify_event(vec![PathBuf::from("/a")]))
            .unwrap();
        relay(
            Ok(modify_event(vec![PathBuf::from("/b")])),
            &raw_tx,
            &overflowed,
            &failures,
        );
        assert!(overflowed.load(Ordering::SeqCst));
        assert!(raw_rx.try_recv().is_ok());
        relay(
            Ok(modify_event(vec![PathBuf::from("/c")])),
            &raw_tx,
            &overflowed,
            &failures,
        );
        assert_eq!(
            raw_rx.try_recv().map(|e| e.need_rescan()),
            Ok(true),
            "the owed rescan takes the freed slot"
        );
        assert!(
            overflowed.load(Ordering::SeqCst),
            "the event behind it is dropped too"
        );
    }

    #[test]
    fn relay_keeps_a_bounded_list_of_callback_errors() {
        let errors = (0..FAILURES_KEPT + 3)
            .map(|_| Err(max_files_watch_error()))
            .collect();
        let (got, overflowed, failures) = relayed(4, errors);
        assert!(got.is_empty());
        assert!(!overflowed);
        assert_eq!(failures.len(), FAILURES_KEPT);
        assert_eq!(failures[0], max_files_watch_error().to_string());
    }

    struct FakeJar {
        touched: std::cell::RefCell<Vec<PathBuf>>,
        discarded: std::cell::RefCell<Vec<PathBuf>>,
        fail: bool,
    }

    impl FakeJar {
        fn new() -> Self {
            Self {
                touched: Default::default(),
                discarded: Default::default(),
                fail: false,
            }
        }
    }

    impl CookieJar for FakeJar {
        fn touch(&self) -> std::io::Result<PathBuf> {
            if self.fail {
                return Err(std::io::Error::from_raw_os_error(13));
            }
            let cookie = PathBuf::from(format!("/jar/{}", self.touched.borrow().len()));
            self.touched.borrow_mut().push(cookie.clone());
            Ok(cookie)
        }

        fn discard(&self, cookie: &Path) {
            self.discarded.borrow_mut().push(cookie.to_path_buf());
        }

        fn holds(&self, event: &FileEvent) -> bool {
            matches!(event, FileEvent::Created(p) | FileEvent::Deleted(p) if p.starts_with("/jar"))
        }
    }

    fn feed(events: Vec<FileEvent>) -> impl FnMut() -> Option<FileEvent> {
        let mut events = events.into_iter();
        move || events.next()
    }

    #[test]
    fn read_to_cookie_returns_what_arrived_before_the_cookie() {
        let jar = FakeJar::new();
        let got = read_to_cookie(
            &jar,
            &mut feed(vec![
                FileEvent::Created(PathBuf::from("/r/a")),
                FileEvent::Deleted(PathBuf::from("/jar/old")),
                FileEvent::Created(PathBuf::from("/jar/0")),
                FileEvent::Created(PathBuf::from("/r/late")),
            ]),
        )
        .unwrap();
        assert_eq!(got, vec![FileEvent::Created(PathBuf::from("/r/a"))]);
        assert_eq!(*jar.discarded.borrow(), vec![PathBuf::from("/jar/0")]);
    }

    #[test]
    fn read_to_cookie_waits_for_a_fresh_cookie_after_a_rescan() {
        let jar = FakeJar::new();
        let got = read_to_cookie(
            &jar,
            &mut feed(vec![
                FileEvent::Rescan,
                FileEvent::Created(PathBuf::from("/jar/0")),
                FileEvent::Created(PathBuf::from("/r/a")),
                FileEvent::Created(PathBuf::from("/jar/1")),
            ]),
        )
        .unwrap();
        assert_eq!(
            got,
            vec![FileEvent::Rescan, FileEvent::Created(PathBuf::from("/r/a"))]
        );
        assert_eq!(
            *jar.touched.borrow(),
            vec![PathBuf::from("/jar/0"), PathBuf::from("/jar/1")]
        );
    }

    #[test]
    fn read_to_cookie_names_the_cookie_that_never_arrived() {
        let jar = FakeJar::new();
        let missing = read_to_cookie(
            &jar,
            &mut feed(vec![FileEvent::Created(PathBuf::from("/r/a"))]),
        )
        .unwrap_err();
        assert!(
            matches!(&missing, Missing::Cookie(p) if p == Path::new("/jar/0")),
            "{missing:?}"
        );
    }

    #[test]
    fn read_to_cookie_reports_a_cookie_it_could_not_touch() {
        let jar = FakeJar {
            fail: true,
            ..FakeJar::new()
        };
        let missing = read_to_cookie(&jar, &mut feed(Vec::new())).unwrap_err();
        assert!(matches!(missing, Missing::Touch(_)), "{missing:?}");
    }

    #[test]
    fn stalled_names_the_cookie_and_any_callback_errors() {
        let bare = stalled(Path::new("/jar/3"), &[]);
        assert_eq!(
            bare,
            "the file watcher's sync cookie /jar/3 did not arrive within 10s"
        );
        let with = stalled(Path::new("/jar/3"), &["one".into(), "two".into()]);
        assert_eq!(with, format!("{bare}; the watcher reported: one; two"));
    }

    #[test]
    fn cookies_live_in_their_own_directory_until_dropped() {
        let cookies = Cookies::create().unwrap();
        let other = Cookies::create().unwrap();
        assert_ne!(cookies.dir, other.dir);
        let dir = cookies.dir.clone();
        assert!(is_real_dir(&dir));
        let first = cookies.touch().unwrap();
        let second = cookies.touch().unwrap();
        assert_ne!(first, second);
        assert!(cookies.holds(&FileEvent::Created(first.clone())));
        assert!(cookies.holds(&FileEvent::Modified(first.clone())));
        assert!(cookies.holds(&FileEvent::Deleted(second.clone())));
        assert!(!cookies.holds(&FileEvent::Created(PathBuf::from("/r/a"))));
        assert!(!cookies.holds(&FileEvent::Rescan));
        cookies.discard(&first);
        drop(cookies);
        assert!(!is_real_dir(&dir));
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
        assert_eq!(translate_event(&overflow_event()), vec![FileEvent::Rescan]);
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
