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
    #[cfg(unix)]
    names: Vec<u8>,
    #[cfg(not(unix))]
    names: Vec<std::ffi::OsString>,
    entries: Vec<Listed>,
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

    #[cfg(unix)]
    pub(crate) fn push(&mut self, name: &OsStr, seen: Seen) {
        use std::os::unix::ffi::OsStrExt;
        let start = self.names.len();
        self.names.extend_from_slice(name.as_bytes());
        self.entries.push(Listed {
            prefix: name_prefix(name),
            start,
            end: self.names.len(),
            seen,
        });
    }

    #[cfg(not(unix))]
    pub(crate) fn push(&mut self, name: &OsStr, seen: Seen) {
        let start = self.names.len();
        self.names.push(name.to_os_string());
        self.entries.push(Listed {
            prefix: name_prefix(name),
            start,
            end: start + 1,
            seen,
        });
    }

    #[cfg(unix)]
    pub(crate) fn name(&self, listed: &Listed) -> &OsStr {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(&self.names[listed.start..listed.end])
    }

    #[cfg(not(unix))]
    pub(crate) fn name(&self, listed: &Listed) -> &OsStr {
        &self.names[listed.start]
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
fn read_into(dir: &Path, listing: &mut Listing) {
    use rustix::fs::{Dir, FileType, Mode, OFlags};
    use std::os::unix::ffi::OsStrExt;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
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
        let seen = match entry.file_type() {
            FileType::Directory => Seen::Dir,
            FileType::RegularFile => Seen::File,
            FileType::Symlink => Seen::Link,
            FileType::Unknown => Seen::Unknown,
            _ => Seen::Other,
        };
        listing.push(OsStr::from_bytes(name), seen);
    }
}

#[cfg(not(unix))]
fn read_into(dir: &Path, listing: &mut Listing) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let seen = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => Seen::Link,
            Ok(kind) if kind.is_dir() => Seen::Dir,
            Ok(kind) if kind.is_file() => Seen::File,
            _ => Seen::Other,
        };
        listing.push(&entry.file_name(), seen);
    }
}

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
}
