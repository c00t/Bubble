use std::{mem::MaybeUninit, ptr::NonNull};

pub use aarc::{
    increment_era, Arc, AsPtr, AtomicArc, AtomicWeak, Guard, RefCount, StrongPtr, Weak
};
