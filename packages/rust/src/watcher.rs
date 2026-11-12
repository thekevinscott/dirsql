use crate::matcher::TableMatcher;
use crate::scanner::{reaches_dir, scan_dirs};
use notify::ErrorKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub use crate::inotify_watch::Watcher;
#[cfg(not(target_os = "linux"))]
pub use crate::notify_watch::Watcher;

const EMFILE: i32 = 24;

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    /// The OS dropped events (an inotify queue overflow); the index may have
    /// diverged from disk and must be rebuilt from a walk.
    Rescan,
    /// A directory that appeared could not be watched, so changes inside it
    /// go unseen.
    Fault(WatchFault),
}

/// Why the watcher no longer sees every change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchFault {
    /// inotify's watch limit is exhausted.
    Limit,
    /// The OS dropped events.
    Overflow,
    /// A directory could not be watched for another reason, described.
    Failed(String),
}

impl WatchFault {
    pub(crate) fn of(err: &notify::Error) -> Self {
        match err.kind {
            ErrorKind::MaxFilesWatch => Self::Limit,
            _ => Self::Failed(describe_watch_error(err, cfg!(target_os = "linux"))),
        }
    }
}

/// A tree to watch, and the skip rules the scan applies within it.
pub struct WatchScope {
    pub root: PathBuf,
    pub ignore: TableMatcher,
}

/// [`watch_tree`] from a directory that appeared after the watcher started.
/// One gone again before its watch lands has nothing to report.
pub(crate) fn watch_new_tree(
    scopes: &[WatchScope],
    dir: &Path,
    walk: &dyn Fn(&WatchScope, &Path) -> Vec<PathBuf>,
    watch: &mut dyn FnMut(&Path) -> Result<(), notify::Error>,
) -> Result<(), WatchFault> {
    watch_tree(scopes, dir, walk, &mut HashSet::new(), &mut |dir| {
        tolerate_missing(watch(dir))
    })
    .map_err(|e| WatchFault::of(&e))
}

/// Watch each directory [`dirs_to_watch`] finds from `dir` that is not in
/// `watched` yet, walking again until a walk finds nothing new. A directory
/// created below another after the walk but before that one's watch landed
/// is reported by no event; only a later walk finds it.
pub(crate) fn watch_tree(
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

pub(crate) fn walk_scope(scope: &WatchScope, dir: &Path) -> Vec<PathBuf> {
    scan_dirs(&scope.root, dir, &scope.ignore)
}

#[cfg(any(test, not(target_os = "linux")))]
/// `err` as not found when `root` does not exist: Windows reports a missing
/// path only as a generic error.
pub(crate) fn missing_root(root: &Path, err: notify::Error) -> notify::Error {
    if root.exists() {
        err
    } else {
        notify::Error::path_not_found().add_path(root.to_path_buf())
    }
}

/// A directory removed since the walk found it is no error: there is nothing
/// left to watch.
pub(crate) fn tolerate_missing(result: Result<(), notify::Error>) -> Result<(), notify::Error> {
    match result {
        Err(e) if is_not_found(&e) => Ok(()),
        other => other,
    }
}

pub(crate) fn is_not_found(error: &notify::Error) -> bool {
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
    fn missing_root_is_not_found_only_when_the_root_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        assert!(is_not_found(&missing_root(&gone, io_error(13))));
        assert!(!is_not_found(&missing_root(dir.path(), io_error(13))));
    }

    #[test]
    fn watch_new_tree_stops_at_a_directory_it_cannot_watch() {
        let scopes = [scope("/r", &[])];
        let mut seen = Vec::new();
        let result = watch_new_tree(
            &scopes,
            Path::new("/r"),
            &|_, dir| vec![dir.to_path_buf(), dir.join("a"), dir.join("b")],
            &mut |dir| {
                seen.push(dir.to_path_buf());
                if dir.ends_with("a") {
                    Err(max_files_watch_error())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, Err(WatchFault::Limit));
        assert_eq!(seen, vec![PathBuf::from("/r"), PathBuf::from("/r/a")]);
    }

    #[test]
    fn watch_new_tree_passes_a_directory_gone_before_its_watch() {
        let scopes = [scope("/r", &[])];
        let mut seen = Vec::new();
        let result = watch_new_tree(
            &scopes,
            Path::new("/r"),
            &|_, dir| vec![dir.to_path_buf(), dir.join("a")],
            &mut |dir| {
                seen.push(dir.to_path_buf());
                Err(io_error(2))
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(seen, vec![PathBuf::from("/r"), PathBuf::from("/r/a")]);
    }

    #[test]
    fn a_fault_is_the_limit_for_max_files_watch_and_described_otherwise() {
        assert_eq!(WatchFault::of(&max_files_watch_error()), WatchFault::Limit);
        let other = io_error(13);
        assert_eq!(
            WatchFault::of(&other),
            WatchFault::Failed(describe_watch_error(&other, cfg!(target_os = "linux")))
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
}
