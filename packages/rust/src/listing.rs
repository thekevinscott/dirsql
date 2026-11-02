//! A directory's entries, read in chunks and sorted by name.
//!
//! Each chunk's names share one buffer rather than one allocation each, which
//! in a directory of a million files is most of what reading it costs.

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

/// One entry of a [`Listing`]: which chunk's buffer its name sits in and
/// where, and what it is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Listed {
    prefix: u64,
    chunk: u32,
    start: u32,
    end: u32,
    pub(crate) seen: Seen,
}

/// The most entries a [`Chunk`] holds, which keeps every offset into its
/// names within a `u32`.
const CHUNK_ENTRIES: usize = 8192;

/// Entries read in a row, at most [`CHUNK_ENTRIES`] of them, with their names.
#[derive(Default)]
pub(crate) struct Chunk {
    index: u32,
    names: Names,
    entries: Vec<Listed>,
}

impl Chunk {
    fn new(index: u32) -> Chunk {
        Chunk {
            index,
            ..Chunk::default()
        }
    }

    fn is_full(&self) -> bool {
        self.entries.len() == CHUNK_ENTRIES
    }

    fn name(&self, listed: &Listed) -> &OsStr {
        self.names.get(listed.start, listed.end)
    }

    fn sort(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        entries.sort_unstable_by(|a, b| by_name(a, b, |listed| self.name(listed)));
        self.entries = entries;
    }

    fn push(&mut self, name: &OsStr, seen: Seen) {
        let start = self.names.add(name);
        self.entries.push(Listed {
            prefix: name_prefix(name),
            chunk: self.index,
            start,
            end: start + span(name),
            seen,
        });
    }
}

/// Gathers entries into chunks, handing each to `emit` once full.
struct Chunker<'e> {
    chunk: Chunk,
    emit: &'e mut dyn FnMut(Chunk),
}

impl<'e> Chunker<'e> {
    fn new(emit: &'e mut dyn FnMut(Chunk)) -> Chunker<'e> {
        Chunker {
            chunk: Chunk::default(),
            emit,
        }
    }

    fn push(&mut self, name: &OsStr, seen: Seen) {
        if self.chunk.is_full() {
            let next = Chunk::new(self.chunk.index + 1);
            (self.emit)(std::mem::replace(&mut self.chunk, next));
        }
        self.chunk.push(name, seen);
    }

    fn finish(self) {
        if !self.chunk.entries.is_empty() {
            (self.emit)(self.chunk);
        }
    }
}

#[derive(Default)]
pub(crate) struct Listing {
    names: Vec<Names>,
    entries: Vec<Listed>,
}

/// One buffer holding every name end to end.
#[cfg(unix)]
#[derive(Default)]
struct Packed(Vec<u8>);

#[cfg(unix)]
impl Packed {
    fn add(&mut self, name: &OsStr) -> u32 {
        use std::os::unix::ffi::OsStrExt;
        let start = offset(self.0.len());
        self.0.extend_from_slice(name.as_bytes());
        start
    }

    fn get(&self, start: u32, end: u32) -> &OsStr {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(&self.0[start as usize..end as usize])
    }
}

/// One allocation per name, for platforms whose names are not bytes. Built
/// under test too, so the Linux unit run covers it.
#[cfg(any(test, not(unix)))]
#[derive(Default)]
struct Owned(Vec<std::ffi::OsString>);

#[cfg(any(test, not(unix)))]
impl Owned {
    fn add(&mut self, name: &OsStr) -> u32 {
        self.0.push(name.to_os_string());
        offset(self.0.len() - 1)
    }

    fn get(&self, start: u32, _end: u32) -> &OsStr {
        &self.0[start as usize]
    }
}

/// `n` as a chunk offset, which a chunk of at most [`CHUNK_ENTRIES`] names of
/// at most a few hundred bytes each never outgrows.
fn offset(n: usize) -> u32 {
    u32::try_from(n).expect("a chunk's names fit in u32 offsets")
}

/// How far a name reaches past its start in a chunk's names.
fn span(name: &OsStr) -> u32 {
    offset(name.len())
}

#[cfg(unix)]
type Names = Packed;
#[cfg(not(unix))]
type Names = Owned;

impl Listing {
    /// The entries of `dir` in the byte order of their names; none when it
    /// cannot be read.
    pub(crate) fn read(dir: &Path) -> Listing {
        Listing::sorted(|emit| read_chunks(dir, emit), &Chunk::sort)
    }

    /// Every chunk `read` hands out, in the byte order of their names. Past
    /// the first chunk, a thread of its own `sort`s each chunk while the next
    /// is read, and the sorted chunks are then merged on every core.
    fn sorted(
        read: impl FnOnce(&mut dyn FnMut(Chunk)),
        sort: &(dyn Fn(&mut Chunk) + Sync),
    ) -> Listing {
        let (send, receive) = std::sync::mpsc::channel::<Chunk>();
        let mut receive = Some(receive);
        let mut first = None;
        std::thread::scope(|scope| {
            let mut sorter = None;
            read(&mut |chunk| {
                if sorter.is_none() {
                    let Some(earlier) = first.take() else {
                        first = Some(chunk);
                        return;
                    };
                    let receive = receive.take().expect("one sorter is started");
                    sorter = Some(scope.spawn(move || sort_each(receive, sort)));
                    send.send(earlier).expect("the sorter is waiting");
                }
                send.send(chunk).expect("the sorter is waiting");
            });
            drop(send);
            match sorter {
                Some(sorter) => merge(sorter.join().expect("sorting does not panic")),
                None => {
                    let mut listing = Listing::default();
                    if let Some(first) = first {
                        listing.add(first);
                    }
                    listing.sort();
                    listing
                }
            }
        })
    }

    pub(crate) fn entries(&self) -> &[Listed] {
        &self.entries
    }

    /// Takes `chunk` in, which must be the next one read.
    fn add(&mut self, chunk: Chunk) {
        debug_assert_eq!(chunk.index as usize, self.names.len());
        self.names.push(chunk.names);
        self.entries.extend(chunk.entries);
    }

    /// Whether an entry is named `name`; the listing must be sorted.
    pub(crate) fn holds(&self, name: &OsStr) -> bool {
        self.entries
            .binary_search_by(|listed| self.name(listed).cmp(name))
            .is_ok()
    }

    pub(crate) fn name(&self, listed: &Listed) -> &OsStr {
        self.names[listed.chunk as usize].get(listed.start, listed.end)
    }

    pub(crate) fn sort(&mut self) {
        let mut entries = std::mem::take(&mut self.entries);
        entries.sort_unstable_by(|a, b| by_name(a, b, |listed| self.name(listed)));
        self.entries = entries;
    }
}

/// How `a` and `b` order by name. Comparing a leading word first keeps most
/// comparisons out of the name buffers, which is most of a sort's cost in a
/// large directory.
fn by_name<'n>(a: &Listed, b: &Listed, name: impl Fn(&Listed) -> &'n OsStr) -> std::cmp::Ordering {
    a.prefix.cmp(&b.prefix).then_with(|| name(a).cmp(name(b)))
}

/// Each chunk `receive` hands over, sorted by `sort`, in the order handed.
fn sort_each(
    receive: std::sync::mpsc::Receiver<Chunk>,
    sort: &(dyn Fn(&mut Chunk) + Sync),
) -> Vec<Chunk> {
    receive
        .into_iter()
        .map(|mut chunk| {
            sort(&mut chunk);
            chunk
        })
        .collect()
}

/// The entries of `sorted`, a sample of a directory, that split it into
/// `runs` runs of about equal size.
fn splits(sorted: &[Listed], runs: usize) -> Vec<Listed> {
    (1..runs)
        .map(|run| sorted[run * sorted.len() / runs])
        .collect()
}

/// `chunks`, each sorted and given in the order read, as one listing in name
/// order. The names are split into a run per core at names sampled from the
/// first chunk, and each run is merged from its share of every chunk on a
/// core of its own.
fn merge(chunks: Vec<Chunk>) -> Listing {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let first = &chunks[0].entries;
    let splits = splits(first, cores);
    let name = |listed: &Listed| chunks[listed.chunk as usize].name(listed);
    let bounds: Vec<Vec<usize>> = chunks
        .iter()
        .map(|chunk| {
            let inner = splits.iter().map(|split| {
                chunk
                    .entries
                    .partition_point(|listed| by_name(listed, split, name).is_lt())
            });
            std::iter::once(0)
                .chain(inner)
                .chain(std::iter::once(chunk.entries.len()))
                .collect()
        })
        .collect();
    let total = chunks.iter().map(|chunk| chunk.entries.len()).sum();
    let mut entries = vec![first[0]; total];
    std::thread::scope(|scope| {
        let mut rest = entries.as_mut_slice();
        for run in 0..cores {
            let len = bounds
                .iter()
                .map(|bounds| bounds[run + 1] - bounds[run])
                .sum();
            let (merged, after) = rest.split_at_mut(len);
            rest = after;
            let (chunks, bounds) = (&chunks, &bounds);
            scope.spawn(move || {
                let mut filled = 0;
                for (chunk, bounds) in chunks.iter().zip(bounds) {
                    let share = &chunk.entries[bounds[run]..bounds[run + 1]];
                    merged[filled..filled + share.len()].copy_from_slice(share);
                    filled += share.len();
                }
                // A stable sort merges the chunks' sorted shares rather than
                // sorting them afresh.
                merged.sort_by(|a, b| by_name(a, b, name));
            });
        }
    });
    Listing {
        names: chunks.into_iter().map(|chunk| chunk.names).collect(),
        entries,
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
fn open_dir(dir: &Path) -> rustix::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC);
    rustix::fs::open(dir, flags, Mode::empty())
}

/// The entries of `dir` in the order the filesystem lists them, handed to
/// `emit` a chunk at a time as they are read; none when it cannot be read.
pub(crate) fn read_chunks(dir: &Path, emit: &mut dyn FnMut(Chunk)) {
    let mut chunker = Chunker::new(emit);
    read_into(dir, &mut chunker);
    chunker.finish();
}

/// Uses `Dir`, not `RawDir`: on a deep tree of small directories the walk ran
/// about twice as slow reading through `RawDir`, with no gain on a flat
/// directory of a million entries.
#[cfg(unix)]
fn read_into(dir: &Path, listing: &mut Chunker<'_>) {
    use rustix::fs::Dir;
    use std::os::unix::ffi::OsStrExt;
    let Ok(fd) = open_dir(dir) else {
        return;
    };
    let Ok(mut entries) = Dir::new(fd) else {
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
fn read_dir_into(dir: &Path, listing: &mut Chunker<'_>) {
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

    fn chunks_of(names: impl IntoIterator<Item = (String, Seen)>) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut emit = |chunk| chunks.push(chunk);
        let mut chunker = Chunker::new(&mut emit);
        for (name, seen) in names {
            chunker.push(OsStr::new(&name), seen);
        }
        chunker.finish();
        chunks
    }

    fn listing_of(names: &[(&str, Seen)]) -> Listing {
        let mut listing = Listing::default();
        let named = names.iter().map(|&(name, seen)| (name.to_string(), seen));
        for chunk in chunks_of(named) {
            listing.add(chunk);
        }
        listing
    }

    fn numbered(count: usize) -> impl Iterator<Item = (String, Seen)> {
        (0..count).map(|i| (format!("n{i:05}"), Seen::File))
    }

    #[test]
    fn a_listing_sorts_its_names_into_byte_order() {
        let pushed = ["abcdefghZ", "b", "abcdefgh", "ab", "abcdefghA", "abc", "B"];
        let mut listing = listing_of(&pushed.map(|name| (name, Seen::File)));
        listing.sort();
        assert_eq!(
            names(&listing),
            ["B", "ab", "abc", "abcdefgh", "abcdefghA", "abcdefghZ", "b"]
        );
    }

    #[test]
    fn a_sorted_entry_keeps_what_it_was_seen_as() {
        let mut listing = listing_of(&[("z", Seen::Dir), ("y", Seen::Link), ("x", Seen::Unknown)]);
        listing.sort();
        let seen: Vec<_> = listing.entries().iter().map(|listed| listed.seen).collect();
        assert_eq!(seen, [Seen::Unknown, Seen::Link, Seen::Dir]);
    }

    #[test]
    fn a_listing_hands_back_each_name_as_pushed() {
        let listing = listing_of(&[("first", Seen::File), ("second", Seen::Other)]);
        assert_eq!(names(&listing), ["first", "second"]);
    }

    #[test]
    fn a_chunk_holds_at_most_chunk_entries_and_the_rest_start_the_next() {
        let chunks = chunks_of(numbered(2 * CHUNK_ENTRIES + 1));
        let shape: Vec<_> = chunks
            .iter()
            .map(|chunk| (chunk.index, chunk.entries.len()))
            .collect();
        assert_eq!(shape, [(0, CHUNK_ENTRIES), (1, CHUNK_ENTRIES), (2, 1)]);
    }

    #[test]
    fn a_chunker_of_no_entries_hands_out_no_chunk() {
        assert!(chunks_of(numbered(0)).is_empty());
    }

    #[test]
    fn a_listing_sorts_names_from_every_chunk_together() {
        let count = CHUNK_ENTRIES + 2;
        let mut listing = Listing::default();
        for chunk in chunks_of(numbered(count).collect::<Vec<_>>().into_iter().rev()) {
            listing.add(chunk);
        }
        listing.sort();
        let expected: Vec<_> = numbered(count).map(|(name, _)| name).collect();
        let got: Vec<_> = names(&listing)
            .into_iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, expected);
    }

    #[cfg(unix)]
    #[test]
    fn read_chunks_hands_out_a_wide_directory_a_chunk_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..=CHUNK_ENTRIES {
            std::fs::File::create(dir.path().join(format!("f{i}"))).unwrap();
        }
        let mut sizes = Vec::new();
        read_chunks(dir.path(), &mut |chunk| sizes.push(chunk.entries.len()));
        assert_eq!(sizes, [CHUNK_ENTRIES, 1]);
    }

    #[test]
    fn a_sorted_listing_holds_exactly_the_names_pushed() {
        let mut listing = Listing::default();
        for chunk in
            chunks_of([".gitignore", "b", ".git", "a"].map(|name| (name.to_string(), Seen::File)))
        {
            listing.add(chunk);
        }
        listing.sort();
        assert!(listing.holds(OsStr::new(".git")));
        assert!(listing.holds(OsStr::new("a")));
        assert!(!listing.holds(OsStr::new(".gi")));
        assert!(!Listing::default().holds(OsStr::new(".git")));
    }

    fn sorted_from(chunks: Vec<Chunk>) -> Listing {
        let mut chunks = chunks.into_iter();
        Listing::sorted(|emit| chunks.by_ref().for_each(&mut *emit), &Chunk::sort)
    }

    fn named_and_seen(listing: &Listing) -> Vec<(String, Seen)> {
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

    /// Names past the eight bytes a prefix compares, alternately seen as a
    /// directory and a file, in byte order.
    fn long_names(count: usize) -> Vec<(String, Seen)> {
        (0..count)
            .map(|i| {
                let seen = if i % 2 == 0 { Seen::Dir } else { Seen::File };
                (format!("shared-prefix-{i:05}"), seen)
            })
            .collect()
    }

    #[test]
    fn a_listing_sorted_from_several_chunks_holds_every_entry_in_byte_order() {
        let count = 3 * CHUNK_ENTRIES + 5;
        let expected = long_names(count);
        let shuffled = (0..count).map(|i| expected[i * 7919 % count].clone());
        assert_eq!(named_and_seen(&sorted_from(chunks_of(shuffled))), expected);
    }

    #[test]
    fn a_listing_sorted_from_chunks_read_in_reverse_holds_every_entry_in_byte_order() {
        let count = 2 * CHUNK_ENTRIES + 1;
        let expected = long_names(count);
        let reversed = expected.iter().rev().cloned();
        assert_eq!(named_and_seen(&sorted_from(chunks_of(reversed))), expected);
    }

    #[test]
    fn a_listing_sorted_from_one_chunk_holds_it_in_byte_order() {
        let pushed = [("b", Seen::File), ("c", Seen::Dir), ("a", Seen::Link)];
        let owned = pushed.map(|(name, seen)| (name.to_string(), seen));
        let expected = [("a", Seen::Link), ("b", Seen::File), ("c", Seen::Dir)]
            .map(|(name, seen)| (name.to_string(), seen));
        assert_eq!(named_and_seen(&sorted_from(chunks_of(owned))), expected);
    }

    #[test]
    fn a_listing_sorted_from_no_chunk_is_empty() {
        assert!(sorted_from(Vec::new()).entries().is_empty());
    }

    #[test]
    fn a_wide_listing_sorts_a_chunk_before_its_last_is_read() {
        use std::sync::{Condvar, Mutex};
        let sorted = (Mutex::new(false), Condvar::new());
        let sort = |chunk: &mut Chunk| {
            chunk.sort();
            *sorted.0.lock().unwrap() = true;
            sorted.1.notify_all();
        };
        let mut chunks = chunks_of(numbered(2 * CHUNK_ENTRIES + 1)).into_iter();
        let mut sorted_early = false;
        let listing = Listing::sorted(
            |emit| {
                emit(chunks.next().unwrap());
                emit(chunks.next().unwrap());
                let done = sorted.0.lock().unwrap();
                let wait = std::time::Duration::from_secs(10);
                let (done, _) = sorted
                    .1
                    .wait_timeout_while(done, wait, |done| !*done)
                    .unwrap();
                sorted_early = *done;
                drop(done);
                chunks.by_ref().for_each(&mut *emit);
            },
            &sort,
        );
        assert!(
            sorted_early,
            "no chunk was sorted until the whole directory was read"
        );
        assert_eq!(listing.entries().len(), 2 * CHUNK_ENTRIES + 1);
    }

    #[test]
    fn splits_cut_a_sorted_sample_into_runs_of_about_equal_size() {
        let named = ["a", "b", "c", "d", "e", "f", "g", "h"].map(|name| (name, Seen::File));
        let listing = listing_of(&named);
        let got: Vec<&OsStr> = splits(listing.entries(), 4)
            .iter()
            .map(|listed| listing.name(listed))
            .collect();
        assert_eq!(got, ["c", "e", "g"]);
    }

    #[test]
    fn read_lists_a_directory_wider_than_a_chunk_in_byte_order() {
        let dir = tempfile::tempdir().unwrap();
        let expected: Vec<_> = numbered(CHUNK_ENTRIES + 3).collect();
        for (name, _) in &expected {
            std::fs::File::create(dir.path().join(name)).unwrap();
        }
        assert_eq!(named_and_seen(&Listing::read(dir.path())), expected);
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

    fn read_with(read: fn(&Path, &mut Chunker<'_>), dir: &Path) -> Vec<(String, Seen)> {
        let mut listing = Listing::default();
        let mut emit = |chunk| listing.add(chunk);
        let mut chunker = Chunker::new(&mut emit);
        read(dir, &mut chunker);
        chunker.finish();
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
}
