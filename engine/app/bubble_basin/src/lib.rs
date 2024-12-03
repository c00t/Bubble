#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]
//! Basin api
//!
//! Basin is an api used to transfer data between plugins. When you want to communicate with
//! other plugins, you should use basin. It's implemented using lock-free algorithms, so it's
//! thread-safe and efficient as far as possible. Since Basin Api has added many features,
//! **storing data in Basin will be much slower than in raw memory**. For systems requiring
//! high performance, **they will have their own high-speed data representation methods and
//! only use Basin for data exchange with other systems**.
//!

pub mod buffer;

pub mod prototype;

pub mod prototype_241202;

mod tt {
    use std::arch::x86_64::__m128;

    use rkyv::{Archive, Deserialize, Serialize};

    // #[derive(Archive, Deserialize, Serialize)]
    // #[repr(C)]
    // pub struct Vec4 {
    //     x: f32,
    //     y: f32,
    //     z: f32,
    //     w: f32,
    // }

    /// Archive type of it is ArchivedXVec4([f32::Archived;N])
    /// which is ArchivedXVec4([f32_le;N]) for little-endian
    #[derive(Archive, Deserialize, Serialize)]
    #[repr(C)]
    pub struct XVec4([f32; 4]);

    // #[derive(Archive, Deserialize, Serialize)]
    // #[repr(C)]
    // pub struct MVec4(

    //     __m128
    // );
}
