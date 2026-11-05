use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder, Glob, gitconfig_excludes_path};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, OnceLock};

/// One directory's `.gitignore` as the walk matches against it.
///
/// `Gitignore::matched` borrows a match buffer from a pool that only its
/// first caller's thread reaches without a lock; every other walk thread
/// contends on the rest. So each thread slot matches against a copy of its
/// own, compiled again on first use there.
pub(crate) struct Frame {
    gitignore: Gitignore,
    copies: Box<[OnceLock<Option<Gitignore>>]>,
    compile: Box<dyn Fn() -> Option<Gitignore> + Send + Sync>,
}

static SLOTS: LazyLock<usize> =
    LazyLock::new(|| std::thread::available_parallelism().map_or(1, usize::from));

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static SLOT: usize = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
}

impl Frame {
    /// The `.gitignore` in `dir`, if one exists. An unparsable file degrades
    /// to no frame rather than failing the scan.
    pub(crate) fn load(dir: &Path) -> Option<Frame> {
        Frame::from_file(dir, dir.join(".gitignore"))
    }

    /// The ignore file `file`, its patterns matched relative to `dir`, if it
    /// exists.
    pub(crate) fn from_file(dir: &Path, file: PathBuf) -> Option<Frame> {
        let dir = dir.to_path_buf();
        Frame::compiled_by(move || {
            if !file.is_file() {
                return None;
            }
            let mut builder = GitignoreBuilder::new(&dir);
            builder.add(&file);
            builder.build().ok()
        })
    }

    /// The repo-wide exclude files of the repo rooted at `repo`, lowest
    /// precedence first: `core.excludesFile`, then `$GIT_DIR/info/exclude`.
    /// Every `.gitignore` outranks both.
    pub(crate) fn repo_excludes(repo: &Path) -> Vec<Frame> {
        let info_exclude = git_dir(repo).map(|dir| common_dir(&dir).join("info").join("exclude"));
        [gitconfig_excludes_path(), info_exclude]
            .into_iter()
            .flatten()
            .filter_map(|file| Frame::from_file(repo, file))
            .collect()
    }

    /// A frame for `dir` holding `lines`, as though read from its `.gitignore`.
    #[cfg(test)]
    pub(crate) fn from_lines(dir: &Path, lines: &[&str]) -> Frame {
        let dir = dir.to_path_buf();
        let lines: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
        Frame::compiled_by(move || {
            let mut builder = GitignoreBuilder::new(&dir);
            for line in &lines {
                builder.add_line(None, line).ok()?;
            }
            builder.build().ok()
        })
        .expect("in-memory gitignore lines compile")
    }

    fn compiled_by(
        compile: impl Fn() -> Option<Gitignore> + Send + Sync + 'static,
    ) -> Option<Frame> {
        Some(Frame {
            gitignore: compile()?,
            copies: (0..*SLOTS).map(|_| OnceLock::new()).collect(),
            compile: Box::new(compile),
        })
    }

    pub(crate) fn gitignore(&self) -> &Gitignore {
        &self.gitignore
    }

    /// `Gitignore::matched`, on this thread's copy.
    pub(crate) fn matched(&self, path: &Path, is_dir: bool) -> Match<&Glob> {
        SLOT.with(|slot| self.copy(*slot)).matched(path, is_dir)
    }

    /// The copy for thread `slot`, or the shared one when a copy no longer
    /// compiles.
    fn copy(&self, slot: usize) -> &Gitignore {
        self.copies[slot % self.copies.len()]
            .get_or_init(&*self.compile)
            .as_ref()
            .unwrap_or(&self.gitignore)
    }
}

/// The git dir of the repo rooted at `repo`: `.git` itself, or where a `.git`
/// file (a linked worktree's or a submodule's) points.
fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot_git = repo.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    Some(repo.join(target))
}

/// The directory a linked worktree's `git_dir` shares with its main repo,
/// where `info/exclude` lives; `git_dir` itself for any other repo.
fn common_dir(git_dir: &Path) -> PathBuf {
    fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .and_then(|text| text.lines().next().map(|line| git_dir.join(line.trim())))
        .unwrap_or_else(|| git_dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_frame_from_a_missing_file_is_none() {
        assert!(Frame::from_file(Path::new("/r"), PathBuf::from("/r/no-such-file")).is_none());
    }

    #[test]
    fn a_frame_from_a_file_matches_relative_to_its_dir() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("rules");
        fs::write(&file, "*.log\n").unwrap();
        let frame = Frame::from_file(dir.path(), file).unwrap();
        assert!(
            frame
                .matched(&dir.path().join("sub/a.log"), false)
                .is_ignore()
        );
        assert_eq!(frame.gitignore().path(), dir.path());
    }

    #[test]
    fn repo_excludes_reads_info_exclude() {
        let repo = TempDir::new().unwrap();
        fs::create_dir_all(repo.path().join(".git/info")).unwrap();
        fs::write(repo.path().join(".git/info/exclude"), "a.txt\n").unwrap();
        let ignored = Frame::repo_excludes(repo.path())
            .iter()
            .any(|frame| frame.matched(&repo.path().join("a.txt"), false).is_ignore());
        assert!(ignored);
    }

    #[test]
    fn the_git_dir_of_a_plain_repo_is_dot_git() {
        let repo = TempDir::new().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        assert_eq!(git_dir(repo.path()), Some(repo.path().join(".git")));
    }

    #[test]
    fn the_git_dir_of_a_git_file_is_where_it_points() {
        let repo = TempDir::new().unwrap();
        fs::write(repo.path().join(".git"), "gitdir: ../elsewhere\n").unwrap();
        assert_eq!(git_dir(repo.path()), Some(repo.path().join("../elsewhere")));
    }

    #[test]
    fn a_git_file_without_a_gitdir_line_has_no_git_dir() {
        let repo = TempDir::new().unwrap();
        fs::write(repo.path().join(".git"), "nonsense\n").unwrap();
        assert_eq!(git_dir(repo.path()), None);
    }

    #[test]
    fn no_dot_git_has_no_git_dir() {
        let repo = TempDir::new().unwrap();
        assert_eq!(git_dir(repo.path()), None);
    }

    #[test]
    fn a_linked_worktree_git_dir_shares_its_common_dir() {
        let git_dir = TempDir::new().unwrap();
        fs::write(git_dir.path().join("commondir"), "../..\n").unwrap();
        assert_eq!(common_dir(git_dir.path()), git_dir.path().join("../.."));
    }

    #[test]
    fn a_git_dir_without_commondir_is_its_own_common_dir() {
        let git_dir = TempDir::new().unwrap();
        assert_eq!(common_dir(git_dir.path()), git_dir.path());
    }

    #[test]
    fn a_frame_matches_as_its_gitignore_does() {
        let frame = Frame::from_lines(Path::new("/r"), &["*.log", "!keep.log", "dist/"]);
        assert!(frame.matched(Path::new("/r/a.log"), false).is_ignore());
        assert!(
            frame
                .matched(Path::new("/r/keep.log"), false)
                .is_whitelist()
        );
        assert!(frame.matched(Path::new("/r/dist"), true).is_ignore());
        assert!(frame.matched(Path::new("/r/dist"), false).is_none());
    }

    #[test]
    fn each_thread_compiles_a_copy_of_its_own_once() {
        let compiled = std::sync::Arc::new(AtomicUsize::new(0));
        let count = std::sync::Arc::clone(&compiled);
        let frame = Frame::compiled_by(move || {
            count.fetch_add(1, Ordering::Relaxed);
            Some(Gitignore::empty())
        })
        .unwrap();
        frame.matched(Path::new("/r/a"), false);
        frame.matched(Path::new("/r/b"), false);
        assert_eq!(compiled.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_thread_whose_copy_no_longer_compiles_matches_the_first() {
        let first = std::sync::atomic::AtomicBool::new(true);
        let frame = Frame::compiled_by(move || {
            first.swap(false, Ordering::Relaxed).then(|| {
                let mut builder = GitignoreBuilder::new("/r");
                builder.add_line(None, "*.log").ok()?;
                builder.build().ok()
            })?
        })
        .unwrap();
        assert!(frame.matched(Path::new("/r/a.log"), false).is_ignore());
    }

    #[test]
    fn a_frame_serves_more_threads_than_it_has_copies() {
        let frame = Frame::from_lines(Path::new("/r"), &["*.log"]);
        let slot = frame.copies.len() * frame.copies.len();
        assert!(frame.copy(slot).matched("/r/a.log", false).is_ignore());
    }

    #[test]
    fn a_frame_reports_the_directory_of_its_gitignore() {
        let frame = Frame::from_lines(Path::new("/r"), &["*.log"]);
        assert_eq!(frame.gitignore().path(), Path::new("/r"));
    }
}
