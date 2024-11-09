use crate::bon::bon;

use super::prelude::*;

#[define_interface(TestInterface)]
#[derive(Debug)]
struct TestInterfaceStruct1 {
    #[allow(dead_code)]
    val: i32,
}

#[bon]
impl TestInterfaceStruct1 {
    #[builder]
    pub fn new() -> InterfaceHandle<DynTestInterface> {
        let handle: AnyInterfaceHandle = Box::new(TestInterfaceStruct1 { val: 0 }).into();
        handle.downcast()
    }
}

#[define_interface(TestInterface)]
#[derive(Debug)]
struct TestInterfaceStruct2 {
    #[allow(dead_code)]
    val: i32,
}

#[bon]
impl TestInterfaceStruct2 {
    #[builder]
    pub fn new() -> InterfaceHandle<DynTestInterface> {
        let handle: AnyInterfaceHandle = Box::new(TestInterfaceStruct2 { val: 0 }).into();
        handle.downcast()
    }
}

#[declare_interface((0,1,0), bubble_core::api::interfaces::TestInterface)]
pub trait TestInterface: Interface {
    fn test1(&self) -> i32;
    fn test2(&self) -> i32;
}

impl TestInterface for TestInterfaceStruct1 {
    fn test1(&self) -> i32 {
        todo!()
    }

    fn test2(&self) -> i32 {
        todo!()
    }
}

impl TestInterface for TestInterfaceStruct2 {
    fn test1(&self) -> i32 {
        todo!()
    }

    fn test2(&self) -> i32 {
        todo!()
    }
}
