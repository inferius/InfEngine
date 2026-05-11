# ADR 0002: Renderer is built on wgpu

- **Status:** accepted
- **Date:** 2026-05-11
- **Deciders:** project owner

## Context

The renderer is the largest single subsystem in any game engine and the most expensive to retarget across graphics APIs. InfEngine needs to support, at minimum:

- Windows (DirectX 12 or Vulkan)
- Linux (Vulkan)
- macOS (Metal) - eventually
- Web (WebGPU) - eventually

Writing and maintaining four separate backends is prohibitively expensive for a small team. We need an abstraction layer.

Candidates considered:

- **`wgpu`** - safe Rust wrapper over Vulkan / DX12 / Metal / GLES / WebGPU, used by Firefox, Deno, Bevy, Rerun, and many others.
- **`ash`** - raw Vulkan bindings. Requires writing Vulkan-only backends or a custom abstraction.
- **Hand-rolled abstraction over `ash` + DX12 bindings.** Maximum control, maximum work.
- **`bgfx`** (via FFI). Mature C++ library, but FFI overhead and not Rust-native.

## Decision

The renderer is built on **`wgpu`** as the single cross-platform graphics abstraction.

## Consequences

### Positive

- A single codepath covers all target platforms (Vulkan, DX12, Metal, WebGPU).
- WGSL as the shader language is portable across all backends; no need to manage HLSL / GLSL / MSL variants.
- Active development with backing from Mozilla and the WebGPU standardization group.
- Validation layer catches API misuse early.
- Existing crates for `wgpu`-based UI (`egui`), debugging (`wgpu-profiler`), and ecosystem integrations.

### Negative

- A thin layer of overhead compared to raw Vulkan / DX12. In practice the overhead is small; AAA rendering is gated by GPU time, not API call cost.
- Some advanced features (mesh shaders, ray tracing extensions) lag behind the underlying APIs. Acceptable: we are not targeting bleeding-edge graphics features for the foreseeable future.
- Vendor-specific GPU bugs may need workarounds we cannot apply at the `wgpu` layer. Mitigation: report upstream, accept an occasional pinned `wgpu` version.

## Alternatives rejected

- **Raw `ash` (Vulkan only).** Maximum performance, but commits us to Vulkan everywhere - blocking macOS without a separate Metal backend.
- **Hand-rolled abstraction.** A multi-year project on its own; would dwarf the rest of the engine work.
- **`bgfx` via FFI.** Mature and capable, but the FFI boundary, build complexity, and non-Rust toolchain conflict with the rest of the project's design.
