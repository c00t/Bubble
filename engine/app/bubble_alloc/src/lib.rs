use std::alloc::{GlobalAlloc, Layout};

use mimalloc_rust::*;

#[global_allocator]
static GLOBAL_ALLOC: GlobalMiMalloc = GlobalMiMalloc;

#[allow(improper_ctypes_definitions)]
#[no_mangle]
#[inline]
extern "C" fn alloc(layout: Layout) -> *mut u8 {
    unsafe { GlobalMiMalloc::alloc(&GLOBAL_ALLOC, layout) }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
#[inline]
extern "C" fn dealloc(ptr: *mut u8, _layout: Layout) {
    unsafe { GlobalMiMalloc::dealloc(&GLOBAL_ALLOC, ptr, _layout) }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
#[inline]
extern "C" fn alloc_zeroed(layout: Layout) -> *mut u8 {
    unsafe { GlobalMiMalloc::alloc_zeroed(&GLOBAL_ALLOC, layout) }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
#[inline]
extern "C" fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    unsafe { GlobalMiMalloc::realloc(&GLOBAL_ALLOC, ptr, layout, new_size) }
}
