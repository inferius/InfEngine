# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `infengine_window::InputState` and `FrameContext` (Phase 1.4): the engine tracks pressed keys via the `Key` enum and exposes a per-frame snapshot plus a `delta_time` (seconds) to `AppHandler::on_update`. Focus loss clears the input state to avoid stuck keys.
- `sprite_demo` (Phase 1.4): WASD or arrow keys move the sprite at 1.4 NDC units/sec (frame-rate independent), `R` recenters, motion clamps to the screen. Diagonal motion is normalized so it isn't faster than axis-aligned motion.
- `infengine_render::sprite` (Phase 1.3): textured-quad pipeline with a packed `position + uv` vertex format, a `vec4` transform UBO (offset_xy + scale_xy), bind group covering uniform + texture + sampler, and a procedurally generated 128x128 checkerboard sRGB texture uploaded via `queue.write_texture`. Pipeline uses `TriangleStrip` with `ALPHA_BLENDING` so transparent sprites remain composable later. Exposed through `Renderer::set_sprite_transform`.

### Changed
- `AppHandler::on_update` now takes `&FrameContext<'_>` in addition to the `EngineWindow`, surfacing `delta_time` and the current `InputState` to application code.
- `infengine_render`: minimal wgpu-backed `Renderer` (Phase 1.2) that owns an `Instance`, `Surface`, `Device`, and `Queue`, configures an sRGB swapchain with `AutoVsync` present mode, and clears the framebuffer to a solid color every frame. Recovers from `Lost` / `Outdated` surface errors by reconfiguring.
- `infengine_window`: minimal winit-based event loop wrapping `EngineWindow`, `AppHandler` trait, and an engine-side `Key` enum that does not leak winit types.
- `examples/sprite_demo`: Phase 1.2 proof of concept - opens a window, initializes the wgpu renderer, presents a vsynced cleared frame every tick, and exits on Escape.
- Initial Cargo workspace with `infengine_core`, `infengine_window`, `infengine_render`, and `infengine_assets` crate placeholders.
- Dual MIT / Apache-2.0 licensing.
- GitHub Actions CI matrix for Windows and Linux on stable Rust.
- Initial ADRs: language choice (Rust), renderer (`wgpu`), ECS (`bevy_ecs` standalone).
- Project README, CONTRIBUTING guide, Code of Conduct, and Security policy.
