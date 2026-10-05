//! A depth-first walk whose directories are read on spare cores.
//!
//! Exploring a directory (reading and judging its entries) is independent
//! of every other directory, so workers explore ahead of the walk while the
//! calling thread hands out what they found in depth-first order.


/// What exploring a directory yields: a leaf to hand out, or a directory to
/// explore in its place.
pub(crate) enum Step<D, L> {
    Dir(D),
    Leaf(L),
}

/// Hand `visit` every leaf under `root`, depth first in the order `explore`
/// yields each directory's steps.
pub(crate) fn walk_in_order<D: Send, L: Send>(
    root: D,
    explore: &(dyn Fn(D) -> Vec<Step<D, L>> + Sync),
    visit: &mut dyn FnMut(L),
) {
    for step in explore(root) {
        match step {
            Step::Dir(dir) => walk_in_order(dir, explore, visit),
            Step::Leaf(leaf) => visit(leaf),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::thread;
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
        let tree = explore_tree(4, 3);
        let threads = Mutex::new(HashSet::new());
        let explore = |path: String| {
            threads.lock().unwrap().insert(thread::current().id());
            thread::sleep(Duration::from_millis(2));
            tree(path)
        };
        walk_in_order(String::from("r"), &explore, &mut |_| {});
        assert!(threads.into_inner().unwrap().len() > 1);
    }
}
