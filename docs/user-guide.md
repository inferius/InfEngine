# InfEngine User Guide

This document covers the 2D workflow as shipped at the end of Phase 5.

## Running the demo

Prerequisites:

- Rust **stable**, pinned via `rust-toolchain.toml` (1.95+).
- A modern GPU with Vulkan (Linux/Windows) or DirectX 12 (Windows).
- On Linux additionally: `libxkbcommon-dev`, `libwayland-dev`, `libudev-dev`.

```sh
cargo run -p sprite_demo
```

The first run generates `assets/sample.png`, `assets/chime.wav`, and
`assets/scene.ron`. Subsequent runs load the existing files.

## Default controls

| Key / mouse                | Action                                  |
|----------------------------|-----------------------------------------|
| `WASD` or arrows           | Move the currently selected entity      |
| `Q` / `E`                  | Zoom out / in (clamped to `0.25..=4.0`) |
| `R`                        | Recenter selection and reset zoom       |
| `Space`                    | Play a synthesized 440 Hz beep          |
| `C`                        | Play `assets/chime.wav` from disk       |
| `Esc` or window close      | Quit                                    |
| Left mouse drag (viewport) | Pick the topmost sprite and drag it     |

## Editor side panel sections

The left side panel is collapsible. Each section is independent.

- **Top header.** Shows phase tag, FPS, sprite count per frame, asset
  reload count, and beep count. Holds the **Play / Stop** button plus
  the live `Bodies` counter.
- **Scene.** Save / Load / Reset buttons for `assets/scene.ron`.
  Background color picker drives `Renderer::set_clear_color`.
- **Camera.** Toggle "Follow selection" (camera tracks the selected
  entity); zoom slider (logarithmic); "Debug overlay" toggle for the
  world origin cross, the selection AABB, and the line from origin to
  the selection.
- **Hierarchy.** Scrollable list of every entity in the scene. Rows
  show a `S` prefix when the entity has a sprite component and the
  entity id. Click selects, "X" removes. Buttons at the bottom add a
  new entity with or without a default sprite component.
- **Assets.** Listing of files under `assets/` (recursive, sorted).
  Clicking an entry records it in `selected_asset` for upcoming
  drag-to-scene workflows.
- **Selection.** Name (text field), id (read-only), transform
  (X / Y / scale-x / scale-y sliders), and sprite editor (size /
  color / atlas region via combo box, or "Add sprite component" when
  the entity is empty).

## Scene file format

`assets/scene.ron` uses RON pretty-printed via `ron::ser::PrettyConfig`.
The full schema lives in `crates/infengine_core/src/scene.rs`:

```ron
(
    name: "demo",
    entities: [
        (
            id: (1),
            name: "player",
            transform: (
                position: (0.0, 300.0),
                scale: (1.0, 1.0),
            ),
            sprite: Some((
                size: (64.0, 64.0),
                color: (1.0, 0.85, 0.4, 1.0),
                region: checker,
            )),
            physics: Some((
                body: dynamic,
                shape: box,
                linear_damping: 0.0,
                restitution: 0.3,
                friction: 0.5,
            )),
        ),
    ],
    next_id: 5,
    clear_color: (0.05, 0.10, 0.18),
    gravity: (0.0, -1200.0),
)
```

Field rules:

- `id` is preserved across saves; do not reuse a value.
- `region` is one of `full`, `checker`, `dots`, `stripes`, `cross`
  (the demo atlas regions). New regions arrive with the real asset
  pipeline in a later phase.
- `physics.body` is one of `dynamic`, `static`, `kinematic`.
- `physics.shape` is either `box` (uses the sprite size) or
  `circle: ( radius: 32.0 )`.
- `gravity` is in world units / second^2 (positive y is up; default
  `-1200`).

Hand-edit the file while the demo is running and the watcher will
re-decode any images that change. Scene file changes require pressing
**Load** in the Scene section to take effect.

## Hot reload

`AssetServer::watch()` spawns a `notify` watcher rooted at `assets/`.
Image hot reload is wired in the demo: edit `assets/sample.png`
externally and the `RELOADS` counter increments.

## Audio

- `AudioSystem::play_beep(freq_hz, duration_secs, volume)` synthesizes
  a sine tone with a 5 ms fade in / out.
- `AudioSystem::load_sound(path)` decodes WAV / OGG / FLAC / MP3 via
  kira + symphonia and returns a cloneable `Sound`. `play_sound` plays
  a one-shot. The demo loads `assets/chime.wav`.

## Physics

The play loop is mode-gated:

- **Edit mode** (default). Inspector edits, drag, WASD all write to
  scene state. Physics bodies are not stepped.
- **Play mode**. The current scene is snapshotted, `PhysicsWorld` is
  rebuilt, and `step(dt) -> write_back(scene)` runs every frame. Press
  **Stop** to restore the snapshot.

Static bodies keep their authored position even while playing; dynamic
and kinematic bodies have their positions written back from rapier.

## How to build your own scene

1. Hit **Reset** (or delete `assets/scene.ron`) to start clean.
2. Use **Add sprite entity** in Hierarchy to create entities.
3. Edit each entity's name, position, size, color, and region in
   **Selection**.
4. Optionally drag them in the viewport to position visually.
5. Hit **Save** to persist.
6. Hit **Play** to test physics. Tweak `restitution` / `friction` in
   the scene file if needed.

## Adding a new crate

Workspace members live under `crates/`. To add a sibling:

1. `cargo new --lib crates/infengine_yourname`.
2. Add it to the `members` list in the root `Cargo.toml`.
3. Adopt `version.workspace = true`, `edition.workspace = true`,
   `license.workspace = true`, etc., in the new crate's `Cargo.toml`
   (see `crates/infengine_audio/Cargo.toml` as a template).
4. Apply workspace lints with `[lints] workspace = true`.

## Troubleshooting

- **Window opens then closes.** Look for `failed to initialize
  renderer` or `selected GPU adapter` lines in the log. Vulkan
  validation layer warnings are expected on systems without the
  Vulkan SDK installed.
- **Inspector edits do not stick.** Confirm you pressed **Save**;
  scene state lives in memory until persisted.
- **Audio silent.** Verify the OS audio device is selectable (the
  log line `audio system initialized` confirms init succeeded).
- **`cargo build` slow.** First build downloads ~700 MB of crate
  source and compiles `wgpu`, `kira`, `rapier2d`, `egui`, and the
  `symphonia` codec bundle. Subsequent incremental builds are fast.
