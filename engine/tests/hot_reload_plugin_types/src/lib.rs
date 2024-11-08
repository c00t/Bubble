#![feature(ptr_metadata)]
use bubble_core::api::prelude::*;

// First declare the trait
#[declare_api((0,1,0), hot_reload_plugin::test_api::HotReloadTestApi)]
pub trait HotReloadTestApi: Api {
    /// Returns a test string to verify the API is working
    fn get_test_string(&self) -> String;

    /// Increments and returns the counter value
    fn increment_counter(&self) -> usize;

    /// Gets the current counter value without incrementing
    fn get_counter(&self) -> usize;

    /// Sets a test value
    fn set_value(&self, val: usize);

    /// Gets the current test value
    fn get_value(&self) -> usize;

    /// Resets both counter and value to 0
    fn reset(&self);
}
