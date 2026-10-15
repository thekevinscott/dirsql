use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder, Glob};
use std::path::Path;
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
        let dir = dir.to_path_buf();
        Frame::compiled_by(move || {
            let file = dir.join(".gitignore");
            if !file.is_file() {
                return None;
            }
            let mut builder = GitignoreBuilder::new(&dir);
            builder.add(&file);
            builder.build().ok()
        })
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

    /// `Gitignore::matched`, on this thread's copy, or the shared one when a
    /// copy no longer compiles.
    pub(crate) fn matched(&self, path: &Path, is_dir: bool) -> Match<&Glob> {
        let slot = SLOT.with(|slot| slot % self.copies.len());
        self.copies[slot]
            .get_or_init(&*self.compile)
            .as_ref()
            .unwrap_or(&self.gitignore)
            .matched(path, is_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_frame_reports_the_directory_of_its_gitignore() {
        let frame = Frame::from_lines(Path::new("/r"), &["*.log"]);
        assert_eq!(frame.gitignore().path(), Path::new("/r"));
    }
}
