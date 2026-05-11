//! csharp_demo - Phase 8.1 of InfEngine.
//!
//! Bootstrap CoreCLR via `infengine_scripting::ScriptHost`, load the
//! `InfEngine.Scripts` sample assembly, and call two
//! `[UnmanagedCallersOnly]` static methods.
//!
//! Build the C# side first:
//!
//! ```sh
//! cd scripts/InfEngine.Scripts
//! dotnet build -c Release
//! ```
//!
//! Then `cargo run -p csharp_demo`.

use std::path::PathBuf;
use std::process::ExitCode;

use infengine_scripting::{ScriptHost, default_engine_api};
use tracing::{error, info};

const SCRIPT_DIR: &str = "scripts/InfEngine.Scripts.Host/bin/Release/net10.0";
const RUNTIME_CONFIG: &str = "InfEngine.Scripts.Host.runtimeconfig.json";
const ASSEMBLY: &str = "InfEngine.Scripts.Host.dll";

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let base = PathBuf::from(SCRIPT_DIR);
    let runtime_config = base.join(RUNTIME_CONFIG);
    let assembly = base.join(ASSEMBLY);

    if !runtime_config.exists() || !assembly.exists() {
        error!(
            runtime_config = %runtime_config.display(),
            assembly = %assembly.display(),
            "missing build output - run `dotnet build -c Release` inside scripts/InfEngine.Scripts first"
        );
        return ExitCode::from(2);
    }

    let host = match ScriptHost::new(&runtime_config) {
        Ok(h) => h,
        Err(err) => {
            error!(error = %err, "failed to bootstrap C# scripting host");
            return ExitCode::from(3);
        }
    };
    info!(?host, "host ready");

    let scripts = match host.load_assembly(&assembly) {
        Ok(a) => a,
        Err(err) => {
            error!(error = %err, "load_assembly failed");
            return ExitCode::from(4);
        }
    };

    // Push the engine API table into the managed side.
    let engine_type = "InfEngine.Scripts.Host.Engine, InfEngine.Scripts.Host";
    match scripts.get_fn_register_api(engine_type, "RegisterApi") {
        Ok(register) => {
            let api = default_engine_api();
            info!("registering engine API with C# side...");
            register(&api as *const _);
        }
        Err(err) => {
            error!(error = %err, "resolve Engine.RegisterApi failed");
            return ExitCode::from(5);
        }
    }

    let type_name = "InfEngine.Scripts.Host.Hello, InfEngine.Scripts.Host";
    match scripts.get_fn_void(type_name, "HelloFromCSharp") {
        Ok(f) => {
            info!("calling HelloFromCSharp...");
            f();
        }
        Err(err) => {
            error!(error = %err, "resolve HelloFromCSharp failed");
            return ExitCode::from(5);
        }
    }

    match scripts.get_fn_void(type_name, "EngineRoundTrip") {
        Ok(f) => {
            info!("calling EngineRoundTrip...");
            f();
        }
        Err(err) => {
            error!(error = %err, "resolve EngineRoundTrip failed");
            return ExitCode::from(8);
        }
    }

    match scripts.get_fn_int(type_name, "LifeUniverseEverything") {
        Ok(f) => {
            let value = f();
            info!(value, "LifeUniverseEverything returned");
            if value != 42 {
                error!(value, "expected 42");
                return ExitCode::from(6);
            }
        }
        Err(err) => {
            error!(error = %err, "resolve LifeUniverseEverything failed");
            return ExitCode::from(7);
        }
    }

    info!("C# bootstrap demo finished successfully");
    ExitCode::SUCCESS
}
