#![allow(static_mut_refs)]
use abi_stable::std_types::{RBox, RStr};
use dyntls::Context;
use std::cell::RefCell;
use std::collections::BTreeMap as Map;
use std::mem::MaybeUninit;
use std::sync::{Mutex, Once};

std::thread_local! {
    static PLUGIN_TLS: RefCell<Map<RStr<'static>, RBox<()>>> = RefCell::new(Default::default());
}

static PLUGIN_STATIC_ONCE: Once = Once::new();
static mut PLUGIN_STATIC: MaybeUninit<Mutex<Map<RStr<'static>, RBox<()>>>> = MaybeUninit::uninit();

fn static_map() -> std::sync::MutexGuard<'static, Map<RStr<'static>, RBox<()>>> {
    PLUGIN_STATIC_ONCE.call_once(|| unsafe {
        PLUGIN_STATIC.write(Default::default());
    });
    unsafe { PLUGIN_STATIC.assume_init_mut() }.lock().unwrap()
}

pub unsafe extern "C" fn tls(id: &RStr<'static>, init: extern "C" fn() -> RBox<()>) -> *const () {
    PLUGIN_TLS.with(|m| {
        let m_r = m.borrow();
        if m_r.contains_key(id) {
            // We leak the reference from PLUGIN_TLS as well as the reference out of the RefCell,
            // however this will be safe because:
            // 1. the reference will be used shortly within the thread's runtime (not sending to
            //    another thread) due to the `with` implementation, and
            // 2. the RefCell guard is protecting access/changes to the map, however we _only_ ever
            //    add to the map if a key does not exist (so this box won't disappear on us).
            m_r.get(id).unwrap().as_ref() as *const ()
        } else {
            drop(m_r);
            let mut m_w = m.borrow_mut();
            m_w.insert(id.clone(), init());
            m_w.get(id).unwrap().as_ref() as *const ()
        }
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

/// Get the context.
///
/// Separate instances of `Context` will always be identical.
pub fn get() -> Context {
    Context {
        tls: tls,
        statics: statics,
    }
}
