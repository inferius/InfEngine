//! InfEngine plugin SDK - C-ABI core.
//!
//! Every type is `#[repr(C)]`. Every function pointer is
//! `extern "C-unwind"`. The surface is **language-agnostic** so
//! the Rust dylib host (Wave 7) and the C# CLR host (Phase K)
//! can consume the same definitions without any duplication.
//! `C-unwind` allows panics to propagate across the FFI boundary
//! so the host's `catch_unwind` can isolate a misbehaving
//! plugin (a plain `extern "C"` would abort the process).
//!
//! # Layering
//!
//! ```text
//! plugins (Rust dylib, future C# DLL)
//!     |
//!     v
//! infengine_sdk_rs (Rust idioms)   <-- Wave 7
//! infengine_sdk_cs (P/Invoke)      <-- Phase K
//!     |
//!     v
//! infengine_sdk  (C-ABI core - THIS crate)
//!     |
//!     v
//! engine plugin host (DylibHost in Wave 7, ClrHost in Phase K)
//! ```
//!
//! # `unsafe`
//!
//! The entire FFI surface is `unsafe` by nature. Callers must
//! respect the documented contracts: pointer lifetimes, NUL-
//! terminated UTF-8 strings, non-NULL where required. The Rust
//! wrapper (`infengine_sdk_rs`) hides this surface and exposes a
//! safe `Plugin` trait + `Engine` handle on top.
//!
//! # ABI versioning
//!
//! `ABI_MAJOR` bumps when the binary layout of any public struct
//! changes (re-order, insert, drop a field, change a vtable
//! signature). `ABI_MINOR` bumps for additive changes only -
//! e.g. a new `Option<...>` vtable slot appended at the end.
//!
//! Plugins ship the `ABI_MAJOR` / `ABI_MINOR` they were built
//! against inside their [`PluginInfo`]. The host rejects a
//! plugin whose `abi_major` doesn't match its own; a plugin
//! whose `abi_minor` exceeds the host's is also rejected (the
//! plugin uses a vtable slot the host doesn't know about).

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_void};

// ---------------------------------------------------------------
// ABI version
// ---------------------------------------------------------------

/// Major ABI version. Bump on every binary-incompatible change.
pub const ABI_MAJOR: u32 = 0;

/// Minor ABI version. Bump on every additive change (new vtable
/// slot at the tail, new helper free fn).
pub const ABI_MINOR: u32 = 1;

// ---------------------------------------------------------------
// Opaque engine handle
// ---------------------------------------------------------------

/// Opaque handle to the host's engine state. Plugins receive
/// `*mut EngineHandle` on every dispatch + pass it back to the
/// [`EngineApi`] functions. The plugin must never dereference
/// the pointer directly - it's an opaque cookie owned by the
/// host.
///
/// The `c_void` field gives the type a non-zero size so it
/// behaves like a real opaque struct under both Rust and C
/// (zero-sized types have inconsistent ABI behaviour across
/// languages).
#[repr(C)]
pub struct EngineHandle {
    _private: [c_void; 0],
}

impl std::fmt::Debug for EngineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineHandle").finish()
    }
}

// ---------------------------------------------------------------
// Plugin metadata
// ---------------------------------------------------------------

/// Plugin metadata returned by the registration entry point.
///
/// All string pointers are UTF-8, NUL-terminated. The PLUGIN
/// owns the backing storage; it must remain valid for the
/// lifetime of the loaded plugin (host releases the library on
/// unload, which also drops the strings).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PluginInfo {
    /// ABI major version the plugin was built against.
    pub abi_major: u32,
    /// ABI minor version the plugin was built against.
    pub abi_minor: u32,
    /// Plugin name, UTF-8, NUL-terminated. Used in chat
    /// commands + log lines.
    pub name: *const c_char,
    /// Plugin version, UTF-8, NUL-terminated. Free-form.
    pub version: *const c_char,
}

// ---------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------

/// Per-block-event payload (place + break).
///
/// Coordinates are absolute world voxel coords. `block_id` is
/// the block id at the cell AFTER the event (so a break event
/// carries the cell's NEW block, normally air = 0; a place
/// event carries the block that was placed).
///
/// `player_id` is the originating player. `0` = engine-driven
/// (worldgen settle, liquid spread, ...).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockEvent {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block_id: u16,
    /// Explicit padding so a C compiler sees an unambiguous
    /// 2-byte gap before `player_id`. Without it the alignment
    /// rules differ between Rust and (some) C compilers.
    pub _pad0: [u8; 2],
    pub player_id: u32,
}

/// Per-chat-event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ChatEvent {
    /// Speaker id; 0 = console / system.
    pub speaker_id: u32,
    /// Explicit padding before the pointer for predictable
    /// layout on 64-bit targets (so x86_64 + aarch64 agree).
    pub _pad0: u32,
    /// Message body, UTF-8, NUL-terminated. The HOST owns the
    /// storage; only valid for the duration of the callback.
    /// Plugins that want to keep the string MUST copy it.
    pub msg: *const c_char,
}

/// Per-tick payload.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TickEvent {
    /// Seconds since the previous tick.
    pub dt_seconds: f32,
    /// Day-time phase, in seconds since dawn. The engine wraps
    /// at the end of a 24-h cycle.
    pub day_time: f32,
    /// Current dimension name, UTF-8, NUL-terminated. HOST owns
    /// the storage; only valid for the duration of the callback.
    pub current_dim: *const c_char,
}

// ---------------------------------------------------------------
// Plugin vtable
// ---------------------------------------------------------------

/// Plugin vtable: the callbacks the host invokes on the plugin.
///
/// Every entry is `Option<...>` so a plugin only fills in what
/// it cares about. The host MUST NULL-check each slot before
/// invoking. The order of fields is locked - inserting a new
/// callback in the middle is an ABI-major change. Appending a
/// new `Option<...>` at the end is an ABI-minor change.
#[repr(C)]
#[derive(Default)]
pub struct PluginVTable {
    pub on_init:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle)>,
    pub on_shutdown:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle)>,
    pub on_tick:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle, ev: TickEvent)>,
    pub on_block_place:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle, ev: BlockEvent)>,
    pub on_block_break:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle, ev: BlockEvent)>,
    pub on_chat_msg:
        Option<unsafe extern "C-unwind" fn(handle: *mut EngineHandle, ev: ChatEvent)>,
}

impl std::fmt::Debug for PluginVTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginVTable")
            .field("on_init", &self.on_init.is_some())
            .field("on_shutdown", &self.on_shutdown.is_some())
            .field("on_tick", &self.on_tick.is_some())
            .field("on_block_place", &self.on_block_place.is_some())
            .field("on_block_break", &self.on_block_break.is_some())
            .field("on_chat_msg", &self.on_chat_msg.is_some())
            .finish()
    }
}

// ---------------------------------------------------------------
// Engine API
// ---------------------------------------------------------------

/// 3-channel RGB colour for block registration. Wrapping the
/// `[f32; 3]` in a `#[repr(C)]` struct makes the ABI fully
/// FFI-safe across all targets (raw `[f32; 3]` is FFI-clean
/// in practice but clippy's `improper_ctypes` lint flags the
/// raw array form). Plugins construct this directly.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }
    pub const fn from_array(a: [f32; 3]) -> Self {
        Self { r: a[0], g: a[1], b: a[2] }
    }
    pub const fn into_array(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

/// Engine API exposed to plugins. The host fills these slots
/// once on plugin load + the plugin keeps the pointer for the
/// life of the load.
///
/// All function pointers are `unsafe extern "C-unwind" fn` because
/// they accept raw pointers + NUL-terminated strings. The Rust
/// wrapper validates inputs before calling.
#[repr(C)]
pub struct EngineApi {
    /// Read the block id at a world voxel. Returns 0 (air) for
    /// out-of-range coords or unloaded chunks.
    pub world_get: unsafe extern "C-unwind" fn(
        handle: *mut EngineHandle,
        x: i32,
        y: i32,
        z: i32,
    ) -> u16,
    /// Write a block id at a world voxel. Out-of-range coords
    /// silently no-op. Triggers the engine's normal edit-
    /// broadcast path so MP peers see the change.
    pub world_set: unsafe extern "C-unwind" fn(
        handle: *mut EngineHandle,
        x: i32,
        y: i32,
        z: i32,
        block_id: u16,
    ),
    /// Push a chat line to the local chat log. UTF-8,
    /// NUL-terminated. NULL pointer = no-op.
    pub chat_send: unsafe extern "C-unwind" fn(
        handle: *mut EngineHandle,
        msg: *const c_char,
    ),
    /// Register (or look up) a named block. Returns the
    /// assigned block id; returns 0 if the name is already
    /// taken AND the existing block is engine-owned, or if
    /// allocation failed. Wave 7 reserves a small pool of
    /// plugin-owned block ids (1000..=1999) for the host to
    /// hand out from.
    pub block_register: unsafe extern "C-unwind" fn(
        handle: *mut EngineHandle,
        name: *const c_char,
        color_rgb: Rgb,
    ) -> u16,
    /// Log at INFO level. NULL pointer = no-op.
    pub log_info: unsafe extern "C-unwind" fn(msg: *const c_char),
    /// Log at WARN level. NULL pointer = no-op.
    pub log_warn: unsafe extern "C-unwind" fn(msg: *const c_char),
}

impl std::fmt::Debug for EngineApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineApi")
            .field("world_get", &(self.world_get as *const ()))
            .field("world_set", &(self.world_set as *const ()))
            .field("chat_send", &(self.chat_send as *const ()))
            .field("block_register", &(self.block_register as *const ()))
            .field("log_info", &(self.log_info as *const ()))
            .field("log_warn", &(self.log_warn as *const ()))
            .finish()
    }
}

// ---------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------

/// Symbol name the host looks up via `dlsym` / `GetProcAddress`.
///
/// Every plugin shared library MUST export an `extern "C"`
/// function with this exact name + the signature:
///
/// ```text
/// extern "C" fn infengine_plugin_register(
///     out_vtable: *mut PluginVTable,
///     out_api: *mut *const EngineApi,
/// ) -> PluginInfo;
/// ```
///
/// `out_vtable` points to a zero-initialised vtable; the plugin
/// fills in the slots it implements. `out_api` is an out-param
/// the host writes BEFORE calling register - the plugin can
/// read it to grab the engine API table pointer for later use.
///
/// The Rust wrapper's `declare_plugin!` macro generates this
/// entry point automatically.
pub const PLUGIN_REGISTER_SYMBOL: &[u8] = b"infengine_plugin_register";

// ---------------------------------------------------------------
// Tests: lock the binary layout.
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// If any of these sizes changes, you've made an ABI-major
    /// change - bump `ABI_MAJOR` AND update this test.
    #[test]
    fn repr_c_layout_locked() {
        // Event payloads.
        assert_eq!(std::mem::size_of::<BlockEvent>(), 20);
        assert_eq!(std::mem::size_of::<ChatEvent>(), 16);
        assert_eq!(std::mem::size_of::<TickEvent>(), 16);
        // Rgb: 3x f32.
        assert_eq!(std::mem::size_of::<Rgb>(), 12);
        // PluginInfo: 2x u32 + 2x ptr (16 on 64-bit).
        assert_eq!(
            std::mem::size_of::<PluginInfo>(),
            8 + 2 * std::mem::size_of::<*const c_char>()
        );
        // Vtable: 6 fn-ptr slots.
        assert_eq!(
            std::mem::size_of::<PluginVTable>(),
            6 * std::mem::size_of::<*const ()>()
        );
        // EngineApi: 6 fn-ptr slots.
        assert_eq!(
            std::mem::size_of::<EngineApi>(),
            6 * std::mem::size_of::<*const ()>()
        );
    }

    #[test]
    fn rgb_roundtrip_array() {
        let a = [0.25, 0.5, 0.75];
        let r = Rgb::from_array(a);
        assert_eq!(r.into_array(), a);
        assert_eq!(r.r, 0.25);
        assert_eq!(r.g, 0.5);
        assert_eq!(r.b, 0.75);
    }

    #[test]
    fn block_event_field_offsets() {
        // BlockEvent is the most ABI-sensitive payload (mixed
        // i32 / u16 / u32). Lock every field offset.
        let probe = BlockEvent {
            x: 0,
            y: 0,
            z: 0,
            block_id: 0,
            _pad0: [0; 2],
            player_id: 0,
        };
        let base = &probe as *const BlockEvent as usize;
        let off_x = (&probe.x as *const i32 as usize) - base;
        let off_y = (&probe.y as *const i32 as usize) - base;
        let off_z = (&probe.z as *const i32 as usize) - base;
        let off_blk = (&probe.block_id as *const u16 as usize) - base;
        let off_pid = (&probe.player_id as *const u32 as usize) - base;
        assert_eq!(off_x, 0);
        assert_eq!(off_y, 4);
        assert_eq!(off_z, 8);
        assert_eq!(off_blk, 12);
        assert_eq!(off_pid, 16);
    }

    #[test]
    fn abi_version_constants_match_doc() {
        // If you bump these, you bump them deliberately and
        // update the crate docs at the top of `lib.rs`.
        assert_eq!(ABI_MAJOR, 0);
        assert_eq!(ABI_MINOR, 1);
    }

    #[test]
    fn vtable_default_is_all_none() {
        let v = PluginVTable::default();
        assert!(v.on_init.is_none());
        assert!(v.on_shutdown.is_none());
        assert!(v.on_tick.is_none());
        assert!(v.on_block_place.is_none());
        assert!(v.on_block_break.is_none());
        assert!(v.on_chat_msg.is_none());
    }

    #[test]
    fn plugin_register_symbol_is_stable() {
        // Locked to keep the host's dlsym call stable.
        assert_eq!(PLUGIN_REGISTER_SYMBOL, b"infengine_plugin_register");
    }
}
