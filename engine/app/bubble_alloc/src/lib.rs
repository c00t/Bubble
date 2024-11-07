#![feature(allocator_api)]
//! This module contains the global allocator for the Bubble runtime.
//!
//! It is used to allocate memory for the runtime and its plugins.
//!
//! ## Notes
//!
//! Currently, tracy memory and tracy log can't be used together. Because tracy-client only accept 1 connection.
//! It may be fixed in the future by automatically compile tracy-client-sys into a dynamic library.
//! 
//! You should set `TRACY_CLIENT_LIB` and `TRACY_CLIENT_LIB_PATH` to use link TracyClient danamically.
//! It'll be maintained by Buck2.

#[cfg(feature = "mimalloc")]
pub mod mimalloc {
    use mimalloc_rust::*;
    use std::alloc::{GlobalAlloc, Layout};
    
    #[cfg(feature = "tracy_memory")]
    use tracy_client::ProfiledAllocator;

    #[cfg(not(feature = "tracy_memory"))]
    #[global_allocator]
    static GLOBAL_ALLOC: GlobalMiMalloc = GlobalMiMalloc;

    #[cfg(feature = "tracy_memory")]
    #[global_allocator]
    static GLOBAL_ALLOC: ProfiledAllocator<GlobalMiMalloc> = ProfiledAllocator::new(GlobalMiMalloc,100);

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc(layout: Layout) -> *mut u8 {
        unsafe { GLOBAL_ALLOC.alloc(layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn dealloc(ptr: *mut u8, _layout: Layout) {
        unsafe { GLOBAL_ALLOC.dealloc(ptr, _layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc_zeroed(layout: Layout) -> *mut u8 {
        unsafe { GLOBAL_ALLOC.alloc_zeroed(layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { GLOBAL_ALLOC.realloc(ptr, layout, new_size) }
    }
}

#[cfg(feature = "system")]
pub mod system {
    use std::alloc::{Global, GlobalAlloc, Layout};

    #[cfg(feature = "tracy_memory")]
    use tracy_client::ProfiledAllocator;

    #[cfg(feature = "tracy_memory")]
    #[global_allocator]
    static GLOBAL_ALLOC: ProfiledAllocator<Global> = ProfiledAllocator::new(Global,100);

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc(layout: Layout) -> *mut u8 {
        #[cfg(feature = "tracy_memory")]
        unsafe { GLOBAL_ALLOC.alloc(layout) }
        #[cfg(not(feature = "tracy_memory"))]
        unsafe { std::alloc::alloc(layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn dealloc(ptr: *mut u8, _layout: Layout) {
        #[cfg(feature = "tracy_memory")]
        unsafe { GLOBAL_ALLOC.dealloc(ptr, _layout) }
        #[cfg(not(feature = "tracy_memory"))]
        unsafe { std::alloc::dealloc(ptr, _layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn alloc_zeroed(layout: Layout) -> *mut u8 {
        #[cfg(feature = "tracy_memory")]
        unsafe { GLOBAL_ALLOC.alloc_zeroed(layout) }
        #[cfg(not(feature = "tracy_memory"))]
        unsafe { std::alloc::alloc_zeroed(layout) }
    }

    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    #[inline]
    extern "C" fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        #[cfg(feature = "tracy_memory")]
        unsafe { GLOBAL_ALLOC.realloc(ptr, layout, new_size) }
        #[cfg(not(feature = "tracy_memory"))]
        unsafe { std::alloc::realloc(ptr, layout, new_size) }
    }
}
