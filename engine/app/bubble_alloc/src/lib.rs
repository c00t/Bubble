#![feature(allocator_api)]

#[cfg(feature = "mimalloc")]
pub mod mimalloc {
    use mimalloc_rust::*;
    use std::alloc::{GlobalAlloc, Layout };

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
}

#[cfg(feature = "system")]
pub mod system {
    use std::alloc::{GlobalAlloc, Layout, Global };
    
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc(layout: Layout) -> *mut u8 {
        unsafe { std::alloc::alloc(layout) }
    }
    
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn dealloc(ptr: *mut u8, _layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, _layout) }
    }
    
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc_zeroed(layout: Layout) -> *mut u8 {
        unsafe { std::alloc::alloc_zeroed(layout) }
    }
    
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { std::alloc::realloc(ptr, layout, new_size) }
    }    
}
