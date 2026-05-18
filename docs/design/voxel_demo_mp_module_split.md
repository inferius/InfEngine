# voxel_demo_mp - module split plan

Status: **DRAFT - not implemented**. Filed at user request after
the file hit ~20 kLOC. The goal is to break the single
`examples/voxel_demo_mp/src/main.rs` into a handful of focused
modules so finding "how does water work" doesn't mean grepping
through inventory, networking, and mob AI in the same file.

## Why module-only (no new crates)

This is the demo example crate. Pulling pieces into separate
crates means new `Cargo.toml`s, public-API surfaces, and inter-
crate visibility decisions for code that's not yet stable. Rust
modules give us the navigation benefit without the API churn.
Once a subsystem stabilises we can promote it to a crate (the
liquid simulation is the obvious first candidate).

## Target layout

```
examples/voxel_demo_mp/src/
  main.rs              # 2-3 kLOC: AppHandler glue + tick loop
  blocks.rs            # BlockType, F_* flags, BLOCK_TYPES, helpers
  liquid.rs            # Liquid sim + rendering helpers + tests
  decorations.rs       # torch / flower / snow position registries
  drops.rs             # DropItem entity, pickup, magnet
  inventory.rs         # hotbar + inventory grid + slot logic
  chat.rs              # chat log + /commands + key gating
  net.rs               # NetRole, wire messages, encode/decode
  mining.rs            # MiningState, tool tier, break SFX
  player.rs            # player physics, swim, breath, damage
  worldgen.rs          # WorldGen enum + chunk generators
  mob_ai.rs            # sheep / mob movement + damage
  ui.rs                # main menu, F6 debug, pause overlay
```

Order of operations matters - move the most self-contained
pieces first, then untangle the rest.

## Phasing

Each phase = one PR. Each PR keeps `cargo test`, `cargo clippy
-- -D warnings`, and the binary all green. No behaviour
changes inside a split commit - rendering / spread / pickup
must stay byte-identical.

### Phase 1: `blocks.rs` (low risk)

Pulls out:
- `BlockType` + every `BLOCK_TYPES` row.
- `F_RAY_PASS / F_AABB_PASS / F_NO_PLACE / F_WASHABLE / ...`
  constants.
- `block_has_flag`, `block_is_ray_passable`,
  `block_is_aabb_passable`, `block_is_placement_blocked`,
  `block_is_washable`, `block_is_replaceable`,
  `block_is_swim_medium`, `block_is_liquid`,
  `block_skips_chunk_mesh`, `block_material`.

Nothing here touches `Voxel` state, so it moves clean with
`pub(super)` on everything `main.rs` reaches into. Rough size:
~600 lines out of `main.rs`.

### Phase 2: `liquid.rs` (medium)

Pulls out:
- `Liquid` struct + `LIQUIDS` constant + `WATER_ID` /
  `LAVA_ID`.
- `LiquidPlanStep`, `LiquidTape`.
- `put_water_update`.
- `liquid_plan_step`, `liquid_apply_plan`,
  `liquid_merge_step`, `liquid_get_new_state`.
- `liquid_surface_height_at`, `liquid_corner_height_at`,
  `distance_to_drop_at`.
- `liquid_for`, `bucket_to_liquid_id`,
  `liquid_to_bucket_id`.
- The `#[cfg(test)] mod water_tests` module - moves wholesale,
  the helpers it needs (`block_is_washable`) now live in
  `blocks.rs` and import via `use crate::blocks::*;`.

`Voxel::tick_liquid_step` + `Voxel::tick_liquid_merge` stay in
`main.rs` (they touch `self.net`, `self.edit_history`,
`self.drops` etc) but their bodies are already thin wrappers
that just call the free fns - those calls just gain a
`liquid::` prefix.

Rough size: ~800 lines out, plus the 200 lines of tests.

### Phase 3: `net.rs` (medium)

Pulls out:
- `NetRole` enum, `Server` + `Client` variants.
- Every `*Msg` wire struct.
- The `vx.*` topic constants.
- The bincode encode/decode helpers.

Wire layout has to stay byte-identical so multiplayer keeps
working with existing servers. Easy to verify with a
single-machine offline replay.

### Phase 4: `decorations.rs` (low)

Pulls out the floor/wall decoration registries:
- `snow_positions: HashMap<[i32;3], u8>` + accumulator.
- `torch_positions: HashSet<[i32;3]>`.
- `torch_orient: HashMap<[i32;3], u8>`.
- `decoration_supported`,
  `dislodge_unsupported_decorations`.

These are tightly coupled to `Voxel` state - might end up as a
`Voxel::decorations(&self) -> Decorations` accessor returning a
helper struct that owns the registries.

### Phase 5: `drops.rs` (low)

`DropItem` entity, pickup magnet, drop physics step, spawn /
despawn broadcasts.

### Phase 6: `inventory.rs` (low)

Slot indexing, stack merging, F-key shortcuts, `/give`.

### Phase 7: `chat.rs` (low)

Chat input panel, log buffer, `/give` etc. command dispatch
table.

### Phase 8: `mining.rs` (low)

`MiningState`, tool effectiveness / tier gating, dig SFX.

### Phase 9: `player.rs` (medium)

Player physics + swim + breath + damage state. The swim block
in particular has direct reads on `self.world`,
`self.liquid_level`, ... so the split exposes the
`World` + liquid registry as a borrow into the player module
rather than `&mut Voxel`.

### Phase 10: `mob_ai.rs` + `worldgen.rs` + `ui.rs`

Cleanup of the remainder. `worldgen` already references its own
small set of helpers; `ui` is mostly `egui` immediate-mode draw
calls. `mob_ai` is the most state-dependent and should move
last.

## What doesn't move

`Voxel` struct itself + its `AppHandler` impl + the top-level
tick / render dispatch live in `main.rs`. After the split
`main.rs` is ~2-3 kLOC of "wire the modules together", which is
about the maximum a reader should have to hold in their head at
once.

## Risk + mitigation

- **Behaviour drift**: tests cover liquids decently (18 cases)
  but most of the rest has no automated coverage. Mitigation:
  each phase is its own PR, the binary is run + the changed
  area exercised before merge.
- **Visibility creep**: `pub(super)` is the default. Promote to
  `pub(crate)` only when a sibling module actually reaches
  across.
- **Module bloat**: if any one module crosses ~2 kLOC again,
  it's a sign the split missed a boundary; pause and re-cut.
