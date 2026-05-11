# InfEngine

> **Status:** Phase 9 complete - standalone scene editor with undo/redo,
> 2D translate gizmos, project export, and a runtime player binary.
> 3D rendering, 3D physics, and C# scripting are wired in earlier
> phases.

An open-source 2D + 3D game engine written in Rust, with an integrated
editor and (eventually) C# scripting. Aiming to be a long-term
alternative to Unity / Unreal for indie and hobby developers.

## Goals

- **2D + 3D in one engine** - shared editor, asset pipeline, runtime.
- **Editor-first** - Unity-style scene-graph editor as a first-class
  part of the project, not an afterthought.
- **Cross-platform** - Windows and Linux first; macOS, Web (WASM), and
  mobile later.
- **Plugin-friendly** - game code and mods authored in C# via an
  embedded .NET runtime; native (Rust) plugins also supported.
- **Modular** - every subsystem lives in its own crate, behind a
  trait-based API.
- **Open source, dual-licensed** - MIT or Apache-2.0,
  contributor-friendly from day one.

## What works today (Phase 5)

- **Renderer** (`infengine_render`, wgpu 27): batched 2D sprite
  pipeline with instance-driven transforms, a procedural texture
  atlas, an orthographic `Camera2D` (pan + zoom), a bitmap font, a
  tilemap, and a debug-line overlay (rects, circles, polylines).
  1200+ instances per frame in a single draw call at ~120 FPS on a
  modern GPU.
- **Window + input** (`infengine_window`): winit 0.30 wrapper with an
  `AppHandler` trait, frame `delta_time`, and a `Key` /
  `InputState` abstraction that does not leak winit types into game
  code.
- **Editor** (`infengine_editor`, egui 0.33): docked left side panel
  with collapsing sections for Scene (Save / Load / Reset),
  Camera (follow + zoom), Hierarchy (add / remove entities),
  Assets (file listing under `assets/`), and Selection
  (transform + sprite editors). Mouse picking + drag in the
  viewport.
- **Scene model** (`infengine_core::scene`): typed `Entity` with
  optional `SpriteComponent` and `PhysicsComponent`. Round-trips
  through RON files.
- **Assets** (`infengine_assets`): typed `AssetHandle<T>`, cache,
  PNG/JPG/WebP loading via `image`, hot reload via `notify`,
  recursive file listing.
- **Audio** (`infengine_audio`, kira 0.10): synthesized beeps plus
  on-disk WAV / OGG / FLAC / MP3 loading.
- **Physics** (`infengine_physics`, rapier2d 0.28): dynamic / static /
  kinematic bodies with box or circle colliders; edit / play mode
  toggle with snapshot restore.

## Running the demo

```sh
cargo run -p sprite_demo
```

First run writes `assets/sample.png`, `assets/chime.wav`, and
`assets/scene.ron`. Subsequent runs load the existing files. See
[`docs/user-guide.md`](docs/user-guide.md) for the controls,
scene format, and editor walkthrough.

## Non-goals (for now)

- Console targets (PlayStation / Xbox / Switch).
- Photorealistic AAA rendering. "Looks great on a modern GPU" is
  the bar, not "matches Unreal Nanite".
- VR / AR as a Phase 0-8 target.

## Building from source

Requirements:

- Rust **stable**, pinned via `rust-toolchain.toml` (1.95+).
- A modern GPU with Vulkan (Linux/Windows) or DirectX 12 (Windows).
- On Linux additionally: `libxkbcommon-dev`, `libwayland-dev`,
  `libudev-dev`.

```sh
git clone <repo-url> InfEngine
cd InfEngine
cargo build --workspace
cargo test --workspace
```

## Documentation

- [`docs/user-guide.md`](docs/user-guide.md) - controls, scene file
  format, editor workflow, troubleshooting.
- [`docs/architecture.md`](docs/architecture.md) - crate graph,
  frame lifecycle, dependency pins, design trade-offs.
- [`docs/adr/`](docs/adr/) - Architecture Decision Records.

## Repository layout

```
InfEngine/
├── crates/
│   ├── infengine_core/      Math + scene model
│   ├── infengine_window/    winit wrapper, event loop, input
│   ├── infengine_render/    wgpu renderer (2D for now)
│   ├── infengine_assets/    Asset cache, hot reload
│   ├── infengine_audio/     kira-backed audio
│   ├── infengine_editor/    egui-based editor / dev UI
│   └── infengine_physics/   rapier2d-backed physics
├── examples/
│   └── sprite_demo/         Reference 2D demo
├── docs/                    User and architecture docs
├── .github/workflows/       CI configuration
└── Cargo.toml               Workspace manifest
```

## Roadmap

- **Phase 0-4 (done):** bootstrap, 2D renderer, asset pipeline, audio,
  editor MVP.
- **Phase 5 (done):** scene serialization, hierarchy, asset browser,
  mouse picking, 2D physics, disk-loaded audio.
- **Phase 6 (done):** 3D rendering (perspective camera, mesh +
  material pipeline, PBR baseline, glTF 2.0 loader).
- **Phase 7 (done):** 3D physics via rapier3d.
- **Phase 8 (done):** C# scripting via embedded .NET 10 (hot reload
  through collectible ALC is deferred).
- **Phase 9 (done):** standalone scene editor, project format,
  undo / redo, 2D translate gizmo, runtime player + export pipeline.
  See [`docs/editor-guide.md`](docs/editor-guide.md).
- **Phase 10+:** Blueprint-style visual scripting, networking,
  particles, terrain, AI navigation, 3D editor viewport with rotation
  / scale gizmos.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The repository is private
during the early-phase shakedown; it goes public once Phase 6 lands.

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
