//! Engine -> plugin event dispatch hooks.
//!
//! Each `dispatch_*` fn is called from the corresponding engine
//! code path with a `&mut Voxel`. The fn updates the registry's
//! facade pointer to the live voxel + iterates every loaded
//! plugin, invoking the matching vtable slot under a panic
//! catcher.
//!
//! Plugins are dispatched single-threaded on the engine main
//! thread.
//!
//! `set_current_dim` lands its call site when the dim-switch
//! code (separate ticket) gains an awareness hook; allow
//! `dead_code` on the module for now so the W7.3 commit lands
//! clippy-clean.
#![allow(dead_code)]

use super::host;

use crate::Voxel;

/// Update the facade's voxel pointer + run `f(plugin)` for every
/// loaded plugin. Restores the facade pointer to null after the
/// iteration so a misbehaving plugin can't read stale memory if
/// it caches the handle.
fn for_each_plugin<F>(voxel: &mut Voxel, mut f: F)
where
    F: FnMut(&mut super::host::LoadedPlugin, *mut super::api_impl::EngineFacade),
{
    // Snapshot the raw voxel pointer for the duration of the
    // dispatch. The split borrow keeps `registry` exclusive
    // (we drain into a temporary Vec, run callbacks against
    // the raw voxel ptr, then move the Vec back).
    let Some(registry) = voxel.plugins.take() else {
        return;
    };
    let mut registry = registry;
    let voxel_ptr: *mut Voxel = voxel as *mut Voxel;
    // SAFETY: voxel is an exclusive borrow for the duration
    // of the dispatch; the raw ptr exists only inside this
    // fn. We restore the registry to its slot before
    // returning so the in-place ownership stays consistent.
    unsafe {
        (*registry.facade_ptr).voxel = voxel_ptr;
    }
    for plug in registry.plugins.iter_mut() {
        f(plug, registry.facade_ptr);
    }
    // SAFETY: same exclusive borrow; clear so cached
    // handles read NULL.
    unsafe {
        (*registry.facade_ptr).voxel = std::ptr::null_mut();
    }
    voxel.plugins = Some(registry);
}

/// Fire `on_tick` on every plugin. Called from the engine's
/// main update loop with the per-frame delta.
pub(crate) fn dispatch_tick(voxel: &mut Voxel, dt: f32, day_time: f32) {
    // Build a fresh dim CString outside the loop so all plugins
    // see the same name + we avoid re-allocating per plugin.
    let dim_cstr = {
        let mut s = String::from("overworld");
        if let Some(registry) = voxel.plugins.as_ref() {
            // We can't access voxel.current_dim here because
            // we'd alias `voxel` with `registry` - read the
            // pre-cached one on registry instead.
            s = registry
                .current_dim_cstr
                .to_str()
                .unwrap_or("overworld")
                .to_string();
        }
        std::ffi::CString::new(s).unwrap_or_else(|_| {
            std::ffi::CString::new("overworld").unwrap()
        })
    };
    for_each_plugin(voxel, |plug, facade_ptr| {
        host::dispatch_tick(plug, facade_ptr, dt, day_time, &dim_cstr);
    });
}

/// Fire `on_block_place` on every plugin.
pub(crate) fn dispatch_block_place(
    voxel: &mut Voxel,
    x: i32,
    y: i32,
    z: i32,
    block_id: u16,
    player_id: u32,
) {
    for_each_plugin(voxel, |plug, facade_ptr| {
        host::dispatch_block_event(
            plug, facade_ptr, true, x, y, z, block_id, player_id,
        );
    });
}

/// Fire `on_block_break` on every plugin.
pub(crate) fn dispatch_block_break(
    voxel: &mut Voxel,
    x: i32,
    y: i32,
    z: i32,
    block_id: u16,
    player_id: u32,
) {
    for_each_plugin(voxel, |plug, facade_ptr| {
        host::dispatch_block_event(
            plug, facade_ptr, false, x, y, z, block_id, player_id,
        );
    });
}

/// Fire `on_chat_msg` on every plugin. `speaker` is 0 for
/// system messages.
pub(crate) fn dispatch_chat(voxel: &mut Voxel, speaker: u32, msg: &str) {
    let Ok(msg_c) = std::ffi::CString::new(msg) else {
        // Interior NUL - skip dispatch (plugin won't see the
        // message; chat is best-effort).
        return;
    };
    for_each_plugin(voxel, |plug, facade_ptr| {
        host::dispatch_chat(plug, facade_ptr, speaker, &msg_c);
    });
}

/// Update the registry's cached current-dim string. Call
/// whenever the active dim changes so subsequent on_tick
/// events see the right dim name.
pub(crate) fn set_current_dim(voxel: &mut Voxel, dim: &str) {
    let Some(registry) = voxel.plugins.as_mut() else {
        return;
    };
    if let Ok(c) = std::ffi::CString::new(dim) {
        registry.current_dim_cstr = c;
    }
}
