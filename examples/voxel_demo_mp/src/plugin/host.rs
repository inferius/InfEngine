//! `DylibHost`: load + drive a single Rust dylib plugin.
//!
//! Reads `infengine_plugin_register` via `libloading::Library`,
//! retrieves the [`PluginVTable`] + [`PluginInfo`] the plugin
//! returns, checks the ABI version, and stores everything in a
//! `LoadedPlugin`. Dispatch helpers wrap each callback in
//! `catch_unwind` so a panicking plugin disables itself instead
//! of taking down the engine.
//!
//! Several items here (PluginError variants, LoadedPlugin fields
//! like `disable_reason`) get wired into the `/plugins` chat
//! command in W7.5; allow `dead_code` on the module so the
//! W7.3 commit lands clippy-clean.
#![allow(dead_code)]

use std::ffi::{CStr, CString};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use infengine_sdk::{
    ABI_MAJOR, ABI_MINOR, BlockEvent, ChatEvent, EngineApi,
    EngineHandle, PLUGIN_REGISTER_SYMBOL, PluginInfo, PluginVTable,
    TickEvent,
};

use super::api_impl::EngineFacade;

/// Signature of the symbol every plugin .dll must export.
/// Matches the contract documented in `infengine_sdk::PLUGIN_REGISTER_SYMBOL`.
pub(crate) type PluginRegisterFn = unsafe extern "C-unwind" fn(
    out_vtable: *mut PluginVTable,
    out_api: *mut *const EngineApi,
) -> PluginInfo;

/// Plugin load / dispatch errors. Surfaced through tracing and
/// the `/plugins` chat command.
#[derive(Debug)]
pub(crate) enum PluginError {
    /// `libloading::Library::new(path)` failed (file missing,
    /// wrong arch, dependency missing, ...).
    LibLoad(String),
    /// The library doesn't export `infengine_plugin_register`.
    MissingSymbol,
    /// `abi_major` doesn't match the host's compiled value.
    AbiMajorMismatch { plugin: u32, host: u32 },
    /// `abi_minor` exceeds the host's compiled value (plugin
    /// uses a vtable slot we don't know about).
    AbiMinorAhead { plugin: u32, host: u32 },
    /// The plugin returned an empty or NUL-containing name.
    BadName,
    /// Catch-all for free-form messages.
    Other(String),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginError::LibLoad(msg) => write!(f, "load failed: {msg}"),
            PluginError::MissingSymbol => write!(
                f,
                "missing exported symbol `infengine_plugin_register`"
            ),
            PluginError::AbiMajorMismatch { plugin, host } => write!(
                f,
                "ABI major mismatch (plugin={plugin}, host={host})"
            ),
            PluginError::AbiMinorAhead { plugin, host } => write!(
                f,
                "plugin ABI minor {plugin} > host {host}"
            ),
            PluginError::BadName => write!(f, "plugin returned empty / invalid name"),
            PluginError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for PluginError {}

/// One loaded plugin record. Owns the `libloading::Library` so
/// the dylib stays mapped for as long as we hold the vtable
/// pointers, plus copies of the name + version strings.
pub(crate) struct LoadedPlugin {
    /// The path the plugin was loaded from. Kept so
    /// `/plugins reload` can re-run the load.
    pub(crate) path: PathBuf,
    /// Plugin name (Rust-side String copy of `PluginInfo.name`).
    pub(crate) name: String,
    /// Plugin version (Rust-side String copy of `PluginInfo.version`).
    pub(crate) version: String,
    /// True when a previous callback panicked / errored - skip
    /// future dispatches until reload.
    pub(crate) disabled: bool,
    /// Optional reason a plugin is disabled. Shown by `/plugins`.
    pub(crate) disable_reason: Option<String>,
    /// Vtable handed back by the plugin. Function pointers
    /// inside it dangle the moment `lib` drops, so the lib
    /// MUST outlive every dispatch.
    pub(crate) vtable: PluginVTable,
    /// The dylib handle. Dropping this unmaps the library +
    /// invalidates every fn pointer in `vtable`. Keep last.
    pub(crate) lib: libloading::Library,
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("disabled", &self.disabled)
            .field("path", &self.path)
            .finish()
    }
}

impl LoadedPlugin {
    /// Mark the plugin disabled with a reason.
    pub(crate) fn disable(&mut self, reason: impl Into<String>) {
        self.disabled = true;
        self.disable_reason = Some(reason.into());
    }

    /// Fire `on_shutdown` once (if implemented) inside a panic
    /// catcher. Used on registry drop + manual reload.
    pub(crate) fn shutdown_safely(&mut self, facade_ptr: *mut EngineFacade) {
        let Some(cb) = self.vtable.on_shutdown else {
            return;
        };
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: facade_ptr lives for the registry's
            // lifetime; cb is a valid fn ptr while `self.lib`
            // is alive (we are still holding it).
            unsafe { cb(facade_ptr as *mut EngineHandle) };
        }));
    }
}

/// Dylib host: loads a plugin from a path + builds a
/// `LoadedPlugin`. Stateless - one host = one helper module.
pub(crate) struct DylibHost;

impl DylibHost {
    /// Load + initialise a plugin from `path`. Calls
    /// `on_init` as the final step (with panic catching).
    pub(crate) fn load(
        path: &Path,
        api: &'static EngineApi,
        facade_ptr: *mut EngineFacade,
    ) -> Result<LoadedPlugin, PluginError> {
        // SAFETY: libloading::Library::new is unsafe because
        // arbitrary dylib initialisers run on load. We trust
        // the user's plugin directory.
        let lib = unsafe {
            libloading::Library::new(path)
                .map_err(|e| PluginError::LibLoad(format!("{e}")))?
        };
        // SAFETY: looking up an exported symbol; the lifetime
        // is tied to the library, which we keep around in the
        // returned LoadedPlugin.
        let register: libloading::Symbol<PluginRegisterFn> = unsafe {
            lib.get(PLUGIN_REGISTER_SYMBOL)
                .map_err(|_| PluginError::MissingSymbol)?
        };

        let mut vtable = PluginVTable::default();
        let mut api_slot: *const EngineApi = api as *const EngineApi;

        // SAFETY: passing valid out-params; the plugin's
        // register fn fills them in per the SDK contract.
        let info = unsafe {
            register(
                &mut vtable as *mut PluginVTable,
                &mut api_slot as *mut *const EngineApi,
            )
        };

        if info.abi_major != ABI_MAJOR {
            return Err(PluginError::AbiMajorMismatch {
                plugin: info.abi_major,
                host: ABI_MAJOR,
            });
        }
        if info.abi_minor > ABI_MINOR {
            return Err(PluginError::AbiMinorAhead {
                plugin: info.abi_minor,
                host: ABI_MINOR,
            });
        }

        if info.name.is_null() {
            return Err(PluginError::BadName);
        }
        // SAFETY: SDK contract says name + version are valid
        // NUL-terminated UTF-8 owned by the plugin for the
        // life of the load. We copy them into owned Strings
        // immediately so we don't rely on the raw ptrs.
        let name = unsafe { CStr::from_ptr(info.name) }
            .to_str()
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            return Err(PluginError::BadName);
        }
        let version = if info.version.is_null() {
            "0.0.0".to_string()
        } else {
            // SAFETY: same SDK contract as above.
            unsafe { CStr::from_ptr(info.version) }
                .to_str()
                .unwrap_or("")
                .to_string()
        };

        let mut plugin = LoadedPlugin {
            path: path.to_path_buf(),
            name,
            version,
            disabled: false,
            disable_reason: None,
            vtable,
            lib,
        };

        // Fire on_init under a panic catcher.
        if let Some(init) = plugin.vtable.on_init {
            let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: facade_ptr is valid for the
                // registry's lifetime; init is a valid fn
                // ptr while lib is held.
                unsafe { init(facade_ptr as *mut EngineHandle) };
            }));
            if res.is_err() {
                plugin.disable("panic in on_init");
                tracing::warn!(
                    plugin = %plugin.name,
                    "plugin disabled - on_init panicked"
                );
            }
        }

        Ok(plugin)
    }
}

// Dispatch helpers shared by `events::*`. Each one runs the
// plugin's vtable slot under a `catch_unwind` shield and
// disables the plugin on panic.

pub(crate) fn dispatch_tick(
    plug: &mut LoadedPlugin,
    facade_ptr: *mut EngineFacade,
    dt: f32,
    day_time: f32,
    current_dim: &CString,
) {
    if plug.disabled {
        return;
    }
    let Some(cb) = plug.vtable.on_tick else { return };
    let ev = TickEvent {
        dt_seconds: dt,
        day_time,
        current_dim: current_dim.as_ptr(),
    };
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: facade_ptr lives for the registry lifetime;
        // current_dim outlives the call.
        unsafe { cb(facade_ptr as *mut EngineHandle, ev) };
    }));
    if res.is_err() {
        plug.disable("panic in on_tick");
        tracing::warn!(plugin = %plug.name, "plugin disabled - on_tick panicked");
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_block_event(
    plug: &mut LoadedPlugin,
    facade_ptr: *mut EngineFacade,
    place: bool,
    x: i32,
    y: i32,
    z: i32,
    block_id: u16,
    player_id: u32,
) {
    if plug.disabled {
        return;
    }
    let cb = if place {
        plug.vtable.on_block_place
    } else {
        plug.vtable.on_block_break
    };
    let Some(cb) = cb else { return };
    let ev = BlockEvent {
        x,
        y,
        z,
        block_id,
        _pad0: [0; 2],
        player_id,
    };
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: see dispatch_tick.
        unsafe { cb(facade_ptr as *mut EngineHandle, ev) };
    }));
    if res.is_err() {
        let kind = if place { "on_block_place" } else { "on_block_break" };
        plug.disable(format!("panic in {kind}"));
        tracing::warn!(plugin = %plug.name, kind, "plugin disabled - panic");
    }
}

pub(crate) fn dispatch_chat(
    plug: &mut LoadedPlugin,
    facade_ptr: *mut EngineFacade,
    speaker: u32,
    msg: &CStr,
) {
    if plug.disabled {
        return;
    }
    let Some(cb) = plug.vtable.on_chat_msg else { return };
    let ev = ChatEvent {
        speaker_id: speaker,
        _pad0: 0,
        msg: msg.as_ptr(),
    };
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: see dispatch_tick.
        unsafe { cb(facade_ptr as *mut EngineHandle, ev) };
    }));
    if res.is_err() {
        plug.disable("panic in on_chat_msg");
        tracing::warn!(plugin = %plug.name, "plugin disabled - on_chat_msg panicked");
    }
}
