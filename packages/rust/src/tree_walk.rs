//! A depth-first walk whose directories are read on spare cores.
//!
//! Exploring a directory (reading and judging its entries) is independent
//! of every other directory, so workers explore ahead of the walk while the
//! calling thread hands out what they found in depth-first order.

use std::cell::RefCell;
use std::cmp::Ordering as Order;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

thread_local! {
    /// How many workers are exploring for the walk this thread explores for.
    static BUSY: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
}

/// How many of its workers the walk the calling thread explores for has
/// exploring right now; none when the walk has not spread across the cores.
pub(crate) fn busy_workers() -> usize {
    BUSY.with_borrow(|busy| busy.as_ref().map_or(0, |busy| busy.load(Ordering::Relaxed)))
}

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
struct Pending<D, L>(Arc<Slot<D, L>>);

impl<D, L> PartialEq for Pending<D, L> {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key
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
        other.0.key.cmp(&self.0.key)
    }
}

struct Shared<'e, D, L> {
    open: Mutex<Open<D, L>>,
    found_dir: Condvar,
    found: AtomicUsize,
    busy: Arc<AtomicUsize>,
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
        busy: Arc::default(),
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
                            // Counted before it starts, so a directory judged
                            // meanwhile already sees the core as taken.
                            shared.busy.fetch_add(1, Ordering::Relaxed);
                            scope.spawn(|| {
                                BUSY.set(Some(Arc::clone(&shared.busy)));
                                shared.work();
                            });
                        }
                        BUSY.set(Some(Arc::clone(&shared.busy)));
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
        while let Some(slot) = self.claim() {
            let unclaimed = lock(&slot.dir).take();
            let Some(dir) = unclaimed else {
                continue;
            };
            let items = self.explore(dir, &slot.key);
            let mut explored = lock(&slot.explored);
            explored.items = Some(items);
            if explored.awaited {
                slot.ready.notify_one();
            }
        }
    }

    /// The next directory found, or `None` once the walk is over.
    fn claim(&self) -> Option<Arc<Slot<D, L>>> {
        let mut open = self.open_lock();
        loop {
            if open.done {
                self.busy.fetch_sub(1, Ordering::Relaxed);
                return None;
            }
            if let Some(Pending(slot)) = open.slots.pop() {
                return Some(slot);
            }
            self.busy.fetch_sub(1, Ordering::Relaxed);
            open = self
                .found_dir
                .wait(open)
                .expect("exploring a directory does not panic");
            self.busy.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Explore `dir`, at `key`, registering the directories it holds for any
    /// thread to claim.
    fn explore(&self, dir: D, key: &[u32]) -> Vec<Item<D, L>> {
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
        if dirs.is_empty() {
            return items;
        }
        self.found.fetch_add(dirs.len(), Ordering::Relaxed);
        let found = dirs.len();
        self.open_lock().slots.extend(dirs.into_iter().map(Pending));
        // The thread that found them takes one next, so only the rest need
        // a waiting worker.
        for _ in 1..found {
            self.found_dir.notify_one();
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

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

    fn pending(key: &[u32]) -> Pending<(), ()> {
        Pending(Arc::new(Slot {
            key: key.to_vec(),
            dir: Mutex::new(None),
            explored: Mutex::new(Explored {
                items: None,
                awaited: false,
            }),
            ready: Condvar::new(),
        }))
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
            .map(|Pending(slot)| slot.key.clone())
            .collect();
        assert_eq!(order, vec![vec![0], vec![0, 1, 5], vec![0, 2], vec![1]]);
    }

    #[test]
    fn a_walk_of_few_directories_stays_on_the_calling_thread() {
        assert_eq!(threads_exploring(3, 1), 1);
    }

    #[test]
    fn a_walk_of_a_few_dozen_directories_is_shared_across_threads() {
        assert!(threads_exploring(4, 2) > 1);
    }

    /// Whether `busy_workers` satisfies `done` within ten seconds.
    fn busy_workers_reach(done: impl Fn(usize) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if done(busy_workers()) {
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn a_walk_that_stays_on_the_calling_thread_has_no_busy_workers() {
        let tree = explore_tree(3, 1);
        let seen = Mutex::new(Vec::new());
        let explore = |path: String| {
            seen.lock().unwrap().push(busy_workers());
            tree(path)
        };
        walk_in_order(String::from("r"), &explore, &mut |_| {});
        assert_eq!(seen.into_inner().unwrap(), [0, 0, 0, 0]);
    }

    #[test]
    fn a_spread_walk_counts_its_workers_busy() {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let tree = explore_tree(8, 1);
        let seen = Mutex::new(Vec::new());
        let explore = |path: String| {
            seen.lock().unwrap().push(busy_workers());
            tree(path)
        };
        walk_in_order(String::from("r"), &explore, &mut |_| {});
        let most = seen.into_inner().unwrap().into_iter().max();
        assert_eq!(most > Some(0), cores > 1);
    }

    #[test]
    fn a_worker_waiting_for_a_directory_is_not_busy() {
        let tree = explore_tree(8, 1);
        let others_idle = Mutex::new(None);
        let explore = |path: String| {
            if path == "r/0" {
                *others_idle.lock().unwrap() = Some(busy_workers_reach(|busy| busy <= 1));
            }
            tree(path)
        };
        walk_in_order(String::from("r"), &explore, &mut |_| {});
        assert_eq!(others_idle.into_inner().unwrap(), Some(true));
    }

    #[test]
    fn a_finished_walk_leaves_no_worker_busy() {
        walk_in_order(String::from("r"), &explore_tree(8, 1), &mut |_| {});
        assert_eq!(busy_workers(), 0);
    }

    #[test]
    fn a_walk_spawns_its_workers_once() {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        assert!(threads_exploring(4, 4) <= cores);
    }
}
