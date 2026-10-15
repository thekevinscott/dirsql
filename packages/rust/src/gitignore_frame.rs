#[cfg(test)]
mod tests {
    use super::*;
    use ignore::gitignore::GitignoreBuilder;

    fn gitignore(lines: &[&str]) -> Gitignore {
        let mut builder = GitignoreBuilder::new("/r");
        for line in lines {
            builder.add_line(None, line).unwrap();
        }
        builder.build().unwrap()
    }

    #[test]
    fn a_thread_matches_against_the_copy_rebuilt_for_it() {
        let frame = Frame::new(gitignore(&["*.log"]), || Some(gitignore(&["*.md"])));
        assert!(frame.matched(Path::new("/r/a.md"), false).is_ignore());
        assert!(frame.matched(Path::new("/r/a.log"), false).is_none());
    }

    #[test]
    fn a_frame_whose_copy_cannot_be_rebuilt_matches_its_own_gitignore() {
        let frame = Frame::new(gitignore(&["*.log", "!keep.log"]), || None);
        assert!(frame.matched(Path::new("/r/a.log"), false).is_ignore());
        assert!(frame.matched(Path::new("/r/keep.log"), false).is_whitelist());
    }

    #[test]
    fn a_frame_reports_the_gitignore_it_was_made_from() {
        let frame = Frame::new(gitignore(&["*.log"]), || None);
        assert_eq!(frame.gitignore().path(), Path::new("/r"));
    }
}
