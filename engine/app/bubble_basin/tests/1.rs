use bubble_basin::prototype::rkyv_prototype::*;
use rkyv::rancor::Error;
#[test]
pub fn versioned_example() {
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
        rkyv::access::<ArchivedVersioned<SemverExample<0, 1, 0, _>>, Error>(&bytes010).unwrap();
    assert_eq!(view_010_as_010.a(), 1);
    assert_eq!(view_010_as_010.b(), 2);
    let view_020_as_020 =
        rkyv::access::<ArchivedVersioned<SemverExample<0, 2, 0, _>>, Error>(&bytes020).unwrap();
    assert_eq!(view_020_as_020.a(), 1);
    assert_eq!(view_020_as_020.b(), 2);
    assert_eq!(view_020_as_020.c(), "hello");

    let view_020_as_010 =
        rkyv::access::<ArchivedVersioned<SemverExample<0, 1, 0, _>>, Error>(&bytes020).unwrap();
    assert_eq!(view_020_as_010.a(), 1);
    assert_eq!(view_020_as_010.b(), 2);
    // rkyv isn't support forward compatibility, it's a problem
    // just don't support it?
    // or just use bilrost when need communication between different versions?
    let view_010_as_020 =
        rkyv::access::<ArchivedVersioned<SemverExample<0, 2, 0, _>>, Error>(&bytes010);
    if let Ok(view) = view_010_as_020 {
        assert_eq!(view.a(), 1);
        assert_eq!(view.b(), 2);
        assert!(false);
    } else {
        assert!(true);
    }
}
