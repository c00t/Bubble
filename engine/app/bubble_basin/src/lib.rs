//! Basin api
//!
//! Basin is an api used to transfer data between plugins. When you want to communicate with
//! other plugins, you should use basin. It's implemented using lock-free algorithms, so it's
//! thread-safe and efficient as far as possible. Since Basin Api has added many features,
//! **storing data in Basin will be much slower than in raw memory**. For systems requiring
//! high performance, **they will have their own high-speed data representation methods and
//! only use Basin for data exchange with other systems**.
//!

pub mod prototype;
