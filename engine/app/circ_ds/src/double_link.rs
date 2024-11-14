use std::sync::atomic::Ordering;

use circ::{AtomicRc, Guard, Rc, RcObject, Snapshot, Weak};
use crossbeam_utils::CachePadded;

pub struct Output<'g, T> {
    found: Snapshot<'g, Node<T>>,
}

impl<'g, T> Output<'g, T> {
    pub fn output(&self) -> &T {
        self.found
            .as_ref()
            .map(|node| node.item.as_ref().unwrap())
            .unwrap()
    }
}

struct Node<T> {
    item: Option<T>,
    prev: Weak<Node<T>>,
    next: CachePadded<AtomicRc<Node<T>>>,
}

unsafe impl<T> RcObject for Node<T> {
    fn pop_edges(&mut self, out: &mut Vec<Rc<Self>>) {
        out.push(self.next.take());
    }
    // const UNIQUE_OUTDEGREE: bool = true;

    // #[inline]
    // fn pop_outgoings(&mut self, result: &mut Vec<Rc<Self, Guard>>)
    // where
    //     Self: Sized,
    // {
    //     result.push(self.next.take());
    // }

    // #[inline]
    // fn pop_unique(&mut self) -> Rc<Self, Guard>
    // where
    //     Self: Sized,
    // {
    //     self.next.take()
    // }
}

impl<T> Node<T> {
    fn sentinel() -> Self {
        Self {
            item: None,
            prev: Weak::null(),
            next: CachePadded::new(AtomicRc::null()),
        }
    }

    fn new(item: T) -> Self {
        Self {
            item: Some(item),
            prev: Weak::null(),
            next: CachePadded::new(AtomicRc::null()),
        }
    }
}

unsafe impl<T: Sync> Sync for Node<T> {}
unsafe impl<T: Sync> Send for Node<T> {}

pub struct DoubleLink<T: Sync + Send> {
    head: CachePadded<AtomicRc<Node<T>>>,
    tail: CachePadded<AtomicRc<Node<T>>>,
}

impl<T: Sync + Send> Default for DoubleLink<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Sync + Send> DoubleLink<T> {
    #[inline]
    pub fn new() -> Self {
        let sentinel = Rc::new(Node::sentinel());
        // Note: In RC-based SMRs(CDRC, CIRC, ...), `sentinel.prev` MUST NOT be set to the self.
        // It will make a loop after the first enqueue, blocking the entire reclamation.
        Self {
            head: CachePadded::new(AtomicRc::from(sentinel.clone())),
            tail: CachePadded::new(AtomicRc::from(sentinel)),
        }
    }

    #[inline]
    pub fn enqueue(&self, item: T, cs: &Guard) {
        let [mut node, sub] = Rc::new_many(Node::new(item));

        loop {
            let ltail = self.tail.load(Ordering::Acquire, cs);
            unsafe { node.deref_mut() }.prev = ltail.downgrade().counted();

            // Try to help the previous enqueue to complete.
            let lprev = ltail
                .as_ref()
                .and_then(|ltail| ltail.prev.upgrade())
                .map(|lprev_rc| lprev_rc.snapshot(cs));
            if let Some(lprev) = lprev {
                if let Some(lprev) = lprev.as_ref() {
                    if lprev.next.load(Ordering::SeqCst, cs).is_null() {
                        lprev.next.store(ltail.counted(), Ordering::Relaxed, cs);
                    }
                }
            }
            match self
                .tail
                .compare_exchange(ltail, node, Ordering::SeqCst, Ordering::SeqCst, cs)
            {
                Ok(_) => {
                    unsafe { ltail.deref() }
                        .next
                        .store(sub, Ordering::Release, cs);
                    return;
                }
                Err(e) => node = e.desired,
            }
        }
    }

    #[inline]
    pub fn dequeue<'g>(&self, cs: &'g Guard) -> Option<Output<'g, T>> {
        loop {
            let lhead = self.head.load(Ordering::Acquire, cs);
            let lnext = unsafe { lhead.deref().next.load(Ordering::Acquire, cs) };
            // Check if this queue is empty.
            if lnext.is_null() {
                return None;
            }

            if self
                .head
                .compare_exchange(
                    lhead,
                    lnext.counted(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    cs,
                )
                .is_ok()
            {
                return Some(Output { found: lnext });
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::DoubleLink;
    use circ::cs;

    #[test]
    fn simple() {
        let context = dyntls_host::get();
        unsafe { context.initialize() };
        let queue: DoubleLink<u32> = DoubleLink::new();
        let guard = &cs();
        // assert!(queue.dequeue(guard).is_none());
        queue.enqueue(1, guard);
        queue.enqueue(2, guard);
        queue.enqueue(3, guard);
        assert_eq!(*queue.dequeue(guard).unwrap().output(), 1);
        assert_eq!(*queue.dequeue(guard).unwrap().output(), 2);
        assert_eq!(*queue.dequeue(guard).unwrap().output(), 3);
        assert!(queue.dequeue(guard).is_none());
    }
}
