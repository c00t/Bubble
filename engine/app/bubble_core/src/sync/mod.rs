use std::{mem::MaybeUninit, ptr::NonNull};

pub use aarc::{Arc, AsPtr, AtomicArc, AtomicWeak, Guard, RefCount, StrongPtr, Weak};

pub(crate) struct ArcAtomicArcErased {
    ptr: NonNull<MaybeUninit<()>>,
}

impl ArcAtomicArcErased {
    /// Convert from `Arc<AtomicArc<T>>` to ArcAtomicArcErased.
    pub unsafe fn from_arc<T>(arc: Arc<AtomicArc<T>>) -> Self {
        std::mem::transmute(arc)
    }

    pub unsafe fn into_arc<T>(this: Self) -> Arc<AtomicArc<T>> {
        std::mem::transmute(this)
    }

    pub unsafe fn ref_into_arc<T>(this: &Self) -> Arc<AtomicArc<T>> {
        // read mem at this ref
        let len = size_of::<Self>();
        let src = this as *const Self as *const u8;
        let mut uninit = MaybeUninit::<Arc<AtomicArc<T>>>::uninit();
        let target = uninit.as_mut_ptr() as *mut u8;
        std::ptr::copy_nonoverlapping(src, target, len);
        uninit.assume_init().clone()
    }
}
