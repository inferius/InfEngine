//! C# scripting host backed by the .NET CoreCLR runtime.
//!
//! Phase 8.1 ships the bootstrap: locate and load `hostfxr` via
//! `netcorehost`, initialize a runtime against a `runtimeconfig.json`,
//! and acquire a delegate loader. Subsequent phases will use the loader
//! to resolve C# entry points and wire engine callbacks.
//!
//! ## Requirements
//!
//! - .NET 10 SDK installed (any compatible TFM works; the demo uses
//!   `net10.0`).
//! - A `runtimeconfig.json` describing which runtime version to load.
//!   The provided `scripts/InfEngine.Scripts` project emits one next
//!   to its assembly during `dotnet publish`.

// FFI boundary requires `unsafe`. Other crates retain `forbid(unsafe_code)`.
#![deny(unsafe_op_in_unsafe_fn)]

use std::path::{Path, PathBuf};

use netcorehost::hostfxr::{
    AssemblyDelegateLoader, GetManagedFunctionError, Hostfxr, HostfxrContext,
    InitializedForRuntimeConfig, ManagedFunction,
};
use netcorehost::pdcstring::PdCString;
use tracing::info;

/// Re-export: a callable `extern "system"` function pointer with no
/// arguments returning `()`.
pub type FnVoid = ManagedFunction<extern "system" fn()>;
/// Re-export: a callable `extern "system"` function pointer with no
/// arguments returning `i32`.
pub type FnInt = ManagedFunction<extern "system" fn() -> i32>;
/// Re-export: a callable function used to register the engine API
/// struct with the managed side.
pub type FnRegisterApi = ManagedFunction<extern "system" fn(*const EngineApi)>;
/// Re-export: per-frame tick callback. The host passes the frame's
/// delta-time in seconds.
pub type FnTick = ManagedFunction<extern "system" fn(f32)>;
/// Re-export: load a game assembly into the collectible ALC. Returns
/// `1` on success, `0` on failure (the managed side logs the reason).
pub type FnLoadGame = ManagedFunction<extern "system" fn(*const u8, usize) -> u8>;

/// Engine functions exposed to C# code. Layout is FFI-stable so that
/// the same `#[repr(C)]` struct can be marshalled on the managed side.
///
/// Every function pointer uses the C calling convention; on the C#
/// side they are declared `delegate* unmanaged[Cdecl]<...>`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EngineApi {
    pub log_info: extern "C" fn(msg: *const u8, len: usize),
    pub time_secs: extern "C" fn() -> f32,
    /// Read the current player position. Writes `(x, y)` into the caller's outputs.
    pub get_player_pos: extern "C" fn(out_x: *mut f32, out_y: *mut f32),
    /// Set the player position for the next renderer pass.
    pub set_player_pos: extern "C" fn(x: f32, y: f32),
}

impl std::fmt::Debug for EngineApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineApi").finish_non_exhaustive()
    }
}

/// Build the default engine-API struct with implementations rooted in
/// `infengine_scripting`. Callers can override individual functions to
/// hook into game-specific behavior.
pub fn default_engine_api() -> EngineApi {
    EngineApi {
        log_info: rust_log_info,
        time_secs: rust_time_secs,
        get_player_pos: rust_get_player_pos,
        set_player_pos: rust_set_player_pos,
    }
}

/// Per-frame context that the host updates before invoking C# tick
/// callbacks. Stored as a thread-local because `[UnmanagedCallersOnly]`
/// extern functions cannot accept a host context pointer.
#[derive(Debug, Default)]
pub struct FrameContext {
    pub player_pos: [f32; 2],
}

thread_local! {
    static FRAME_CTX: std::cell::RefCell<FrameContext> = const {
        std::cell::RefCell::new(FrameContext { player_pos: [0.0, 0.0] })
    };
}

/// Write the current frame's context. Call **before** invoking a C#
/// tick callback.
pub fn set_frame_context(ctx: FrameContext) {
    FRAME_CTX.with(|c| *c.borrow_mut() = ctx);
}

/// Read the current frame's context. Call **after** a C# tick callback
/// to pull any state the script mutated back into the host.
pub fn take_frame_context() -> FrameContext {
    FRAME_CTX.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

extern "C" fn rust_log_info(msg: *const u8, len: usize) {
    if msg.is_null() || len == 0 {
        return;
    }
    // SAFETY: caller passes a buffer at least `len` bytes valid for the
    // duration of the call. We immediately copy into a Rust string.
    let bytes = unsafe { std::slice::from_raw_parts(msg, len) };
    let text = String::from_utf8_lossy(bytes);
    tracing::info!(target: "csharp", "{}", text);
}

extern "C" fn rust_time_secs() -> f32 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_secs_f32()
}

extern "C" fn rust_get_player_pos(out_x: *mut f32, out_y: *mut f32) {
    if out_x.is_null() || out_y.is_null() {
        return;
    }
    FRAME_CTX.with(|c| {
        let c = c.borrow();
        // SAFETY: caller guarantees both output pointers are valid for
        // the lifetime of the call.
        unsafe {
            *out_x = c.player_pos[0];
            *out_y = c.player_pos[1];
        }
    });
}

extern "C" fn rust_set_player_pos(x: f32, y: f32) {
    FRAME_CTX.with(|c| {
        c.borrow_mut().player_pos = [x, y];
    });
}

/// Errors raised while bootstrapping or driving the CoreCLR host.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("path conversion failed: {0}")]
    Path(String),
    #[error("failed to load hostfxr: {0}")]
    LoadHostfxr(#[from] netcorehost::nethost::LoadHostfxrError),
    #[error("failed to initialize runtime: {0}")]
    InitRuntime(netcorehost::error::HostingError),
    #[error("runtime config not found: {0}")]
    RuntimeConfigMissing(PathBuf),
    #[error("assembly not found: {0}")]
    AssemblyMissing(PathBuf),
    #[error("failed to get delegate loader: {0}")]
    DelegateLoader(netcorehost::error::HostingError),
    #[error("failed to resolve C# function {type_name}::{method_name}: {source}")]
    ResolveFunction {
        type_name: String,
        method_name: String,
        source: GetManagedFunctionError,
    },
}

/// Top-level scripting host. Owns the loaded `hostfxr` plus an
/// initialized runtime context.
pub struct ScriptHost {
    _hostfxr: Hostfxr,
    context: HostfxrContext<InitializedForRuntimeConfig>,
    runtime_config: PathBuf,
}

impl std::fmt::Debug for ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHost")
            .field("runtime_config", &self.runtime_config)
            .finish()
    }
}

impl ScriptHost {
    /// Bootstrap a scripting host against `runtime_config_path`. The path
    /// must point at a `.runtimeconfig.json` that exists on disk and
    /// references an installed .NET runtime (10.x recommended).
    pub fn new(runtime_config_path: impl AsRef<Path>) -> Result<Self, ScriptError> {
        let runtime_config = runtime_config_path.as_ref().to_path_buf();
        if !runtime_config.exists() {
            return Err(ScriptError::RuntimeConfigMissing(runtime_config));
        }

        let hostfxr = netcorehost::nethost::load_hostfxr()?;
        let config_pd = path_to_pdcstring(&runtime_config)?;
        let context = hostfxr
            .initialize_for_runtime_config(config_pd)
            .map_err(ScriptError::InitRuntime)?;

        info!(
            runtime_config = %runtime_config.display(),
            "C# scripting host initialized"
        );

        Ok(Self {
            _hostfxr: hostfxr,
            context,
            runtime_config,
        })
    }

    /// Path of the runtime config that initialized this host.
    pub fn runtime_config(&self) -> &Path {
        &self.runtime_config
    }

    /// Access the initialized hostfxr context. Reserved for sibling
    /// modules that resolve delegates against a specific assembly.
    pub fn context(&self) -> &HostfxrContext<InitializedForRuntimeConfig> {
        &self.context
    }

    /// Load a managed assembly. The path must point to a `.dll`
    /// produced by `dotnet build` (or `dotnet publish`).
    pub fn load_assembly(
        &self,
        assembly_path: impl AsRef<Path>,
    ) -> Result<ScriptAssembly, ScriptError> {
        let assembly = assembly_path.as_ref().to_path_buf();
        if !assembly.exists() {
            return Err(ScriptError::AssemblyMissing(assembly));
        }
        let pd = path_to_pdcstring(&assembly)?;
        let loader = self
            .context
            .get_delegate_loader_for_assembly(pd)
            .map_err(ScriptError::DelegateLoader)?;
        info!(assembly = %assembly.display(), "C# assembly loaded");
        Ok(ScriptAssembly {
            loader,
            assembly_path: assembly,
        })
    }
}

/// A loaded managed assembly. Function pointers resolved against it
/// remain valid for the lifetime of the [`ScriptHost`] that produced
/// it.
pub struct ScriptAssembly {
    loader: AssemblyDelegateLoader,
    assembly_path: PathBuf,
}

impl std::fmt::Debug for ScriptAssembly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptAssembly")
            .field("assembly_path", &self.assembly_path)
            .finish()
    }
}

impl ScriptAssembly {
    pub fn path(&self) -> &Path {
        &self.assembly_path
    }

    /// Resolve a `[UnmanagedCallersOnly] static void method()` entry
    /// point in the loaded assembly.
    pub fn get_fn_void(&self, type_name: &str, method_name: &str) -> Result<FnVoid, ScriptError> {
        let t = pdc_from_str(type_name)?;
        let m = pdc_from_str(method_name)?;
        self.loader
            .get_function_with_unmanaged_callers_only::<fn()>(&t, &m)
            .map_err(|source| ScriptError::ResolveFunction {
                type_name: type_name.to_owned(),
                method_name: method_name.to_owned(),
                source,
            })
    }

    /// Resolve a `[UnmanagedCallersOnly] static void method(EngineApi*)`
    /// entry point used to push the engine API into the managed side.
    pub fn get_fn_register_api(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Result<FnRegisterApi, ScriptError> {
        let t = pdc_from_str(type_name)?;
        let m = pdc_from_str(method_name)?;
        self.loader
            .get_function_with_unmanaged_callers_only::<fn(*const EngineApi)>(&t, &m)
            .map_err(|source| ScriptError::ResolveFunction {
                type_name: type_name.to_owned(),
                method_name: method_name.to_owned(),
                source,
            })
    }

    /// Resolve `byte HotLoader.LoadGame(byte*, nuint)`.
    pub fn get_fn_load_game(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Result<FnLoadGame, ScriptError> {
        let t = pdc_from_str(type_name)?;
        let m = pdc_from_str(method_name)?;
        self.loader
            .get_function_with_unmanaged_callers_only::<fn(*const u8, usize) -> u8>(&t, &m)
            .map_err(|source| ScriptError::ResolveFunction {
                type_name: type_name.to_owned(),
                method_name: method_name.to_owned(),
                source,
            })
    }

    /// Resolve a per-frame tick callback: `void method(float dt)`.
    pub fn get_fn_tick(&self, type_name: &str, method_name: &str) -> Result<FnTick, ScriptError> {
        let t = pdc_from_str(type_name)?;
        let m = pdc_from_str(method_name)?;
        self.loader
            .get_function_with_unmanaged_callers_only::<fn(f32)>(&t, &m)
            .map_err(|source| ScriptError::ResolveFunction {
                type_name: type_name.to_owned(),
                method_name: method_name.to_owned(),
                source,
            })
    }

    /// Resolve a `[UnmanagedCallersOnly] static int method()` entry
    /// point in the loaded assembly.
    pub fn get_fn_int(&self, type_name: &str, method_name: &str) -> Result<FnInt, ScriptError> {
        let t = pdc_from_str(type_name)?;
        let m = pdc_from_str(method_name)?;
        self.loader
            .get_function_with_unmanaged_callers_only::<fn() -> i32>(&t, &m)
            .map_err(|source| ScriptError::ResolveFunction {
                type_name: type_name.to_owned(),
                method_name: method_name.to_owned(),
                source,
            })
    }
}

fn pdc_from_str(s: &str) -> Result<PdCString, ScriptError> {
    PdCString::from_os_str(std::ffi::OsStr::new(s)).map_err(|e| ScriptError::Path(format!("{e}")))
}

fn path_to_pdcstring(path: &Path) -> Result<PdCString, ScriptError> {
    PdCString::from_os_str(path.as_os_str()).map_err(|e| ScriptError::Path(format!("{e}")))
}
