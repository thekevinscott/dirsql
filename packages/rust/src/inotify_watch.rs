use crate::watcher::{
    FileEvent, WatchScope, tolerate_missing, walk_scope, watch_tree, watch_tree_quietly,
};
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsStr;
use std::hash::Hash;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ENOSPC: i32 = 28;

/// A file is read once its writer closes it, so a create or a write in
/// progress is never asked for; a directory's creation is, to watch it.
const MASK: WatchMask = WatchMask::CLOSE_WRITE
    .union(WatchMask::CREATE)
    .union(WatchMask::DELETE)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MOVE_SELF);

/// Reads inotify itself, one non-recursive watch per directory the scan
/// enters, so [`drain`](Self::drain) can empty the kernel's queue on demand.
pub struct Watcher {
    source: RefCell<Box<dyn Source + Send>>,
    queued: RefCell<VecDeque<FileEvent>>,
}

trait Source {
    /// Queue everything readable now, without blocking.
    fn read(&mut self, queued: &mut VecDeque<FileEvent>);
    /// Block until more may be readable, or `left` runs out.
    fn wait(&self, left: Option<Duration>) -> io::Result<()>;
}

struct Reader {
    inotify: Inotify,
    dirs: Dirs<WatchDescriptor>,
    scopes: Vec<WatchScope>,
    buffer: Vec<u8>,
}

impl Watcher {
    pub fn new(path: &Path) -> Result<Self, notify::Error> {
        Self::over(&[path.to_path_buf()])
    }

    pub fn over(paths: &[PathBuf]) -> Result<Self, notify::Error> {
        let everything =
            crate::matcher::TableMatcher::new(&[], &[]).expect("no patterns to compile");
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

    /// Watch each scope's root and every directory below it the scan would
    /// enter.
    pub fn scoped(scopes: Vec<WatchScope>) -> Result<Self, notify::Error> {
        let inotify = Inotify::init().map_err(notify::Error::io)?;
        let mut dirs = Dirs::default();
        let mut watched = HashSet::new();
        let mut add = |dir: &Path| dirs.watch(dir, &mut |dir| add_watch(&inotify, dir));
        for scope in &scopes {
            add(&scope.root)?;
            watched.insert(scope.root.clone());
        }
        for scope in &scopes {
            watch_tree(
                &scopes,
                &scope.root,
                &walk_scope,
                &mut watched,
                &mut |dir| tolerate_missing(add(dir)),
            )?;
        }
        Ok(Self::reading(Reader {
            inotify,
            dirs,
            scopes,
            buffer: vec![0; 64 * 1024],
        }))
    }

    fn reading(source: impl Source + Send + 'static) -> Self {
        Self {
            source: RefCell::new(Box::new(source)),
            queued: RefCell::new(VecDeque::new()),
        }
    }

    pub fn recv(&self) -> Option<FileEvent> {
        self.next(None)
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileEvent> {
        self.next(Some(Instant::now() + timeout))
    }

    /// Every event the kernel has queued, in order. A writer's `close` or
    /// `rename` returns only once its event is queued, so every write
    /// finished before this call is in the result.
    pub fn drain(&self) -> Vec<FileEvent> {
        let mut queued = self.queued.borrow_mut();
        self.source.borrow_mut().read(&mut queued);
        queued.drain(..).collect()
    }

    pub fn try_recv_all(&self) -> Vec<FileEvent> {
        self.drain()
    }

    fn next(&self, deadline: Option<Instant>) -> Option<FileEvent> {
        let mut source = self.source.borrow_mut();
        let mut queued = self.queued.borrow_mut();
        loop {
            source.read(&mut queued);
            if let Some(event) = queued.pop_front() {
                return Some(event);
            }
            let left = match deadline {
                Some(deadline) => Some(deadline.checked_duration_since(Instant::now())?),
                None => None,
            };
            source.wait(left).ok()?;
        }
    }
}

impl Source for Reader {
    /// Read the fd until it would block, queueing what each event amounts to.
    fn read(&mut self, queued: &mut VecDeque<FileEvent>) {
        let Self {
            inotify,
            dirs,
            scopes,
            buffer,
        } = self;
        loop {
            let watches = inotify.watches();
            let Ok(events) = inotify.read_events(buffer) else {
                return;
            };
            for event in events {
                let found = dirs.step(
                    scopes,
                    &walk_scope,
                    &event.wd,
                    event.mask,
                    event.name,
                    &mut |dir| add_watch_with(&watches, dir),
                    &mut |wd| {
                        let _ = watches.clone().remove(wd);
                    },
                );
                queued.extend(found);
            }
        }
    }

    fn wait(&self, left: Option<Duration>) -> io::Result<()> {
        let timeout = left
            .map(rustix::event::Timespec::try_from)
            .transpose()
            .map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut fds = [rustix::event::PollFd::new(
            &self.inotify,
            rustix::event::PollFlags::IN,
        )];
        match rustix::event::poll(&mut fds, timeout.as_ref()) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn add_watch(inotify: &Inotify, dir: &Path) -> Result<WatchDescriptor, notify::Error> {
    add_watch_with(&inotify.watches(), dir)
}

fn add_watch_with(
    watches: &inotify::Watches,
    dir: &Path,
) -> Result<WatchDescriptor, notify::Error> {
    watches
        .clone()
        .add(dir, MASK)
        .map_err(|e| watch_error(e).add_path(dir.to_path_buf()))
}

/// inotify reports an exhausted watch limit as `ENOSPC`.
fn watch_error(e: io::Error) -> notify::Error {
    if e.raw_os_error() == Some(ENOSPC) {
        notify::Error::new(notify::ErrorKind::MaxFilesWatch)
    } else {
        notify::Error::io(e)
    }
}

/// What a single inotify event, on a path inside a watched directory, says
/// about that path.
#[derive(Debug, PartialEq)]
enum Change {
    /// A file is finished: closed after a write, or renamed into place.
    Publish,
    /// A file is gone, or a watched directory is gone or moved.
    Remove,
    /// A directory appeared, by `mkdir` or by being renamed into place.
    DirIn,
    /// A directory left by deletion or by a rename away.
    DirOut,
}

const GONE_ITSELF: EventMask = EventMask::DELETE_SELF.union(EventMask::MOVE_SELF);
const ARRIVED: EventMask = EventMask::CREATE.union(EventMask::MOVED_TO);
const LEFT: EventMask = EventMask::DELETE.union(EventMask::MOVED_FROM);
const FINISHED: EventMask = EventMask::CLOSE_WRITE.union(EventMask::MOVED_TO);

fn classify(mask: EventMask) -> Option<Change> {
    let dir = mask.contains(EventMask::ISDIR);
    if mask.intersects(GONE_ITSELF) {
        Some(Change::Remove)
    } else if mask.intersects(ARRIVED) && dir {
        Some(Change::DirIn)
    } else if mask.intersects(LEFT) {
        Some(if dir { Change::DirOut } else { Change::Remove })
    } else if mask.intersects(FINISHED) {
        Some(Change::Publish)
    } else {
        None
    }
}

/// The path each watch stands for.
struct Dirs<W> {
    paths: HashMap<W, PathBuf>,
}

impl<W> Default for Dirs<W> {
    fn default() -> Self {
        Self {
            paths: HashMap::new(),
        }
    }
}

impl<W: Eq + Hash + Clone> Dirs<W> {
    fn watch(
        &mut self,
        dir: &Path,
        add: &mut dyn FnMut(&Path) -> Result<W, notify::Error>,
    ) -> Result<(), notify::Error> {
        let wd = add(dir)?;
        self.paths.insert(wd, dir.to_path_buf());
        Ok(())
    }

    /// Stop watching `dir` and every directory below it, handing each watch
    /// to `remove`.
    fn unwatch_below(&mut self, dir: &Path, remove: &mut dyn FnMut(W)) {
        let gone: Vec<W> = self
            .paths
            .iter()
            .filter(|(_, path)| path.starts_with(dir))
            .map(|(wd, _)| wd.clone())
            .collect();
        for wd in gone {
            self.paths.remove(&wd);
            remove(wd);
        }
    }

    /// The event one inotify event amounts to, after watching any directory
    /// it brings into the tree and unwatching any it takes out.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        scopes: &[WatchScope],
        walk: &dyn Fn(&WatchScope, &Path) -> Vec<PathBuf>,
        wd: &W,
        mask: EventMask,
        name: Option<&OsStr>,
        add: &mut dyn FnMut(&Path) -> Result<W, notify::Error>,
        remove: &mut dyn FnMut(W),
    ) -> Option<FileEvent> {
        if mask.contains(EventMask::Q_OVERFLOW) {
            // Directories created while events were being dropped have no
            // watch yet.
            for scope in scopes {
                watch_tree_quietly(scopes, &scope.root, walk, &mut |dir| self.watch(dir, add));
            }
            return Some(FileEvent::Rescan);
        }
        if mask.contains(EventMask::IGNORED) {
            self.paths.remove(wd);
            return None;
        }
        let dir = self.paths.get(wd)?;
        let path = name.map_or_else(|| dir.clone(), |name| dir.join(name));
        Some(match classify(mask)? {
            Change::Publish => FileEvent::Created(path),
            Change::Remove => FileEvent::Deleted(path),
            Change::DirIn => {
                watch_tree_quietly(scopes, &path, walk, &mut |dir| self.watch(dir, add));
                FileEvent::Created(path)
            }
            Change::DirOut => {
                self.unwatch_below(&path, remove);
                FileEvent::Deleted(path)
            }
        })
    }
}

#[cfg(test)]
fn mask(names: &[&str]) -> EventMask {
    names.iter().fold(EventMask::empty(), |mask, name| {
        mask | match *name {
            "close_write" => EventMask::CLOSE_WRITE,
            "create" => EventMask::CREATE,
            "modify" => EventMask::MODIFY,
            "attrib" => EventMask::ATTRIB,
            "delete" => EventMask::DELETE,
            "moved_from" => EventMask::MOVED_FROM,
            "moved_to" => EventMask::MOVED_TO,
            "delete_self" => EventMask::DELETE_SELF,
            "move_self" => EventMask::MOVE_SELF,
            "isdir" => EventMask::ISDIR,
            "ignored" => EventMask::IGNORED,
            "q_overflow" => EventMask::Q_OVERFLOW,
            other => panic!("no mask named {other}"),
        }
    })
}

#[cfg(test)]
fn os_error(raw_os_error: i32) -> io::Error {
    io::Error::from_raw_os_error(raw_os_error)
}

#[cfg(test)]
fn everything(root: &str) -> WatchScope {
    WatchScope {
        root: PathBuf::from(root),
        ignore: crate::matcher::TableMatcher::new(&[], &[]).unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Effectful tests reading a real inotify fd live in `tests/watcher.rs`
    // (unit-lint isolation).

    #[test]
    fn classify_publishes_a_closed_write_and_a_file_renamed_in() {
        assert_eq!(classify(mask(&["close_write"])), Some(Change::Publish));
        assert_eq!(classify(mask(&["moved_to"])), Some(Change::Publish));
    }

    #[test]
    fn classify_reads_nothing_on_create_modify_or_metadata() {
        assert_eq!(classify(mask(&["create"])), None);
        assert_eq!(classify(mask(&["modify"])), None);
        assert_eq!(classify(mask(&["attrib"])), None);
    }

    #[test]
    fn classify_removes_a_deleted_or_renamed_away_file() {
        assert_eq!(classify(mask(&["delete"])), Some(Change::Remove));
        assert_eq!(classify(mask(&["moved_from"])), Some(Change::Remove));
    }

    #[test]
    fn classify_removes_a_watched_directory_deleted_or_moved() {
        assert_eq!(classify(mask(&["delete_self"])), Some(Change::Remove));
        assert_eq!(classify(mask(&["move_self"])), Some(Change::Remove));
        assert_eq!(
            classify(mask(&["move_self", "isdir"])),
            Some(Change::Remove)
        );
    }

    #[test]
    fn classify_tells_a_directory_coming_in_from_one_going_out() {
        assert_eq!(classify(mask(&["create", "isdir"])), Some(Change::DirIn));
        assert_eq!(classify(mask(&["moved_to", "isdir"])), Some(Change::DirIn));
        assert_eq!(classify(mask(&["delete", "isdir"])), Some(Change::DirOut));
        assert_eq!(
            classify(mask(&["moved_from", "isdir"])),
            Some(Change::DirOut)
        );
    }

    #[test]
    fn watch_error_names_an_exhausted_watch_limit() {
        assert!(matches!(
            watch_error(os_error(ENOSPC)).kind,
            notify::ErrorKind::MaxFilesWatch
        ));
        assert!(matches!(
            watch_error(os_error(2)).kind,
            notify::ErrorKind::Io(ref e) if e.raw_os_error() == Some(2)
        ));
    }

    fn watching(entries: &[(u32, &str)]) -> Dirs<u32> {
        Dirs {
            paths: entries
                .iter()
                .map(|(wd, path)| (*wd, PathBuf::from(path)))
                .collect(),
        }
    }

    struct Run {
        dirs: Dirs<u32>,
        added: Vec<PathBuf>,
        removed: Vec<u32>,
    }

    fn step(
        mut dirs: Dirs<u32>,
        scopes: &[WatchScope],
        wd: u32,
        names: &[&str],
        name: Option<&str>,
    ) -> (Option<FileEvent>, Run) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut next = 100;
        let event = dirs.step(
            scopes,
            &|_, dir| vec![dir.to_path_buf(), dir.join("sub")],
            &wd,
            mask(names),
            name.map(OsStr::new),
            &mut |dir| {
                added.push(dir.to_path_buf());
                next += 1;
                Ok(next)
            },
            &mut |wd| removed.push(wd),
        );
        (
            event,
            Run {
                dirs,
                added,
                removed,
            },
        )
    }

    #[test]
    fn step_publishes_a_closed_file_at_its_path() {
        let (event, run) = step(
            watching(&[(1, "/r")]),
            &[],
            1,
            &["close_write"],
            Some("a.md"),
        );
        assert_eq!(event, Some(FileEvent::Created(PathBuf::from("/r/a.md"))));
        assert!(run.added.is_empty());
    }

    #[test]
    fn step_removes_a_deleted_file() {
        let (event, _) = step(watching(&[(1, "/r")]), &[], 1, &["delete"], Some("a.md"));
        assert_eq!(event, Some(FileEvent::Deleted(PathBuf::from("/r/a.md"))));
    }

    #[test]
    fn step_reports_nothing_for_a_created_file() {
        let (event, run) = step(watching(&[(1, "/r")]), &[], 1, &["create"], Some("a.md"));
        assert_eq!(event, None);
        assert!(run.added.is_empty());
    }

    #[test]
    fn step_drops_an_event_from_an_unknown_watch() {
        let (event, _) = step(
            watching(&[(1, "/r")]),
            &[],
            2,
            &["close_write"],
            Some("a.md"),
        );
        assert_eq!(event, None);
    }

    #[test]
    fn step_names_the_watched_directory_itself_when_the_event_has_no_name() {
        let (event, _) = step(watching(&[(1, "/r")]), &[], 1, &["delete_self"], None);
        assert_eq!(event, Some(FileEvent::Deleted(PathBuf::from("/r"))));
    }

    #[test]
    fn step_watches_a_new_directory_and_below_before_reporting_it() {
        let (event, run) = step(
            watching(&[(1, "/r")]),
            &[everything("/r")],
            1,
            &["create", "isdir"],
            Some("d"),
        );
        assert_eq!(event, Some(FileEvent::Created(PathBuf::from("/r/d"))));
        assert_eq!(
            run.added,
            vec![PathBuf::from("/r/d"), PathBuf::from("/r/d/sub")]
        );
        assert_eq!(run.dirs.paths.get(&101), Some(&PathBuf::from("/r/d")));
        assert_eq!(run.dirs.paths.get(&102), Some(&PathBuf::from("/r/d/sub")));
    }

    #[test]
    fn step_unwatches_a_directory_renamed_away_and_everything_below() {
        let (event, run) = step(
            watching(&[(1, "/r"), (2, "/r/d"), (3, "/r/d/sub"), (4, "/r/dd")]),
            &[],
            1,
            &["moved_from", "isdir"],
            Some("d"),
        );
        assert_eq!(event, Some(FileEvent::Deleted(PathBuf::from("/r/d"))));
        let mut removed = run.removed;
        removed.sort_unstable();
        assert_eq!(removed, vec![2, 3]);
        let mut left: Vec<_> = run.dirs.paths.into_keys().collect();
        left.sort_unstable();
        assert_eq!(left, vec![1, 4]);
    }

    #[test]
    fn step_forgets_a_watch_the_kernel_dropped() {
        let (event, run) = step(
            watching(&[(1, "/r"), (2, "/r/d")]),
            &[],
            2,
            &["ignored"],
            None,
        );
        assert_eq!(event, None);
        assert!(run.removed.is_empty());
        assert_eq!(run.dirs.paths.len(), 1);
        assert!(run.dirs.paths.contains_key(&1));
    }

    #[test]
    fn step_turns_an_overflow_into_a_rescan_after_rewatching_every_scope() {
        let (event, run) = step(
            watching(&[(1, "/r")]),
            &[everything("/r"), everything("/s")],
            u32::MAX,
            &["q_overflow"],
            None,
        );
        assert_eq!(event, Some(FileEvent::Rescan));
        assert_eq!(
            run.added,
            vec![
                PathBuf::from("/r"),
                PathBuf::from("/r/sub"),
                PathBuf::from("/s"),
                PathBuf::from("/s/sub"),
            ]
        );
    }

    #[test]
    fn dirs_watch_records_nothing_when_the_watch_fails() {
        let mut dirs = watching(&[]);
        let result = dirs.watch(Path::new("/r"), &mut |_| Err(watch_error(os_error(ENOSPC))));
        assert!(result.is_err());
        assert!(dirs.paths.is_empty());
    }

    /// Hands out one batch per read; a wait only fails once `fail_wait` is set.
    struct Script {
        reads: VecDeque<Vec<FileEvent>>,
        fail_wait: bool,
    }

    impl Source for Script {
        fn read(&mut self, queued: &mut VecDeque<FileEvent>) {
            queued.extend(self.reads.pop_front().unwrap_or_default());
        }

        fn wait(&self, _left: Option<Duration>) -> io::Result<()> {
            if self.fail_wait {
                Err(io::ErrorKind::Other.into())
            } else {
                Ok(())
            }
        }
    }

    fn scripted(reads: Vec<Vec<FileEvent>>, fail_wait: bool) -> Watcher {
        Watcher::reading(Script {
            reads: reads.into(),
            fail_wait,
        })
    }

    fn created(path: &str) -> FileEvent {
        FileEvent::Created(PathBuf::from(path))
    }

    #[test]
    fn recv_returns_what_the_first_read_found_in_order() {
        let watcher = scripted(vec![vec![created("/r/a"), created("/r/b")]], false);
        assert_eq!(watcher.recv(), Some(created("/r/a")));
        assert_eq!(watcher.recv(), Some(created("/r/b")));
    }

    #[test]
    fn recv_waits_and_reads_again_until_something_arrives() {
        let watcher = scripted(vec![vec![], vec![], vec![created("/r/a")]], false);
        assert_eq!(watcher.recv(), Some(created("/r/a")));
    }

    #[test]
    fn recv_timeout_reads_again_after_waiting_within_the_deadline() {
        let watcher = scripted(vec![vec![], vec![created("/r/a")]], false);
        assert_eq!(
            watcher.recv_timeout(Duration::from_secs(60)),
            Some(created("/r/a"))
        );
    }

    #[test]
    fn recv_timeout_gives_up_once_the_deadline_passes() {
        let watcher = scripted(vec![], false);
        assert_eq!(watcher.recv_timeout(Duration::ZERO), None);
    }

    #[test]
    fn recv_gives_up_when_waiting_fails() {
        let watcher = scripted(vec![vec![]], true);
        assert_eq!(watcher.recv(), None);
    }

    #[test]
    fn drain_returns_every_event_queued_so_far() {
        let watcher = scripted(vec![vec![created("/r/a"), created("/r/b")]], false);
        assert_eq!(watcher.drain(), vec![created("/r/a"), created("/r/b")]);
        assert_eq!(watcher.try_recv_all(), vec![]);
    }

    #[test]
    fn drain_hands_over_what_an_earlier_read_left_queued() {
        let watcher = scripted(vec![vec![created("/r/a"), created("/r/b")]], false);
        assert_eq!(watcher.recv(), Some(created("/r/a")));
        assert_eq!(watcher.try_recv_all(), vec![created("/r/b")]);
    }
}
