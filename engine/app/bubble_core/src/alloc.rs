//! Custom memory allocator for Bubble applications
//!
//! This module provides a custom memory allocator implementation that is used by all binary crates
//! in the Bubble project. It exports a dynamic library-based allocator through the `dyalloc` submodule
//! which implements the standard [`GlobalAlloc`] trait.
//!
//! The allocator is loaded dynamically at runtime from a platform-specific shared library:
//! - Windows: `bubble_alloc.dll`
//! - Other platforms: `libbubble_alloc.so`/`libbubble_alloc.dylib`
//!
//! When the `custom_alloc_lib` feature is enabled, the library path can be customized via
//! the `BUBBLE_ALLOC_LIB` environment variable.

pub(crate) mod dyalloc {
    use std::alloc::{GlobalAlloc, Layout};
    #[allow(unused_imports)]
    use std::compile_error;

    #[allow(improper_ctypes)]
    #[cfg_attr(
        all(target_os = "windows", not(feature = "custom_alloc_lib")),
        link(name = "bubble_alloc.dll", kind = "dylib")
    )]
    #[cfg_attr(
        all(not(target_os = "windows"), not(feature = "custom_alloc_lib")),
        link(name = "bubble_alloc", kind = "dylib")
    )]
    #[cfg_attr(
        feature = "custom_alloc_lib",
        link(name = env!("BUBBLE_ALLOC_LIB"), kind = "dylib")
    )]
    unsafe extern "C" {
        pub fn alloc(layout: Layout) -> *mut u8;
        pub fn dealloc(ptr: *mut u8, _layout: Layout);
        pub fn alloc_zeroed(layout: Layout) -> *mut u8;
        pub fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8;
    }

    #[global_allocator]
    static ALLOC: Alloc = Alloc;

    struct Alloc;

    unsafe impl GlobalAlloc for Alloc {
        #[inline]
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            alloc(layout)
        }
        #[inline]
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            alloc_zeroed(layout)
        }
        #[inline]
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            realloc(ptr, layout, new_size)
        }
        #[inline]
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            dealloc(ptr, layout);
        }
    }
}
