//! Rust-idiomatic wrapper over the [`infengine_sdk`] C-ABI core.
//!
//! Plugins implement the safe [`Plugin`] trait + use the
//! [`declare_plugin!`] macro to generate the C-ABI entry point.
//! The macro:
//! 1. Builds a static instance of the plugin type (default-
//!    constructed).
//! 2. Wires every Plugin trait method into a thin
//!    `extern "C"` shim stored in the host-provided
//!    [`raw::PluginVTable`].
//! 3. Stores the host's [`raw::EngineApi`] pointer the host
//!    handed us via the `out_api` out-param so [`Engine`]
//!    methods can dispatch later.
//! 4. Returns a [`raw::PluginInfo`] populated from the trait's
//!    `name()` / `version()` plus the current `ABI_MAJOR` /
//!    `ABI_MINOR` constants.
//!
//! # Example
//!
//! ```ignore
//! use infengine_sdk_rs::{Engine, Plugin, declare_plugin};
//!
//! #[derive(Default)]
//! struct MyPlug;
//!
//! impl Plugin for MyPlug {
//!     fn name() -> &'static str { "my_plug" }
//!     fn version() -> &'static str { "0.1.0" }
//!     fn on_init(&mut self, engine: &mut Engine) {
//!         engine.log_info("hello");
//!     }
//! }
//!
//! declare_plugin!(MyPlug);
//! ```

#![allow(clippy::missing_safety_doc)]

use std::ffi::{CString, NulError};
use std::fmt;

pub use infengine_sdk::{ABI_MAJOR, ABI_MINOR};

/// Re-export of the raw C-ABI surface for advanced users + the
/// `declare_plugin!` macro. Most plugin authors should never
/// need this module - the safe [`Plugin`] + [`Engine`] APIs
/// above cover the common path.
pub mod raw {
    pub use infengine_sdk::*;
}

// ---------------------------------------------------------------
// Errors
// ---------------------------------------------------------------

/// Errors surfaced by the [`Engine`] wrapper. The C-ABI side has
/// no Result type so each [`Engine`] method converts a host
/// failure into one of these.
#[derive(Debug)]
pub enum EngineError {
    /// A string the plugin passed contained an interior NUL
    /// byte. UTF-8 can't carry NUL through a C string.
    NulInString(NulError),
    /// The host refused a `block_register` call (duplicate
    /// name with an engine-owned block, or out-of-budget).
    BlockRegisterRefused {
        /// The name the plugin attempted to register.
        name: String,
    },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NulInString(e) => {
                write!(f, "string contained an interior NUL: {e}")
            }
            EngineError::BlockRegisterRefused { name } => {
                write!(f, "host refused to register block {name:?}")
            }
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::NulInString(e) => Some(e),
            _ => None,
        }
    }
}

impl From<NulError> for EngineError {
    fn from(e: NulError) -> Self {
        EngineError::NulInString(e)
    }
}

// ---------------------------------------------------------------
// Plugin trait
// ---------------------------------------------------------------

/// Idiomatic plugin trait. Plugin types must be `Default` (the
/// generated entry point default-constructs a static instance
/// at first call).
///
/// Every method except `name` + `version` has an empty default
/// impl so a plugin only writes the callbacks it cares about.
/// Callbacks are invoked on the engine's main thread.
pub trait Plugin: Default + Send + 'static {
    /// Plugin name. Surfaced by `/plugins` + log lines. Should
    /// be unique - the host uses it as the disable / reload
    /// key.
    fn name() -> &'static str;
    /// Plugin version string. Free-form; semver recommended.
    fn version() -> &'static str;

    fn on_init(&mut self, _engine: &mut Engine) {}
    fn on_shutdown(&mut self, _engine: &mut Engine) {}
    fn on_tick(
        &mut self,
        _engine: &mut Engine,
        _dt: f32,
        _day_time: f32,
        _current_dim: &str,
    ) {
    }
    fn on_block_place(
        &mut self,
        _engine: &mut Engine,
        _pos: [i32; 3],
        _block: u16,
        _player: u32,
    ) {
    }
    fn on_block_break(
        &mut self,
        _engine: &mut Engine,
        _pos: [i32; 3],
        _block: u16,
        _player: u32,
    ) {
    }
    fn on_chat_msg(
        &mut self,
        _engine: &mut Engine,
        _speaker: u32,
        _msg: &str,
    ) {
    }
}

// ---------------------------------------------------------------
// Engine handle
// ---------------------------------------------------------------

/// Safe wrapper over `*mut EngineHandle + *const EngineApi`.
///
/// Methods translate Rust strings into NUL-terminated C strings
/// (allocating a `CString` per call), validate API pointers,
/// and return `Result` where the host can refuse the call.
///
/// The wrapper is `!Send + !Sync` - plugin callbacks fire on
/// the engine's main thread, and crossing into another thread
/// with the raw handle is UB per the SDK contract.
pub struct Engine {
    handle: *mut raw::EngineHandle,
    api: *const raw::EngineApi,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("handle", &self.handle)
            .field("api", &self.api)
            .finish()
    }
}

impl Engine {
    /// Construct from raw FFI pointers. The dispatch shim
    /// emitted by `declare_plugin!` calls this on every event.
    ///
    /// # Safety
    /// `handle` and `api` must be valid for the duration of
    /// the callback, and the caller must own thread-affinity
    /// to the engine's main thread.
    pub unsafe fn from_raw(
        handle: *mut raw::EngineHandle,
        api: *const raw::EngineApi,
    ) -> Self {
        Self { handle, api }
    }

    /// Block id at `(x, y, z)`. Returns 0 (air) for out-of-
    /// range or unloaded coords. NULL api = 0.
    pub fn world_get(&self, x: i32, y: i32, z: i32) -> u16 {
        if self.api.is_null() {
            return 0;
        }
        // SAFETY: `self.api` is non-null (checked above) +
        // valid for the callback's duration per the SDK
        // contract. `world_get` is an unsafe extern "C" fn
        // that just reads the world; the handle pointer is
        // opaque and dereferenced only by the host.
        unsafe { ((*self.api).world_get)(self.handle, x, y, z) }
    }

    /// Write block id at `(x, y, z)`. Out-of-range silently
    /// no-ops. NULL api = no-op.
    pub fn world_set(&mut self, x: i32, y: i32, z: i32, id: u16) {
        if self.api.is_null() {
            return;
        }
        // SAFETY: `self.api` is non-null + valid for the
        // callback. The host's world_set accepts any (x, y, z)
        // + clamps internally; the handle is opaque to us.
        unsafe { ((*self.api).world_set)(self.handle, x, y, z, id) }
    }

    /// Push a chat line to the local chat log. Empty messages
    /// are accepted (push an empty line); interior-NUL strings
    /// are rejected.
    pub fn chat_send(&mut self, msg: &str) -> Result<(), EngineError> {
        if self.api.is_null() {
            return Ok(());
        }
        let cstr = CString::new(msg)?;
        // SAFETY: `cstr` lives until end of scope; the host
        // copies the string before returning. NUL-termination
        // is guaranteed by CString.
        unsafe { ((*self.api).chat_send)(self.handle, cstr.as_ptr()) }
        Ok(())
    }

    /// Register a plugin-owned block. Returns the assigned
    /// block id, or `Err(BlockRegisterRefused)` on duplicate /
    /// budget exhaustion. `color` is `[R, G, B]` in 0..=1.
    pub fn block_register(
        &mut self,
        name: &str,
        color: [f32; 3],
    ) -> Result<u16, EngineError> {
        if self.api.is_null() {
            return Err(EngineError::BlockRegisterRefused {
                name: name.to_string(),
            });
        }
        let cstr = CString::new(name)?;
        // SAFETY: `cstr` stays alive for the call. The host
        // copies the name string before returning.
        let id = unsafe {
            ((*self.api).block_register)(
                self.handle,
                cstr.as_ptr(),
                raw::Rgb::from_array(color),
            )
        };
        if id == 0 {
            return Err(EngineError::BlockRegisterRefused {
                name: name.to_string(),
            });
        }
        Ok(id)
    }

    /// Log at INFO. Empty messages and interior-NUL strings
    /// are silently dropped (logging never errors).
    pub fn log_info(&self, msg: &str) {
        if self.api.is_null() {
            return;
        }
        let Ok(cstr) = CString::new(msg) else { return };
        // SAFETY: `cstr` lives until end of scope. The host
        // copies the message into its tracing pipeline.
        unsafe { ((*self.api).log_info)(cstr.as_ptr()) }
    }

    /// Log at WARN. Same fail-quiet semantics as `log_info`.
    pub fn log_warn(&self, msg: &str) {
        if self.api.is_null() {
            return;
        }
        let Ok(cstr) = CString::new(msg) else { return };
        // SAFETY: same as log_info.
        unsafe { ((*self.api).log_warn)(cstr.as_ptr()) }
    }
}

// ---------------------------------------------------------------
// String marshalling helpers exposed for `declare_plugin!`
// ---------------------------------------------------------------

/// Convert a host-provided `*const c_char` (UTF-8, NUL-
/// terminated) into a borrowed `&str`. Returns `""` for NULL
/// or non-UTF-8 input - the wrapper never panics on bad host
/// input.
///
/// # Safety
/// `ptr` must be NULL or point at a valid NUL-terminated
/// UTF-8 buffer that lives for the duration of the returned
/// borrow.
pub unsafe fn cstr_to_str<'a>(ptr: *const std::os::raw::c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: caller-asserted NUL-terminated string. We don't
    // panic on invalid UTF-8; we fall back to empty string.
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    cstr.to_str().unwrap_or("")
}

// ---------------------------------------------------------------
// declare_plugin! macro
// ---------------------------------------------------------------

/// Build the C-ABI entry point + vtable shims from a `Plugin`
/// impl. Invoke once per plugin cdylib at the crate root.
///
/// The macro:
/// - Stores a `static` `OnceLock<Mutex<$plugin_ty>>` for the
///   plugin state. Single-threaded by SDK contract so the
///   Mutex never contends; using one keeps the macro generic
///   over plugin types that are `!Sync`.
/// - Stores a `static` `AtomicPtr<EngineApi>` for the engine
///   API pointer.
/// - Emits `extern "C" fn infengine_plugin_register` that
///   captures the api pointer + fills in the vtable.
/// - Emits an `extern "C" fn` shim per Plugin method that
///   re-creates an `Engine`, locks the state, dispatches.
///
/// String literals for `name` / `version` go through a
/// trailing NUL byte trick: `concat!(STR, "\0")` so we can
/// return their `.as_ptr()` directly as a stable C string.
#[macro_export]
macro_rules! declare_plugin {
    ($plugin_ty:ty) => {
        // Plugin state. OnceLock<Mutex<_>> survives across
        // dispatch calls; Mutex never contends because the
        // SDK promises single-threaded dispatch.
        static __INFENGINE_PLUGIN_STATE: ::std::sync::OnceLock<
            ::std::sync::Mutex<$plugin_ty>,
        > = ::std::sync::OnceLock::new();

        // Engine API pointer the host wrote into us at
        // register time. We read it on every dispatch shim.
        static __INFENGINE_ENGINE_API: ::std::sync::atomic::AtomicPtr<
            $crate::raw::EngineApi,
        > = ::std::sync::atomic::AtomicPtr::new(::std::ptr::null_mut());

        fn __infengine_state() -> &'static ::std::sync::Mutex<$plugin_ty> {
            __INFENGINE_PLUGIN_STATE
                .get_or_init(|| ::std::sync::Mutex::new(<$plugin_ty as ::std::default::Default>::default()))
        }

        fn __infengine_engine(
            handle: *mut $crate::raw::EngineHandle,
        ) -> $crate::Engine {
            let api = __INFENGINE_ENGINE_API
                .load(::std::sync::atomic::Ordering::Acquire);
            // SAFETY: host guarantees `handle` valid for the
            // callback + `api` was stored at register time.
            unsafe { $crate::Engine::from_raw(handle, api) }
        }

        unsafe extern "C" fn __infengine_on_init(
            handle: *mut $crate::raw::EngineHandle,
        ) {
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_init(&mut p, &mut eng);
            }
        }

        unsafe extern "C" fn __infengine_on_shutdown(
            handle: *mut $crate::raw::EngineHandle,
        ) {
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_shutdown(&mut p, &mut eng);
            }
        }

        unsafe extern "C" fn __infengine_on_tick(
            handle: *mut $crate::raw::EngineHandle,
            ev: $crate::raw::TickEvent,
        ) {
            // SAFETY: host guarantees `ev.current_dim` valid
            // for the duration of the callback.
            let dim = unsafe { $crate::cstr_to_str(ev.current_dim) };
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_tick(
                    &mut p,
                    &mut eng,
                    ev.dt_seconds,
                    ev.day_time,
                    dim,
                );
            }
        }

        unsafe extern "C" fn __infengine_on_block_place(
            handle: *mut $crate::raw::EngineHandle,
            ev: $crate::raw::BlockEvent,
        ) {
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_block_place(
                    &mut p,
                    &mut eng,
                    [ev.x, ev.y, ev.z],
                    ev.block_id,
                    ev.player_id,
                );
            }
        }

        unsafe extern "C" fn __infengine_on_block_break(
            handle: *mut $crate::raw::EngineHandle,
            ev: $crate::raw::BlockEvent,
        ) {
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_block_break(
                    &mut p,
                    &mut eng,
                    [ev.x, ev.y, ev.z],
                    ev.block_id,
                    ev.player_id,
                );
            }
        }

        unsafe extern "C" fn __infengine_on_chat_msg(
            handle: *mut $crate::raw::EngineHandle,
            ev: $crate::raw::ChatEvent,
        ) {
            // SAFETY: host guarantees `ev.msg` valid for the
            // duration of the callback.
            let msg = unsafe { $crate::cstr_to_str(ev.msg) };
            if let Ok(mut p) = __infengine_state().lock() {
                let mut eng = __infengine_engine(handle);
                <$plugin_ty as $crate::Plugin>::on_chat_msg(
                    &mut p,
                    &mut eng,
                    ev.speaker_id,
                    msg,
                );
            }
        }

        /// C-ABI entry point the host's `dlsym` /
        /// `GetProcAddress` looks up by name.
        ///
        /// # Safety
        /// `out_vtable` must be a non-NULL, writable pointer
        /// to a zero-initialised `PluginVTable`. `out_api`
        /// must be a non-NULL pointer to a slot the host
        /// already wrote the api pointer into.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn infengine_plugin_register(
            out_vtable: *mut $crate::raw::PluginVTable,
            out_api: *mut *const $crate::raw::EngineApi,
        ) -> $crate::raw::PluginInfo {
            // Read the host-written engine API pointer and
            // store it in our static so dispatch shims can
            // look it up later without the host having to
            // pass it through every event.
            if !out_api.is_null() {
                // SAFETY: caller promises `out_api` is a
                // valid, writable slot the host wrote into.
                let api_ptr = unsafe { *out_api };
                __INFENGINE_ENGINE_API.store(
                    api_ptr as *mut $crate::raw::EngineApi,
                    ::std::sync::atomic::Ordering::Release,
                );
            }
            if !out_vtable.is_null() {
                // SAFETY: caller promises `out_vtable` is a
                // valid, writable, zero-init slot.
                unsafe {
                    (*out_vtable).on_init = Some(__infengine_on_init);
                    (*out_vtable).on_shutdown = Some(__infengine_on_shutdown);
                    (*out_vtable).on_tick = Some(__infengine_on_tick);
                    (*out_vtable).on_block_place =
                        Some(__infengine_on_block_place);
                    (*out_vtable).on_block_break =
                        Some(__infengine_on_block_break);
                    (*out_vtable).on_chat_msg = Some(__infengine_on_chat_msg);
                }
            }
            // Leak NUL-terminated CStrings for the name +
            // version once. The plugin dylib stays loaded for
            // the engine's life so a one-shot leak is fine -
            // and keeps PluginInfo's pointers stable across
            // every dispatch.
            static NAME_CSTR: ::std::sync::OnceLock<::std::ffi::CString> =
                ::std::sync::OnceLock::new();
            static VERSION_CSTR: ::std::sync::OnceLock<::std::ffi::CString> =
                ::std::sync::OnceLock::new();
            let name = NAME_CSTR.get_or_init(|| {
                ::std::ffi::CString::new(<$plugin_ty as $crate::Plugin>::name())
                    .unwrap_or_else(|_| ::std::ffi::CString::new("anonymous").unwrap())
            });
            let version = VERSION_CSTR.get_or_init(|| {
                ::std::ffi::CString::new(<$plugin_ty as $crate::Plugin>::version())
                    .unwrap_or_else(|_| ::std::ffi::CString::new("0.0.0").unwrap())
            });
            $crate::raw::PluginInfo {
                abi_major: $crate::ABI_MAJOR,
                abi_minor: $crate::ABI_MINOR,
                name: name.as_ptr(),
                version: version.as_ptr(),
            }
        }
    };
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::c_char;

    // A capturing test API. The unsafe extern "C" fns can't
    // close over state, so we route every call through a
    // thread-local sink + check the sink after the call.
    thread_local! {
        static LAST_LOG: std::cell::RefCell<Vec<String>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static LAST_BLOCK_SET: std::cell::RefCell<Vec<(i32, i32, i32, u16)>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static REGISTERED_BLOCKS: std::cell::RefCell<Vec<String>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn test_world_get(
        _h: *mut raw::EngineHandle,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> u16 {
        42
    }

    unsafe extern "C" fn test_world_set(
        _h: *mut raw::EngineHandle,
        x: i32,
        y: i32,
        z: i32,
        id: u16,
    ) {
        LAST_BLOCK_SET.with(|s| s.borrow_mut().push((x, y, z, id)));
    }

    unsafe extern "C" fn test_chat_send(
        _h: *mut raw::EngineHandle,
        msg: *const c_char,
    ) {
        // SAFETY: test invariant - msg is always a valid CString.
        let s = unsafe { std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned() };
        LAST_LOG.with(|l| l.borrow_mut().push(format!("chat:{s}")));
    }

    unsafe extern "C" fn test_block_register(
        _h: *mut raw::EngineHandle,
        name: *const c_char,
        _color: raw::Rgb,
    ) -> u16 {
        // SAFETY: test invariant.
        let n =
            unsafe { std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned() };
        let dup = REGISTERED_BLOCKS.with(|r| r.borrow().iter().any(|x| x == &n));
        if dup {
            return 0;
        }
        REGISTERED_BLOCKS.with(|r| r.borrow_mut().push(n));
        1000
    }

    unsafe extern "C" fn test_log_info(msg: *const c_char) {
        // SAFETY: test invariant.
        let s = unsafe { std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned() };
        LAST_LOG.with(|l| l.borrow_mut().push(format!("info:{s}")));
    }

    unsafe extern "C" fn test_log_warn(msg: *const c_char) {
        // SAFETY: test invariant.
        let s = unsafe { std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned() };
        LAST_LOG.with(|l| l.borrow_mut().push(format!("warn:{s}")));
    }

    fn test_api() -> raw::EngineApi {
        raw::EngineApi {
            world_get: test_world_get,
            world_set: test_world_set,
            chat_send: test_chat_send,
            block_register: test_block_register,
            log_info: test_log_info,
            log_warn: test_log_warn,
        }
    }

    fn reset_sinks() {
        LAST_LOG.with(|l| l.borrow_mut().clear());
        LAST_BLOCK_SET.with(|s| s.borrow_mut().clear());
        REGISTERED_BLOCKS.with(|r| r.borrow_mut().clear());
    }

    fn make_engine(api: &raw::EngineApi) -> Engine {
        // SAFETY: test owns api; handle is null which is fine
        // because the test extern "C" fns ignore it.
        unsafe { Engine::from_raw(std::ptr::null_mut(), api as *const _) }
    }

    #[test]
    fn engine_world_get_dispatches() {
        let api = test_api();
        let eng = make_engine(&api);
        assert_eq!(eng.world_get(1, 2, 3), 42);
    }

    #[test]
    fn engine_world_set_dispatches() {
        reset_sinks();
        let api = test_api();
        let mut eng = make_engine(&api);
        eng.world_set(7, 8, 9, 11);
        let v = LAST_BLOCK_SET.with(|s| s.borrow().clone());
        assert_eq!(v, vec![(7, 8, 9, 11)]);
    }

    #[test]
    fn engine_string_marshalling_empty_ok() {
        reset_sinks();
        let api = test_api();
        let mut eng = make_engine(&api);
        eng.chat_send("").unwrap();
        eng.log_info("");
        eng.log_warn("");
        let v = LAST_LOG.with(|l| l.borrow().clone());
        assert_eq!(v, vec!["chat:", "info:", "warn:"]);
    }

    #[test]
    fn engine_string_marshalling_utf8_ok() {
        reset_sinks();
        let api = test_api();
        let mut eng = make_engine(&api);
        // Diacritics flow through (UTF-8 has no NULs in these).
        eng.chat_send("Pribezte si pro radost").unwrap();
        let v = LAST_LOG.with(|l| l.borrow().clone());
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("Pribezte"));
    }

    #[test]
    fn engine_string_marshalling_rejects_interior_nul() {
        let api = test_api();
        let mut eng = make_engine(&api);
        let err = eng.chat_send("bad\0msg").unwrap_err();
        assert!(matches!(err, EngineError::NulInString(_)));
    }

    #[test]
    fn engine_log_silently_drops_interior_nul() {
        reset_sinks();
        let api = test_api();
        let eng = make_engine(&api);
        // log_* never errors - bad strings just don't log.
        eng.log_info("bad\0msg");
        eng.log_warn("bad\0msg");
        let v = LAST_LOG.with(|l| l.borrow().clone());
        assert!(v.is_empty());
    }

    #[test]
    fn engine_block_register_returns_id() {
        reset_sinks();
        let api = test_api();
        let mut eng = make_engine(&api);
        let id = eng.block_register("fancy", [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(id, 1000);
    }

    #[test]
    fn engine_block_register_fails_on_duplicate() {
        reset_sinks();
        let api = test_api();
        let mut eng = make_engine(&api);
        let _ = eng.block_register("dup", [0.5; 3]).unwrap();
        let err = eng.block_register("dup", [0.5; 3]).unwrap_err();
        assert!(matches!(err, EngineError::BlockRegisterRefused { .. }));
    }

    #[test]
    fn engine_block_register_rejects_interior_nul() {
        let api = test_api();
        let mut eng = make_engine(&api);
        let err = eng.block_register("bad\0name", [0.5; 3]).unwrap_err();
        assert!(matches!(err, EngineError::NulInString(_)));
    }

    #[test]
    fn engine_null_api_is_safe() {
        // SAFETY: testing the null-api guard path.
        let eng = unsafe {
            Engine::from_raw(std::ptr::null_mut(), std::ptr::null())
        };
        assert_eq!(eng.world_get(0, 0, 0), 0);
        eng.log_info("ignored");
        eng.log_warn("ignored");
    }

    /// Walks through the cstr_to_str helper a few ways.
    #[test]
    fn cstr_to_str_handles_null() {
        // SAFETY: the helper explicitly handles NULL.
        let s = unsafe { cstr_to_str(std::ptr::null()) };
        assert_eq!(s, "");
    }

    #[test]
    fn cstr_to_str_roundtrip_utf8() {
        let c = CString::new("hello mundo").unwrap();
        // SAFETY: c stays alive for the borrow.
        let s = unsafe { cstr_to_str(c.as_ptr()) };
        assert_eq!(s, "hello mundo");
    }

    #[test]
    fn engine_error_has_block_name() {
        let e = EngineError::BlockRegisterRefused {
            name: "stone".into(),
        };
        let txt = format!("{e}");
        assert!(txt.contains("stone"));
    }

    // Default-no-op plugin: trait compiles with empty body.
    #[derive(Default)]
    struct EmptyPlug;
    impl Plugin for EmptyPlug {
        fn name() -> &'static str {
            "empty"
        }
        fn version() -> &'static str {
            "0.0.0"
        }
    }

    #[test]
    fn plugin_trait_default_no_op() {
        let api = test_api();
        let mut eng = make_engine(&api);
        let mut p = EmptyPlug;
        // Every callback exists + no-ops.
        p.on_init(&mut eng);
        p.on_shutdown(&mut eng);
        p.on_tick(&mut eng, 0.016, 0.5, "overworld");
        p.on_block_place(&mut eng, [0, 0, 0], 1, 1);
        p.on_block_break(&mut eng, [0, 0, 0], 1, 1);
        p.on_chat_msg(&mut eng, 1, "hello");
    }

    #[test]
    fn plugin_name_version_constants() {
        assert_eq!(EmptyPlug::name(), "empty");
        assert_eq!(EmptyPlug::version(), "0.0.0");
    }
}
