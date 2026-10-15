//! A value each thread reaches through a clone of its own.
//!
//! A `Regex` keeps its match scratch in a pool every thread shares. With a
//! walk's workers all matching at once, taking and returning that scratch
//! contends, and a thread that loses the race builds fresh scratch for the
//! one match. A cloned `Regex` has a pool of its own.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

#[derive(Debug, Clone)]
pub(crate) struct PerThread<T> {
    value: T,
    /// Shared by every clone of this `PerThread`, so they share each thread's
    /// copy; a thread's copy is dropped once no clone holds the token.
    token: Arc<()>,
}

type Copies = Vec<(Weak<()>, Rc<dyn Any>)>;

thread_local! {
    static COPIES: RefCell<Copies> = const { RefCell::new(Vec::new()) };
}

impl<T: Clone + 'static> PerThread<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value,
            token: Arc::new(()),
        }
    }

    /// `f` applied to the calling thread's copy of the value, cloned on the
    /// thread's first call.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let copy = COPIES.with_borrow_mut(|copies| self.copy_in(copies));
        f(copy
            .downcast_ref::<T>()
            .expect("a token's copy is of the type it was made from"))
    }

    fn copy_in(&self, copies: &mut Copies) -> Rc<dyn Any> {
        let token = Arc::as_ptr(&self.token);
        if let Some((_, copy)) = copies.iter().find(|(held, _)| held.as_ptr() == token) {
            return Rc::clone(copy);
        }
        copies.retain(|(held, _)| held.strong_count() > 0);
        let copy: Rc<dyn Any> = Rc::new(self.value.clone());
        copies.push((Arc::downgrade(&self.token), Rc::clone(&copy)));
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn address(shared: &PerThread<Box<u32>>) -> usize {
        shared.with(|value| std::ptr::from_ref::<u32>(value).addr())
    }

    fn held() -> usize {
        COPIES.with_borrow(Vec::len)
    }

    #[test]
    fn a_thread_reaches_one_copy_on_every_call() {
        let shared = PerThread::new(Box::new(7));
        assert_eq!(address(&shared), address(&shared));
        assert_eq!(shared.with(|value| **value), 7);
    }

    #[test]
    fn each_thread_reaches_a_copy_of_its_own() {
        let shared = PerThread::new(Box::new(7));
        let original = std::ptr::from_ref::<u32>(&shared.value).addr();
        // Both threads hold their copy until each has its address, so a freed
        // copy's memory cannot come back as the other's.
        let both_hold = std::sync::Barrier::new(2);
        let addresses: Vec<usize> = thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        let at = address(&shared);
                        both_hold.wait();
                        at
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_ne!(addresses[0], addresses[1]);
        assert!(!addresses.contains(&original));
    }

    #[test]
    fn clones_share_a_thread_copy() {
        let shared = PerThread::new(Box::new(7));
        assert_eq!(address(&shared), address(&shared.clone()));
    }

    #[test]
    fn a_dropped_value_frees_its_copy_and_a_live_one_keeps_it() {
        thread::spawn(|| {
            let kept = PerThread::new(Box::new(1));
            let kept_at = address(&kept);
            address(&PerThread::new(Box::new(2)));
            assert_eq!(held(), 2);
            let fresh = PerThread::new(Box::new(3));
            assert_eq!(fresh.with(|value| **value), 3);
            assert_eq!(held(), 2);
            assert_eq!(address(&kept), kept_at);
        })
        .join()
        .unwrap();
    }
}
