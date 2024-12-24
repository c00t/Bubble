use std::ops::Deref;

pub use atomic;
pub use circ;

use circ::{Rc, RcObject};

/// A common wrapper for types that doesn't contain any Rc edges, because you can't create a name for them.
pub struct NoLinkWrap<T>(pub(crate) T);

impl<T> Deref for NoLinkWrap<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl<T> RcObject for NoLinkWrap<T> {
    fn pop_edges(&mut self, out: &mut Vec<Rc<Self>>) {
        // no edges
    }
}
