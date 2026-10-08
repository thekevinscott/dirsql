//! A directory's entries, read in one pass and sorted by name.
//!
//! The names share one buffer rather than one allocation each, which in a
//! directory of a million files is most of what reading it costs.

use std::ffi::OsStr;
use std::path::Path;

/// What a directory entry says it is, before any symlink is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    Dir,
    File,
    Link,
    /// A fifo, socket or device: never a row, never entered.
    Other,
    /// The filesystem did not say; the walk has to ask.
    Unknown,
}

/// One entry of a [`Listing`]: where its name sits, and what it is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Listed {
    prefix: u64,
    start: usize,
    end: usize,
    pub(crate) seen: Seen,
}

#[derive(Default)]
pub(crate) struct Listing {
    names: Names,
    entries: Vec<Listed>,
}

/// One buffer holding every name end to end.
#[cfg(unix)]
#[derive(Default)]
struct Packed(Vec<u8>);

#[cfg(unix)]
impl Packed {
    fn add(&mut self, name: &OsStr) -> usize {
        use std::os::unix::ffi::OsStrExt;
        let start = self.0.len();
        self.0.extend_from_slice(name.as_bytes());
        start
    }

    fn get(&self, start: usize, end: usize) -> &OsStr {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(&self.0[start..end])
    }
}

/// One allocation per name, for platforms whose names are not bytes. Built
/// under test too, so the Linux unit run covers it.
#[cfg(any(test, not(unix)))]
#[derive(Default)]
struct Owned(Vec<std::ffi::OsString>);

#[cfg(any(test, not(unix)))]
impl Owned {
    fn add(&mut self, name: &OsStr) -> usize {
        self.0.push(name.to_os_string());
        self.0.len() - 1
    }

    fn get(&self, start: usize, _end: usize) -> &OsStr {
        &self.0[start]
    }
}

#[cfg(unix)]
type Names = Packed;
#[cfg(not(unix))]
type Names = Owned;

/// The entries of a directory, each judged, in the byte order of their names.
pub(crate) struct Judged<R> {
    listing: Listing,
    results: Vec<Option<R>>,
}

impl<R> Judged<R> {
    /// Hands `f` each entry's name and judgment, in name order.
    pub(crate) fn each(mut self, f: &mut dyn FnMut(&OsStr, R)) {
        for (listed, result) in self.listing.entries().iter().zip(self.results.iter_mut()) {
            if let Some(result) = result.take() {
                f(self.listing.name(listed), result);
            }
        }
    }
}

/// Reads `dir` and judges each entry with `judge`.
pub(crate) fn read_judged<R: Send>(
    dir: &Path,
    judge: &(dyn Fn(&OsStr, Seen) -> R + Sync),
) -> Judged<R> {
    let listing = Listing::read(dir);
    let results = listing
        .entries()
        .iter()
        .map(|listed| Some(judge(listing.name(listed), listed.seen)))
        .collect();
    Judged { listing, results }
}

impl Listing {
    /// The entries of `dir` in the byte order of their names; none when it
    /// cannot be read.
    pub(crate) fn read(dir: &Path) -> Listing {
        let mut listing = Listing::default();
        read_into(dir, &mut listing);
        listing.sort();
        listing
    }

    pub(crate) fn entries(&self) -> &[Listed] {
        &self.entries
    }

    pub(crate) fn push(&mut self, name: &OsStr, seen: Seen) {
        let start = self.names.add(name);
        self.entries.push(Listed {
            prefix: name_prefix(name),
            start,
            end: start + name.len(),
            seen,
        });
    }

    pub(crate) fn name(&self, listed: &Listed) -> &OsStr {
        self.names.get(listed.start, listed.end)
    }

    pub(crate) fn sort(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        // Comparing a leading word first keeps most comparisons out of the
        // name buffer, which is most of the sort's cost in a large directory.
        entries.sort_unstable_by(|a, b| {
            a.prefix
                .cmp(&b.prefix)
                .then_with(|| self.name(a).cmp(self.name(b)))
        });
        self.entries = entries;
    }
}

#[cfg(unix)]
fn seen(kind: rustix::fs::FileType) -> Seen {
    use rustix::fs::FileType;
    match kind {
        FileType::Directory => Seen::Dir,
        FileType::RegularFile => Seen::File,
        FileType::Symlink => Seen::Link,
        FileType::Unknown => Seen::Unknown,
        _ => Seen::Other,
    }
}

#[cfg(unix)]
fn read_into(dir: &Path, listing: &mut Listing) {
    use rustix::fs::{Dir, Mode, OFlags};
    use std::os::unix::ffi::OsStrExt;
    let flags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC);
    let Ok(fd) = rustix::fs::open(dir, flags, Mode::empty()) else {
        return;
    };
    let Ok(mut entries) = Dir::read_from(&fd) else {
        return;
    };
    while let Some(Ok(entry)) = entries.read() {
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        listing.push(OsStr::from_bytes(name), seen(entry.file_type()));
    }
}

#[cfg(any(test, not(unix)))]
fn read_dir_into(dir: &Path, listing: &mut Listing) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let seen = entry.file_type().map_or(Seen::Other, |kind| {
            seen_flags(kind.is_symlink(), kind.is_dir(), kind.is_file())
        });
        listing.push(&entry.file_name(), seen);
    }
}

#[cfg(any(test, not(unix)))]
fn seen_flags(symlink: bool, dir: bool, file: bool) -> Seen {
    match (symlink, dir, file) {
        (true, _, _) => Seen::Link,
        (_, true, _) => Seen::Dir,
        (_, _, true) => Seen::File,
        _ => Seen::Other,
    }
}

#[cfg(not(unix))]
use read_dir_into as read_into;

/// A name's first eight bytes as a big-endian word, which orders names as
/// their bytes do as far as those bytes go.
fn name_prefix(name: &OsStr) -> u64 {
    let bytes = name.as_encoded_bytes();
    let mut word = [0u8; 8];
    let len = bytes.len().min(8);
    word[..len].copy_from_slice(&bytes[..len]);
    u64::from_be_bytes(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(listing: &Listing) -> Vec<&OsStr> {
        listing
            .entries()
            .iter()
            .map(|listed| listing.name(listed))
            .collect()
    }

    #[test]
    fn a_listing_sorts_its_names_into_byte_order() {
        let mut listing = Listing::default();
        for name in ["abcdefghZ", "b", "abcdefgh", "ab", "abcdefghA", "abc", "B"] {
            listing.push(OsStr::new(name), Seen::File);
        }
        listing.sort();
        assert_eq!(
            names(&listing),
            ["B", "ab", "abc", "abcdefgh", "abcdefghA", "abcdefghZ", "b"]
        );
    }

    #[test]
    fn a_sorted_entry_keeps_what_it_was_seen_as() {
        let mut listing = Listing::default();
        listing.push(OsStr::new("z"), Seen::Dir);
        listing.push(OsStr::new("y"), Seen::Link);
        listing.push(OsStr::new("x"), Seen::Unknown);
        listing.sort();
        let seen: Vec<_> = listing.entries().iter().map(|listed| listed.seen).collect();
        assert_eq!(seen, [Seen::Unknown, Seen::Link, Seen::Dir]);
    }

    #[test]
    fn a_listing_hands_back_each_name_as_pushed() {
        let mut listing = Listing::default();
        listing.push(OsStr::new("first"), Seen::File);
        listing.push(OsStr::new("second"), Seen::Other);
        assert_eq!(names(&listing), ["first", "second"]);
    }

    #[test]
    fn name_prefix_orders_names_as_their_bytes_do() {
        let mut names = ["abcdefghZ", "abcdefgh", "abcdefghA", "ab", "b", "abc", ""];
        names.sort_by_key(|name| (name_prefix(OsStr::new(name)), *name));
        assert_eq!(
            names,
            ["", "ab", "abc", "abcdefgh", "abcdefghA", "abcdefghZ", "b"]
        );
    }

    #[test]
    fn name_prefix_is_the_first_eight_bytes_padded_with_zeros() {
        assert_eq!(name_prefix(OsStr::new("ab")), 0x6162_0000_0000_0000);
        assert_eq!(
            name_prefix(OsStr::new("abcdefghZ")),
            u64::from_be_bytes(*b"abcdefgh")
        );
    }

    fn read_with(read: fn(&Path, &mut Listing), dir: &Path) -> Vec<(String, Seen)> {
        let mut listing = Listing::default();
        read(dir, &mut listing);
        listing.sort();
        listing
            .entries()
            .iter()
            .map(|listed| {
                (
                    listing.name(listed).to_string_lossy().into_owned(),
                    listed.seen,
                )
            })
            .collect()
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file"), "x").unwrap();
        std::fs::create_dir(dir.path().join("dir")).unwrap();
        dir
    }

    fn fixture_entries() -> Vec<(String, Seen)> {
        [("dir", Seen::Dir), ("file", Seen::File)]
            .map(|(name, seen)| (name.to_string(), seen))
            .to_vec()
    }

    #[cfg(unix)]
    #[test]
    fn read_lists_each_entry_of_a_directory_sorted_and_classified() {
        let dir = fixture();
        let listing = Listing::read(dir.path());
        let got: Vec<_> = listing
            .entries()
            .iter()
            .map(|listed| {
                (
                    listing.name(listed).to_string_lossy().into_owned(),
                    listed.seen,
                )
            })
            .collect();
        assert_eq!(got, fixture_entries());
    }

    #[cfg(unix)]
    #[test]
    fn read_into_lists_the_entries_without_dot_and_dot_dot() {
        let dir = fixture();
        assert_eq!(read_with(read_into, dir.path()), fixture_entries());
    }

    #[test]
    fn read_dir_into_classifies_what_std_reports() {
        let dir = fixture();
        assert_eq!(read_with(read_dir_into, dir.path()), fixture_entries());
    }

    #[test]
    fn flags_say_what_an_entry_was_seen_as() {
        assert_eq!(seen_flags(true, false, false), Seen::Link);
        assert_eq!(seen_flags(false, true, false), Seen::Dir);
        assert_eq!(seen_flags(false, false, true), Seen::File);
        assert_eq!(seen_flags(false, false, false), Seen::Other);
    }

    #[test]
    fn an_unreadable_directory_lists_nothing() {
        let dir = fixture();
        let file = dir.path().join("file");
        let missing = dir.path().join("missing");
        for path in [&file, &missing] {
            assert!(Listing::read(path).entries().is_empty());
            assert!(read_with(read_dir_into, path).is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_file_type_maps_to_what_it_was_seen_as() {
        use rustix::fs::FileType;
        let kinds = [
            (FileType::Directory, Seen::Dir),
            (FileType::RegularFile, Seen::File),
            (FileType::Symlink, Seen::Link),
            (FileType::Unknown, Seen::Unknown),
            (FileType::Socket, Seen::Other),
            (FileType::Fifo, Seen::Other),
        ];
        for (kind, expected) in kinds {
            assert_eq!(seen(kind), expected);
        }
    }

    #[test]
    fn owned_names_come_back_by_the_index_they_were_added_at() {
        let mut names = Owned::default();
        let first = names.add(OsStr::new("first"));
        let second = names.add(OsStr::new("second"));
        assert_eq!(names.get(first, 0), "first");
        assert_eq!(names.get(second, 0), "second");
    }
    const LARGE: usize = 3 * 4096;

    fn large_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..LARGE {
            std::fs::write(dir.path().join(format!("f{n:05}")), "").unwrap();
        }
        dir
    }

    fn judged_names(dir: &Path) -> Vec<String> {
        let mut got = Vec::new();
        read_judged(dir, &|name, _| name.len() > 0).each(&mut |name, keep| {
            if keep {
                got.push(name.to_string_lossy().into_owned());
            }
        });
        got
    }

    #[cfg(unix)]
    #[test]
    fn a_large_directory_is_judged_whole_and_handed_back_in_name_order() {
        let dir = large_dir();
        let want: Vec<String> = (0..LARGE).map(|n| format!("f{n:05}")).collect();
        assert_eq!(judged_names(dir.path()), want);
    }

    #[cfg(unix)]
    #[test]
    fn a_large_directory_is_judged_on_more_than_one_thread() {
        let dir = large_dir();
        let threads = std::sync::Mutex::new(std::collections::HashSet::new());
        read_judged(dir.path(), &|_, _| {
            threads.lock().unwrap().insert(std::thread::current().id());
        })
        .each(&mut |_, ()| {});
        let judged_on = threads.lock().unwrap().len();
        assert!(judged_on > 1, "judged on {judged_on} thread(s)");
    }

    #[cfg(unix)]
    #[test]
    fn a_small_directory_is_judged_on_the_calling_thread() {
        let dir = fixture();
        let caller = std::thread::current().id();
        let mut on_caller = Vec::new();
        read_judged(dir.path(), &|_, _| std::thread::current().id() == caller)
            .each(&mut |_, here| on_caller.push(here));
        assert_eq!(on_caller, vec![true, true]);
    }
}
