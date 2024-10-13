pub use abi_stable as macro_support;
use abi_stable::reexports::SelfOps;
use abi_stable::std_types::{RBox, RStr};
use std::cell::Cell;
use std::cell::OnceCell;
#[allow(unused_imports)]
use std::compile_error;
use std::marker;
use std::ptr::addr_of;

/// Compiler error is both plugin and host feature are enabled
#[cfg(all(feature = "plugin", feature = "host", not(debug_assertions)))]
compile_error!("Both plugin and host features are enabled");

/// A thread id represent system-wide thread id
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct SysThreadId(u64);

crate::thread_local! {
    /// A threaq local [`ThreadId`] that can be used to identify the current thread
    static SYSTHREAD_ID: OnceCell<SysThreadId> = OnceCell::new();
}

impl SysThreadId {
    /// Get the current thread id
    pub fn current() -> Self {
        SYSTHREAD_ID.with(|id| *id.get_or_init(|| SysThreadId(gettid::gettid())))
    }
}

/// A key into dynamic compatible thread-local storage.
pub struct LocalKey<T: 'static> {
    #[doc(hidden)]
    pub read: unsafe extern "C" fn() -> *const T,
}

impl<T: 'static> LocalKey<Cell<T>> {
    pub fn get(&'static self) -> T
    where
        T: Copy,
    {
        self.with(Cell::get)
    }
}

/// Create one or more thread-local values.
///
/// This macro has identical syntax to `std::thread_local!`, without `const` support.
#[macro_export]
macro_rules! thread_local {
    () => {};
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; $($rest:tt)*) => {
        $crate::__thread_local_inner!($(#[$attr])* $vis $name: $t = $init);
        $crate::thread_local!($($rest)*);
    };
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr) => {
        $crate::__thread_local_inner!($(#[$attr])* $vis $name: $t = $init);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __thread_local_inner {
    ($(#[$attr:meta])* $vis:vis $name:ident: $t:ty = $init:expr) => {
        $(#[$attr])*
        $vis static $name: $crate::os::thread::LocalKey<$t> = {
            use $crate::os::thread::macro_support::{pointer_trait::TransmuteElement, std_types::{RBox, RStr}};

            extern "C" fn __thread_local_init() -> RBox<()> {
                let __thread_local_val: $t = $init;
                unsafe { RBox::new(__thread_local_val).transmute_element() }
            }

            ::std::thread_local! {
                static VALUE: &'static $t = $crate::os::thread::__get_tls::<$t>(
                    &mut RStr::from_str(std::concat!(std::stringify!($name), std::stringify!($t), std::module_path!())),
                    __thread_local_init,
                );
            }

            unsafe extern "C" fn __thread_local_read() -> *const $t {
                VALUE.with(|v| *v as *const $t)
            }

            $crate::os::thread::LocalKey { read: __thread_local_read }
        };
    }
}

/// Create a lazily-initialized static value.
///
/// This macro has identical syntax to `lazy_static::lazy_static!`.
#[macro_export]
macro_rules! lazy_static {
    () => {};
    ($(#[$attr:meta])* static ref $N:ident : $T:ty = $e:expr; $($t:tt)*) => {
        $crate::__lazy_static_inner!($(#[$attr])* () static ref $N : $T = $e; $($t)*);
    };
    ($(#[$attr:meta])* pub static ref $N:ident : $T:ty = $e:expr; $($t:tt)*) => {
        $crate::__lazy_static_inner!($(#[$attr])* (pub) static ref $N : $T = $e; $($t)*);
    };
    ($(#[$attr:meta])* pub ($($vis:tt)+) static ref $N:ident : $T:ty = $e:expr; $($t:tt)*) => {
        $crate::__lazy_static_inner!($(#[$attr])* (pub ($($vis)+)) static ref $N : $T = $e; $($t)*);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __lazy_static_inner {
    ($(#[$attr:meta])* ($($vis:tt)*) static ref $N:ident : $T:ty = $e:expr; $($t:tt)*) => {
        #[allow(missing_copy_implementations)]
        #[allow(non_camel_case_types)]
        #[allow(dead_code)]
        $(#[$attr])*
        $($vis)* struct $N {__private_field: ()}
        #[doc(hidden)]
        $($vis)* static $N: $N = $N {__private_field: ()};
        impl std::ops::Deref for $N {
            type Target = $T;
            fn deref(&self) -> &$T {
                use $crate::os::thread::macro_support::{pointer_trait::TransmuteElement, std_types::{RBox, RStr}};
                use std::sync::Once;
                use std::mem::MaybeUninit;

                extern "C" fn __initialize() -> RBox<()> {
                    let __val: $T = $e;
                    unsafe { RBox::new(__val).transmute_element() }
                }

                static VALUE_ONCE: Once = Once::new();
                static mut VALUE: MaybeUninit<&'static $T> = MaybeUninit::uninit();
                VALUE_ONCE.call_once(|| {
                    unsafe {
                        VALUE.write($crate::__get_static(
                            &RStr::from_str(std::concat!(std::stringify!($N), std::stringify!($T), std::module_path!())),
                            __initialize
                        ));
                    }
                });

                unsafe { VALUE.assume_init_read() }
            }
        }
        $crate::lazy_static!($($t)*);
    };
}

#[cfg(feature = "host")]
mod host {
    use abi_stable::std_types::{RBox, RStr};
    use std::cell::RefCell;
    use std::collections::BTreeMap as Map;
    use std::mem::MaybeUninit;
    use std::sync::{Mutex, Once};

    std::thread_local! {
        static PLUGIN_TLS: RefCell<Map<RStr<'static>, RBox<()>>> = RefCell::new(Default::default());
    }

    static PLUGIN_STATIC_ONCE: Once = Once::new();
    static mut PLUGIN_STATIC: MaybeUninit<Mutex<Map<RStr<'static>, RBox<()>>>> =
        MaybeUninit::uninit();

    fn static_map() -> std::sync::MutexGuard<'static, Map<RStr<'static>, RBox<()>>> {
        PLUGIN_STATIC_ONCE.call_once(|| unsafe {
            PLUGIN_STATIC.write(Default::default());
        });
        unsafe { PLUGIN_STATIC.assume_init_mut() }.lock().unwrap()
    }

    pub unsafe extern "C" fn tls(
        id: &RStr<'static>,
        init: extern "C" fn() -> RBox<()>,
    ) -> *const () {
        PLUGIN_TLS.with(|m| {
            let mut m = m.borrow_mut();
            if !m.contains_key(id) {
                m.insert(id.clone(), init());
            }
            // We leak the reference from PLUGIN_TLS as well as the reference out of the RefCell,
            // however this will be safe because:
            // 1. the reference will be used shortly within the thread's runtime (not sending to
            //    another thread) due to the `with` implementation, and
            // 2. the RefCell guard is protecting access/changes to the map, however we _only_ ever
            //    add to the map if a key does not exist (so this box won't disappear on us).
            m.get(id).unwrap().as_ref() as *const ()
        })
    }

    pub unsafe extern "C" fn statics(
        id: &RStr<'static>,
        init: extern "C" fn() -> RBox<()>,
    ) -> *const () {
        let mut m = static_map();
        if !m.contains_key(id) {
            m.insert(id.clone(), init());
        }
        // We leak the reference from PLUGIN_STATIC as well as the reference out of the Mutex,
        // however this will be safe because:
        // 1. the reference will have static lifetime once initially created, and
        // 2. the Mutex guard is protecting access/changes to the map, however we _only_ ever
        //    add to the map if a key does not exist (so this box won't disappear on us).
        m.get(id).unwrap().as_ref() as *const ()
    }

    pub fn reset() {
        PLUGIN_TLS.with(|m| m.borrow_mut().clear());
        static_map().clear();
    }
}

type AccessFunction =
    unsafe extern "C" fn(&RStr<'static>, extern "C" fn() -> RBox<()>) -> *const ();

/// The context to be installed in plugins.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Context {
    tls: AccessFunction,
    statics: AccessFunction,
}

impl Context {
    /// Initialize the thread local storage and static storage.
    ///
    /// # Safety
    /// This must be called only once in each plugin, and prior to the plugin accessing any
    /// thread-local values or static values managed by this library. Otherwise UB may occur.
    pub unsafe fn initialize(&self) {
        HOST_TLS = Some(self.tls);
        HOST_STATICS = Some(self.statics);
    }
}

#[cfg(feature = "host")]
impl Context {
    /// Get the context.
    ///
    /// Separate instances of `Context` will always be identical.
    pub fn get() -> Self {
        Context {
            tls: host::tls,
            statics: host::statics,
        }
    }

    /// Reset the thread-local storage for the current thread.
    ///
    /// This destructs all values and returns the state to a point as if no values have yet been
    /// accessed on the current thread.
    pub fn reset() {
        host::reset();
    }
}

#[cfg(feature = "host")]
static mut HOST_TLS: Option<AccessFunction> = Some(host::tls);

#[cfg(not(feature = "host"))]
static mut HOST_TLS: Option<AccessFunction> = None;

#[cfg(feature = "host")]
static mut HOST_STATICS: Option<AccessFunction> = Some(host::statics);

#[cfg(not(feature = "host"))]
static mut HOST_STATICS: Option<AccessFunction> = None;

#[doc(hidden)]
pub fn __get_tls<T>(id: &mut RStr<'static>, init: extern "C" fn() -> RBox<()>) -> &'static T {
    let host_tls =
        unsafe { HOST_TLS.as_ref() }.expect("host thread local storage improperly initialized");

    unsafe { (host_tls(id, init) as *const T).as_ref().unwrap() }
}

#[doc(hidden)]
pub fn __get_static<T>(id: &RStr<'static>, init: extern "C" fn() -> RBox<()>) -> &'static T {
    let host_statics =
        unsafe { HOST_STATICS.as_ref() }.expect("host static storage improperly initialized");
    unsafe { (host_statics(id, init) as *const T).as_ref().unwrap() }
}

impl<T: 'static> LocalKey<T> {
    /// Acquires a reference to the value in this TLS key.
    ///
    /// If neither `host` nor `plugin` features are enabled, this will panic.
    #[cfg(any(feature = "host", feature = "plugin"))]
    #[inline(always)]
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(unsafe { (self.read)().as_ref().unwrap() })
    }

    /// Acquires a reference to the value in this TLS key.
    ///
    /// If neither `host` nor `plugin` features are enabled, this will panic.
    #[cfg(not(any(feature = "host", feature = "plugin")))]
    pub fn with<F, R>(&'static self, _f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        panic!("plugin_tls built without 'host' or 'plugin' enabled")
    }
}

#[macro_export]
macro_rules! scoped_thread_local {
    ($(#[$attrs:meta])* $vis:vis static $name:ident: $ty:ty) => (
        $(#[$attrs])*
        $vis static $name: $crate::os::thread::ScopedKey<$ty> = $crate::os::thread::ScopedKey {
            inner: {
                $crate::thread_local!(static FOO: ::std::cell::Cell<*const ()> = {
                    ::std::cell::Cell::new(::std::ptr::null())
                });
                &FOO
            },
            _marker: ::std::marker::PhantomData,
        };
    )
}

/// Type representing a thread local storage key corresponding to a reference
/// to the type parameter `T`.
///
/// Keys are statically allocated and can contain a reference to an instance of
/// type `T` scoped to a particular lifetime. Keys provides two methods, `set`
/// and `with`, both of which currently use closures to control the scope of
/// their contents.
pub struct ScopedKey<T> {
    #[doc(hidden)]
    pub inner: &'static LocalKey<Cell<*const ()>>,
    #[doc(hidden)]
    pub _marker: marker::PhantomData<T>,
}

unsafe impl<T> Sync for ScopedKey<T> {}

impl<T> ScopedKey<T> {
    /// Inserts a value into this scoped thread local storage slot for a
    /// duration of a closure.
    ///
    /// While `f` is running, the value `t` will be returned by `get` unless
    /// this function is called recursively inside of `f`.
    ///
    /// Upon return, this function will restore the previous value, if any
    /// was available.
    ///
    /// # Examples
    ///
    /// ```
    /// #[macro_use]
    /// extern crate bubble_core;
    ///
    /// scoped_thread_local!(static FOO: u32);
    ///
    /// # fn main() {
    /// FOO.set(&100, || {
    ///     let val = FOO.with(|v| *v);
    ///     assert_eq!(val, 100);
    ///
    ///     // set can be called recursively
    ///     FOO.set(&101, || {
    ///         // ...
    ///     });
    ///
    ///     // Recursive calls restore the previous value.
    ///     let val = FOO.with(|v| *v);
    ///     assert_eq!(val, 100);
    /// });
    /// # }
    /// ```
    pub fn set<F, R>(&'static self, t: &T, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        struct Reset {
            key: &'static LocalKey<Cell<*const ()>>,
            val: *const (),
        }
        impl Drop for Reset {
            fn drop(&mut self) {
                self.key.with(|c| c.set(self.val));
            }
        }
        let prev = self.inner.with(|c| {
            let prev = c.get();
            c.set(t as *const T as *const ());
            prev
        });
        let _reset = Reset {
            key: self.inner,
            val: prev,
        };
        f()
    }

    /// Gets a value out of this scoped variable.
    ///
    /// This function takes a closure which receives the value of this
    /// variable.
    ///
    /// # Panics
    ///
    /// This function will panic if `set` has not previously been called.
    ///
    /// # Examples
    ///
    /// ```no_run, no_test
    /// #[macro_use]
    /// extern crate bubble_core;
    ///
    /// scoped_thread_local!(static FOO: u32);
    ///
    /// # fn main() {
    /// FOO.with(|slot| {
    ///     // work with `slot`
    /// # drop(slot);
    /// });
    /// # }
    /// ```
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let val = self.inner.with(|c| c.get());
        assert!(
            !val.is_null(),
            "cannot access a scoped thread local \
                                 variable without calling `set` first"
        );
        unsafe { f(&*(val as *const T)) }
    }

    /// Test whether this TLS key has been `set` for the current thread.
    pub fn is_set(&'static self) -> bool {
        self.inner.with(|c| !c.get().is_null())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Sender};
    use std::thread;

    scoped_thread_local!(static FOO: u32);

    #[test]
    fn smoke() {
        scoped_thread_local!(static BAR: u32);

        assert!(!BAR.is_set());
        BAR.set(&1, || {
            assert!(BAR.is_set());
            BAR.with(|slot| {
                assert_eq!(*slot, 1);
            });
        });
        assert!(!BAR.is_set());
    }

    #[test]
    fn cell_allowed() {
        scoped_thread_local!(static BAR: Cell<u32>);

        BAR.set(&Cell::new(1), || {
            BAR.with(|slot| {
                assert_eq!(slot.get(), 1);
            });
        });
    }

    #[test]
    fn scope_item_allowed() {
        assert!(!FOO.is_set());
        FOO.set(&1, || {
            assert!(FOO.is_set());
            FOO.with(|slot| {
                assert_eq!(*slot, 1);
            });
        });
        assert!(!FOO.is_set());
    }

    #[test]
    fn panic_resets() {
        struct Check(Sender<u32>);
        impl Drop for Check {
            fn drop(&mut self) {
                FOO.with(|r| {
                    self.0.send(*r).unwrap();
                })
            }
        }

        let (tx, rx) = channel();
        let t = thread::spawn(|| {
            FOO.set(&1, || {
                let _r = Check(tx);

                FOO.set(&2, || panic!());
            });
        });

        assert_eq!(rx.recv().unwrap(), 1);
        assert!(t.join().is_err());
    }

    #[test]
    fn attrs_allowed() {
        scoped_thread_local!(
            /// Docs
            static BAZ: u32
        );

        scoped_thread_local!(
            #[allow(non_upper_case_globals)]
            static quux: u32
        );

        let _ = BAZ;
        let _ = quux;
    }
}
