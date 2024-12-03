#![feature(prelude_import)]
//! Basin api
//!
//! Basin is an api used to transfer data between plugins. When you want to communicate with
//! other plugins, you should use basin. It's implemented using lock-free algorithms, so it's
//! thread-safe and efficient as far as possible. Since Basin Api has added many features,
//! **storing data in Basin will be much slower than in raw memory**. For systems requiring
//! high performance, **they will have their own high-speed data representation methods and
//! only use Basin for data exchange with other systems**.
//!
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;

// pub mod prototype;

// pub mod prototype_241202;

mod tt {
    use std::arch::x86_64::__m128;

    use rkyv::{Archive, Deserialize, Serialize};

    // #[derive(Archive, Deserialize, Serialize)]
    // #[repr(C)]
    // pub struct Vec4 {
    //     x: f32,
    //     y: f32,
    //     z: f32,
    //     w: f32,
    // }

    #[repr(C)]
    pub struct XVec4([f32; 4]);
    #[automatically_derived]
    ///An archived [`XVec4`]
    #[bytecheck(crate = :: rkyv :: bytecheck)]
    #[repr(C)]
    pub struct ArchivedXVec4(<[f32; 4] as ::rkyv::Archive>::Archived)
    where
        [f32; 4]: ::rkyv::Archive;
    #[automatically_derived]
    unsafe impl<__C: ::rkyv::bytecheck::rancor::Fallible + ?::core::marker::Sized>
        ::rkyv::bytecheck::CheckBytes<__C> for ArchivedXVec4
    where
        [f32; 4]: ::rkyv::Archive,
        <__C as ::rkyv::bytecheck::rancor::Fallible>::Error: ::rkyv::bytecheck::rancor::Trace,
        <[f32; 4] as ::rkyv::Archive>::Archived: ::rkyv::bytecheck::CheckBytes<__C>,
    {
        unsafe fn check_bytes(
            value: *const Self,
            context: &mut __C,
        ) -> ::core::result::Result<(), <__C as ::rkyv::bytecheck::rancor::Fallible>::Error>
        {
            <<[f32; 4] as ::rkyv::Archive>::Archived as
                            ::rkyv::bytecheck::CheckBytes<__C>>::check_bytes(

                        // #[derive(Archive, Deserialize, Serialize)]
                        // #[repr(C)]
                        // pub struct MVec4(

                        //     __m128
                        // );
                        &raw const (*value).0,
                        context).map_err(|e|
                        {
                            <<__C as ::rkyv::bytecheck::rancor::Fallible>::Error as
                                    ::rkyv::bytecheck::rancor::Trace>::trace(e,
                                ::rkyv::bytecheck::TupleStructCheckContext {
                                    tuple_struct_name: "ArchivedXVec4",
                                    field_index: 0usize,
                                })
                        })?;
            ::core::result::Result::Ok(())
        }
    }
    #[automatically_derived]
    ///The resolver for an archived [`XVec4`]
    pub struct XVec4Resolver(<[f32; 4] as ::rkyv::Archive>::Resolver)
    where
        [f32; 4]: ::rkyv::Archive;
    impl ::rkyv::Archive for XVec4
    where
        [f32; 4]: ::rkyv::Archive,
    {
        type Archived = ArchivedXVec4;
        type Resolver = XVec4Resolver;
        const COPY_OPTIMIZATION: ::rkyv::traits::CopyOptimization<Self> = unsafe {
            ::rkyv::traits::CopyOptimization::enable_if(
                0 + ::core::mem::size_of::<[f32; 4]>() == ::core::mem::size_of::<XVec4>()
                    && <[f32; 4] as ::rkyv::Archive>::COPY_OPTIMIZATION.is_enabled()
                    && {
                        builtin # offset_of(XVec4, 0)
                    } == {
                        builtin # offset_of(ArchivedXVec4, 0)
                    },
            )
        };
        #[allow(clippy::unit_arg)]
        fn resolve(&self, resolver: Self::Resolver, out: ::rkyv::Place<Self::Archived>) {
            let field_ptr = unsafe { &raw mut (*out.ptr()).0 };
            let field_out = unsafe { ::rkyv::Place::from_field_unchecked(out, field_ptr) };
            <[f32; 4] as ::rkyv::Archive>::resolve(&self.0, resolver.0, field_out);
        }
    }
    unsafe impl ::rkyv::traits::Portable for ArchivedXVec4
    where
        [f32; 4]: ::rkyv::Archive,
        <[f32; 4] as ::rkyv::Archive>::Archived: ::rkyv::traits::Portable,
    {
    }
    #[automatically_derived]
    impl<__D: ::rkyv::rancor::Fallible + ?Sized> ::rkyv::Deserialize<XVec4, __D>
        for ::rkyv::Archived<XVec4>
    where
        [f32; 4]: ::rkyv::Archive,
        <[f32; 4] as ::rkyv::Archive>::Archived: ::rkyv::Deserialize<[f32; 4], __D>,
    {
        fn deserialize(
            &self,
            deserializer: &mut __D,
        ) -> ::core::result::Result<XVec4, <__D as ::rkyv::rancor::Fallible>::Error> {
            let __this = self;
            ::core::result::Result::Ok(
                XVec4(
                    <<[f32; 4] as ::rkyv::Archive>::Archived as ::rkyv::Deserialize<
                        [f32; 4],
                        __D,
                    >>::deserialize(&__this.0, deserializer)?,
                ),
            )
        }
    }
    #[automatically_derived]
    impl<__S: ::rkyv::rancor::Fallible + ?Sized> ::rkyv::Serialize<__S> for XVec4
    where
        [f32; 4]: ::rkyv::Serialize<__S>,
    {
        fn serialize(
            &self,
            serializer: &mut __S,
        ) -> ::core::result::Result<
            <Self as ::rkyv::Archive>::Resolver,
            <__S as ::rkyv::rancor::Fallible>::Error,
        > {
            let __this = self;
            ::core::result::Result::Ok(XVec4Resolver(
                <[f32; 4] as ::rkyv::Serialize<__S>>::serialize(&__this.0, serializer)?,
            ))
        }
    }
}
