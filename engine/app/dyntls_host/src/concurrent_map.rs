use circ::Guard;

pub trait OutputHolder<V> {
    fn output(&self) -> &V;
}

pub trait ConcurrentMap<K, V> {
    type Output<'a>: OutputHolder<V>
    where
        Self: 'a;

    fn new() -> Self;
    fn get<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>>;
    fn insert<'l>(&self, key: K, value: V, cs: &'l Guard) -> bool;
    fn remove<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>>;
}
