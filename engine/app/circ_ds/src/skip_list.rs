use std::{fmt::Display, mem::forget, sync::atomic::Ordering};

use circ::{cs, AtomicRc, Guard, Rc, RcObject, Snapshot};

use crate::some_or;

use super::concurrent_map::{ConcurrentMap, OutputHolder};

const MAX_HEIGHT: usize = 32;

type Tower<K, V> = [AtomicRc<Node<K, V>>; MAX_HEIGHT];

pub struct Node<K, V> {
    key: K,
    value: V,
    next: Tower<K, V>,
    height: usize,
}

unsafe impl<K, V> RcObject for Node<K, V> {
    fn pop_edges(&mut self, out: &mut Vec<Rc<Self>>) {
        out.extend(self.next.iter_mut().filter_map(|next| {
            let rc = next.take();
            if rc.is_null() {
                None
            } else {
                Some(rc)
            }
        }));
    }
    // const UNIQUE_OUTDEGREE: bool = false;

    // #[inline]
    // fn pop_outgoings(&mut self, result: &mut Vec<Rc<Self, Guard>>)
    // where
    //     Self: Sized,
    // {
    //     result.extend(self.next.iter_mut().filter_map(|next| {
    //         let rc = next.take();
    //         if rc.is_null() {
    //             None
    //         } else {
    //             Some(rc)
    //         }
    //     }));
    // }

    // #[inline]
    // fn pop_unique(&mut self) -> Rc<Self, Guard>
    // where
    //     Self: Sized,
    // {
    //     unimplemented!()
    // }
}

impl<K, V> Node<K, V>
where
    K: Default,
    V: Default,
{
    pub fn new(key: K, value: V) -> Self {
        let cs = unsafe { &circ::unprotected_cs() };
        let height = Self::generate_height();
        let next: [AtomicRc<Node<K, V>>; MAX_HEIGHT] = Default::default();
        for link in next.iter().take(height) {
            link.store(Rc::null().with_tag(2), Ordering::Relaxed, cs);
        }
        Self {
            key,
            value,
            next,
            height,
        }
    }

    pub fn head() -> Self {
        Self {
            key: K::default(),
            value: V::default(),
            next: Default::default(),
            height: MAX_HEIGHT,
        }
    }

    fn generate_height() -> usize {
        // returns 1 with probability 3/4
        if rand::random::<usize>() % 4 < 3 {
            return 1;
        }
        // returns h with probability 2^(−(h+1))
        let mut height = 2;
        while height < MAX_HEIGHT && rand::random::<bool>() {
            height += 1;
        }
        height
    }

    pub fn mark_tower(&self, cs: &Guard) -> bool {
        for level in (0..self.height).rev() {
            loop {
                let aux = self.next[level].load(Ordering::Acquire, cs);
                if aux.tag() & 1 != 0 {
                    if level == 0 {
                        return false;
                    }
                    break;
                }
                match self.next[level].compare_exchange_tag(
                    aux,
                    1 | aux.tag(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    cs,
                ) {
                    Ok(_) => break,
                    Err(e) => {
                        // If the level 0 pointer was already marked, somebody else removed the node.
                        if level == 0 && (e.current.tag() & 1) != 0 {
                            return false;
                        }
                        if e.current.tag() & 1 != 0 {
                            break;
                        }
                    }
                }
            }
        }
        true
    }
}

pub struct Cursor<'g, K, V> {
    preds: [Snapshot<'g, Node<K, V>>; MAX_HEIGHT],
    succs: [Snapshot<'g, Node<K, V>>; MAX_HEIGHT],
    found: Option<Snapshot<'g, Node<K, V>>>,
}

impl<'g, K, V> OutputHolder<V> for Snapshot<'g, Node<K, V>> {
    fn output(&self) -> &V {
        self.as_ref().map(|node| &node.value).unwrap()
    }
}

impl<'g, K, V> Cursor<'g, K, V> {
    fn new(head: &AtomicRc<Node<K, V>>, guard: &'g Guard) -> Self {
        let head = head.load(Ordering::Acquire, guard);
        Self {
            preds: [head; MAX_HEIGHT],
            succs: [Snapshot::null(); MAX_HEIGHT],
            found: None,
        }
    }
}

pub struct SkipList<K, V> {
    head: AtomicRc<Node<K, V>>,
}

impl<K, V> Default for SkipList<K, V>
where
    K: Ord + Clone + Default,
    V: Clone + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> SkipList<K, V>
where
    K: Ord + Clone + Default,
    V: Clone + Default,
{
    pub fn new() -> Self {
        Self {
            head: AtomicRc::new(Node::head()),
        }
    }

    fn find_optimistic<'g>(&self, key: &K, cs: &'g Guard) -> Option<Snapshot<'g, Node<K, V>>> {
        let mut pred = self.head.load(Ordering::Acquire, cs);
        let mut level = MAX_HEIGHT;
        while level >= 1
            && unsafe { pred.deref() }.next[level - 1]
                .load(Ordering::Relaxed, cs)
                .is_null()
        {
            level -= 1;
        }

        let mut curr = Snapshot::null();
        while level >= 1 {
            level -= 1;
            curr = unsafe { pred.deref() }.next[level].load(Ordering::Acquire, cs);

            loop {
                let curr_node = some_or!(curr.as_ref(), break);
                let succ = curr_node.next[level].load(Ordering::Acquire, cs);

                if succ.tag() != 0 {
                    curr = succ;
                    continue;
                }

                if curr_node.key < *key {
                    pred = curr;
                    curr = succ;
                } else {
                    break;
                }
            }
        }

        if let Some(curr_node) = curr.as_ref() {
            if curr_node.key == *key {
                return Some(curr);
            }
        }
        None
    }

    fn find<'g>(&self, key: &K, cs: &'g Guard) -> Cursor<'g, K, V> {
        'search: loop {
            let mut cursor = Cursor::new(&self.head, cs);
            let head = cursor.preds[0];

            let mut level = MAX_HEIGHT;
            while level >= 1
                && unsafe { head.deref() }.next[level - 1]
                    .load(Ordering::Relaxed, cs)
                    .is_null()
            {
                level -= 1;
            }

            let mut pred = head;
            while level >= 1 {
                level -= 1;
                let mut curr = unsafe { pred.deref() }.next[level].load(Ordering::Acquire, cs);
                // If the next node of `pred` is marked, that means `pred` is removed and we have
                // to restart the search.
                if curr.tag() == 1 {
                    continue 'search;
                }

                while let Some(curr_ref) = curr.as_ref() {
                    let mut succ = curr_ref.next[level].load(Ordering::Acquire, cs);

                    if succ.tag() == 1 {
                        succ = succ.with_tag(0);
                        if self.help_unlink(&pred, &curr, &succ, level, cs) {
                            curr = succ;
                            continue;
                        } else {
                            // On failure, we cannot do anything reasonable to continue
                            // searching from the current position. Restart the search.
                            continue 'search;
                        }
                    }

                    // If `succ` contains a key that is greater than or equal to `key`, we're
                    // done with this level.
                    match curr_ref.key.cmp(key) {
                        std::cmp::Ordering::Greater => break,
                        std::cmp::Ordering::Equal => {
                            cursor.found = Some(curr);
                            break;
                        }
                        std::cmp::Ordering::Less => {}
                    }

                    // Move one step forward.
                    pred = curr;
                    curr = succ;
                }

                cursor.preds[level] = pred;
                cursor.succs[level] = curr;
            }

            return cursor;
        }
    }

    fn help_unlink(
        &self,
        pred: &Snapshot<Node<K, V>>,
        curr: &Snapshot<Node<K, V>>,
        succ: &Snapshot<Node<K, V>>,
        level: usize,
        cs: &Guard,
    ) -> bool {
        match unsafe { pred.deref() }.next[level].compare_exchange(
            curr.with_tag(0),
            succ.counted(),
            Ordering::Release,
            Ordering::Relaxed,
            cs,
        ) {
            Ok(rc) => {
                rc.finalize(cs);
                true
            }
            Err(e) => {
                e.desired.finalize(cs);
                false
            }
        }
    }

    pub fn insert(&self, key: K, value: V, cs: &Guard) -> bool {
        let mut cursor = self.find(&key, cs);
        if cursor.found.is_some() {
            return false;
        }

        let inner = Node::new(key, value);
        let height = inner.height;
        let mut new_node_iter = Rc::new_many_iter(inner, height);
        let mut new_node = new_node_iter.next().unwrap();
        let new_node_clone = new_node.clone();
        let new_node_ref = unsafe { new_node_clone.deref() };

        loop {
            new_node_ref.next[0].store(cursor.succs[0].counted(), Ordering::Relaxed, cs);

            match unsafe { cursor.preds[0].deref() }.next[0].compare_exchange(
                cursor.succs[0],
                new_node,
                Ordering::SeqCst,
                Ordering::SeqCst,
                cs,
            ) {
                Ok(_) => break,
                Err(e) => new_node = e.desired,
            }

            // We failed. Let's search for the key and try again.
            cursor = self.find(&new_node_ref.key, cs);
            if cursor.found.is_some() {
                // unsafe { new_node.into_inner_unchecked() };
                new_node_clone.finalize(cs);
                new_node.finalize(cs);
                forget(new_node_iter);
                return false;
            }
        }

        // The new node was successfully installed.
        // Build the rest of the tower above level 0.
        'build: for level in 1..height {
            let mut new_node = new_node_iter.next().unwrap();
            loop {
                let next = new_node_ref.next[level].load(Ordering::Acquire, cs);

                // If the current pointer is marked, that means another thread is already
                // removing the node we've just inserted. In that case, let's just stop
                // building the tower.
                if (next.tag() & 1) != 0 {
                    new_node_iter.abort(cs);
                    break 'build;
                }

                if new_node_ref.next[level]
                    .compare_exchange(
                        next,
                        cursor.succs[level].counted(),
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        cs,
                    )
                    .is_err()
                {
                    new_node_iter.abort(cs);
                    break 'build;
                }

                // Try installing the new node at the current level.
                match unsafe { cursor.preds[level].deref() }.next[level].compare_exchange(
                    cursor.succs[level],
                    new_node,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    cs,
                ) {
                    // Success! Continue on the next level.
                    Ok(_) => break,
                    Err(e) => new_node = e.desired,
                }

                // Installation failed.
                cursor = self.find(&new_node_ref.key, cs);
            }
        }

        true
    }

    pub fn remove<'g>(&self, key: &K, cs: &'g Guard) -> Option<Snapshot<'g, Node<K, V>>> {
        loop {
            let cursor = self.find(key, cs);
            let node = unsafe { cursor.found?.deref() };

            // Try removing the node by marking its tower.
            if node.mark_tower(cs) {
                for level in (0..node.height).rev() {
                    let mut next = node.next[level].load(Ordering::Acquire, cs);
                    if (next.tag() & 2) != 0 {
                        continue;
                    }
                    // Try linking the predecessor and successor at this level.
                    next = next.with_tag(0);
                    if !self.help_unlink(
                        &cursor.preds[level],
                        cursor.found.as_ref().unwrap(),
                        &next,
                        level,
                        cs,
                    ) {
                        self.find(key, cs);
                        break;
                    }
                }
                return Some(cursor.found.unwrap());
            }
        }
    }
}

impl<K, V> ConcurrentMap<K, V> for SkipList<K, V>
where
    K: Ord + Clone + Default,
    V: Clone + Default + Display,
{
    type Output<'a>
        = Snapshot<'a, Node<K, V>>
    where
        Self: 'a;

    fn new() -> Self {
        SkipList::new()
    }

    #[inline(always)]
    fn get<'g>(&self, key: &K, cs: &'g Guard) -> Option<Self::Output<'g>> {
        self.find_optimistic(key, cs)
    }

    #[inline(always)]
    fn insert(&self, key: K, value: V, cs: &Guard) -> bool {
        self.insert(key, value, cs)
    }

    #[inline(always)]
    fn remove<'g>(&self, key: &K, cs: &'g Guard) -> Option<Self::Output<'g>> {
        self.remove(key, cs)
    }
}

#[cfg(test)]
mod tests {
    use super::SkipList;
    use crate::concurrent_map;

    #[test]
    #[ignore = "currently only support manual test to see memory usage in tracy"]
    fn smoke_skip_list() {
        concurrent_map::tests::smoke::<SkipList<i32, String>>();
    }
}
