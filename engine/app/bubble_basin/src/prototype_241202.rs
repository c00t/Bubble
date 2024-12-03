//! In this file, I'm prototyping how to derive `BasinType`(not designed yet) for types
//! that will be stored in Basin.
//!
//! [ ] BasinType should be a dyn-compatible trait? or just a marker trait?
//!

use std::sync::atomic::AtomicU64;

/// The Basin Store will store different type of BasinType generated objects.
///
/// It's a thread-safe data storage.
struct Basin {
    // A internal data storage.
    storage: sharded_slab::Slab<BasinObject>,
    /// A buffer api, will be used to store buffer data. The buffer will be loaded into memory dynamically
    /// with dynamic eviction. When you do want to keep the buffer in memory, give it a zero weight.
    buffer_api: (),
    types: circ_ds::natarajan_mittal_tree::NMTreeMap<
        bubble_core::api::FixedId,
        mirror_mirror::TypeDescriptor,
    >,
    uuid_to_basin_id: circ_ds::natarajan_mittal_tree::NMTreeMap<uuid::Uuid, BasinId>,
}

/// Store the id returned by [`sharded_slab::Slab`]
///
/// It's present a plain id data, can't express the ownership of underlying basin object.
pub enum BasinId {
    /// A runtime id, will be used for a object that is created in runtime.
    Runtime(usize),
    /// A persistent id, will be used for a object that will be saved to disk.
    Persistent(uuid::Uuid),
}

pub struct TypeBasinId<T> {
    id: BasinId,
    _marker: std::marker::PhantomData<T>,
}

pub struct BasinRefId(BasinId);
pub struct TypedBasinRefId<T> {
    id: BasinRefId,
    _marker: std::marker::PhantomData<T>,
}

pub struct BasinOwnedId(BasinId);
pub struct TypedBasinOwnedId<T> {
    id: BasinOwnedId,
    _marker: std::marker::PhantomData<T>,
}

#[repr(transparent)]
pub struct BasinObject(bubble_core::sync::circ::AtomicRc<ObjectInner>);

/// TODO: Some inner fields may can be move out to [`BasinObject`].
struct ObjectInner {
    // store a ref id to the storage in itself,
    id: BasinId,
    data: Box<()>,
    version: AtomicU64,
    // a object can use another object as prototype, and override some of its properties.
    // the override is a u64 bitflag, so a object can have up to 64 properties to override.
    override_flags: AtomicU64,
    // a reference to the prototype object, if it has one.
    prototype: Option<BasinId>,
    owner: Option<BasinId>,
    uuid: Option<uuid::Uuid>,
    // type of this object, will be checked when a typed reference created to ref `data`.
    type_id: bubble_core::api::FixedId,
}

unsafe impl bubble_core::sync::circ::RcObject for ObjectInner {
    fn pop_edges(&mut self, out: &mut Vec<bubble_core::sync::circ::Rc<Self>>) {}
}

// If the user id defining a type a relation between two balls,
// which has a position, a velocity, a force, a mass, a pattern(with a star count, a color property)
// a ref to mesh...
//
// It's in a world.entity that will be stored in Basin and be serialized to disk when the engine close.
// And this type define by user can also instantiate a Ball in runtime.
//
// And a instance can also be used as a prototype for other instances, the data stored inside basin should take
// this into consideration.

/// User will define the type Ball as following:
/// It's used to give a overall type definition, will all versions of that type.
///
/// It won't be directly stored in Basin.
///
/// TODO: Will this type be stored in source code?
// #[derive(BasinType)]
pub struct Ball {
    pub position: glam::Vec3,
    pub velocity: glam::Vec3,
    pub force: glam::Vec3,
    pub mass: f32,
    pub pattern: BallPattern,
    // #[basin_as(set)]
    pub sub_objects: Vec<SubObjectOfABall>,
    // #[basin_as(buffer)]
    pub mesh: RawMesh,
}

/// It will be stored in Basin as BasinBall.
///
/// It's a separate struct from [`Ball`].
///
/// It's the meta data of a [`Ball`], without large blob data like [`RawMesh`].
///
/// It's the type that will implement serde::Serialize/Deserialize to store as human readable data in disk.
///
/// Because that they are small, it's efficient to serialize/deserialize them without zero-copy.
pub struct BasinBall {
    pub position: Property<glam::Vec3>,
    pub velocity: Property<glam::Vec3>,
    pub force: Property<glam::Vec3>,
    pub mass: Property<f32>,
    pub pattern: Property<BallPattern>,
    pub sub_objects: BasinSetProperty<SubObjectOfABall>,
    pub mesh: BufferProperty<RawMesh>,
}

/// A look through property, it will just store the value of that property.
///
/// So it's used to store small data, or data that you don't want to store in basin storage.
///
/// like a bool, a u32, a f32, a Vec3, a Mat4, a Color...
pub enum Property<T> {
    Prototype,
    Overriden(T),
}

pub enum OwnedBasinProperty<T> {
    Prototype,
    Instance(TypedBasinOwnedId<T>),
}

pub enum RefBasinProperty<T> {
    Prototype,
    Overriden(TypedBasinRefId<T>),
}

/// it's typed because that [`mirror-mirror`] will use that info to generate the type descriptor.
///
/// It's a buffer property, will be stored in basin storage.
///
/// TODO: Do we need a hash of this property?
pub enum BufferProperty<T: rkyv::Archive> {
    Prototype,
    Overriden(BufferId<T>),
}

pub enum BasinSetProperty<T> {
    Prototype,
    Overriden(OverridenSet<T>),
}

pub enum OverridenSet<T> {
    Refs {
        added: std::collections::HashSet<TypedBasinRefId<T>>,
        removed: std::collections::HashSet<TypedBasinRefId<T>>,
        instanced: std::collections::HashSet<TypedBasinRefId<T>>,
    },
    Owned {
        added: std::collections::HashSet<TypedBasinOwnedId<T>>,
        // to remove from the set, we only have the ref id.
        removed: std::collections::HashSet<TypedBasinRefId<T>>,
        instanced: std::collections::HashSet<TypedBasinOwnedId<T>>,
    },
}

pub enum OrderedArrayProperty<T> {
    Prototype,
    Overriden(OverridenOrderedArray<T>),
}

pub enum OverridenOrderedArray<T> {
    Refs {
        added: std::collections::HashSet<TypedBasinRefId<T>>,
        removed: std::collections::HashSet<TypedBasinRefId<T>>,
        instanced: std::collections::HashSet<TypedBasinRefId<T>>,
    },
    Owned {
        added: std::collections::HashSet<TypedBasinOwnedId<T>>,
        removed: std::collections::HashSet<TypedBasinOwnedId<T>>,
        instanced: std::collections::HashSet<TypedBasinOwnedId<T>>,
    },
}

pub enum UnorderedArrayProperty<T> {
    Prototype,
    Overriden(OverridenUnorderedArray<T>),
}

pub enum OverridenUnorderedArray<T> {
    Refs {
        added: std::collections::HashSet<TypedBasinRefId<T>>,
        removed: std::collections::HashSet<TypedBasinRefId<T>>,
        instanced: std::collections::HashSet<TypedBasinRefId<T>>,
    },
    Owned {
        added: std::collections::HashSet<TypedBasinOwnedId<T>>,
        removed: std::collections::HashSet<TypedBasinOwnedId<T>>,
        instanced: std::collections::HashSet<TypedBasinOwnedId<T>>,
    },
}

/// A buffer id, it's typed because that [`mirror-mirror`] will use that info to generate the type descriptor.
///
/// This type should implement [`rkyv::Archive`], so it can be serialized to/from disk fast without copy.
pub struct BufferId<T: rkyv::Archive> {
    id: usize,
    _marker: std::marker::PhantomData<T>,
}

pub struct BallPattern {
    pub star_count: u32,
    pub color_rgba: glam::U8Vec4,
}

pub struct SubObjectOfABall {
    pub mass: f32,
}

use rkyv::{Archive, Deserialize, Serialize};
/// A raw mesh, which should be stored in disk as a binary buffer.
///
/// It will be serialized to/from disk using [`rkyv`].
///
/// So it should be marked as a `Buffer`.
#[derive(Archive, Deserialize, Serialize)]
pub struct RawMesh {
    pub vertices: Vec<glam::Vec3>,
    pub indices: Vec<u32>,
    pub normals: Vec<glam::Vec3>,
    pub uvs: Vec<glam::Vec2>,
}
