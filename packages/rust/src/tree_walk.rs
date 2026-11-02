//! A depth-first walk whose directories are read on spare cores.
//!
//! Exploring a directory (reading and judging its entries) is independent
//! of every other directory, so workers explore ahead of the walk while the
//! calling thread hands out what they found in depth-first order.

use std::cmp::Ordering as Order;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

/// What exploring a directory yields: a leaf to hand out, or a directory to
/// explore in its place.
pub(crate) enum Step<D, L> {
    Dir(D),
    Leaf(L),
}

/// Once a walk has found this many directories it shares them across the
/// cores; below it, spawning workers costs more than the exploring.
const PARALLEL_DIRS: usize = 8;

enum Item<D, L> {
    Dir(Arc<Slot<D, L>>),
    Leaf(L),
}

/// One directory found: unexplored until a thread takes it, then its items
/// once that thread is through. Each directory has its own locks, so the walk
/// waiting on one never contends with workers exploring others.
struct Slot<D, L> {
    /// The directory's place in the walk: each step's index among its
    /// parent's directories, root first, so keys order as the walk reaches
    /// them.
    key: Vec<u32>,
    dir: Mutex<Option<D>>,
    explored: Mutex<Explored<D, L>>,
    ready: Condvar,
}

struct Explored<D, L> {
    items: Option<Vec<Item<D, L>>>,
    /// Whether the walk is waiting on these items, so a worker signals only
    /// then.
    awaited: bool,
}

/// Directories found and not yet claimed. Workers take the one the walk will
/// reach first, so they explore just ahead of it rather than deep in a subtree
/// it is far from, which would leave it to explore its next directory itself.
struct Open<D, L> {
    slots: BinaryHeap<Pending<D, L>>,
    done: bool,
}

/// A found directory, ordered so the heap's greatest is the earliest key.
/// The key's leading bytes ride along as `rank`, so most comparisons never
/// leave the heap's own memory: the heap is ordered under the lock every
/// worker takes, and a comparison reaching into a slot another core wrote
/// held that lock long enough to serialize the workers.
struct Pending<D, L> {
    rank: u128,
    slot: Arc<Slot<D, L>>,
}

impl<D, L> Pending<D, L> {
    fn new(slot: Arc<Slot<D, L>>) -> Self {
        Pending {
            rank: rank(&slot.key),
            slot,
        }
    }
}

/// The first sixteen bytes of `key` encoded so that bytes order as keys do:
/// each index plus one, in one byte below 0x80, two below 0x4000, else a
/// 0xC0 byte and four more. No encoded index starts with a zero byte, so a
/// key ranks below every key it is a prefix of.
fn rank(key: &[u32]) -> u128 {
    let mut bytes = [0u8; 16];
    let mut at = 0;
    for &index in key {
        let [b0, b1, b2, b3] = index.saturating_add(1).to_be_bytes();
        let encoded: &[u8] = if index < 0x7F {
            &[b3]
        } else if index < 0x3FFF {
            &[0x80 + b2, b3]
        } else {
            &[0xC0, b0, b1, b2, b3]
        };
        for &byte in encoded {
            let Some(slot) = bytes.get_mut(at) else {
                return u128::from_be_bytes(bytes);
            };
            *slot = byte;
            at += 1;
        }
    }
    u128::from_be_bytes(bytes)
}

impl<D, L> PartialEq for Pending<D, L> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Order::Equal
    }
}

impl<D, L> Eq for Pending<D, L> {}

impl<D, L> PartialOrd for Pending<D, L> {
    fn partial_cmp(&self, other: &Self) -> Option<Order> {
        Some(self.cmp(other))
    }
}

impl<D, L> Ord for Pending<D, L> {
    fn cmp(&self, other: &Self) -> Order {
        other
            .rank
            .cmp(&self.rank)
            .then_with(|| other.slot.key.cmp(&self.slot.key))
    }
}

/// A directory's items, and the directories among them.
type Found<D, L> = (Vec<Item<D, L>>, Vec<Arc<Slot<D, L>>>);

struct Shared<'e, D, L> {
    open: Mutex<Open<D, L>>,
    found_dir: Condvar,
    found: AtomicUsize,
    explore: &'e (dyn Fn(D) -> Vec<Step<D, L>> + Sync),
}

/// Hand `visit` every leaf under `root`, depth first in the order `explore`
/// yields each directory's steps, exploring directories on every core.
pub(crate) fn walk_in_order<D: Send, L: Send>(
    root: D,
    explore: &(dyn Fn(D) -> Vec<Step<D, L>> + Sync),
    visit: &mut dyn FnMut(L),
) {
    let shared = Shared {
        open: Mutex::new(Open {
            slots: BinaryHeap::new(),
            done: false,
        }),
        found_dir: Condvar::new(),
        found: AtomicUsize::new(1),
        explore,
    };
    thread::scope(|scope| {
        let mut spawned = false;
        let mut stack = vec![shared.explore(root, &[]).into_iter()];
        while let Some(items) = stack.last_mut() {
            match items.next() {
                Some(Item::Leaf(leaf)) => visit(leaf),
                Some(Item::Dir(slot)) => {
                    if !spawned && shared.found.load(Ordering::Relaxed) >= PARALLEL_DIRS {
                        spawned = true;
                        let workers = thread::available_parallelism().map_or(1, usize::from);
                        for _ in 1..workers {
                            scope.spawn(|| shared.work());
                        }
                    }
                    stack.push(shared.obtain(&slot).into_iter());
                }
                None => {
                    stack.pop();
                }
            }
        }
        shared.open_lock().done = true;
        shared.found_dir.notify_all();
    });
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("exploring a directory does not panic")
}

impl<D, L> Shared<'_, D, L> {
    fn open_lock(&self) -> MutexGuard<'_, Open<D, L>> {
        lock(&self.open)
    }

    /// Directory `slot`'s items: explored here if no worker has claimed it,
    /// else once the worker that has is through.
    fn obtain(&self, slot: &Slot<D, L>) -> Vec<Item<D, L>> {
        let unclaimed = lock(&slot.dir).take();
        if let Some(dir) = unclaimed {
            return self.explore(dir, &slot.key);
        }
        let mut explored = lock(&slot.explored);
        loop {
            if let Some(items) = explored.items.take() {
                return items;
            }
            explored.awaited = true;
            explored = slot
                .ready
                .wait(explored)
                .expect("exploring a directory does not panic");
        }
    }

    fn work(&self) {
        let mut next = self.claim();
        while let Some(slot) = next {
            let unclaimed = lock(&slot.dir).take();
            next = unclaimed
                .and_then(|dir| self.explore_taking_next(&slot, dir))
                .or_else(|| self.claim());
        }
    }

    /// Explore `dir`, `slot`'s directory, for whoever awaits it, taking back
    /// the next directory to explore if one is open. Registering what it
    /// found and taking the next under one hold halves how often a worker
    /// takes the lock every worker shares.
    fn explore_taking_next(&self, slot: &Slot<D, L>, dir: D) -> Option<Arc<Slot<D, L>>> {
        let (items, dirs) = self.explore_only(dir, &slot.key);
        let next = self.register(dirs, true);
        let mut explored = lock(&slot.explored);
        explored.items = Some(items);
        if explored.awaited {
            slot.ready.notify_one();
        }
        next
    }

    /// The next directory found, or `None` once the walk is over.
    fn claim(&self) -> Option<Arc<Slot<D, L>>> {
        let mut open = self.open_lock();
        loop {
            if open.done {
                return None;
            }
            if let Some(Pending { slot, .. }) = open.slots.pop() {
                return Some(slot);
            }
            open = self
                .found_dir
                .wait(open)
                .expect("exploring a directory does not panic");
        }
    }

    /// Explore `dir`, at `key`, registering the directories it holds for any
    /// thread to claim.
    fn explore(&self, dir: D, key: &[u32]) -> Vec<Item<D, L>> {
        let (items, dirs) = self.explore_only(dir, key);
        if !dirs.is_empty() {
            self.register(dirs, false);
        }
        items
    }

    /// Open `dirs` to any thread, taking the earliest open directory back
    /// when `take` is set.
    fn register(&self, dirs: Vec<Arc<Slot<D, L>>>, take: bool) -> Option<Arc<Slot<D, L>>> {
        let found = dirs.len();
        let mut open = self.open_lock();
        open.slots.extend(dirs.into_iter().map(Pending::new));
        let taken = take.then(|| open.slots.pop()).flatten();
        drop(open);
        // The thread that found them takes one next, so only the rest need
        // a waiting worker.
        for _ in 1..found {
            self.found_dir.notify_one();
        }
        taken.map(|pending| pending.slot)
    }

    /// Explore `dir`, at `key`.
    fn explore_only(&self, dir: D, key: &[u32]) -> Found<D, L> {
        let steps = (self.explore)(dir);
        let mut dirs = Vec::new();
        let items: Vec<Item<D, L>> = steps
            .into_iter()
            .map(|step| match step {
                Step::Leaf(leaf) => Item::Leaf(leaf),
                Step::Dir(dir) => {
                    let index = u32::try_from(dirs.len()).unwrap_or(u32::MAX);
                    let slot = Arc::new(Slot {
                        key: [key, &[index]].concat(),
                        dir: Mutex::new(Some(dir)),
                        explored: Mutex::new(Explored {
                            items: None,
                            awaited: false,
                        }),
                        ready: Condvar::new(),
                    });
                    dirs.push(Arc::clone(&slot));
                    Item::Dir(slot)
                }
            })
            .collect();
        if !dirs.is_empty() {
            self.found.fetch_add(dirs.len(), Ordering::Relaxed);
        }
        (items, dirs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::Duration;

    /// A tree of `fanout` directories per level, `depth` levels deep, each
    /// holding one leaf named for its path, with the leaf before its
    /// subdirectories.
    fn explore_tree(fanout: usize, depth: usize) -> impl Fn(String) -> Vec<Step<String, String>> {
        move |path: String| {
            let mut steps = vec![Step::Leaf(format!("{path}/f"))];
            if path.matches('/').count() < depth {
                steps.extend((0..fanout).map(|i| Step::Dir(format!("{path}/{i}"))));
            }
            steps
        }
    }

    fn depth_first(path: &str, fanout: usize, depth: usize, out: &mut Vec<String>) {
        out.push(format!("{path}/f"));
        if path.matches('/').count() < depth {
            for i in 0..fanout {
                depth_first(&format!("{path}/{i}"), fanout, depth, out);
            }
        }
    }

    #[test]
    fn leaves_come_out_depth_first_in_the_order_explored() {
        let explore = explore_tree(3, 4);
        let mut leaves = Vec::new();
        walk_in_order(String::from("r"), &explore, &mut |leaf| leaves.push(leaf));
        let mut expected = Vec::new();
        depth_first("r", 3, 4, &mut expected);
        assert_eq!(leaves, expected);
    }

    #[test]
    fn a_lone_directory_yields_its_leaves() {
        let explore = |(): ()| vec![Step::Leaf(1), Step::Leaf(2)];
        let mut leaves = Vec::new();
        walk_in_order((), &explore, &mut |leaf| leaves.push(leaf));
        assert_eq!(leaves, [1, 2]);
    }

    #[test]
    fn sibling_directories_are_explored_on_more_than_one_thread() {
        assert!(threads_exploring(4, 4) > 1);
    }

    fn threads_exploring(fanout: usize, depth: usize) -> usize {
        let tree = explore_tree(fanout, depth);
        let threads = Mutex::new(HashSet::new());
        let explore = |path: String| {
            threads.lock().unwrap().insert(thread::current().id());
            thread::sleep(Duration::from_millis(2));
            tree(path)
        };
        walk_in_order(String::from("r"), &explore, &mut |_| {});
        threads.into_inner().unwrap().len()
    }

    fn slot(key: &[u32]) -> Arc<Slot<(), ()>> {
        Arc::new(Slot {
            key: key.to_vec(),
            dir: Mutex::new(None),
            explored: Mutex::new(Explored {
                items: None,
                awaited: false,
            }),
            ready: Condvar::new(),
        })
    }

    fn pending(key: &[u32]) -> Pending<(), ()> {
        Pending::new(slot(key))
    }

    #[test]
    fn registering_opens_the_directories_and_may_take_back_the_earliest() {
        let explore = |(): ()| Vec::new();
        let shared = Shared {
            open: Mutex::new(Open {
                slots: BinaryHeap::new(),
                done: false,
            }),
            found_dir: Condvar::new(),
            found: AtomicUsize::new(0),
            explore: &explore,
        };
        assert!(
            shared
                .register(vec![slot(&[1]), slot(&[0])], false)
                .is_none()
        );
        let taken = shared.register(vec![slot(&[2])], true);
        assert_eq!(taken.map(|slot| slot.key.clone()), Some(vec![0]));
        assert_eq!(shared.open_lock().slots.len(), 2);
    }

    #[test]
    fn open_directories_are_equal_exactly_when_their_keys_are() {
        assert!(pending(&[0, 1]) == pending(&[0, 1]));
        assert!(pending(&[0, 1]) != pending(&[0, 2]));
    }

    #[test]
    fn the_open_directories_yield_the_one_the_walk_reaches_first() {
        let mut open = BinaryHeap::new();
        for key in [&[1][..], &[0, 2], &[0], &[0, 1, 5]] {
            open.push(pending(key));
        }
        let order: Vec<Vec<u32>> = std::iter::from_fn(|| open.pop())
            .map(|pending| pending.slot.key.clone())
            .collect();
        assert_eq!(order, vec![vec![0], vec![0, 1, 5], vec![0, 2], vec![1]]);
    }

    #[test]
    fn ranks_order_as_their_keys_do() {
        let boundaries = [0, 1, 0x7E, 0x7F, 0x80, 0x3FFE, 0x3FFF, 0x4000, u32::MAX - 1];
        let mut keys: Vec<Vec<u32>> = vec![vec![]];
        for &a in &boundaries {
            keys.push(vec![a]);
            for &b in &boundaries {
                keys.push(vec![a, b]);
            }
        }
        keys.sort();
        let ranks: Vec<u128> = keys.iter().map(|key| rank(key)).collect();
        assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn keys_past_sixteen_encoded_bytes_are_ordered_by_the_whole_key() {
        let deep = [0x4000; 4];
        let mut open = BinaryHeap::new();
        for last in [2, 1] {
            open.push(pending(&[&deep[..], &[last]].concat()));
        }
        assert_eq!(open.pop().unwrap().slot.key.last(), Some(&1));
    }

    #[test]
    fn a_walk_of_few_directories_stays_on_the_calling_thread() {
        assert_eq!(threads_exploring(3, 1), 1);
    }

    #[test]
    fn a_walk_of_a_few_dozen_directories_is_shared_across_threads() {
        assert!(threads_exploring(4, 2) > 1);
    }

    #[test]
    fn a_walk_spawns_its_workers_once() {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        assert!(threads_exploring(4, 4) <= cores);
    }
}
