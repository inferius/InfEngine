# InfEngine Architecture (Phase 5)

High-level overview of how the workspace fits together at the end of
the 2D toolchain phase.

## Crate graph

```
                            sprite_demo (bin)
                                 |
       +-----+-----+-----+-----+-----+-----+-----+
       |     |     |     |     |     |     |     |
   core  window render assets audio editor physics
                                 |     |     |     |
                                 +-----+     +-----+
                                 |              |
                              notify          rapier2d
                              image
                              kira
                              egui (via editor)
```

- `infengine_core`: math (`glam` re-export) and scene model (entities,
  components, RON serialization). Foundation; no GPU / OS dependencies.
- `infengine_window`: thin `winit` wrapper. Owns the event loop and
  pumps events to an `AppHandler` trait. Re-exports raw winit via
  `raw_winit` so editor crates can hook keyboard / mouse without
  pulling winit themselves.
- `infengine_render`: wgpu-backed renderer. 2D-only at Phase 5.
  Internal modules: `camera`, `sprite`, `font`, `tilemap`, `debug`.
  Owns one wgpu device + queue + surface and the sprite/debug pipelines.
- `infengine_assets`: asset cache rooted at one content directory.
  Loads PNG/JPG/WebP via `image`; provides typed `AssetHandle<T>` and
  a `notify` filesystem watcher.
- `infengine_audio`: kira wrapper. Synthesizes sine beeps and loads
  WAV/OGG/FLAC/MP3 via kira + symphonia.
- `infengine_physics`: rapier2d wrapper. Mirrors scene entities into
  rigid bodies and colliders; supports edit / play mode toggle.
- `infengine_editor`: egui + egui-wgpu + egui-winit binding. Renders
  on top of the world surface inside `Renderer::render`'s overlay
  callback.

## Frame lifecycle (in sprite_demo)

```
winit -> AppHandler::on_window_event
            -> EditorUi::handle_engine_event (egui-winit)
            -> Demo: cursor/buttons unless egui wants pointer
AppHandler::on_update
   1. Drain asset events (reload images, refresh listing)
   2. Apply movement / drag / WASD to selected entity
   3. If play mode: physics.sync_from_scene -> step -> write_back
   4. Update camera (follow + zoom)
   5. Queue draws (tilemap + sprites + debug)
   6. Build egui UI (inspector + hierarchy + assets)
   7. Renderer::render(|d, q, e, v, s| editor.render(...))
      -> clear + sprite pass + debug pass + egui pass + present
```

## Public APIs at a glance

- `infengine_window::run(config, app)` blocks until the window closes,
  calling `AppHandler::on_start`, `on_window_event`, `on_key`,
  `on_resize`, `on_update`, `on_exit`.
- `infengine_render::Renderer::new(window)` creates the wgpu stack
  and the 2D pipelines.
- `Renderer::render(|d, q, e, v, s| ... )` acquires the next surface
  image, runs the sprite + debug passes, calls the overlay closure,
  and presents.
- `infengine_core::scene::Scene::load(path)` / `save(path)` round-trips
  a scene through RON.
- `infengine_physics::PhysicsWorld::rebuild(scene)` resets the world
  to match the scene; `step(dt)` advances; `write_back(scene)` writes
  positions back; `sync_from_scene(scene)` teleports bodies from edits.

## Boundaries and trade-offs

- **No bevy_ecs yet.** The scene is a flat `Vec<Entity>` with
  optional components. Migration to bevy_ecs is planned once the
  component count grows or systems need parallel scheduling.
- **Renderer owns the wgpu device.** Crates that need GPU resources
  borrow it through `Renderer::wgpu_device()` / `wgpu_queue()` /
  `surface_format()`. Editor builds its own egui_wgpu renderer on top.
- **Editor leaks winit types** through `raw_winit`. Documented as a
  pragmatic carve-out (ADR 0001 mentions Rust as the engine language;
  editor integration is an explicit exception). Will revisit when
  the editor lives in its own binary.
- **Asset format constants are duplicated.** `AtlasRegion::uv_rect`
  in `infengine_core` mirrors the renderer's atlas layout; updates
  must touch both. A later phase will fold this into a proper asset
  manifest.

## Dependency pins

| Crate     | Version | Why                                     |
|-----------|---------|-----------------------------------------|
| wgpu      | 27      | egui-wgpu 0.33 requires it              |
| winit     | 0.30    | ApplicationHandler trait                |
| egui      | 0.33    | Latest with wgpu 27 support             |
| kira      | 0.10    | Default audio backend                   |
| rapier2d  | 0.28    | 2D physics                              |
| image     | 0.25    | PNG/JPG/WebP decode                     |
| notify    | 8       | Filesystem watcher                      |
| glam      | 0.30    | Math                                    |
| ron       | 0.10    | Scene serialization                     |
| bevy_ecs  | 0.17    | Workspace pin; not used yet             |

## Build profile

Workspace `Cargo.toml` sets:

- `dev`: `opt-level = 1`, with `opt-level = 3` for all transitive
  deps. Game logic compiles fast; physics / wgpu run at release speed
  even in dev builds.
- `release`: thin LTO + `codegen-units = 1` + `debug = "line-tables-only"`.

## CI

`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings`, `cargo test --workspace`, and a release
build on Ubuntu and Windows.
