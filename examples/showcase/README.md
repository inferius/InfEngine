# InfEngine Showcase

End-to-end example exercising every major engine system in one
project: scene + classes + audio + lights + physics + multiple
blueprints (entity, class, global, library) + C# scripting hook.

## What's in the project

| Entity         | Components                                                      | Logic                                              |
|----------------|-----------------------------------------------------------------|----------------------------------------------------|
| Floor          | Transform3D, MeshComponent (cube, grey), Physics3DComponent     | Static; collides with player                       |
| Player         | Transform3D, MeshComponent (red), Physics3DComponent, AudioComponent | `player_bp.ron` (Entity BP) - OnStart prints sentinel 777; OnTick calls C# `Engine.TimeSeconds` |
| Pickup_NE/NW/SE/SW | Transform3D, MeshComponent (green), class `Pickup`          | `pickup_spin.ron` (Class BP) - shared by all four; OnTick increments `Angle` variable, writes Y rotation |
| Light_Warm     | Transform3D, MeshComponent, LightComponent (orange)             | `light_pulse.ron` (Entity BP) - drives Y position from accumulating phase |
| Light_Cool     | Transform3D, MeshComponent, LightComponent (blue)               | Static                                             |

## Blueprints

- **`lib_math.ron`** - global library exposing function `double` (Input * 2). Registered in `Project.blueprint_libraries` so any blueprint in the project can call it via Call Function.
- **`scene_arena.ron`** - global scene blueprint. OnStart prints 1.0; OnTick uses `time_seconds`, `add`, `greater_than`, `branch`, `set_float_var` to emit a beat every second.
- **`pickup_spin.ron`** - class blueprint for the `Pickup` class. Variables: `Angle`. Demonstrates `get_float_var`, `delta_time`, `multiply`, `add`, `set_float_var`, `set_rotation`, `sequence`.
- **`player_bp.ron`** - per-entity blueprint on Player. Demonstrates `print` and `call_cs_method` referencing `Engine.TimeSeconds`.
- **`light_pulse.ron`** - per-entity blueprint on Light_Warm. Drives Y position via accumulating phase variable.

## Run it

### 1. Generate the project files

```sh
cargo run -p showcase --bin showcase_setup
```

Outputs everything under `examples/showcase/project/`. The setup
binary is idempotent - rerun any time the underlying engine schema
changes.

### 2. Open in the editor

```sh
cargo run -p editor_app -- examples/showcase/project/project.ron
```

Pick the **Arena** scene from the left panel, then press **Play**.

What you should see in the log:

- `value=1` from the scene global blueprint's OnStart.
- `value=777` from the player blueprint's OnStart.
- Every second: another scene `print` carrying the current
  `time_seconds`.
- Per tick: a `blueprint -> C# call (stub)` line with method
  `Engine.TimeSeconds` (the C# reverse-call wiring is still a TODO
  in `docs/design/blueprint_csharp_callable.md`; the stub host
  records the call so you can see the path fires).
- Pickups spin around their Y axis.
- Light_Warm bobs up and down.

### 3. Edit the blueprints live

Double-click any pickup in the hierarchy to open its blueprint.
Try changing the `2` literal on the `multiply` node's `pin_b` to
`5` and saving - the running pickups speed up immediately thanks
to the Phase 9 hot-reload pipeline.

### 4. Build a standalone runtime

```sh
cargo build --release -p infengine_runtime
target/release/infengine_runtime.exe examples/showcase/project/project.ron
```

Same scene, no editor UI - the blueprint VM is fully embedded in
the runtime binary.

## Assets

The player's autoplay audio component points at
`assets/audio/ambient.ogg`. Drop any short OGG / WAV there to hear
it; missing file just logs a warning.

## What this demonstrates

- **Scene system**: 3D entities with transforms, meshes, physics,
  lights, audio.
- **Classes**: four pickups share a single class blueprint.
- **Multi-blueprint runtime**: each entity can carry several
  blueprints; the player has its entity BP plus inherits nothing
  (no class).
- **Cross-blueprint library**: `lib_math.ron` is loaded at Play
  start and its functions are visible to every other blueprint.
- **Variable persistence + math**: pickups + light pulse rely on
  the variable map persisting across ticks.
- **Engine-API nodes**: `set_rotation`, `set_position`,
  `delta_time`, `time_seconds`, `print`, `call_cs_method`,
  `branch`, `greater_than`, `add`, `multiply`, `get/set_float_var`,
  `sequence`.
- **C# integration scaffolding**: the player BP invokes
  `Engine.TimeSeconds` via the `call_cs_method` node; the full
  reverse-call pipeline is documented and in progress.
- **Editor workflow**: open / Play / hot-reload / standalone build
  all work with this single project.
