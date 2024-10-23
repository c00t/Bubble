//! The application api used for engine which will be used in main executable, which only run on the main threads,
//!
//! Some Api methods should only be called on the main thread.
#![feature(ptr_metadata)]
use bon::bon;
use bubble_core::api::{prelude::*, Api};

#[define_api(ApplicationApi)]
struct Application {}

#[bon]
impl Application {
    #[builder]
    pub fn new() -> ApiHandle<dyn ApplicationApi> {
        todo!()
    }
}

impl ApplicationApi for Application {
    fn tick(&self) -> async_ffi::FfiFuture<bool> {
        todo!()
    }
}

#[declare_api((0,1,0), application::ApplicationApi)]
pub trait ApplicationApi: Api {
    // a tick future which should be run per frame
    fn tick(&self) -> async_ffi::FfiFuture<bool>;
}
