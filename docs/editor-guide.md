# InfEngine Editor Guide

`editor_app` is the standalone scene editor shipped at the end of
Phase 9. It is the recommended way to author InfEngine projects.

## Quickstart

```sh
# 1. From the workspace root, build + run the editor against a fresh
# project (creates ./project.ron on first launch).
cargo run -p infengine_editor_app

# 2. Or point it at an existing project file.
cargo run -p infengine_editor_app path/to/MyGame/project.ron
```

The editor opens a 1440x900 window with a docked **inspector** panel
on the left and the live scene viewport filling the rest.

## Project model

A project is a directory containing:

- `project.ron` - the manifest. Lists the project name, asset root,
  and an ordered list of scenes (`ProjectScene { name, path }`).
- One scene `.ron` per entry (paths relative to the project root).
- An asset directory referenced by `asset_root` (defaults to
  `assets/`).

The editor edits one scene at a time. Switching scenes saves nothing
automatically; press **Save scene** before switching if needed. The
**Save project** button persists the manifest itself (scene list,
asset root, project name).

## Inspector sections

**Header.** Project name + FPS counter + **Undo / Redo** buttons with
current stack depths. Status line below shows the result of the last
operation.

**Scenes.** Selectable list of project scenes. Buttons:

- *Save scene* writes the active scene to its file.
- *Save project* writes `project.ron`.
- *New scene* allocates `scene_{N}.ron` next to the project file,
  registers it, and opens it.
- *Export -> dist/* copies the project, every scene file, and the
  asset directory into `<project_root>/dist/`. See **Exporting**.
- Background color picker drives the renderer's clear color.

**Hierarchy.** Scrollable list of every entity in the active scene.
Click selects, the small "X" deletes. *Add sprite* creates a new
entity with a default 64x64 checker sprite; *Add empty* creates an
entity without any visual.

**Assets.** Lists files found under the project's asset root,
recursive + sorted. Refresh button rescans the directory.

**Selection.** Edits the currently-selected entity:

- Name (text field) and immutable id.
- Transform: X / Y drag values, scale-X / scale-Y sliders.
- Sprite component toggle. When enabled: size sliders, color picker
  (RGBA), and region combo (Full / Checker / Dots / Stripes / Cross).

## Viewport controls

| Input                    | Action                                       |
|--------------------------|----------------------------------------------|
| Left-click on body       | Select + free drag                           |
| Left-click on **red** arm | Drag the selected entity along X only        |
| Left-click on **green** arm | Drag along Y only                          |
| Left-click on **yellow** center square | Free drag                       |
| `Q` / `E`                | Zoom out / in (clamped to `0.25..=4.0`)      |
| `Ctrl+Z`                 | Undo                                          |
| `Ctrl+Y` or `Ctrl+Shift+Z` | Redo                                       |
| `Esc` or window close    | Quit                                          |

The translate gizmo arms render only around the currently selected
entity. They live in world space, so they scale visually with zoom.

## Undo / Redo

Snapshot-based: the entire `Scene` is cloned onto an `undo_stack`
before any structural change (drag start, add entity, remove entity).
`redo_stack` accumulates whatever undo throws away. The stack is
bounded at 64 entries; opening a different scene clears both stacks
because they belong to the previous workspace.

What is *not* snapshot-tracked yet: inspector slider edits and color
pickers. Combine them with a drag-start to checkpoint, or save the
scene before a risky change.

## Exporting

Press **Export -> dist/** to assemble a runtime bundle under
`<project_root>/dist/`. The bundle contains:

- `project.ron`
- Every scene file referenced by the manifest, preserving relative paths.
- The asset directory (recursive copy).
- A `README.txt` with build instructions.

To actually ship the bundle, build the runtime binary once per target
platform:

```sh
cargo build --release -p infengine_runtime
```

Then copy `target/release/infengine_runtime` (or `.exe` on Windows)
next to `dist/project.ron`. Running the binary opens the project and
plays the first scene.

## Roadmap

Phase 9 ships the editor MVP. Open work:

- Per-entity components beyond sprite: physics, audio, scripting
  (already implemented in the wider engine, not yet exposed in the
  editor).
- 3D viewport with rotation + scale gizmos (Phase 9.3 was 2D only).
- Live Play mode that runs scripting + physics inside the editor
  without exporting.
- Project switcher (open / recents) rather than command-line argv only.
- Asset import (drag image -> auto-create entity referencing it).
