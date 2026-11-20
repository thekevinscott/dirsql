//! The file behind the default index database: a uniquely named SQLite file
//! in SQLite's own temp directory, deleted with its journal, `-wal` and
//! `-shm` companions once every connection to it is gone.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TempDbFile {
    path: PathBuf,
}

impl TempDbFile {
    /// Create an empty, owner-only database file in the first candidate
    /// directory that accepts one.
    pub(crate) fn create() -> io::Result<Self> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no temp directory");
        for dir in candidates(|name| std::env::var_os(name)) {
            match create_in(&dir) {
                Ok(path) => return Ok(Self { path }),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDbFile {
    fn drop(&mut self) {
        for file in companions(&self.path) {
            let _ = std::fs::remove_file(file);
        }
    }
}

fn create_in(dir: &Path) -> io::Result<PathBuf> {
    loop {
        let path = dir.join(file_name(
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(_) => return Ok(path),
            // A file left by a crashed process that had the same pid.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

fn file_name(pid: u32, n: u64) -> String {
    format!("dirsql-{pid}-{n}.db")
}

/// The database file and the files SQLite keeps beside it.
fn companions(path: &Path) -> [PathBuf; 4] {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    [
        path.to_path_buf(),
        with("-journal"),
        with("-wal"),
        with("-shm"),
    ]
}

/// Where SQLite itself would put a temp file, in its order.
#[cfg(unix)]
fn candidates(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    ["SQLITE_TMPDIR", "TMPDIR"]
        .into_iter()
        .filter_map(|name| env(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .chain(["/var/tmp", "/usr/tmp", "/tmp", "."].map(PathBuf::from))
        .collect()
}

#[cfg(windows)]
fn candidates(_env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    vec![std::env::temp_dir()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_carries_the_pid_and_the_counter() {
        assert_eq!(file_name(42, 7), "dirsql-42-7.db");
    }

    #[test]
    fn companions_are_the_database_and_its_journal_wal_and_shm() {
        assert_eq!(
            companions(Path::new("/t/dirsql-1-0.db")),
            [
                PathBuf::from("/t/dirsql-1-0.db"),
                PathBuf::from("/t/dirsql-1-0.db-journal"),
                PathBuf::from("/t/dirsql-1-0.db-wal"),
                PathBuf::from("/t/dirsql-1-0.db-shm"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn candidates_follow_sqlite_order() {
        let env = |name: &str| match name {
            "SQLITE_TMPDIR" => Some(OsString::from("/sqlite")),
            "TMPDIR" => Some(OsString::from("/tmpdir")),
            _ => None,
        };
        assert_eq!(
            candidates(env),
            ["/sqlite", "/tmpdir", "/var/tmp", "/usr/tmp", "/tmp", "."].map(PathBuf::from)
        );
    }

    #[cfg(unix)]
    #[test]
    fn candidates_skip_unset_and_empty_variables() {
        let env = |name: &str| (name == "TMPDIR").then(OsString::new);
        assert_eq!(
            candidates(env),
            ["/var/tmp", "/usr/tmp", "/tmp", "."].map(PathBuf::from)
        );
    }
}
