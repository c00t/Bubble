use crate::api::prelude::*;
use crate::bon::bon;
use circ_ds::{
    concurrent_map::ConcurrentMap,
    natarajan_mittal_tree::{IterOrder, NMTreeMap},
};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeatureFlag(FixedId);

impl FeatureFlag {
    pub fn new<T: FixedTypeId>() -> Self {
        FeatureFlag(T::TYPE_ID)
    }
}

/// Feature flags api used as an runtime feature flags system
///
/// When your plugin want to test some features dynamicly without use compile-time cargo features,
/// you can use it to enable or disable a feature.
#[declare_api((0,1,0), bubble_core::utils::feature_flags_api::FeatureFlagsApi)]
pub trait FeatureFlagsApi: Api {
    /// The [`FeatureFlag`] enabled or not?
    fn is_enabled(&self, flag: FeatureFlag) -> bool;
    /// Enable a [`FeatureFlag`]
    fn set_enabled(&self, flag: FeatureFlag, enabled: bool);
    /// Return a list of all enabled feature flags.
    fn all_enabled(&self) -> Vec<FeatureFlag>;
}

#[define_api(FeatureFlagsApi)]
struct FeatureFlagsRegistry {
    /// Temporary use NMTreeMap instead of concurrent hash map.
    flags: NMTreeMap<FeatureFlag, ()>,
}

#[bon]
impl FeatureFlagsRegistry {
    #[builder]
    pub fn new() -> ApiHandle<dyn FeatureFlagsApi> {
        let feature_flags_api: AnyApiHandle = Box::new(Self {
            flags: NMTreeMap::new(),
        })
        .into();
        feature_flags_api.downcast()
    }
}

impl FeatureFlagsApi for FeatureFlagsRegistry {
    fn is_enabled(&self, flag: FeatureFlag) -> bool {
        let cs = circ::cs();
        self.flags.get(&flag, &cs).is_some()
    }

    fn set_enabled(&self, flag: FeatureFlag, enabled: bool) {
        let cs = circ::cs();
        if enabled {
            self.flags.insert(flag, (), &cs);
        } else {
            self.flags.remove(&flag, &cs);
        }
    }

    fn all_enabled(&self) -> Vec<FeatureFlag> {
        let cs = circ::cs();
        self.flags
            .iter(IterOrder::Ascending, &cs)
            .map(|f| f.0.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let context = dyntls_host::get();
        unsafe { context.initialize() };
        let feature_flags_api = FeatureFlagsRegistry::builder().build();
        let guard = circ::cs();
        let local_handler = feature_flags_api.get(&guard).unwrap();

        struct FeatureFlag1;
        struct FeatureFlag2;

        fixed_type_id! {
            FeatureFlag1;
            FeatureFlag2;
        }

        let flag1 = FeatureFlag::new::<FeatureFlag1>();
        let flag2 = FeatureFlag::new::<FeatureFlag2>();

        assert!(!local_handler.is_enabled(flag1));
        assert!(!local_handler.is_enabled(flag2));

        local_handler.set_enabled(flag1, true);
        assert!(local_handler.is_enabled(flag1));
        assert!(!local_handler.is_enabled(flag2));

        local_handler.set_enabled(flag2, true);
        assert!(local_handler.is_enabled(flag1));
        assert!(local_handler.is_enabled(flag2));

        local_handler.set_enabled(flag1, false);
        assert!(!local_handler.is_enabled(flag1));
        assert!(local_handler.is_enabled(flag2));

        let all_enabled = local_handler.all_enabled();
        assert_eq!(all_enabled.len(), 1);
        assert!(all_enabled.contains(&flag2));
    }
}
