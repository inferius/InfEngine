# infengine_sdk

C-ABI core of the InfEngine plugin system. Defines the language-
agnostic surface every host + every wrapper consumes.

See `docs/plugin_sdk.md` for the full guide.

## Contract

- All structs are `#[repr(C)]`.
- All function pointers are `extern "C-unwind"`.
- `ABI_MAJOR` bumps on any layout-breaking change.
- `ABI_MINOR` bumps on additive changes (new vtable slot at the
  tail of `PluginVTable`, new helper free fn).
- Plugins ship the version they were built against in
  `PluginInfo`; the host rejects mismatches.

## Wrappers

| Wrapper | Status |
|---------|--------|
| `infengine_sdk_rs` | Wave 7 - shipping. |
| `infengine_sdk_cs` | Phase K - planned. |

The Rust dylib host (`voxel_demo_mp::plugin::DylibHost`) and the
future CLR host both load plugins through the **same** C-ABI
defined here. Adding the CLR backend is purely additive - no
SDK rewrite needed.
