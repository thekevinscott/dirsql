//! A depth-first walk whose directories are read on spare cores.
//!
//! Exploring a directory (reading and judging its entries) is independent
//! of every other directory, so workers explore ahead of the walk while the
//! calling thread hands out what they found in depth-first order.

use std::sync::{Condvar, Mutex};
use std::thread;

/// What exploring a directory yields: a leaf to hand out, or a directory to
/// explore in its place.
pub(crate) enum Step<D, L> {
    Dir(D),
    Leaf(L),
}

/// Once a walk has found this many directories it shares them across the
/// cores; below it, spawning workers costs more than the exploring.
const PARALLEL_DIRS: usize = 16;

enum Item<L> {
    Dir(usize),
    Leaf(L),
}

struct Queue<D, L> {
    /// Each directory found, until a thread claims it.
    unclaimed: Vec<Option<D>>,
    /// Each explored directory's items, until the walk reaches it.
    explored: Vec<Option<Vec<Item<L>>>>,
    /// Unclaimed directories, the next one the walk will want on top.
    open: Vec<usize>,
    done: bool,
}

struct Shared<'e, D, L> {
    queue: Mutex<Queue<D, L>>,
    changed: Condvar,
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
        queue: Mutex::new(Queue {
            unclaimed: vec![Some(root)],
            explored: vec![None],
            open: Vec::new(),
            done: false,
        }),
        changed: Condvar::new(),
        explore,
    };
    thread::scope(|scope| {
        let mut spawned = false;
        let mut stack = vec![shared.obtain(0).into_iter()];
        while let Some(items) = stack.last_mut() {
            match items.next() {
                Some(Item::Leaf(leaf)) => visit(leaf),
                Some(Item::Dir(id)) => {
                    if !spawned && shared.found() >= PARALLEL_DIRS {
                        spawned = true;
                        let workers = thread::available_parallelism().map_or(1, usize::from);
                        for _ in 1..workers {
                            scope.spawn(|| shared.work());
                        }
                    }
                    stack.push(shared.obtain(id).into_iter());
                }
                None => {
                    stack.pop();
                }
            }
        }
        shared.lock().done = true;
        shared.changed.notify_all();
    });
}

impl<D, L> Shared<'_, D, L> {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue<D, L>> {
        self.queue
            .lock()
            .expect("exploring a directory does not panic")
    }

    fn found(&self) -> usize {
        self.lock().unclaimed.len()
    }

    /// Directory `id`'s items: explored here if no worker has claimed it,
    /// else once the worker that has is through.
    fn obtain(&self, id: usize) -> Vec<Item<L>> {
        let mut queue = self.lock();
        loop {
            if let Some(items) = queue.explored[id].take() {
                return items;
            }
            if let Some(dir) = queue.unclaimed[id].take() {
                drop(queue);
                return self.explore(dir);
            }
            queue = self
                .changed
                .wait(queue)
                .expect("exploring a directory does not panic");
        }
    }

    fn work(&self) {
        let mut queue = self.lock();
        loop {
            if queue.done {
                return;
            }
            let claimed = queue.open.pop().map(|id| (id, queue.unclaimed[id].take()));
            match claimed {
                Some((id, Some(dir))) => {
                    drop(queue);
                    let items = self.explore(dir);
                    queue = self.lock();
                    queue.explored[id] = Some(items);
                    self.changed.notify_all();
                }
                Some((_, None)) => {}
                None => {
                    queue = self
                        .changed
                        .wait(queue)
                        .expect("exploring a directory does not panic");
                }
            }
        }
    }

    /// Explore `dir`, registering the directories it holds for any thread
    /// to claim.
    fn explore(&self, dir: D) -> Vec<Item<L>> {
        let steps = (self.explore)(dir);
        let mut queue = self.lock();
        let mut dirs = Vec::new();
        let items = steps
            .into_iter()
            .map(|step| match step {
                Step::Leaf(leaf) => Item::Leaf(leaf),
                Step::Dir(dir) => {
                    let id = queue.unclaimed.len();
                    queue.unclaimed.push(Some(dir));
                    queue.explored.push(None);
                    dirs.push(id);
                    Item::Dir(id)
                }
            })
            .collect();
        queue.open.extend(dirs.into_iter().rev());
        self.changed.notify_all();
        items
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

    #[test]
    fn a_walk_of_few_directories_stays_on_the_calling_thread() {
        assert_eq!(threads_exploring(3, 1), 1);
    }

    #[test]
    fn a_walk_of_a_few_dozen_directories_stays_on_the_calling_thread() {
        assert_eq!(threads_exploring(4, 2), 1);
    }

    #[test]
    fn a_walk_of_under_a_hundred_directories_stays_on_the_calling_thread() {
        assert_eq!(threads_exploring(4, 3), 1);
    }

    #[test]
    fn a_walk_spawns_its_workers_once() {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        assert!(threads_exploring(4, 4) <= cores);
    }
}
