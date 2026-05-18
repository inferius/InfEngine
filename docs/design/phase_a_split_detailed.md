# Phase A - module split (detailed action plan)

Status: **DRAFT - action plan**. Concretes the layout, file
moves, visibility, and PR sequence for splitting
`examples/voxel_demo_mp/src/main.rs` (currently 18 599 lines)
into focused modules. Per user direction, the multi-world save
directory layout is prepared in this phase too so the dedicated
server (Phase B) lands against a stable disk structure.

## Target layout

```
examples/voxel_demo_mp/src/
  main.rs              # ~2 kLOC: Voxel + AppHandler + bin entry
  blocks.rs            # BlockType, F_* flags, BLOCK_TYPES, helpers
  liquid.rs            # Liquid sim + rendering + tests + mesh build
  net.rs               # NetRole, wire messages, encode/decode
  world_save/
    mod.rs             # SavedWorld struct, save / load dispatch
    layout.rs          # paths, slugs, meta.json schema
  decorations.rs       # torch / flower / snow registries
  drops.rs             # DropItem entity, pickup, magnet
  inventory.rs         # hotbar + craft grid + slot logic
  chat.rs              # chat log + slash commands
  mining.rs            # MiningState, tool tier, break SFX
  player.rs            # player physics, swim, breath, damage
  worldgen.rs          # WorldGen enum + chunk generators
  mob_ai.rs            # mob movement + damage
  ui.rs                # main menu, F6 debug, pause overlay
```

Total module count: 13 modules + 2 sub-modules (`world_save/`)
+ `main.rs`. Expected `main.rs` size after: ~2 kLOC.

## Per-module move list

Sequence: top-down, each module = one PR. Move target +
expected line count + cross-module dependencies.

### A1. `blocks.rs` (~600 lines)

Moves:
- `BlockType` struct.
- `BLOCK_TYPES` const array.
- `F_RAY_PASS`, `F_AABB_PASS`, `F_NO_PLACE`,
  `F_TRANSPARENT_RENDER`, `F_SWIM`, `F_DEDICATED_RENDER`,
  `F_REPLACEABLE`, `F_WASHABLE` constants.
- `block_type(id)`, `block_has_flag`.
- `block_is_ray_passable`, `block_is_aabb_passable`,
  `block_is_placement_blocked`, `block_is_washable`,
  `block_is_replaceable`, `block_is_swim_medium`,
  `block_skips_chunk_mesh`, `block_is_transparent_render`.
- `block_material`, `block_required_tier`.
- `tool_tier`, `tool_effectiveness`.
- `TORCH_ID`, `GLOWSTONE_ID`, `WATER_ID`, `LAVA_ID`,
  `BUCKET_EMPTY_ID`, `BUCKET_WATER_ID`, `BUCKET_LAVA_ID`
  constants - they're really block ids, belong here.
- `liquid_to_bucket_id`, `bucket_to_liquid_id` helpers.

Dependencies up: none (leaf module).
Dependencies down: `infengine_voxel::BlockId`.

Visibility: `pub(crate)` for everything `main.rs` reaches into.

PR risk: low. Pure data + helper functions, no state.

### A2. `liquid.rs` + winding fix (~1100 lines)

Moves:
- `Liquid` struct + `LIQUIDS` const + `liquid_for`,
  `block_is_liquid`.
- `LiquidPlanStep`, `LiquidTape`.
- `put_water_update`, `liquid_plan_step`, `liquid_apply_plan`,
  `liquid_merge_step`, `liquid_get_new_state`.
- `liquid_surface_height_at`, `liquid_corner_height_at`,
  `distance_to_drop_at`.
- `liquid_flow_dir` (free fn version).
- `push_quad_flow` mesh helper.
- The full `#[cfg(test)] mod water_tests`.

Voxel methods left in main.rs (they touch `self.net`,
`self.edit_history`, drops, mesh):
- `Voxel::tick_liquids`, `Voxel::tick_liquid_step`,
  `Voxel::tick_liquid_merge`, `Voxel::maybe_rebuild_liquid_mesh`.

These all already call the free fns - their bodies stay thin.

**Bonus**: normalise quad winding in `push_quad_flow` so every
face is CCW from the outward side. Then re-enable
`cull_mode: Some(wgpu::Face::Back)` on the transparent pipeline
(reverts the underwater-grid artifact properly).

Dependencies up: `blocks::*` (for `block_is_washable` etc).
Dependencies down: `infengine_voxel::{BlockId, World, Chunk}`.

PR risk: medium. Test module catches regressions; winding fix
needs careful verification (top-from-above + side-from-outside).

### A3. `net.rs` (~600 lines)

Moves:
- `NetRole` enum.
- All `*Msg` wire structs (`EditMsg`, `PlayerMsg`, `JoinMsg`,
  `HurtMsg`, `InvSnapshotMsg`, `InvActionMsg`, ...).
- `vx.*` topic constants if any.
- bincode encode/decode helpers if any standalone.
- `ServerStateV2`, `PersistedInv` (currently in main).

`NetServer` / `NetClient` come from `infengine_net` crate -
not moved.

Dependencies up: `blocks::BlockId` (re-exported from voxel
crate).
Dependencies down: `infengine_net::*`, `bincode`, `serde`.

PR risk: low-medium. Wire format must stay byte-identical so
existing clients keep connecting; verify with a 2-machine sanity
test before merge.

### A4. `world_save/` module + multi-world dirs

This is the prep step the user asked for - the dedicated server
(Phase B) reuses this layout, the Worlds menu (Phase G) lists
slots from this directory.

Files:
- `world_save/mod.rs`: high-level save / load entry points,
  `SavedWorld` struct.
- `world_save/layout.rs`: path resolution, slug generation,
  `meta.json` schema (serde struct).

New on-disk layout:

```
%APPDATA%/InfEngine/worlds/<slug>/   (Windows)
~/.local/share/infengine/worlds/<slug>/   (Linux/macOS)
  meta.json
  player.bin
  world/
    state.bin          # current single-file save format
                       # (per-chunk snapshots = Phase B follow-up)
```

Migration: on first startup with the new layout, check for the
legacy `voxel_server_state.bin` in the working dir. If present
+ no `worlds/` dir, create `worlds/legacy/world/state.bin` and
move it there. Print a one-line log.

`meta.json`:

```json
{
  "name": "First world",
  "slug": "first-world-2026-05-18",
  "worldgen": "classic",
  "seed": 17539214,
  "created_at": "2026-05-18T10:21:33Z",
  "last_played": "2026-05-18T17:42:01Z",
  "play_time_seconds": 3825,
  "format_version": 1
}
```

Voxel methods that move:
- `save_server_state`, `load_server_state` become
  `world_save::save(voxel, slug)` / `world_save::load(voxel,
  slug)`.

Voxel state kept in main.rs:
- `last_state_save: Option<Instant>` (autosave throttle).
- `current_world_slug: Option<String>` (which save we're in).

Dependencies up: `blocks::*`, `liquid::Liquid`, `net::*`.
Dependencies down: `serde`, `bincode`, `std::fs`,
`directories` crate (NEW) for cross-platform data dir.

PR risk: medium-high. Save migration must not break existing
players. Run on a copy of the old `voxel_server_state.bin`
manually.

### A5. `decorations.rs` + `drops.rs` + `inventory.rs` (~1200 lines combined)

Three small leaf modules, can land together or separately.

**`decorations.rs`**:
- `decoration_supported`, `dislodge_unsupported_decorations`.
- Snow / torch / glowstone helper logic.
- Snow particle accumulation if any.

The state (`snow_positions`, `torch_positions`, `torch_orient`)
stays on `Voxel`; the module exports `fn try_*` helpers that
take `&mut HashMap`.

**`drops.rs`**:
- `DropItem` struct.
- Drop physics step, pickup magnet.
- Drop spawn / despawn broadcasts.
- `DROP_ATTRACT_RADIUS`, `DROP_COLLECT_RADIUS`,
  `DROP_MAGNET_STRENGTH` constants.

Voxel methods that move:
- `spawn_drop`, `tick_drops`, `tick_drop_pickups`.

State stays on `Voxel` (`dropped_items: Vec<DropItem>`,
`next_drop_id: u32`).

**`inventory.rs`**:
- `INVENTORY_SLOTS`, `HOTBAR_SLOTS` constants.
- `ClientInv`, `PersistedInv` (move from net.rs).
- Stack-merging logic.
- `/give` argument parser if pulled out of chat.

PR risk: low. Each module is small + isolated.

### A6. `chat.rs` + `mining.rs` + `player.rs` + `worldgen.rs` + `mob_ai.rs` + `ui.rs`

Final batch. Each ~400-800 lines.

**`chat.rs`**:
- Chat log, input state, history.
- Slash command dispatch table - currently a huge `match`,
  pull out into per-command functions.

**`mining.rs`**:
- `MiningState`, tier gating, dig SFX selection.

**`player.rs`**:
- Player physics + swim + breath + damage state machine.
- Player AABB collision helpers (`body_aabb`,
  `aabb_intersects_solid`, ...).
- `apply_player_damage_kind`, fall-damage tracker.

**`worldgen.rs`**:
- `WorldGen` enum, current() lookup, atomic register.
- Chunk generator function (currently `generate_chunk`).
- Tree placement, surface decoration.

**`mob_ai.rs`**:
- Mob struct, sheep/ wolf / etc behaviour.
- `damage_mob`, mob spawn / despawn.

**`ui.rs`**:
- Main menu draw fns.
- F6 debug panel.
- Pause / settings overlays.
- Crosshair, hotbar HUD, world picker combo.

PR risk: low-medium. `player.rs` is the largest + most
state-dependent (reads liquid, snow, mining). Move last in
this batch.

## After Phase A - what's NOT moved

Stays in `main.rs`:
- `Voxel` struct definition (the megastate).
- `impl AppHandler for Voxel` (engine entry point).
- `main()` fn.
- `Settings` struct (small, mostly UI-touched).
- High-level orchestration: `on_start`, `on_update`,
  `on_window_event`.

Expected `main.rs` size after Phase A: ~2 kLOC.

## Cross-cutting decisions

### Visibility

Default: `pub(crate)` for items reached from `main.rs`. Promote
to `pub` only when a sibling example (e.g. `voxel_demo`) needs
them - which today is zero items.

### Module hierarchy via files vs folders

Files for now (`blocks.rs`, `liquid.rs`, ...). If any single
module crosses ~2 kLOC, promote to a folder (`module/mod.rs` +
sub-files). `world_save/` is the only initial folder because it
naturally splits into `layout` (paths / slugs) + `mod`
(orchestration).

### Tests location

Move per-module tests into the same file (e.g.
`#[cfg(test)] mod tests` at the end of `liquid.rs`). Behaviour
unchanged but `cargo test -p voxel_demo_mp <module>::tests`
filters cleanly.

### Common imports

Each module: `use super::*;` is a code smell at this scale.
Prefer explicit `use crate::blocks::{F_RAY_PASS,
block_is_washable};` so a reader can trace each name's origin.

### One PR per A-step, byte-identical behaviour

Each A-step:
1. Branch off main.
2. Make the file move.
3. Compile + `cargo clippy --workspace --all-targets -- -D
   warnings` clean.
4. Run `cargo test -p voxel_demo_mp`.
5. Run the binary, verify the touched subsystem (e.g. for A2
   run water + look at edges + dig a cliff).
6. Commit + merge.

If a step exposes a bug, file as a follow-up - don't fix in the
move PR. Keeps each split mechanical and reviewable.

## What enables Phase B

After A4 ships, the world is on-disk under
`worlds/<slug>/`. A headless server binary (Phase B) reuses the
exact same save layout - it opens / creates one of those slugs
and runs `tick` loops sans renderer.

The order matters:
- A1-A3: clean up the demo enough that B can split off cleanly.
- A4: settle the save format the server will own.
- A5-A6: tidy the rest. B can start in parallel with A5 / A6
  if review bandwidth allows.

## Open items (re-ask if user has opinions)

- Save format inside `worlds/<slug>/world/state.bin`: keep the
  current EditMsg log, or switch to per-chunk bincode snapshots
  in this phase? Per-chunk is the eventual goal (see
  `world_save_load.md`); EditMsg-log works fine for small
  worlds. Recommendation: ship EditMsg log in A4, swap to
  snapshots when the first slot crosses ~10 MB.
- `directories` crate dependency: cross-platform data dir
  resolver. Standard, ~2 deps. Recommendation: yes.
- Server (Phase B) name: `infengine_server` (rolls off "MC
  server" naming) vs `voxel_demo_server` (clearly tied to the
  current demo). Recommendation: `voxel_demo_server` for now -
  it IS the demo's server, the engine has no "server" of its
  own yet.
