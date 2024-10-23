use bon::bon;

use super::prelude::*;

#[define_interface(TestInterface)]
#[derive(Debug)]
pub struct TestInterfaceStruct {
    val: i32,
}

#[bon]
impl TestInterfaceStruct {
    #[builder]
    pub fn new() -> InterfaceHandle<DynTestInterface> {
        let handle: AnyInterfaceHandle = Box::new(TestInterfaceStruct { val: 0 }).into();
        handle.downcast()
    }
}

#[declare_interface((0,1,0), bubble_core::api::interfaces::TestInterface)]
pub trait TestInterface: Interface {
    fn test1(&self) -> i32;
    fn test2(&self) -> i32;
}

impl TestInterface for TestInterfaceStruct {
    fn test1(&self) -> i32 {
        todo!()
    }

    fn test2(&self) -> i32 {
        todo!()
    }
}
