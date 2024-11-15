pub mod rkyv_prototype {
    use rkyv::{rancor::Error, with::AsBox, Archive, Deserialize, Serialize};

    // macro applied to this struct
    pub struct Example {
        a: i32,
        b: u32,
        // #[version(start = (0,2,0))]
        c: String,
    }

    impl Example {
        pub fn builder() -> ExampleBuilder {
            ExampleBuilder {
                a: 0,
                b: 0,
                c: Default::default(),
            }
        }
        pub fn to_versioned<const MAJOR: u64, const MINOR: u64, const PATCH: u64, T>(
            self,
        ) -> Versioned<SemverExample<MAJOR, MINOR, PATCH, T>>
        where
            Self: Into<Versioned<SemverExample<MAJOR, MINOR, PATCH, T>>>,
        {
            Versioned::from(self.into())
        }

        pub fn try_access(bytes: &[u8]) -> Option<(u64, u64, u64)> {
            if matches!(
                rkyv::access::<
                    ArchivedVersioned<SemverExample<0, 1, 0, private::Example0_1_0>>,
                    Error,
                >(bytes),
                Ok(_)
            ) {
                return Some((0, 1, 0));
            }
            if matches!(
                rkyv::access::<
                    ArchivedVersioned<SemverExample<0, 2, 0, private::Example0_2_0>>,
                    Error,
                >(bytes),
                Ok(_)
            ) {
                return Some((0, 2, 0));
            }
            None
        }
    }

    #[derive(Archive, Deserialize, Serialize)]
    #[repr(transparent)]
    pub struct SemverExample<const MAJOR: u64, const MINOR: u64, const PATCH: u64, T>(pub T);

    impl ArchivedVersioned<SemverExample<0, 1, 0, private::Example0_1_0>> {
        pub fn a(&self) -> <i32 as Archive>::Archived {
            self.0 .0.a.into()
        }
        pub fn b(&self) -> <u32 as Archive>::Archived {
            self.0 .0.b.into()
        }
    }

    impl ArchivedVersioned<SemverExample<0, 2, 0, private::Example0_2_0>> {
        pub fn a(&self) -> <i32 as Archive>::Archived {
            self.0 .0.a.into()
        }
        pub fn b(&self) -> <u32 as Archive>::Archived {
            self.0 .0.b.into()
        }
        pub fn c(&self) -> &<String as Archive>::Archived {
            &self.0 .0.c
        }
    }

    // it's intended to be private, but leaked to crate users
    // we make it private because i don't want it to be displayed by rust-analyzer recommendation
    mod private {
        use super::{Example, SemverExample, Versioned};
        use rkyv::{Archive, Deserialize, Serialize};
        // use declare to:
        // to truncated hash? or just `Example{VERSION}`? this type will be leaked to crate users
        #[doc(hidden)]
        #[derive(Archive, Deserialize, Serialize)]
        pub struct Example0_1_0 {
            pub a: i32,
            pub b: u32,
        }

        impl From<Example> for SemverExample<0, 1, 0, Example0_1_0> {
            #[inline]
            fn from(value: Example) -> Self {
                Self(Example0_1_0 {
                    a: value.a,
                    b: value.b,
                })
            }
        }

        impl From<SemverExample<0, 1, 0, Example0_1_0>>
            for Versioned<SemverExample<0, 1, 0, Example0_1_0>>
        {
            #[inline]
            fn from(value: SemverExample<0, 1, 0, Example0_1_0>) -> Self {
                Versioned(value)
            }
        }

        impl From<Example> for Versioned<SemverExample<0, 1, 0, Example0_1_0>> {
            #[inline]
            fn from(value: Example) -> Self {
                Versioned(SemverExample::<0, 1, 0, Example0_1_0>(Example0_1_0 {
                    a: value.a,
                    b: value.b,
                }))
            }
        }

        #[doc(hidden)]
        #[derive(Archive, Deserialize, Serialize)]
        pub struct Example0_2_0 {
            pub a: i32,
            pub b: u32,
            pub c: String,
        }

        impl From<Example> for SemverExample<0, 2, 0, Example0_2_0> {
            #[inline]
            fn from(value: Example) -> Self {
                Self(Example0_2_0 {
                    a: value.a,
                    b: value.b,
                    c: value.c,
                })
            }
        }

        impl From<SemverExample<0, 2, 0, Example0_2_0>>
            for Versioned<SemverExample<0, 2, 0, Example0_2_0>>
        {
            #[inline]
            fn from(value: SemverExample<0, 2, 0, Example0_2_0>) -> Self {
                Versioned(value)
            }
        }

        impl From<Example> for Versioned<SemverExample<0, 2, 0, Example0_2_0>> {
            #[inline]
            fn from(value: Example) -> Self {
                Versioned(SemverExample::<0, 2, 0, Example0_2_0>(Example0_2_0 {
                    a: value.a,
                    b: value.b,
                    c: value.c,
                }))
            }
        }
    }

    pub struct ExampleBuilder {
        a: i32,
        b: u32,
        c: String,
    }

    impl ExampleBuilder {
        pub fn build(self) -> Example {
            Example {
                a: self.a,
                b: self.b,
                c: self.c,
            }
        }
        pub fn a(mut self, a: i32) -> Self {
            self.a = a;
            self
        }
        pub fn b(mut self, b: u32) -> Self {
            self.b = b;
            self
        }
        pub fn c(mut self, c: String) -> Self {
            self.c = c;
            self
        }
    }

    // a example
    #[derive(Archive, Deserialize, Serialize)]
    #[repr(transparent)]
    pub struct Versioned<T>(#[rkyv(with = AsBox)] pub T);

    #[cfg(test)]
    mod tests {
        use super::*;
        use private::{Example0_1_0, Example0_2_0};
        use rkyv::{rancor::Error, with::AsBox, Archive, Deserialize, Serialize};

        #[test]
        fn test_prototype_rkyv() {
            // workable! types leak to crate users, but it's more ergonomic to just use major/minor/patch instead of type `Example{VERSION}` generic type
            let example010 = Example::builder()
                .a(1)
                .b(2)
                .build()
                .to_versioned::<0, 1, 0, _>();

            let example020 = Example::builder()
                .a(1)
                .b(2)
                .c("hello".to_string())
                .build()
                .to_versioned::<0, 2, 0, _>();

            let bytes010 = rkyv::to_bytes::<Error>(&example010).expect("failed to serialize 010");
            let bytes020 = rkyv::to_bytes::<Error>(&example020).expect("failed to serialize 020");

            let view_010_as_010 =
                rkyv::access::<ArchivedVersioned<Example0_1_0>, Error>(&bytes010).unwrap();
            assert_eq!(view_010_as_010.0.a, 1);
            assert_eq!(view_010_as_010.0.b, 2);
            let view_020_as_020 =
                rkyv::access::<ArchivedVersioned<Example0_2_0>, Error>(&bytes020).unwrap();
            assert_eq!(view_020_as_020.0.a, 1);
            assert_eq!(view_020_as_020.0.b, 2);
            assert_eq!(view_020_as_020.0.c, "hello".to_string());

            let view_020_as_010 =
                rkyv::access::<ArchivedVersioned<Example0_1_0>, Error>(&bytes020).unwrap();
            assert_eq!(view_020_as_010.0.a, 1);
            assert_eq!(view_020_as_010.0.b, 2);
            // rkyv isn't support forward compatibility, it's a problem
            // just don't support it?
            // or just use bilrost when need communication between different versions?
            let view_010_as_020 = rkyv::access::<ArchivedVersioned<Example0_2_0>, Error>(&bytes010);
            assert!(view_010_as_020.is_err());
        }
    }
}

pub mod bilrost_prototype {
    use bilrost::Message;

    // macro applied to this struct
    // user can add tag for fields, but it'll propagate to the versioned struct
    // #[derive(BasinType)]
    struct Example {
        // bilrost tags...
        pub a: i32,
        // bilrost tags...
        pub b: u32,
        // #[version(start = (0,2,0))]
        // bilrost tags...
        pub c: String,
    }
    // generated by #[derive(BasinType)]
    #[derive(Message)]
    struct Example0_1_0 {
        #[bilrost(tag(1))]
        pub a: i32,
        #[bilrost(tag(2))]
        pub b: u32,
    }

    impl From<Example> for Example0_1_0 {
        #[inline]
        fn from(value: Example) -> Self {
            Self {
                a: value.a,
                b: value.b,
            }
        }
    }

    impl From<Example> for VExample<0, 1, 0, Example0_1_0> {
        #[inline]
        fn from(value: Example) -> Self {
            Self(Example0_1_0 {
                a: value.a,
                b: value.b,
            })
        }
    }

    impl From<VExample<0, 1, 0, Example0_1_0>> for Example0_1_0 {
        #[inline]
        fn from(value: VExample<0, 1, 0, Example0_1_0>) -> Self {
            value.0
        }
    }

    #[derive(Message)]
    struct Example0_2_0 {
        #[bilrost(tag(1))]
        pub a: i32,
        #[bilrost(tag(2))]
        pub b: u32,
        #[bilrost(tag(3))]
        pub c: String,
    }

    impl From<Example> for VExample<0, 2, 0, Example0_2_0> {
        #[inline]
        fn from(value: Example) -> Self {
            Self(Example0_2_0 {
                a: value.a,
                b: value.b,
                c: value.c,
            })
        }
    }

    impl From<VExample<0, 2, 0, Example0_2_0>> for Example0_2_0 {
        #[inline]
        fn from(value: VExample<0, 2, 0, Example0_2_0>) -> Self {
            value.0
        }
    }

    impl From<Example> for Example0_2_0 {
        #[inline]
        fn from(value: Example) -> Self {
            Self {
                a: value.a,
                b: value.b,
                c: value.c,
            }
        }
    }

    #[derive(Message)]
    #[repr(transparent)]
    struct VExample<const MAJOR: u64, const MINOR: u64, const PATCH: u64, T>(pub T);

    impl Example {
        fn builder() -> ExampleBuilder {
            ExampleBuilder {
                a: 0,
                b: 0,
                c: Default::default(),
            }
        }
        fn to_versioned<T>(self) -> T
        where
            Self: Into<T>,
        {
            self.into()
        }
    }

    struct ExampleBuilder {
        a: i32,
        b: u32,
        c: String,
    }

    impl ExampleBuilder {
        fn build(self) -> Example {
            Example {
                a: self.a,
                b: self.b,
                c: self.c,
            }
        }
        fn a(mut self, a: i32) -> Self {
            self.a = a;
            self
        }
        fn b(mut self, b: u32) -> Self {
            self.b = b;
            self
        }
        fn c(mut self, c: String) -> Self {
            self.c = c;
            self
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_prototype_bilrost() {
            let example010: Example0_1_0 = Example::builder().a(1).b(2).build().to_versioned();
            let example020: Example0_2_0 = Example::builder()
                .a(1)
                .b(2)
                .c("hello".to_string())
                .build()
                .to_versioned();

            let bytes010 = example010.encode_to_vec();
            let bytes020 = example020.encode_to_vec();

            let view_010_as_010 = Example0_1_0::decode(bytes010.as_ref()).unwrap();
            assert_eq!(view_010_as_010.a, 1);
            assert_eq!(view_010_as_010.b, 2);

            let view_020_as_020 = Example0_2_0::decode(bytes020.as_ref()).unwrap();
            assert_eq!(view_020_as_020.a, 1);
            assert_eq!(view_020_as_020.b, 2);
            assert_eq!(view_020_as_020.c, "hello".to_string());

            let view_020_as_010 = Example0_1_0::decode(bytes020.as_ref()).unwrap();
            assert_eq!(view_020_as_010.a, 1);
            assert_eq!(view_020_as_010.b, 2);

            let view_010_as_020 = Example0_2_0::decode(bytes010.as_ref()).unwrap();
            assert_eq!(view_010_as_020.a, 1);
            assert_eq!(view_010_as_020.b, 2);
            assert_eq!(view_010_as_020.c, "");
        }
    }
}

/// Merge them together
pub mod rkyv_bilrost_prototype {
    use bilrost::Message;
    use private::Example0_1_0;
    use rkyv::{rancor::Error, with::AsBox, Archive, Deserialize, Serialize};

    // The main struct that users interact with
    struct Example {
        pub a: i32,
        pub b: u32,
        pub c: String,
    }

    impl Example {
        fn builder() -> ExampleBuilder {
            ExampleBuilder {
                a: 0,
                b: 0,
                c: String::new(),
            }
        }

        fn to_versioned<const MAJOR: u64, const MINOR: u64, const PATCH: u64, T, U>(
            self,
        ) -> Versioned<U>
        where
            Self: Into<VExample<MAJOR, MINOR, PATCH, T>>,
            Versioned<U>: From<VExample<MAJOR, MINOR, PATCH, T>>,
        {
            Versioned::from(self.into())
        }
    }

    struct VExample<const MAJOR: u64, const MINOR: u64, const PATCH: u64, T>(pub T);

    // Private versioned types
    pub mod private {
        use super::*;

        #[doc(hidden)]
        #[derive(Archive, Deserialize, Serialize, Message)]
        pub struct Example0_1_0 {
            #[bilrost(tag(1))]
            pub a: i32,
            #[bilrost(tag(2))]
            pub b: u32,
        }

        impl From<Example> for VExample<0, 1, 0, Example0_1_0> {
            #[inline]
            fn from(value: Example) -> Self {
                Self(Example0_1_0 {
                    a: value.a,
                    b: value.b,
                })
            }
        }

        impl From<VExample<0, 1, 0, Example0_1_0>> for Versioned<Example0_1_0> {
            #[inline]
            fn from(value: VExample<0, 1, 0, Example0_1_0>) -> Self {
                Versioned(value.0)
            }
        }

        #[doc(hidden)]
        #[derive(Archive, Deserialize, Serialize, Message)]
        pub struct Example0_2_0 {
            #[bilrost(tag(1))]
            pub a: i32,
            #[bilrost(tag(2))]
            pub b: u32,
            #[bilrost(tag(3))]
            pub c: Option<String>, // all appended values should be optional, if you want to keep compatibility
        }

        impl From<Example> for VExample<0, 2, 0, Example0_2_0> {
            #[inline]
            fn from(value: Example) -> Self {
                Self(Example0_2_0 {
                    a: value.a,
                    b: value.b,
                    c: Some(value.c),
                })
            }
        }

        impl From<VExample<0, 2, 0, Example0_2_0>> for Versioned<Example0_2_0> {
            #[inline]
            fn from(value: VExample<0, 2, 0, Example0_2_0>) -> Self {
                Versioned(value.0)
            }
        }
    }

    struct ExampleBuilder {
        a: i32,
        b: u32,
        c: String,
    }

    impl ExampleBuilder {
        fn build(self) -> Example {
            Example {
                a: self.a,
                b: self.b,
                c: self.c,
            }
        }
        fn a(mut self, a: i32) -> Self {
            self.a = a;
            self
        }
        fn b(mut self, b: u32) -> Self {
            self.b = b;
            self
        }
        fn c(mut self, c: String) -> Self {
            self.c = c;
            self
        }
    }

    #[derive(Archive, Deserialize, Serialize, Message)]
    #[repr(transparent)]
    struct Versioned<T>(#[rkyv(with = AsBox)] pub T);

    #[cfg(test)]
    mod tests {
        use super::private::{Example0_1_0, Example0_2_0};
        use super::*;
        use rkyv::rancor::Error;

        #[test]
        fn test_combined_serialization() {
            // Test rkyv serialization, use it for
            let example010 = Example::builder()
                .a(1)
                .b(2)
                .build()
                .to_versioned::<0, 1, 0, _, _>();

            let example020 = Example::builder()
                .a(1)
                .b(2)
                .c("hello".to_string())
                .build()
                .to_versioned::<0, 2, 0, _, _>();

            // rkyv tests
            let bytes010 = rkyv::to_bytes::<Error>(&example010).expect("failed to serialize 010");
            let bytes020 = rkyv::to_bytes::<Error>(&example020).expect("failed to serialize 020");

            let view_010_as_010 =
                rkyv::access::<rkyv::Archived<Versioned<Example0_1_0>>, Error>(&bytes010).unwrap();
            assert_eq!(view_010_as_010.0.a, 1);
            assert_eq!(view_010_as_010.0.b, 2);

            let view_020_as_020 =
                rkyv::access::<rkyv::Archived<Versioned<Example0_2_0>>, Error>(&bytes020).unwrap();
            assert_eq!(view_020_as_020.0.a, 1);
            assert_eq!(view_020_as_020.0.b, 2);
            assert_eq!(view_020_as_020.0.c, Some("hello".to_string()));

            let view_020_as_010 =
                rkyv::access::<rkyv::Archived<Versioned<Example0_1_0>>, Error>(&bytes020).unwrap();
            assert_eq!(view_020_as_010.0.a, 1);
            assert_eq!(view_020_as_010.0.b, 2);
            // even for local storage, forward compatibility is still needed
            // eg.
            // you have a lower version data on your disk,
            // and have a newer version program now
            // how to load the lower version data?
            // just try to determined the version? or use the unsafe access?
            {
                let view_010_as_020 =
                    rkyv::access::<ArchivedVersioned<Example0_2_0>, Error>(&bytes010);
                assert!(view_010_as_020.is_err());
            }
            let view_010_as_020 =
                unsafe { rkyv::access_unchecked::<ArchivedVersioned<Example0_2_0>>(&bytes010) };
            // assert!(view_010_as_020.is_err());
            assert_eq!(view_010_as_020.0.a, 1);
            assert_eq!(view_010_as_020.0.b, 2);
            // assert_eq!(view_010_as_020.0.c, Some("hello".to_string()));

            // bilrost tests
            let bilrost_bytes010 = example010.encode_to_vec();
            let bilrost_bytes020 = example020.encode_to_vec();

            let bilrost_view_010 =
                <Versioned<Example0_1_0>>::decode(bilrost_bytes010.as_ref()).unwrap();
            assert_eq!(bilrost_view_010.0.a, 1);
            assert_eq!(bilrost_view_010.0.b, 2);

            let bilrost_view_020 =
                <Versioned<Example0_2_0>>::decode(bilrost_bytes020.as_ref()).unwrap();
            assert_eq!(bilrost_view_020.0.a, 1);
            assert_eq!(bilrost_view_020.0.b, 2);
            assert_eq!(bilrost_view_020.0.c, Some("hello".to_string()));

            let bilrost_view_020_as_010 =
                <Versioned<Example0_1_0>>::decode(bilrost_bytes020.as_ref()).unwrap();
            assert_eq!(bilrost_view_020_as_010.0.a, 1);
            assert_eq!(bilrost_view_020_as_010.0.b, 2);

            let bilrost_view_010_as_020 =
                <Versioned<Example0_2_0>>::decode(bilrost_bytes010.as_ref()).unwrap();
            assert_eq!(bilrost_view_010_as_020.0.a, 1);
            assert_eq!(bilrost_view_010_as_020.0.b, 2);
            assert_eq!(bilrost_view_010_as_020.0.c, None);
        }
    }
}
