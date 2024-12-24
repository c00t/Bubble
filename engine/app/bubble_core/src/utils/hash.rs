//! BuildHashers for specific types of elements or situations.
//!
//! Note to use the std lib hasher when hashing is not in the critical path, or if you need strong
//! HashDoS resistance.
//!
//! All hashers in this module implement [`fixed_type_id::FixedTypeId`].
//!

use crate::api::prelude::*;
/// Re-export of [`rapidhash`] for convenience.
pub use rapidhash::{self, RapidHasher, RapidInlineHasher};

use std::hash::{BuildHasher, Hasher};

/// A [`BuildHasher`] wrapper for [`rapidhash::RapidInlineBuildHasher`].
///
/// It's a wrapper which implements [`fixed_type_id::FixedTypeId`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RapidInlineBuildHasher(rapidhash::RapidInlineBuildHasher);

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    bubble_core::utils::RapidInlineBuildHasher
}

impl BuildHasher for RapidInlineBuildHasher {
    type Hasher = <rapidhash::RapidInlineBuildHasher as BuildHasher>::Hasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        self.0.build_hasher()
    }
}

/// A [`BuildHasher`] wrapper for [`rapidhash::RapidBuildHasher`].
///
/// It's a wrapper which implements [`fixed_type_id::FixedTypeId`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RapidBuildHasher(rapidhash::RapidBuildHasher);

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    bubble_core::utils::RapidBuildHasher
}

impl BuildHasher for RapidBuildHasher {
    type Hasher = <rapidhash::RapidBuildHasher as BuildHasher>::Hasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        self.0.build_hasher()
    }
}

/// A [`BuildHasher`] for types which [`std::hash::Hash`] implementation is just like
/// [`std::hash::Hasher::write_u64`] **once**.
///
/// This `u64` should distribute your type's key space across the whole `u64`'s range,
/// which typically means it's already a high-quality hash, like [`fixed_type_id::FixedId`].
/// They can definitely be hashed again by any [`Hasher`], but it's wasteful.
///
/// Note that you likely **should not** use a raw u8, u16 or u32 cast to a u64, or even an
/// incrementing u64 counter and write it. They may result in poor performance in some hash-based
/// data structures.
///
/// From [`https://docs.rs/bevy_utils/0.15.0/src/bevy_utils/lib.rs.html#299`]
#[derive(Clone, Default, Debug)]
pub struct PassHash;

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    bubble_core::utils::PassHash
}

impl BuildHasher for PassHash {
    type Hasher = PassHasher;

    fn build_hasher(&self) -> Self::Hasher {
        PassHasher(0)
    }
}

/// A [`Hasher`] from [`PassHash`].
///
/// From [`https://docs.rs/bevy_utils/0.15.0/src/bevy_utils/lib.rs.html#310`]
#[doc(hidden)]
pub struct PassHasher(u64);

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    bubble_core::utils::PassHasher
}

impl Hasher for PassHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _bytes: &[u8]) {
        panic!("PassHasher's users should use `write_u64` to hash themselves.");
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
}

/// A [`BuildHasher`] that builds a hasher that extremly fast when hash integers, or
/// struct of only integers, it may have lower quality compare to other alternatives.
pub struct PrimitiveBuildHasher;

fixed_type_id! {
    #[FixedTypeIdVersion((0,1,0))]
    bubble_core::utils::PrimitiveBuildHasher
}

impl BuildHasher for PrimitiveBuildHasher {
    type Hasher = rustc_hash::FxHasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        rustc_hash::FxHasher::default()
    }
}
