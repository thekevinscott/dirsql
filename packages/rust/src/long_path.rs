//! Opening and stat'ing paths longer than the platform's path limit.
//!
//! A Unix kernel refuses a path past `PATH_MAX` whole, so a refused path is
//! opened again a stretch of components at a time, each stretch relative to
//! the directory the one before reached. Windows needs none of this: std
//! already hands long paths to it in their `\\?\` form.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;

#[cfg(unix)]
use rustix::fd::OwnedFd;
#[cfg(unix)]
use rustix::fs::OFlags;
#[cfg(unix)]
use rustix::io::Errno;

/// The most bytes of path one `openat` is handed: within every Unix
/// `PATH_MAX`, macOS's 1024 the smallest.
#[cfg(unix)]
const STRETCH: usize = 1000;

/// How a directory part-way down a long path is opened: only to resolve the
/// rest from, so on Linux without needing read permission on it, as a whole
/// path would not.
#[cfg(any(target_os = "linux", target_os = "android"))]
const THROUGH: OFlags = OFlags::PATH.union(OFlags::DIRECTORY);
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const THROUGH: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY);

/// How a path is opened only to stat it.
#[cfg(any(target_os = "linux", target_os = "android"))]
const TO_STAT: OFlags = OFlags::PATH;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const TO_STAT: OFlags = OFlags::RDONLY.union(OFlags::NONBLOCK).union(OFlags::NOCTTY);

/// `path` opened with `flags`, however long it is.
#[cfg(unix)]
pub(crate) fn open(path: &Path, flags: OFlags) -> io::Result<OwnedFd> {
    let flags = flags.union(OFlags::CLOEXEC);
    match rustix::fs::open(path, flags, rustix::fs::Mode::empty()) {
        Err(Errno::NAMETOOLONG) => open_in_stretches(path, flags),
        opened => opened,
    }
    .map_err(io::Error::from)
}

#[cfg(unix)]
fn open_in_stretches(path: &Path, flags: OFlags) -> rustix::io::Result<OwnedFd> {
    use rustix::fd::AsFd;
    use rustix::fs::{CWD, Mode, openat};
    let mut at: Option<OwnedFd> = None;
    let mut stretch = std::path::PathBuf::new();
    for component in path.components() {
        let name = component.as_os_str();
        let len = stretch.as_os_str().len();
        if len > 0 && len + 1 + name.len() > STRETCH {
            let from = at.as_ref().map_or(CWD, AsFd::as_fd);
            let through = THROUGH.union(OFlags::CLOEXEC);
            at = Some(openat(from, &stretch, through, Mode::empty())?);
            stretch.clear();
        }
        stretch.push(name);
    }
    let from = at.as_ref().map_or(CWD, AsFd::as_fd);
    openat(from, &stretch, flags, Mode::empty())
}

/// `path` opened to read, however long it is.
#[cfg(unix)]
pub(crate) fn open_file(path: &Path) -> io::Result<File> {
    open(path, OFlags::RDONLY).map(File::from)
}

#[cfg(not(unix))]
pub(crate) fn open_file(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// The metadata of `path`, through a symlink when `follow` is set, however
/// long the path is.
pub(crate) fn metadata(path: &Path, follow: bool) -> io::Result<Metadata> {
    let stat = if follow {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    };
    #[cfg(unix)]
    if stat
        .as_ref()
        .is_err_and(|err| err.raw_os_error() == Some(Errno::NAMETOOLONG.raw_os_error()))
    {
        let flags = if follow {
            TO_STAT
        } else {
            TO_STAT.union(OFlags::NOFOLLOW)
        };
        return File::from(open_in_stretches(path, flags.union(OFlags::CLOEXEC))?).metadata();
    }
    stat
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;
    use tempfile::TempDir;

    const NAME_LEN: usize = 200;

    /// A file `deep/.../leaf` nested `levels` directories of 200-character
    /// names under a fresh directory, built from the bottom up so no call
    /// names a path past the limit. Returns the directory and the leaf's
    /// path relative to it.
    fn nested(levels: usize, leaf: &str, body: &str) -> (TempDir, PathBuf) {
        let staging = TempDir::new().unwrap();
        let name = "D".repeat(NAME_LEN);
        let chain = staging.path().join("chain");
        let parent = staging.path().join("parent");
        std::fs::create_dir(&chain).unwrap();
        std::fs::write(chain.join(leaf), body).unwrap();
        let mut rel = PathBuf::from(leaf);
        for _ in 0..levels {
            std::fs::create_dir(&parent).unwrap();
            std::fs::rename(&chain, parent.join(&name)).unwrap();
            std::fs::rename(&parent, &chain).unwrap();
            rel = Path::new(&name).join(rel);
        }
        let root = TempDir::new().unwrap();
        std::fs::rename(&chain, root.path().join("top")).unwrap();
        (root, Path::new("top").join(rel))
    }

    fn read(path: &Path) -> String {
        let mut text = String::new();
        open_file(path).unwrap().read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn a_short_path_opens_and_stats_as_std_would() {
        let (root, rel) = nested(1, "leaf", "short");
        let path = root.path().join(rel);
        assert_eq!(read(&path), "short");
        assert_eq!(metadata(&path, true).unwrap().len(), 5);
    }

    #[test]
    fn a_path_past_the_limit_opens_and_stats() {
        let (root, rel) = nested(30, "leaf", "deep body");
        let path = root.path().join(rel);
        assert!(path.as_os_str().len() > 4096);
        assert_eq!(read(&path), "deep body");
        let meta = metadata(&path, true).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.len(), 9);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_past_the_limit_opens_as_one() {
        let (root, rel) = nested(30, "leaf", "x");
        let dir = root.path().join(rel.parent().unwrap());
        let fd = open(&dir, OFlags::RDONLY.union(OFlags::DIRECTORY)).unwrap();
        assert!(File::from(fd).metadata().unwrap().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_past_the_limit_stats_as_itself_unless_followed() {
        let (root, rel) = nested(30, "leaf", "target");
        let dir = root.path().join(rel.parent().unwrap());
        let fd = open(&dir, OFlags::RDONLY.union(OFlags::DIRECTORY)).unwrap();
        rustix::fs::symlinkat("leaf", &fd, "link").unwrap();
        let link = dir.join("link");
        assert!(metadata(&link, true).unwrap().is_file());
        #[cfg(any(target_os = "linux", target_os = "android"))]
        assert!(metadata(&link, false).unwrap().is_symlink());
    }

    #[test]
    fn a_missing_path_past_the_limit_is_an_error() {
        let (root, rel) = nested(30, "leaf", "x");
        let missing = root.path().join(rel.with_file_name("absent"));
        assert!(open_file(&missing).is_err());
        assert!(metadata(&missing, true).is_err());
    }

    #[test]
    fn a_missing_short_path_is_an_error() {
        let root = TempDir::new().unwrap();
        assert!(metadata(&root.path().join("absent"), false).is_err());
    }
}
