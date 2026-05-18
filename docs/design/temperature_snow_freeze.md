# Temperature, Snow Layers, and Freezing — Design Doc

Status: **DRAFT — not implemented**. Filed at user request as the
next big system after the water/liquid rework.

## Goal

Add a temperature field to the voxel world. Drive natural snowfall,
snow layer accumulation, snow melt, water freeze, and ice melt off
of it. Goes beyond MC's "biome flag" approach: every voxel has a
scalar temperature that responds to altitude + heat sources + time
of day so behaviour is continuous instead of biome-binary.

## Scope (what's in / out)

In:

- Per-voxel scalar temperature (°C, signed).
- Snow accumulation in 8 sub-voxel layers (already wired) +
  promotion to a full snow block when a 9th layer would land,
  with a fresh layer-1 starting on top of the promoted cube.
  Natural snowfall (server-authoritative).
- Snow melt proportional to local temperature when temp > 0.
- Water source freezes to a new `ICE` block at temp <= some
  threshold; ice melts back to water source at temp > 0.
- Heat sources (torch, lava, glowstone) add a falloff bump to
  the local temperature.
- F6 debug overlay: temperature at the player's targeted voxel.

Out (future):

- Snowstorm weather, wind affecting snow placement.
- Different biome curves (jungle, desert, tundra).
- Ice physics (slippery walk, breaks if too thin).
- Lava cools to obsidian over time without water (deep-cold lava
  freeze).
- Player thermal model (freezing damage, fire warmth).

## Temperature model

`temperature_at(vx, vy, vz) -> f32` (°C).

```
base    = biome_base_temp(world_x, world_z)          // -20 .. +30
alt     = (vy - SEA_LEVEL).max(0) * LAPSE_PER_VOXEL  // 0.15 °C / voxel
heat    = sum_heat_sources_radius(vx, vy, vz, 6)     // torch / lava bumps
temp    = base - alt + heat
```

- `SEA_LEVEL` = current world-gen sea level (look up from
  `WORLDGEN::current()`).
- `LAPSE_PER_VOXEL` ~ 0.15 °C. Tuneable. Real atmosphere is
  ~0.0065 °C/m which is invisible at voxel scale; we exaggerate
  so a mountain summit reliably stays cold.
- `biome_base_temp`: derive from existing world-gen noise. Use
  the same noise we use for biome selection so the temperature
  field matches what the player sees. Initial implementation can
  just hard-code by `WorldGen` variant:
  - Classic: +15
  - Flat: +15
  - Islands: +20
  - Amplified: +5 (high peaks always snowy)
- Heat sources contribute via inverse-square falloff:
  `bump(d) = source_temp / max(1, d^2)` capped at `+15 °C` so
  one torch can't summer-ify a chunk.
  - Torch: +40 base
  - Glowstone: +25 base
  - Lava voxel: +200 base (huge radius)

Implementation: pure free fn over `&World` + `&SnowMap` + heat
source registry. No mutation. Cached per chunk if profiling shows
the lookup is hot.

## Snow

State: `snow_positions: HashMap<[i32; 3], u8>` already exists,
layers in `1..=8`. Rendering: thin slab scaled by `layers / 8`.

### Accumulation (snowfall tick)

Runs on the server, period configurable (e.g. every 5 game seconds).

1. Pick a random column (vx, vz) inside the active simulation
   bounds. Repeat N times per tick (N tuned to game speed -
   maybe ~50 columns / sec when snow weather is active).
2. Find the topmost solid voxel surface_y at that column.
3. Sample `temp = temperature_at(vx, surface_y + 1, vz)`. If
   `temp > -0.5` (small hysteresis), skip - too warm to snow.
4. Locate target cell `(vx, surface_y + 1, vz)`:
   - If air: insert into `snow_positions` with layers = 1.
   - If snow with layers < 8: increment layers.
   - If snow with layers == 8: promote this cell to a full
     `SNOW_BLOCK_ID` voxel (new block id, fully solid 1-voxel
     cube), then start a new layer-1 entry at
     `(vx, surface_y + 2, vz)`. (Cap stacking at, say, 4 blocks
     so a long snowstorm doesn't pile to the cloud ceiling.)
5. Broadcast `vx.snow_layer` packet so clients track the change.

### Melt (every M ticks)

Iterate `snow_positions` (cap candidates per tick so a huge map
doesn't stall):

1. For each `(pos, layers)`:
   - `temp = temperature_at(pos.x, pos.y, pos.z)`.
   - If `temp <= 0`: skip.
   - Compute decay: `melt_per_tick = (temp.clamp(0, 10) / 10.0) *
     MELT_RATE_LAYERS_PER_TICK` (e.g. MELT = 0.5 layers/tick at
     +10 °C, so ~16 ticks to clear a full slab in tropical
     temps).
   - Accumulate a fractional remainder per cell (new
     `snow_melt_accum: HashMap<[i32; 3], f32>`); subtract a
     layer when the accumulator crosses 1.0.
   - Layers -> 0: remove from `snow_positions`, spawn a water
     "puddle" entity (or just disappear - call it sublimation).
2. Full snow blocks (`SNOW_BLOCK_ID`) that melt: demote to
   layer-7 entry in `snow_positions` (or fewer if temp is much
   above 0).

### Promotion to full block

When a layer-8 cell would receive a 9th layer, we replace the
slab with a real `SNOW_BLOCK_ID` voxel - now it's a regular solid
voxel (player walks on top, occludes face culling, etc.). The
next layer goes one cell higher.

`SNOW_BLOCK_ID` needs:

- `BlockType` row with `F_NONE` flags (regular solid, breakable).
- Albedo: same white-blue as snow slab.
- Sound profile = snow.
- Drop on mine: a snowball entity (future) or just an air pickup.
- A reverse path: when melt logic hits a snow block from below
  (i.e. its bottom face is air or low-layer snow), demote back
  to a slab so it visibly thins out.

## Water freeze / ice melt

New `ICE_ID` block.

### Freeze

When water at `temp <= FREEZE_TEMP` (e.g. -2 °C with hysteresis):

1. Iterate `liquid_positions` for water voxels with `level == 0`
   (sources only - flowing water doesn't freeze in MC either).
2. For each, sample temp at the voxel. If <= FREEZE_TEMP, swap
   `BlockId(WATER_ID)` -> `BlockId(ICE_ID)`. Remove from
   `liquid_level`, `liquid_active`, `liquid_positions`. Push an
   `EditMsg` so clients sync.

### Melt

When ice at `temp >= MELT_TEMP` (e.g. +1 °C):

1. Iterate cells holding `ICE_ID`. (Need an index. Could use a
   `HashSet<[i32; 3]>` `ice_positions`, populated on
   `world_set_tracked` analogously to snow.)
2. For each, sample temp. If >= MELT_TEMP, swap back to water
   source. Re-register in liquid maps. Spawn an
   `EditMsg`.

## F6 debug overlay

Add to the existing F6 debug panel:

```
Temp at target: -3.4 °C (base -1.2, alt 2.8, heat +0.6)
Snow layer here: 5/8
Snowfall this tick: 17 / 50 candidates
```

Useful while tuning numbers.

## Phasing

Strictly sequential. Each phase = one commit, tested in-game
before moving on.

1. **A — Temperature scalar**: `temperature_at` free fn. F6 debug
   row. No world mutation. _3 hours._
2. **B — Heat sources**: scan radius for torch/lava/glowstone.
   Add to `temperature_at`. _2 hours._
3. **C — Natural snowfall**: server tick layer increments where
   `temp < 0`. No promotion to full block yet. Snow particle FX
   optional. _4 hours._
4. **D — Snow melt**: melt accumulator, layer decrement, cell
   removal. _3 hours._
5. **E — Layer promotion**: layer 8 + new snow -> SNOW_BLOCK_ID +
   layer 1 above. Demote on melt. Requires new BlockType row +
   net sync. _4 hours._
6. **F — Water freeze + ice melt**: ICE_ID block + bidirectional
   convert loop. _3 hours._
7. **G — Polish**: weather state machine (snow on/off based on
   biome + sky), snowfall particle effect, snowball drop. _open
   ended._

Total Phase A-F: ~19 hours. Phase G open.

## Open questions

- Should the temperature field be visible globally (e.g. mini-map
  isotherms) or only at the target voxel? Vote: target only for
  now, full visualization later.
- Hysteresis: do we want temp -0.5 to start snowing and +0.5 to
  start melting, with a dead zone in between? Avoids
  freeze-melt-freeze loops. Recommended yes.
- Should snowfall affect chunks outside the active simulation
  radius? MC does not. We probably should not either - just
  fake "look new" on chunk load via a snow-on-stale-chunks pass.
- Promote-cap height: how many full snow blocks can stack? MC
  has no cap. We probably want one (e.g. 4) so a long session
  doesn't bury structures.
