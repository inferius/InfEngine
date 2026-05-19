# InfEngine plugin SDK (Wave 7)

Status: **shipping** for Rust dylib plugins. C# / CLR host lands
in Phase K against the same C-ABI surface.

## Crates

| Crate | Purpose |
|-------|---------|
| `crates/infengine_sdk` | Language-agnostic C-ABI core. `#[repr(C)]` types, `extern "C-unwind"` fn pointers. The contract every host AND every wrapper consumes. |
| `crates/infengine_sdk_rs` | Idiomatic Rust wrapper. Safe `Plugin` trait, `Engine` handle, `declare_plugin!` macro. |
| `examples/sample_plugin_rs` | Reference Rust plugin (cdylib). `RainbowPlugin` registers a block + handles a chat trigger. |
| `examples/voxel_demo_mp::plugin` | Engine-side plugin host. Loads + dispatches plugins, exposes `/plugins` chat command. |

## Writing a plugin in Rust

1. Cargo.toml:

   ```toml
   [lib]
   crate-type = ["cdylib", "rlib"]

   [dependencies]
   infengine_sdk_rs = { path = "...crates/infengine_sdk_rs" }
   ```

2. `src/lib.rs`:

   ```rust
   use infengine_sdk_rs::{Engine, Plugin, declare_plugin};

   #[derive(Default)]
   struct MyPlugin;

   impl Plugin for MyPlugin {
       fn name() -> &'static str { "my_plugin" }
       fn version() -> &'static str { "0.1.0" }
       fn on_init(&mut self, engine: &mut Engine) {
           engine.log_info("hello from my_plugin");
       }
   }

   declare_plugin!(MyPlugin);
   ```

3. `cargo build -p my_plugin` produces
   `target/<profile>/my_plugin.{dll,so,dylib}`.

4. Drop it in `<engine_exec_dir>/plugins/` (or set
   `INFENGINE_PLUGIN_DIR=...` for testing).

## ABI versioning

Every plugin carries `abi_major` + `abi_minor` from the
`infengine_sdk` constants it was built against.

- `ABI_MAJOR` mismatch -> plugin rejected at load. Bumped only
  when a struct layout / vtable order changes.
- `ABI_MINOR` higher than the host's -> plugin rejected (the
  plugin uses a vtable slot the host doesn't know about).
- `ABI_MINOR` equal-or-lower than the host -> accepted.

## Chat command

- `/plugins` - list loaded plugins (perm 0).
- `/plugins reload <name>` - unload + re-load (perm 4).
- `/plugins disable <name>` - skip future callbacks (perm 4).

## Threading + safety

Plugin callbacks fire on the engine's main thread,
single-threaded. Plugins MAY spawn their own threads but the
[`Engine`] handle is `!Send` and must never cross into one.

Every dispatch is wrapped in `catch_unwind`; a panicking plugin
disables itself instead of taking the engine down. `extern
"C-unwind"` is used throughout so panics propagate to the host
catcher (a plain `extern "C"` would abort the process).

## Phase K readiness

The C-ABI surface is **deliberately language-agnostic**:

- All structs are `#[repr(C)]` with explicit padding for
  predictable layout.
- All function pointers are `extern "C-unwind"`.
- No Rust-only types in the public API (no `Rc`, `&dyn Trait`,
  lifetimes, generics).
- Strings are NUL-terminated UTF-8 owned by the producer.

A future `infengine_sdk_cs` crate + `ClrHost` Rust module slot
next to `infengine_sdk_rs` + `DylibHost` behind the same
`PluginHost` contract. Phase K is **additive** - no SDK
rewrite required.
