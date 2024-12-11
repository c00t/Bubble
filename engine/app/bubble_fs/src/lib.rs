pub mod fs_ext;
pub mod path_ext;
/// Reexport a drop-in replacement for [`std::fs`] that return [`std::io::Result`].
pub use fs_err as fs;
