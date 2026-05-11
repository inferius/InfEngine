# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Debug-line overlay (Phase 2.7): a second `wgpu` pipeline with `LineList` topology renders colored line segments in world space on top of the sprite pass. Vertex format is `position (vec2) + color (vec4)` and the GPU buffer grows dynamically just like the sprite instance buffer. `Renderer::debug_line`, `debug_rect`, and `debug_circle(center, radius, color, segments)` queue diagnostics that auto-clear at the end of each `render()`.
- Bitmap text rendering (Phase 2.6): a hand-rolled 5x7 ASCII bitmap font is packed into the bottom strip of the sprite atlas (16x6 8x8 cells). `Renderer::draw_text(text, position, pixel_size, color)` queues one `SpriteInstance` per character, sharing the existing batched sprite pipeline so HUD text costs no extra draw calls. Supports digits, uppercase A-Z, and common punctuation; unsupported codepoints render blank. Decision recorded for future replacement by `cosmic-text` once the asset pipeline lands.
- Phase 2.5: `sprite_demo` now drives the camera each frame. The player walks across a 40x30 tilemap, the camera follows the player's world position, and `Q` / `E` zoom out / in via an exponential rate (clamped to `[0.25, 4.0]`). Demonstrates `Camera2D::position` / `zoom` mutation through `Renderer::camera_mut`.
- `infengine_render::Tilemap` and `TileId` (Phase 2.4): grid-based tilemap with a world-space origin, uniform tile size, and a `TileId::EMPTY` sentinel for skipped cells. `Renderer::draw_tilemap` expands each non-empty tile into a `SpriteInstance` and queues it into the existing sprite batch, so a tilemap costs zero extra draw calls. Demo: 24x14 tilemap (336 tiles), 4 decorations, and a player all render in one draw call.
- `infengine_render::demo_atlas` (Phase 2.3): procedurally generated 256x256 four-quadrant texture atlas (`CHECKER`, `DOTS`, `STRIPES`, `CROSS`) plus matching UV sub-rectangle constants. The atlas replaces the previous single-quadrant checker; the existing `SpriteInstance` UV fields drive the sub-rectangle selection. Marked as temporary - retires when the real asset pipeline arrives in Phase 3.
- `SpriteInstance::from_region(position, size, region)` and `SpriteInstance::with_color(color)` helpers so callers can pick an atlas region without manipulating UVs directly.
- `infengine_render::SpriteInstance` and `Renderer::draw_sprite` (Phase 2.2): batched per-instance sprite rendering. Each instance carries a world-space `position`, a `size`, an RGBA color tint, and a UV sub-rectangle (`uv_min` / `uv_max`) so that future atlases work without shader changes. The sprite pipeline now consumes a dynamic instance buffer that grows in power-of-two steps starting from a capacity of 256; one draw call renders every queued instance.
- `infengine_render::Camera2D` (Phase 2.1): orthographic 2D camera with `position`, `zoom`, and viewport size. World-space coordinates use 1 unit = 1 logical pixel at zoom 1.0; `Renderer::camera_mut` exposes it for pan/zoom in later steps.
- `infengine_render::camera::CameraResource` (internal): camera uniform buffer + bind group, bound at `@group(0)` and uploaded once per frame inside `Renderer::render`.
- `infengine_window::InputState` and `FrameContext` (Phase 1.4): the engine tracks pressed keys via the `Key` enum and exposes a per-frame snapshot plus a `delta_time` (seconds) to `AppHandler::on_update`. Focus loss clears the input state to avoid stuck keys.
- `sprite_demo` (Phase 1.4): WASD or arrow keys move the sprite at 1.4 NDC units/sec (frame-rate independent), `R` recenters, motion clamps to the screen. Diagonal motion is normalized so it isn't faster than axis-aligned motion.
- `infengine_render::sprite` (Phase 1.3): textured-quad pipeline with a packed `position + uv` vertex format, a `vec4` transform UBO (offset_xy + scale_xy), bind group covering uniform + texture + sampler, and a procedurally generated 128x128 checkerboard sRGB texture uploaded via `queue.write_texture`. Pipeline uses `TriangleStrip` with `ALPHA_BLENDING` so transparent sprites remain composable later. Exposed through `Renderer::set_sprite_transform`.

### Changed
- `Renderer::set_sprite_transform` was removed; callers now submit one `SpriteInstance` per visible sprite per frame via `Renderer::draw_sprite`. The internal queue is cleared after every `render()` call.
- `infengine_render::sprite` (Phase 2.1): the sprite pipeline now uses two bind groups - camera at `@group(0)` and the texture + sampler at `@group(1)`. The per-sprite transform moved from a uniform into the instance buffer.
- `AppHandler::on_update` now takes `&FrameContext<'_>` in addition to the `EngineWindow`, surfacing `delta_time` and the current `InputState` to application code.
- `infengine_render`: minimal wgpu-backed `Renderer` (Phase 1.2) that owns an `Instance`, `Surface`, `Device`, and `Queue`, configures an sRGB swapchain with `AutoVsync` present mode, and clears the framebuffer to a solid color every frame. Recovers from `Lost` / `Outdated` surface errors by reconfiguring.
- `infengine_window`: minimal winit-based event loop wrapping `EngineWindow`, `AppHandler` trait, and an engine-side `Key` enum that does not leak winit types.
- `examples/sprite_demo`: Phase 1.2 proof of concept - opens a window, initializes the wgpu renderer, presents a vsynced cleared frame every tick, and exits on Escape.
- Initial Cargo workspace with `infengine_core`, `infengine_window`, `infengine_render`, and `infengine_assets` crate placeholders.
- Dual MIT / Apache-2.0 licensing.
- GitHub Actions CI matrix for Windows and Linux on stable Rust.
- Initial ADRs: language choice (Rust), renderer (`wgpu`), ECS (`bevy_ecs` standalone).
- Project README, CONTRIBUTING guide, Code of Conduct, and Security policy.
