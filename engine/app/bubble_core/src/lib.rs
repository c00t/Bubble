pub mod alloc;
pub mod api;
pub mod dll;
pub mod os;
pub mod plugin;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}
