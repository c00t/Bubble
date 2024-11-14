use circ::{AtomicRc, Guard, Rc, RcObject, Snapshot}; //Pointer, , StrongPtr, TaggedCnt};

use super::concurrent_map::{ConcurrentMap, OutputHolder};

use std::{cmp, sync::atomic::Ordering};

use bitflags::bitflags;

static WEIGHT: usize = 2;

// TODO: optimization from the paper? IBR paper doesn't do that

bitflags! {
    /// TODO
    struct Retired: usize {
        const RETIRED = 1usize;
    }
}

impl Retired {
    fn new(retired: bool) -> Self {
        if retired {
            Retired::RETIRED
        } else {
            Retired::empty()
        }
    }

    fn retired(self) -> bool {
        !(self & Retired::RETIRED).is_empty()
    }
}

/// a real node in tree or a wrapper of State node
/// Retired node if Shared ptr of Node has RETIRED tag.
pub struct Node<K, V> {
    key: K,
    value: V,
    size: usize,
    left: AtomicRc<Node<K, V>>,
    right: AtomicRc<Node<K, V>>,
}

unsafe impl<K, V> RcObject for Node<K, V> {
    fn pop_edges(&mut self, out: &mut Vec<Rc<Self>>) {
        out.push(self.left.take());
        out.push(self.right.take());
    }
}

impl<K, V> Node<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn retired_node() -> Rc<Self> {
        Rc::null().with_tag(Retired::new(true).bits())
    }

    fn is_retired(tag: usize) -> bool {
        Retired::from_bits_truncate(tag).retired()
    }

    fn is_retired_spot(node: &Snapshot<Node<K, V>>) -> bool {
        if Self::is_retired(node.tag()) {
            return true;
        }

        if let Some(node_ref) = node.as_ref() {
            let guard = circ::cs();
            Self::is_retired(node_ref.left.load(Ordering::Acquire, &guard).tag())
                || Self::is_retired(node_ref.right.load(Ordering::Acquire, &guard).tag())
        } else {
            false
        }
    }

    fn node_size(node: &Snapshot<Node<K, V>>) -> usize {
        debug_assert!(!Self::is_retired(node.tag()));
        if let Some(node_ref) = node.as_ref() {
            node_ref.size
        } else {
            0
        }
    }

    fn load_children<'g>(&self, cs: &'g Guard) -> (Snapshot<'g, Self>, Snapshot<'g, Self>) {
        let left = self.left.load(Ordering::Acquire, cs);
        let right = self.right.load(Ordering::Acquire, cs);
        (left, right)
    }
}

/// Each op creates a new local state and tries to update (CAS) the tree with it.
struct State<'g, K, V> {
    root_link: &'g AtomicRc<Node<K, V>>,
    curr_root: Snapshot<'g, Node<K, V>>,
}

impl<K, V> OutputHolder<V> for Snapshot<'_, Node<K, V>> {
    fn output(&self) -> &V {
        self.as_ref().map(|node| &node.value).unwrap()
    }
}

impl<'g, K, V> State<'g, K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn new(root_link: &'g AtomicRc<Node<K, V>>, curr_root: Snapshot<'g, Node<K, V>>) -> Self {
        Self {
            root_link,
            curr_root,
        }
    }

    // TODO get ref of K, V and clone here
    fn mk_node(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        guard: &Guard,
    ) -> Rc<Node<K, V>> {
        if Node::is_retired_spot(&left) || Node::is_retired_spot(&right) {
            return Node::retired_node();
        }

        let left_size = Node::node_size(&left);
        let right_size = Node::node_size(&right);

        Rc::new(Node {
            key,
            value,
            size: left_size + right_size + 1,
            left: AtomicRc::from(left.counted()),
            right: AtomicRc::from(right.counted()),
        })
    }

    /// Make a new balanced tree from cur (the root of a subtree) and newly constructed left and right subtree
    fn mk_balanced(
        &mut self,
        cur: &Snapshot<Node<K, V>>,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        if Node::is_retired_spot(cur)
            || Node::is_retired_spot(&left)
            || Node::is_retired_spot(&right)
        {
            return Node::retired_node();
        }

        let cur_ref = unsafe { cur.deref() };
        let key = cur_ref.key.clone();
        let value = cur_ref.value.clone();

        let l_size = Node::node_size(&left);
        let r_size = Node::node_size(&right);

        if r_size > 0
            && ((l_size > 0 && r_size > WEIGHT * l_size) || (l_size == 0 && r_size > WEIGHT))
        {
            self.mk_balanced_left(left, right, key, value, cs)
        } else if l_size > 0
            && ((r_size > 0 && l_size > WEIGHT * r_size) || (r_size == 0 && l_size > WEIGHT))
        {
            self.mk_balanced_right(left, right, key, value, cs)
        } else {
            self.mk_node(left, right, key, value, cs)
        }
    }

    #[inline]
    fn mk_balanced_left(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let right_ref = unsafe { right.deref() };
        let (right_left, right_right) = right_ref.load_children(cs);

        if !self.check_root()
            || Node::is_retired_spot(&right_left)
            || Node::is_retired_spot(&right_right)
        {
            return Node::retired_node();
        }

        if Node::node_size(&right_left) < Node::node_size(&right_right) {
            // single left rotation
            return self.single_left(left, right, right_left, right_right, key, value, cs);
        }

        // double left rotation
        self.double_left(left, right, right_left, right_right, key, value, cs)
    }

    #[inline]
    fn single_left(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        right_left: Snapshot<Node<K, V>>,
        right_right: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let right_ref = unsafe { right.deref() };
        let new_left = AtomicRc::from(self.mk_node(left, right_left, key, value, cs))
            .load(Ordering::Acquire, cs);

        self.mk_node(
            new_left,
            right_right,
            right_ref.key.clone(),
            right_ref.value.clone(),
            cs,
        )
    }

    #[inline]
    fn double_left(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        right_left: Snapshot<Node<K, V>>,
        right_right: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let right_ref = unsafe { right.deref() };
        let right_left_ref = unsafe { right_left.deref() };
        let (right_left_left, right_left_right) = right_left_ref.load_children(cs);

        if !self.check_root()
            || Node::is_retired_spot(&right_left_left)
            || Node::is_retired_spot(&right_left_right)
        {
            return Node::retired_node();
        }

        let new_left = AtomicRc::from(self.mk_node(left, right_left_left, key, value, cs))
            .load(Ordering::Acquire, cs);
        let new_right = AtomicRc::from(self.mk_node(
            right_left_right,
            right_right,
            right_ref.key.clone(),
            right_ref.value.clone(),
            cs,
        ))
        .load(Ordering::Acquire, cs);

        self.mk_node(
            new_left,
            new_right,
            right_left_ref.key.clone(),
            right_left_ref.value.clone(),
            cs,
        )
    }

    #[inline]
    fn mk_balanced_right(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let left_ref = unsafe { left.deref() };
        let (left_left, left_right) = left_ref.load_children(cs);

        if !self.check_root()
            || Node::is_retired_spot(&left_right)
            || Node::is_retired_spot(&left_left)
        {
            return Node::retired_node();
        }

        if Node::node_size(&left_right) < Node::node_size(&left_left) {
            // single right rotation (fig 3)
            return self.single_right(left, right, left_right, left_left, key, value, cs);
        }
        // double right rotation
        self.double_right(left, right, left_right, left_left, key, value, cs)
    }

    #[inline]
    fn single_right(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        left_right: Snapshot<Node<K, V>>,
        left_left: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let left_ref = unsafe { left.deref() };
        let new_right = AtomicRc::from(self.mk_node(left_right, right, key, value, cs))
            .load(Ordering::Acquire, cs);

        self.mk_node(
            left_left,
            new_right,
            left_ref.key.clone(),
            left_ref.value.clone(),
            cs,
        )
    }

    #[inline]
    fn double_right(
        &mut self,
        left: Snapshot<Node<K, V>>,
        right: Snapshot<Node<K, V>>,
        left_right: Snapshot<Node<K, V>>,
        left_left: Snapshot<Node<K, V>>,
        key: K,
        value: V,
        cs: &Guard,
    ) -> Rc<Node<K, V>> {
        let left_ref = unsafe { left.deref() };
        let left_right_ref = unsafe { left_right.deref() };
        let (left_right_left, left_right_right) = left_right_ref.load_children(cs);

        if !self.check_root()
            || Node::is_retired_spot(&left_right_left)
            || Node::is_retired_spot(&left_right_right)
        {
            return Node::retired_node();
        }

        let new_left = AtomicRc::from(self.mk_node(
            left_left,
            left_right_left,
            left_ref.key.clone(),
            left_ref.value.clone(),
            cs,
        ))
        .load(Ordering::Acquire, cs);
        let new_right = AtomicRc::from(self.mk_node(left_right_right, right, key, value, cs))
            .load(Ordering::Acquire, cs);

        self.mk_node(
            new_left,
            new_right,
            left_right_ref.key.clone(),
            left_right_ref.value.clone(),
            cs,
        )
    }

    #[inline]
    fn do_insert(
        &mut self,
        node: Snapshot<Node<K, V>>,
        key: &K,
        value: &V,
        cs: &Guard,
    ) -> (Rc<Node<K, V>>, bool) {
        if Node::is_retired_spot(&node) {
            return (Node::retired_node(), false);
        }

        if node.is_null() {
            return (
                self.mk_node(
                    Snapshot::null(),
                    Snapshot::null(),
                    key.clone(),
                    value.clone(),
                    cs,
                ),
                true,
            );
        }

        let node_ref = unsafe { node.deref() };
        let (left, right) = node_ref.load_children(cs);

        if !self.check_root() || Node::is_retired_spot(&left) || Node::is_retired_spot(&right) {
            return (Node::retired_node(), false);
        }

        match node_ref.key.cmp(key) {
            cmp::Ordering::Equal => (node.counted(), false),
            cmp::Ordering::Less => {
                let (new_right, inserted) = self.do_insert(right, key, value, cs);
                let new_right = AtomicRc::from(new_right).load(Ordering::Acquire, cs);
                (self.mk_balanced(&node, left, new_right, cs), inserted)
            }
            cmp::Ordering::Greater => {
                let (new_left, inserted) = self.do_insert(left, key, value, cs);
                let new_left = AtomicRc::from(new_left).load(Ordering::Acquire, cs);
                (self.mk_balanced(&node, new_left, right, cs), inserted)
            }
        }
    }

    #[inline]
    fn do_remove<'l>(
        &mut self,
        node: &Snapshot<'l, Node<K, V>>,
        key: &K,
        cs: &'l Guard,
    ) -> (Rc<Node<K, V>>, Option<Snapshot<'l, Node<K, V>>>) {
        if Node::is_retired_spot(&node) {
            return (Node::retired_node(), None);
        }

        if node.is_null() {
            return (Rc::null(), None);
        }

        let node_ref = unsafe { node.deref() };
        let (left, right) = node_ref.load_children(cs);

        if !self.check_root() || Node::is_retired_spot(&left) || Node::is_retired_spot(&right) {
            return (Node::retired_node(), None);
        }

        match node_ref.key.cmp(key) {
            cmp::Ordering::Equal => {
                if node_ref.size == 1 {
                    return (Rc::null(), Some(*node));
                }

                if !left.is_null() {
                    let (new_left, succ) = self.pull_rightmost(left, cs);
                    let new_left = AtomicRc::from(new_left).load(Ordering::Acquire, cs);
                    let succ = AtomicRc::from(succ).load(Ordering::Acquire, cs);
                    return (self.mk_balanced(&succ, new_left, right, cs), Some(*node));
                }
                let (new_right, succ) = self.pull_leftmost(right, cs);
                let new_right = AtomicRc::from(new_right).load(Ordering::Acquire, cs);
                let succ = AtomicRc::from(succ).load(Ordering::Acquire, cs);
                (self.mk_balanced(&succ, left, new_right, cs), Some(*node))
            }
            cmp::Ordering::Less => {
                let (new_right, found) = self.do_remove(&right, key, cs);
                let new_right = AtomicRc::from(new_right).load(Ordering::Acquire, cs);
                (self.mk_balanced(&node, left, new_right, cs), found)
            }
            cmp::Ordering::Greater => {
                let (new_left, found) = self.do_remove(&left, key, cs);
                let new_left = AtomicRc::from(new_left).load(Ordering::Acquire, cs);
                (self.mk_balanced(&node, new_left, right, cs), found)
            }
        }
    }

    fn pull_leftmost<'l>(
        &mut self,
        node: Snapshot<'l, Node<K, V>>,
        cs: &'l Guard,
    ) -> (Rc<Node<K, V>>, Rc<Node<K, V>>) {
        if Node::is_retired_spot(&node) {
            return (Node::retired_node(), Node::retired_node());
        }

        let node_ref = unsafe { node.deref() };
        let (left, right) = node_ref.load_children(cs);

        if !self.check_root() || Node::is_retired_spot(&left) || Node::is_retired_spot(&right) {
            return (Node::retired_node(), Node::retired_node());
        }

        if !left.is_null() {
            let (new_left, succ) = self.pull_leftmost(left, cs);
            let new_left = AtomicRc::from(new_left).load(Ordering::Acquire, cs);
            return (self.mk_balanced(&node, new_left, right, cs), succ);
        }
        // node is the leftmost
        let succ = self.mk_node(
            Snapshot::null(),
            Snapshot::null(),
            node_ref.key.clone(),
            node_ref.value.clone(),
            cs,
        );
        (right.counted(), succ)
    }

    fn pull_rightmost<'l>(
        &mut self,
        node: Snapshot<'l, Node<K, V>>,
        cs: &'l Guard,
    ) -> (Rc<Node<K, V>>, Rc<Node<K, V>>) {
        if Node::is_retired_spot(&node) {
            return (Node::retired_node(), Node::retired_node());
        }

        let node_ref = unsafe { node.deref() };
        let (left, right) = node_ref.load_children(cs);

        if !self.check_root() || Node::is_retired_spot(&left) || Node::is_retired_spot(&right) {
            return (Node::retired_node(), Node::retired_node());
        }

        if !right.is_null() {
            let (new_right, succ) = self.pull_rightmost(right, cs);
            let new_right = AtomicRc::from(new_right).load(Ordering::Acquire, cs);
            return (self.mk_balanced(&node, left, new_right, cs), succ);
        }
        // node is the rightmost
        let succ = self.mk_node(
            Snapshot::null(),
            Snapshot::null(),
            node_ref.key.clone(),
            node_ref.value.clone(),
            cs,
        );
        (left.counted(), succ)
    }

    pub fn check_root(&self) -> bool {
        let guard = circ::cs();
        self.curr_root
            .ptr_eq(self.root_link.load(Ordering::Acquire, &guard))
    }
}

pub struct BonsaiTreeMap<K, V> {
    root: AtomicRc<Node<K, V>>,
}

impl<K, V> Default for BonsaiTreeMap<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> BonsaiTreeMap<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    pub fn new() -> Self {
        Self {
            root: AtomicRc::null(),
        }
    }

    pub fn get<'l>(&self, key: &K, cs: &'l Guard) -> Option<Snapshot<'l, Node<K, V>>> {
        loop {
            let mut node = self.root.load(Ordering::Acquire, cs);
            while !node.is_null() && !Node::<K, V>::is_retired(node.tag()) {
                let node_ref = unsafe { node.deref() };
                match key.cmp(&node_ref.key) {
                    cmp::Ordering::Equal => break,
                    cmp::Ordering::Less => node = node_ref.left.load(Ordering::Acquire, cs),
                    cmp::Ordering::Greater => node = node_ref.right.load(Ordering::Acquire, cs),
                }
            }

            if Node::is_retired_spot(&node) {
                continue;
            }

            if node.is_null() {
                return None;
            }

            return Some(node);
        }
    }

    pub fn insert<'l>(&self, key: K, value: V, cs: &'l Guard) -> bool {
        loop {
            let curr_root = self.root.load(Ordering::Acquire, cs);
            let mut state = State::new(&self.root, curr_root);
            let (new_root, inserted) = state.do_insert(curr_root, &key, &value, cs);

            if Node::<K, V>::is_retired(new_root.tag()) {
                continue;
            }

            if self
                .root
                .compare_exchange(curr_root, new_root, Ordering::AcqRel, Ordering::Acquire, cs)
                .is_ok()
            {
                return inserted;
            }
        }
    }

    pub fn remove<'l>(&self, key: &K, cs: &'l Guard) -> Option<Snapshot<'l, Node<K, V>>> {
        loop {
            let curr_root = self.root.load(Ordering::Acquire, cs);
            let mut state = State::new(&self.root, curr_root);
            let (new_root, found) = state.do_remove(&curr_root, key, cs);

            if Node::<K, V>::is_retired(new_root.tag()) {
                continue;
            }

            if self
                .root
                .compare_exchange(curr_root, new_root, Ordering::AcqRel, Ordering::Acquire, cs)
                .is_ok()
            {
                return found;
            }
        }
    }
}

impl<K, V> ConcurrentMap<K, V> for BonsaiTreeMap<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    type Output<'a>
        = Snapshot<'a, Node<K, V>>
    where
        Self: 'a;

    fn new() -> Self {
        BonsaiTreeMap::new()
    }

    fn get<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>> {
        self.get(key, cs)
    }

    fn insert<'l>(&self, key: K, value: V, cs: &'l Guard) -> bool {
        self.insert(key, value, cs)
    }

    fn remove<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>> {
        self.remove(key, cs)
    }
}

#[cfg(test)]
mod tests {
    use super::BonsaiTreeMap;
    use crate::concurrent_map;

    #[test]
    #[ignore = "currently only support manual test to see memory usage in tracy"]
    fn smoke_bonsai_tree() {
        concurrent_map::tests::smoke::<BonsaiTreeMap<i32, String>>();
    }
}
