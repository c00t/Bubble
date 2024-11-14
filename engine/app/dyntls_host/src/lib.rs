#![allow(static_mut_refs)]
mod concurrent_map;
mod list;
mod michael_hash_map;
mod natarajan_mittal_tree;
mod utils;

use abi_stable::std_types::{RBox, RStr};
use dyntls::Context;

use natarajan_mittal_tree::NMTreeMap as Map;
// use michael_hash_map::HashMap as Map;
use concurrent_map::{ConcurrentMap, OutputHolder};
use std::mem::MaybeUninit;
use std::sync::{Mutex, Once, OnceLock};

std::thread_local! {
    static PLUGIN_TLS: Map<RStr<'static>, RBox<()>> = Map::new();
}

static PLUGIN_STATIC_ONCE: Once = Once::new();
static mut PLUGIN_STATIC: MaybeUninit<Map<RStr<'static>, RBox<()>>> = MaybeUninit::uninit();

fn static_map() -> &'static Map<RStr<'static>, RBox<()>> {
    PLUGIN_STATIC_ONCE.call_once(|| unsafe {
        PLUGIN_STATIC.write(Map::new());
    });
    unsafe { PLUGIN_STATIC.assume_init_ref() }
}

pub unsafe extern "C" fn tls(id: &RStr<'static>, init: extern "C" fn() -> RBox<()>) -> *const () {
    PLUGIN_TLS.with(|m| {
        let cs = circ::cs();
        if let Some(node) = m.get(id, &cs) {
            // We leak the reference from PLUGIN_TLS as well as the reference out of the RefCell,
            // however this will be safe because:
            // 1. the reference will be used shortly within the thread's runtime (not sending to
            //    another thread) due to the `with` implementation, and
            // 2. the RefCell guard is protecting access/changes to the map, however we _only_ ever
            //    add to the map if a key does not exist (so this box won't disappear on us).
            node.output().as_ref() as *const ()
        } else {
            m.insert(id.clone(), init(), &cs);
            let x = m.get(id, &cs).unwrap();
            x.output().as_ref() as *const ()
        }
    })
}

pub unsafe extern "C" fn statics(
    id: &RStr<'static>,
    init: extern "C" fn() -> RBox<()>,
) -> *const () {
    let m = static_map();
    let cs = circ::cs();
    if let Some(node) = m.get(id, &cs) {
        node.output().as_ref() as *const ()
    } else {
        m.insert(id.clone(), init(), &cs);
        m.get(id, &cs).unwrap().output().as_ref() as *const ()
    }
}

pub fn reset() {
    // NMTree currently does not support clear
    // PLUGIN_TLS.with(|m| m.borrow_mut().clear());
    // static_map().clear();
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
