//! Plugin host orchestration.
//!
//! Loads dynamic libraries from `<exec_dir>/plugins/` (overrideable
//! via the `INFENGINE_PLUGIN_DIR` env var for tests) using
//! `libloading`. Each library must export
//! `infengine_plugin_register` per the [`infengine_sdk`] contract.
//!
//! Wave 7 ships ONE host backend: the Rust dylib host. Phase K
//! adds a `ClrHost` next to `DylibHost` behind the same
//! `PluginHost` trait so plugins written in C# load through the
//! same registry + chat commands.
//!
//! # Architecture
//!
//! ```text
//! plugin::events::*  <- engine hooks: dispatch_tick, dispatch_place, ...
//!       |
//!       v
//! plugin::PluginRegistry   <- in-memory list of loaded plugins
//!       |
//!       v
//! plugin::host::DylibHost  <- libloading::Library + vtable
//!       |
//!       v
//! infengine_sdk EngineApi  <- the C-ABI surface (api_impl.rs)
//!       |
//!       v
//! plugin::api_impl::*      <- extern "C" fns operating on EngineFacade
//! ```
//!
//! # Lifetime + safety
//!
//! `EngineHandle` is a leaked `Box<EngineFacade>`; the host
//! holds a `*mut EngineFacade` that lives for as long as plugins
//! might call back. Before every dispatch the facade's `voxel`
//! pointer is refreshed so plugin callbacks see the current
//! `Voxel` state.
//!
//! Threading: single-threaded by SDK contract. All plugin
//! callbacks run on the main thread. Plugins MAY spawn their
//! own threads internally; touching the engine from a non-
//! dispatch thread = UB (the wrapper is `!Send`).
//!
//! Panic isolation: every dispatch is wrapped in
//! `catch_unwind`. A panicking plugin gets disabled (skipped on
//! future events) until reload.
//!
//! Some items (`reload`, `find_index`, `current_dim_cstr`) land
//! their call sites in W7.5's `/plugins` chat command + W7.4's
//! sample plugin; allow `dead_code` until then so the W7.3
//! commit lands clippy-clean.
#![allow(dead_code)]

pub(crate) mod api_impl;
pub(crate) mod events;
pub(crate) mod host;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod integration_tests;

use std::path::{Path, PathBuf};

use infengine_sdk::EngineApi;

use host::{DylibHost, LoadedPlugin, PluginError};

/// Environment variable that overrides the plugin discovery
/// directory. When unset, the registry scans `<exec_dir>/plugins/`.
pub(crate) const ENV_PLUGIN_DIR: &str = "INFENGINE_PLUGIN_DIR";

/// In-memory registry of loaded plugins + the shared engine
/// facade pointer.
pub(crate) struct PluginRegistry {
    /// Currently-loaded plugins. Order = load order. A failing /
    /// panicking plugin gets `disabled = true` instead of being
    /// removed so chat commands can still see it.
    pub(crate) plugins: Vec<LoadedPlugin>,
    /// The engine API exposed to plugins. Leaked once so its
    /// address is stable for the entire process; plugins keep
    /// the pointer for as long as they're loaded.
    pub(crate) api: &'static EngineApi,
    /// The opaque facade pointer plugins hand back to API calls.
    /// Leaked once + reused; before every dispatch we update the
    /// facade's voxel-raw pointer so it points at the live state.
    pub(crate) facade_ptr: *mut api_impl::EngineFacade,
    /// Active dim name passed to on_tick. Refreshed by the
    /// caller before each dispatch.
    pub(crate) current_dim_cstr: std::ffi::CString,
}

// SAFETY: PluginRegistry holds a raw pointer to a leaked struct
// that lives for the process. The facade ptr itself is never
// dereferenced off the main thread - plugin dispatch always runs
// on the engine's main thread. Send is required because the
// registry is stored on Voxel, which is built on the main thread
// + accessed from the AppHandler callbacks.
unsafe impl Send for PluginRegistry {}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("plugins", &self.plugins.len())
            .finish()
    }
}

impl PluginRegistry {
    /// Build a fresh registry. The api + facade pointer are
    /// leaked once; the engine reuses the same registry across
    /// hot-reloads.
    pub(crate) fn new() -> Self {
        let api: &'static EngineApi =
            Box::leak(Box::new(api_impl::build_engine_api()));
        let facade_ptr = Box::into_raw(Box::new(api_impl::EngineFacade {
            voxel: std::ptr::null_mut(),
            block_register_budget_next: 1000,
        }));
        Self {
            plugins: Vec::new(),
            api,
            facade_ptr,
            current_dim_cstr: std::ffi::CString::new("overworld").unwrap(),
        }
    }

    /// Where to look for plugin libraries. Tests can override
    /// via the `INFENGINE_PLUGIN_DIR` env var.
    pub(crate) fn discover_dir() -> PathBuf {
        if let Ok(dir) = std::env::var(ENV_PLUGIN_DIR) {
            return PathBuf::from(dir);
        }
        // <exec_dir>/plugins next to the engine binary.
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("plugins"))
            .unwrap_or_else(|| PathBuf::from("plugins"))
    }

    /// Scan the discovery directory + return absolute paths to
    /// every shared-library file under it. Recognises `.dll` on
    /// Windows, `.so` on Linux, `.dylib` on macOS.
    pub(crate) fn discover_plugins(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return found;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let ext_l = ext.to_lowercase();
            let is_lib = match std::env::consts::OS {
                "windows" => ext_l == "dll",
                "macos" => ext_l == "dylib",
                _ => ext_l == "so",
            };
            if is_lib {
                found.push(p);
            }
        }
        found.sort();
        found
    }

    /// Load every shared library under the discovery dir. Errors
    /// are logged + the load is skipped; one bad plugin doesn't
    /// stop the others.
    pub(crate) fn load_all(&mut self, dir: &Path) {
        let paths = Self::discover_plugins(dir);
        for path in paths {
            match DylibHost::load(&path, self.api, self.facade_ptr) {
                Ok(p) => {
                    tracing::info!(
                        plugin = %p.name,
                        version = %p.version,
                        path = %path.display(),
                        "plugin loaded",
                    );
                    self.plugins.push(p);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        err = %e,
                        "plugin load failed",
                    );
                }
            }
        }
    }

    /// Look up a loaded plugin by name (case-insensitive).
    pub(crate) fn find_index(&self, name: &str) -> Option<usize> {
        self.plugins
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Reload (unload + re-load) the plugin at `index`.
    pub(crate) fn reload(&mut self, index: usize) -> Result<(), PluginError> {
        if index >= self.plugins.len() {
            return Err(PluginError::Other("plugin index out of range".into()));
        }
        let path = self.plugins[index].path.clone();
        // Drop the old one first (calls on_shutdown + closes lib).
        let _ = self.plugins.remove(index);
        let new_plug = DylibHost::load(&path, self.api, self.facade_ptr)?;
        self.plugins.insert(index, new_plug);
        Ok(())
    }
}

impl Drop for PluginRegistry {
    fn drop(&mut self) {
        // Unload plugins in reverse order so on_shutdown fires
        // before the lib gets closed.
        while let Some(mut p) = self.plugins.pop() {
            p.shutdown_safely(self.facade_ptr);
        }
        // Reclaim the leaked facade. The api struct stays leaked
        // - plugins MIGHT hold raw pointers to it that we can't
        // invalidate, but this only happens on engine shutdown
        // where the whole process tears down anyway.
        if !self.facade_ptr.is_null() {
            // SAFETY: facade_ptr was created via Box::into_raw
            // in `new()` and no other PluginRegistry shares it.
            unsafe {
                drop(Box::from_raw(self.facade_ptr));
            }
        }
    }
}
