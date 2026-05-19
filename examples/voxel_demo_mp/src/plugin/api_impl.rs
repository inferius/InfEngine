//! Implementation of the [`infengine_sdk::EngineApi`] fn slots.
//!
//! Plugins receive a `*mut EngineHandle` opaque pointer + the
//! API table. Under the hood, the `EngineHandle` is actually a
//! `*mut EngineFacade` - a thin struct holding a raw pointer to
//! the live `Voxel` plus per-call bookkeeping. The host
//! refreshes `facade.voxel` before every dispatch + clears it
//! afterwards.
//!
//! All `extern "C-unwind"` fns here are marked `unsafe` because
//! they accept raw pointers + NUL-terminated UTF-8 strings.
//! Each `unsafe` block carries a `SAFETY:` comment.

use std::ffi::CStr;
use std::os::raw::c_char;

use infengine_sdk::{EngineApi, EngineHandle, Rgb};

use crate::Voxel;

/// Bookkeeping struct hidden behind the opaque `EngineHandle`.
/// The registry leaks one of these on startup; the same pointer
/// gets handed to every plugin dispatch.
pub(crate) struct EngineFacade {
    /// Raw pointer to the live `Voxel`. Refreshed by the
    /// dispatcher before every plugin event + cleared back to
    /// `null_mut` afterwards so a misbehaving plugin caching
    /// the pointer past the callback hits a NULL guard instead
    /// of stale memory.
    pub(crate) voxel: *mut Voxel,
    /// Next plugin-owned block id to hand out from
    /// `block_register`. Pool reserved for plugins: 1000..=1999.
    pub(crate) block_register_budget_next: u16,
}

// SAFETY: EngineFacade is only ever touched on the main thread
// (single-threaded plugin dispatch per the SDK contract). The
// Send bound is needed because the registry that owns the
// leaked facade ptr is stored on Voxel.
unsafe impl Send for EngineFacade {}

/// Convert `*const c_char` -> `Option<&str>` for our internal
/// API impls. Returns `None` for NULL or bad UTF-8 - we never
/// panic on plugin input.
fn cstr_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: SDK contract says plugin args are NUL-terminated
    // UTF-8 for the duration of the call. We don't panic on
    // bad input - the unwrap_or path returns None and the
    // caller no-ops.
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

// ---------------------------------------------------------------
// EngineApi fn implementations
// ---------------------------------------------------------------

unsafe extern "C-unwind" fn world_get_impl(
    h: *mut EngineHandle,
    x: i32,
    y: i32,
    z: i32,
) -> u16 {
    if h.is_null() {
        return 0;
    }
    // SAFETY: caller passed a non-null handle from the
    // registry; we cast it back to EngineFacade. The voxel
    // pointer is only valid during a live dispatch; the
    // dispatcher clears it afterwards.
    let facade = unsafe { &*(h as *const EngineFacade) };
    if facade.voxel.is_null() {
        return 0;
    }
    // SAFETY: voxel ptr was set by the dispatcher to the
    // live &mut Voxel borrow; only read here, no aliasing
    // issue because plugin dispatch is single-threaded.
    let voxel = unsafe { &*facade.voxel };
    voxel.world.get([x, y, z]).0
}

unsafe extern "C-unwind" fn world_set_impl(
    h: *mut EngineHandle,
    x: i32,
    y: i32,
    z: i32,
    block_id: u16,
) {
    if h.is_null() {
        return;
    }
    // SAFETY: same casting + voxel-ptr contract as world_get.
    let facade = unsafe { &mut *(h as *mut EngineFacade) };
    if facade.voxel.is_null() {
        return;
    }
    // SAFETY: voxel ptr live during dispatch; we do an
    // exclusive write.
    let voxel = unsafe { &mut *facade.voxel };
    voxel.world.set([x, y, z], infengine_voxel::BlockId(block_id));
}

unsafe extern "C-unwind" fn chat_send_impl(
    h: *mut EngineHandle,
    msg: *const c_char,
) {
    if h.is_null() {
        return;
    }
    let Some(s) = cstr_arg(msg) else { return };
    // SAFETY: facade ptr cast + voxel-ptr contract per above.
    let facade = unsafe { &mut *(h as *mut EngineFacade) };
    if facade.voxel.is_null() {
        return;
    }
    // SAFETY: voxel ptr live during dispatch.
    let voxel = unsafe { &mut *facade.voxel };
    voxel
        .chat_log
        .push((std::time::Instant::now(), s.to_string()));
}

unsafe extern "C-unwind" fn block_register_impl(
    h: *mut EngineHandle,
    name: *const c_char,
    color: Rgb,
) -> u16 {
    if h.is_null() {
        return 0;
    }
    let Some(name) = cstr_arg(name) else { return 0 };
    if name.is_empty() {
        return 0;
    }
    // SAFETY: facade ptr cast + voxel-ptr contract.
    let facade = unsafe { &mut *(h as *mut EngineFacade) };
    // Lookup vs allocate. Wave 7 doesn't actually create new
    // block types at runtime; the block table is `&'static`.
    // We track plugin-registered (name, id, color) pairs in
    // Voxel::plugin_blocks instead so the chat / HUD can show
    // them. Future work: synthesise atlas tiles + extend the
    // engine block table.
    if facade.voxel.is_null() {
        return 0;
    }
    // SAFETY: voxel ptr live during dispatch.
    let voxel = unsafe { &mut *facade.voxel };
    // Duplicate check: same name => return the existing id.
    if let Some(existing) = voxel
        .plugin_blocks
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
    {
        return existing.id;
    }
    // Refuse if the name collides with an engine-owned block.
    if crate::block::BLOCK_TYPES.iter().any(|b| b.name == name) {
        return 0;
    }
    // Allocate from the 1000..=1999 plugin pool.
    let id = facade.block_register_budget_next;
    if id > 1999 {
        return 0; // pool exhausted
    }
    facade.block_register_budget_next = id.saturating_add(1);
    voxel.plugin_blocks.push(crate::PluginBlock {
        id,
        name: name.to_string(),
        color: color.into_array(),
    });
    id
}

unsafe extern "C-unwind" fn log_info_impl(msg: *const c_char) {
    let Some(s) = cstr_arg(msg) else { return };
    tracing::info!(target: "plugin", "{s}");
}

unsafe extern "C-unwind" fn log_warn_impl(msg: *const c_char) {
    let Some(s) = cstr_arg(msg) else { return };
    tracing::warn!(target: "plugin", "{s}");
}

/// Assemble the EngineApi table. Called once at registry init;
/// the table is leaked and its address handed to every plugin.
pub(crate) fn build_engine_api() -> EngineApi {
    EngineApi {
        world_get: world_get_impl,
        world_set: world_set_impl,
        chat_send: chat_send_impl,
        block_register: block_register_impl,
        log_info: log_info_impl,
        log_warn: log_warn_impl,
    }
}
