# InfEngine

> **Status:** very early — bootstrap phase. Not usable yet.

An open-source 2D + 3D game engine written in Rust, with an integrated editor and C# scripting for game logic. Aiming to be a long-term alternative to Unity / Unreal for indie and hobby developers.

## Goals

- **2D + 3D in one engine** — share an editor, asset pipeline, and runtime.
- **Editor-first** — Unity-style scene graph editor as a first-class part of the project, not an afterthought.
- **Cross-platform** — Windows and Linux first; macOS, Web (WASM), and mobile later.
- **Plugin-friendly** — game code and mods authored in C# via embedded .NET runtime; native (Rust) plugins also supported.
- **Modular** — every subsystem lives in its own crate, behind a stable trait-based API.
- **Open source, dual-licensed** — MIT or Apache-2.0, contributor-friendly from day one.

## Non-goals (for now)

- Console targets (PlayStation / Xbox / Switch) — no NDA-bound code in this repo.
- Photorealistic AAA rendering — we aim for "looks great on a modern GPU", not "matches Unreal Nanite".
- VR/AR as a Phase 0–4 target.

## Project status

InfEngine is in **Phase 0 (Bootstrap)** of the roadmap. The repository structure, build system, and CI exist; no engine functionality has been implemented yet.

See [`docs/roadmap.md`](docs/roadmap.md) once it lands for the multi-year plan.

## Building from source

Requirements:

- Rust **stable** (pinned via `rust-toolchain.toml`, currently 1.95+)
- A modern GPU with Vulkan (Linux/Windows) or DirectX 12 (Windows) support — required once the renderer comes online
- On Linux: development headers for `libxkbcommon`, `libwayland`, and `vulkan-loader` (or X11 equivalents)

```sh
git clone <repo-url> InfEngine
cd InfEngine
cargo build --workspace
```

Run the test suite:

```sh
cargo test --workspace
```

## Repository layout

```
InfEngine/
├── crates/
│   ├── infengine_core/      Core types, math re-exports, time
│   ├── infengine_window/    winit wrapper, event loop, input
│   ├── infengine_render/    wgpu-based renderer
│   └── infengine_assets/    asset loading and hot-reload
├── docs/
│   └── adr/                 Architecture Decision Records
├── .github/workflows/       CI configuration
└── Cargo.toml               Workspace manifest
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The project is currently private during the Phase 1 (proof of concept) period; once a runnable demo exists, the repo will be made public and contributions welcomed.

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless explicitly stated otherwise, any contribution intentionally submitted for inclusion in InfEngine shall be dual-licensed as above, without any additional terms or conditions.
