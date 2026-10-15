//! One statement's rows, handed over in chunks as a scan makes them, so a
//! cursor reads the first rows while the walk is still finding the rest.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};

pub struct RowStream<R> {
    state: Mutex<Streamed<R>>,
    grown: Condvar,
}

struct Streamed<R> {
    chunks: Vec<Arc<Vec<R>>>,
    done: bool,
    failed: bool,
}

impl<R> Default for RowStream<R> {
    fn default() -> Self {
        Self {
            state: Mutex::new(Streamed {
                chunks: Vec::new(),
                done: false,
                failed: false,
            }),
            grown: Condvar::new(),
        }
    }
}

impl<R> RowStream<R> {
    fn lock(&self) -> MutexGuard<'_, Streamed<R>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Append `chunk`; an empty one is dropped, so a chunk a reader is handed
    /// always holds a row.
    pub fn push(&self, chunk: Arc<Vec<R>>) {
        if chunk.is_empty() {
            return;
        }
        self.lock().chunks.push(chunk);
        self.grown.notify_all();
    }

    /// Mark the stream complete; `failed` when the scan did not finish.
    pub fn finish(&self, failed: bool) {
        let mut state = self.lock();
        state.done = true;
        state.failed = failed;
        drop(state);
        self.grown.notify_all();
    }

    fn settled(&self) -> MutexGuard<'_, Streamed<R>> {
        let state = self.lock();
        let state = self
            .grown
            .wait_while(state, |state| !state.done)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!state.failed, "the scan feeding this table panicked");
        state
    }

    /// Chunk `n`, waiting for the scan to make it; `None` once the scan
    /// finished short of it.
    pub fn chunk(&self, n: usize) -> Option<Arc<Vec<R>>> {
        let state = self.lock();
        let state = self
            .grown
            .wait_while(state, |state| !state.done && state.chunks.len() <= n)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!state.failed, "the scan feeding this table panicked");
        state.chunks.get(n).cloned()
    }

    /// Every row, once the scan is through.
    pub fn all(&self) -> Settled<R> {
        Settled::new(self.settled().chunks.clone())
    }

    /// The rows made so far.
    pub fn len(&self) -> usize {
        self.lock().chunks.iter().map(|chunk| chunk.len()).sum()
    }
}

/// A finished stream's rows, addressable by position.
pub struct Settled<R> {
    chunks: Vec<Arc<Vec<R>>>,
    /// The position of each chunk's first row.
    starts: Vec<usize>,
}

impl<R> Settled<R> {
    fn new(chunks: Vec<Arc<Vec<R>>>) -> Self {
        let starts = chunks
            .iter()
            .scan(0, |next, chunk| {
                let start = *next;
                *next += chunk.len();
                Some(start)
            })
            .collect();
        Self { chunks, starts }
    }

    pub fn get(&self, position: usize) -> Option<&R> {
        let n = self
            .starts
            .partition_point(|&start| start <= position)
            .checked_sub(1)?;
        self.chunks[n].get(position - self.starts[n])
    }

    pub fn iter(&self) -> impl Iterator<Item = &R> {
        self.chunks.iter().flat_map(|chunk| chunk.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_of(chunks: &[&[u32]]) -> RowStream<u32> {
        let stream = RowStream::default();
        for chunk in chunks {
            stream.push(Arc::new(chunk.to_vec()));
        }
        stream.finish(false);
        stream
    }

    #[test]
    fn chunks_come_back_in_push_order_then_none() {
        let stream = stream_of(&[&[1, 2], &[3]]);
        assert_eq!(*stream.chunk(0).unwrap(), vec![1, 2]);
        assert_eq!(*stream.chunk(1).unwrap(), vec![3]);
        assert!(stream.chunk(2).is_none());
    }

    #[test]
    fn an_empty_chunk_is_never_handed_out() {
        let stream = stream_of(&[&[], &[1], &[]]);
        assert_eq!(*stream.chunk(0).unwrap(), vec![1]);
        assert!(stream.chunk(1).is_none());
    }

    #[test]
    fn a_made_chunk_is_read_before_the_scan_finishes() {
        let stream = RowStream::default();
        stream.push(Arc::new(vec![7]));
        assert_eq!(*stream.chunk(0).unwrap(), vec![7]);
    }

    #[test]
    fn a_failed_scan_fails_its_readers() {
        let stream = RowStream::<u32>::default();
        stream.finish(true);
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stream.chunk(0)));
        assert!(read.is_err());
        let read =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stream.all().iter().count()));
        assert!(read.is_err());
    }

    #[test]
    fn len_counts_the_rows_made_so_far() {
        let stream = RowStream::default();
        stream.push(Arc::new(vec![1, 2]));
        assert_eq!(stream.len(), 2);
        stream.push(Arc::new(vec![3]));
        assert_eq!(stream.len(), 3);
    }

    #[test]
    fn settled_rows_are_found_by_position_across_chunks() {
        let all = stream_of(&[&[10, 11], &[12], &[13, 14]]).all();
        let found: Vec<_> = (0..6).map(|p| all.get(p).copied()).collect();
        assert_eq!(
            found,
            vec![Some(10), Some(11), Some(12), Some(13), Some(14), None]
        );
        assert_eq!(all.iter().count(), 5);
    }

    #[test]
    fn nothing_is_found_in_an_empty_stream() {
        assert!(stream_of(&[]).all().get(0).is_none());
    }
}
