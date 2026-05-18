# File-size policy

Status: **ACTIVE** - applies to every new commit. Adopted after
Phase A had to retro-split a 18 kLOC `main.rs`.

## Hard limits

| Limit | When hit | Action |
|-------|----------|--------|
| **Soft warn** | File > 500 LOC | Stop, plan a split before adding more |
| **Hard cap** | File > 1000 LOC | Refuse to land; split into sub-modules first |
| **Function** | fn > 100 LOC | Extract helper free fns or sub-fns |
| **Match arm** | arm > 20 LOC | Extract into `cmd_X` / `handler_X` free fn (see chat dispatch pattern) |

Pre-existing files above the limits are grandfathered (see Phase A
chat.rs work) but cannot grow further - any added LOC must be
matched by a removal elsewhere in the same file, or split out.

## Structure before code

When adding a new feature:

1. **Create the module folder first**, even if it starts empty:
   ```
   src/foo/
     mod.rs          # ~50 LOC: re-exports + module-level docs
     state.rs        # core types + state machine
     events.rs       # message handlers
     render.rs       # draw fns
   ```
2. Stub the `mod.rs` with `pub(crate) mod state;` etc. before any
   `pub fn` lands.
3. Per-entity sub-modules when the domain has multiple distinct
   entities. Examples:
   - `block/chest.rs`, `block/furnace.rs` (per-block-kind state)
   - `entity/sheep.rs`, `entity/zombie.rs` (per-mob behaviour)
   - `worldgen/biome/forest.rs` (per-biome rules)
4. Tests co-located with the source they test:
   - `foo/tests.rs` for shared test helpers
   - `#[cfg(test)] mod tests { ... }` at the end of each
     `foo/<x>.rs` for type-specific tests

## Mandatory layouts for upcoming phases

### Phase B - dedicated server CLI

```
examples/voxel_demo_server/
  src/
    main.rs           # ~200 LOC: arg parse + bootstrap + loop
    args.rs           # CLI flag parsing
    runtime/
      mod.rs          # tick loop coordinator
      tick.rs         # per-tick step orchestration
      shutdown.rs     # signal handling + graceful stop
    rpc/
      mod.rs          # stdin JSON REPL dispatch
      cmds.rs         # admin command set (per-cmd fns - same pattern as chat)
    audit.rs          # connection + cmd log writer
```

### Phase C - chests / furnaces / items

```
examples/voxel_demo_mp/src/
  block/
    chest.rs          # ChestState + slot transfer + RMB hook
    furnace.rs        # FurnaceState + cook timer
  item/
    mod.rs            # ItemType registry (separate from BlockType)
    icons.rs          # Atlas UV lookup for items
    stack.rs          # max_stack helpers (where they diverge from blocks)
  container/
    mod.rs            # generic Inventory + transfer ops
    ui.rs             # 27-slot grid widget reused by chest + future containers
```

### Phase D - crafting

```
examples/voxel_demo_mp/src/
  craft/
    mod.rs            # recipe DSL + match_recipe entry
    shaped.rs         # 3x3 pattern match
    shapeless.rs      # bag-of-ingredients match
    recipes.rs        # the static table (or load from RON)
    ui.rs             # 3x3 grid + output preview
```

### Phase E - biomes + trees

```
examples/voxel_demo_mp/src/
  worldgen/
    mod.rs                # variant dispatch (existing pattern)
    height.rs             # height noise + seed
    surface.rs            # surface block pick (grass / sand / snow)
    biome/
      mod.rs              # Biome enum + per-biome params + map noise
      forest.rs
      plains.rs
      desert.rs
      mountains.rs
      ocean.rs
      tundra.rs
      jungle.rs
    tree/
      mod.rs              # L-system base + voxelisation
      oak.rs
      pine.rs
      sequoia.rs          # epic (target the screenshots)
      jungle_bigleaf.rs
      willow.rs
```

### Phase F - dimensions

```
examples/voxel_demo_mp/src/
  dimension/
    mod.rs            # Dimension { id, name, worldgen, gravity, sky_palette }
    portal.rs         # portal block + teleport handler
    registry.rs       # global dimension table
```

### Phase G - UI

```
examples/voxel_demo_mp/src/
  ui/
    mod.rs            # AppState + draw dispatch (current stub - grows)
    theme.rs          # GuiTheme presets
    menu/
      main.rs         # main menu screen
      worlds.rs       # save list (New / Load / Delete)
      settings.rs
      pause.rs
    hud/
      mod.rs          # crosshair + hotbar coordinator
      breath.rs
      health.rs
      hotbar.rs
      target.rs       # wireframe + hover label
    panels/
      inventory.rs    # the 27-slot drag-drop panel
      chat.rs         # chat overlay rendering (not state - that's in chat/)
      debug.rs        # F6 / F3 panels
```

## Why the limits

- Single 18 kLOC `main.rs` made navigation impossible. Phase A
  needed 13+ commits to fix.
- Sub-agents have token budgets. A 3 kLOC file consumes ~50% of
  a Sonnet 4.5 context just to read.
- Reviewers can't hold > ~500 LOC in their head at once.
- Each file is a unit of change: smaller files = smaller diffs
  = faster review + lower risk per merge.

## Enforcement

- **Reviewer (or self) checks LOC count before merge**. CI can
  add a warning script later.
- **Phase B agent + every future feature agent** receives this
  doc as part of its prompt context.
- Single source of truth: this file. Updates land via PR.
