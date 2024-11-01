use std::marker::PhantomData;

pub trait PopTrait {}

pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub struct Id<I, T: PopTrait> {
    pub id: u64,
    /// make sure that this pop object is create inside specific pop instance.
    ticket: u64,
    _phamtom: PhantomData<T>,
    /// indicate whether it's ref of owner
    _phamtom_identity: PhantomData<I>,
}

pub struct AnyId {
    pub id: u64,
}

pub struct PopStore {
    // store data
}

// type erased data return by pop api
pub struct AnyPop {}

impl AnyPop {
    pub fn downcast<T>(&self) -> Option<&T> {
        todo!()
    }
}

/// Should we use a Generic Pop<T> ?
///
/// Pop<R> -> readable
/// Pop<W> -> writable
pub enum Pop {
    /// references to a read instance
    Read(AnyPop),
    /// references to a write instance
    Write(AnyPop),
}

pub struct UndoScope {}

pub trait PopApi {
    /// You can create multiple instance of Pop, so you need to give it a name to distinguish them.
    fn name(&self) -> &'static str;
    fn read(&self, id: AnyId) -> Pop;
    /// return a new create Arc with newly create object behind.
    fn write(&self, id: AnyId) -> Pop;
    /// atomic commit a change to underlying storage.
    fn commit(&self, pop: Pop, undo_scope: Option<UndoScope>);
    fn undo_scope(&self) -> UndoScope;

    fn create_object(&self, undo_scope: Option<UndoScope>) -> AnyId;
    // object_type_from_name_hash
    /// get extension function, don't know why they create this function?
    /// Box<any>
    fn get_aspect(&self);
}

pub fn get_pop(name: &'static str) -> Box<dyn PopApi> {
    todo!()
}
