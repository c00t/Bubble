use std::hash::BuildHasher;

use super::{BasinBufferInterface, Buffer, BufferId, BufferStorage};
use __fixed_type_id::{fstr_to_str, ConstTypeName};
use bubble_core::api::prelude::*;
use circ::Rc;
use quick_cache::{Lifecycle, Weighter};

impl<We, B, L> TraitcastableTo<dyn Interface> for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    const METADATA: std::ptr::DynMetadata<dyn Interface> = {
        let ptr: *const BufferStorage<We, B, L> =
            ::core::ptr::from_raw_parts(::core::ptr::null::<BufferStorage<We, B, L>>(), ());
        let ptr: *const dyn Interface = ptr as _;

        ptr.to_raw_parts().1
    };
}

impl<We, B, L> TraitcastableTo<dyn BasinBufferInterface> for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    const METADATA: std::ptr::DynMetadata<dyn BasinBufferInterface> = {
        let ptr: *const BufferStorage<We, B, L> =
            ::core::ptr::from_raw_parts(::core::ptr::null::<BufferStorage<We, B, L>>(), ());
        let ptr: *const dyn BasinBufferInterface = ptr as _;

        ptr.to_raw_parts().1
    };
}

impl<We, B, L> BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    const TARGETS: &[TraitcastTarget] = &[
        TraitcastTarget::from::<Self, dyn Interface>(),
        TraitcastTarget::from::<Self, dyn BasinBufferInterface>(),
    ];
}

unsafe impl<We, B, L> TraitcastableAny for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    fn traitcast_targets(&self) -> &[TraitcastTarget] {
        Self::TARGETS
    }

    fn type_id(&self) -> FixedId {
        Self::TYPE_ID
    }
}

impl<We, B, L> FixedTypeId for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    const TYPE_NAME: &'static str = fstr_to_str(&Self::TYPE_NAME_FSTR);
}

impl<We, B, L> ConstTypeName for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    const RAW_SLICE: &[&str] = &[
        "bubble_basin::buffer::BufferStorage<",
        We::TYPE_NAME,
        ", ",
        B::TYPE_NAME,
        ", ",
        L::TYPE_NAME,
        ">",
    ];
}
