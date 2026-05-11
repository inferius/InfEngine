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

#![forbid(unsafe_code)]

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
