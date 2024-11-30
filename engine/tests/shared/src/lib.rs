#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]

use bubble_core::{api::prelude::*, thread_local};

pub trait TraitCastableDropSuper {
    fn super_func(&self);
}

pub trait TraitCastableDropSub {
    fn sub_func(&self);
}

#[make_trait_castable(TraitCastableDropSuper, TraitCastableDropSub)]
pub struct TraitCastableDrop {
    pub value: i32,
}

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    dyn TraitCastableDropSuper;
    dyn TraitCastableDropSub;
}

impl TraitCastableDropSuper for TraitCastableDrop {
    fn super_func(&self) {
        println!("super_func: {}", self.value);
    }
}

impl TraitCastableDropSub for TraitCastableDrop {
    fn sub_func(&self) {
        println!("sub_func: {}", self.value);
    }
}

impl Drop for TraitCastableDrop {
    fn drop(&mut self) {
        println!("TraitCastableDrop: {}", self.value);
    }
}

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

pub type StringAlias0 = std::string::String;
pub type StringAlias1 = String;
