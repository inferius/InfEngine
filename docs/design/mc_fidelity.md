# MC fidelity tables - Wave 11 transcribe

Wave 11 of the voxel_demo_mp polish brought two block-data tables
into 1:1 fidelity with the corresponding Minecraft 26.1.2 source.
This doc lists every transcribed value and the precise source
file + line citation behind each one. Future waves that touch
these tables MUST update both the table AND this doc.

Source root:
`J:/Temp/template-mod-template-26.1.2/sources/minecraft-merged-52430b475d-26.1.2-sources/`

## W11.1 - Flammability (FireBlock.bootStrap())

Table lives at `examples/voxel_demo_mp/src/block/flammability.rs`
as `MC_FLAMMABLES: &[FlammableEntry]`. Row count locked at 207.
Source file: `net/minecraft/world/level/block/FireBlock.java`.

| MC name | ignite_odds | burn_odds | FireBlock.java line |
|---|---|---|---|
| oak_planks .. bamboo_mosaic | 5 | 20 | L301..L311 |
| oak_slab .. bamboo_mosaic_slab | 5 | 20 | L312..L322 |
| oak_fence_gate .. bamboo_fence_gate | 5 | 20 | L323..L332 |
| oak_fence .. bamboo_fence | 5 | 20 | L333..L342 |
| oak_stairs .. bamboo_mosaic_stairs | 5 | 20 | L343..L353 |
| oak_log .. bamboo_block | 5 | 5 | L354..L363 |
| stripped_oak_log .. stripped_bamboo_block | 5 | 5 | L364..L373 |
| stripped_oak_wood .. stripped_mangrove_wood | 5 | 5 | L374..L382 |
| oak_wood .. mangrove_wood | 5 | 5 | L383..L391 |
| mangrove_roots | 5 | 20 | L392 |
| oak_leaves .. mangrove_leaves | 30 | 60 | L393..L401 |
| bookshelf | 30 | 20 | L402 |
| tnt | 15 | 100 | L403 |
| short_grass .. cactus_flower | 60 | 100 | L404..L436 |
| white_wool .. black_wool (16 colors) | 30 | 60 | L437..L452 |
| vine | 15 | 100 | L453 |
| coal_block | 5 | 5 | L454 |
| hay_block | 60 | 20 | L455 |
| target | 15 | 20 | L456 |
| white_carpet .. black_carpet (16 colors) | 60 | 20 | L457..L472 |
| pale_moss_block / pale_moss_carpet / pale_hanging_moss | 5 | 100 | L473..L475 |
| dried_kelp_block | 30 | 60 | L476 |
| bamboo | 60 | 60 | L477 |
| scaffolding | 60 | 60 | L478 |
| lectern | 30 | 20 | L479 |
| composter | 5 | 20 | L480 |
| sweet_berry_bush | 60 | 100 | L481 |
| beehive | 5 | 20 | L482 |
| bee_nest | 30 | 20 | L483 |
| azalea_leaves / flowering_azalea_leaves | 30 | 60 | L484..L485 |
| cave_vines / cave_vines_plant | 15 | 60 | L486..L487 |
| spore_blossom | 60 | 100 | L488 |
| azalea / flowering_azalea | 30 | 60 | L489..L490 |
| big_dripleaf / big_dripleaf_stem / small_dripleaf | 60 | 100 | L491..L493 |
| hanging_roots | 30 | 60 | L494 |
| glow_lichen | 15 | 100 | L495 |
| firefly_bush | 60 | 100 | L496 |
| bush | 60 | 100 | L497 |
| acacia_shelf .. spruce_shelf | 30 | 20 | L498..L507 |

Engine-id mapping (`block/flammability.rs::engine_id_to_mc_name`):

- `WOOD_ID` (5) -> `oak_log` -> `(5, 5)`
- `PLANKS_ID` (7) -> `oak_planks` -> `(5, 20)`
- `LEAVES_ID` (6) -> `oak_leaves` -> `(30, 60)`
- `WOOL_ID` (11) -> `white_wool` -> `(30, 60)`
- Every other engine id -> `(0, 0)` (non-flammable)

Checksum: `0x6B584F067ED80FE6` (FNV-ish over names + odds).

## W11.2 - Light emission (Blocks.java .lightLevel(...))

Table lives at `examples/voxel_demo_mp/src/block/light_emission.rs`
as `MC_EMITTERS: &[EmitterEntry]`. Row count locked at 65.
Source file: `net/minecraft/world/level/block/Blocks.java`.

State-dependent emitters store the MAX value (lit / fully charged
/ active). The engine-side resolver returns the MAX since there's
no per-state machinery yet; future state work will route through
a state-aware variant.

| MC name | lightLevel | Blocks.java line |
|---|---|---|
| lava | 15 | L255 |
| brown_mushroom | 1 | L1007 |
| torch | 14 | L1194 |
| wall_torch | 14 | L1199 |
| fire | 15 | L1209 |
| soul_fire | 10 | L1221 |
| furnace (lit) | 13 | L1298 |
| redstone_ore (lit) | 9 | L1895 |
| redstone_torch (lit) | 7 | L1906 |
| redstone_wall_torch (lit) | 7 | L1911 |
| soul_torch | 10 | L2035 |
| soul_wall_torch | 10 | L2040 |
| copper_torch | 14 | L2045 |
| copper_wall_torch | 14 | L2050 |
| glowstone | 15 | L2059 |
| nether_portal | 11 | L2065 |
| jack_o_lantern | 15 | L2084 |
| glow_lichen | 7 | L2371 |
| enchanting_table | 7 | L2481 |
| brewing_stand | 1 | L2485 |
| lava_cauldron | 15 | L2494 |
| end_portal | 15 | L2507 |
| end_portal_frame | 1 | L2519 |
| dragon_egg | 1 | L2530 |
| redstone_lamp (lit) | 15 | L2537 |
| ender_chest | 7 | L2567 |
| beacon | 15 | L2599 |
| light | 15 | L3054 |
| sea_lantern | 15 | L3098 |
| end_rod | 14 | L4125 |
| end_gateway | 15 | L4204 |
| magma_block | 3 | L4238 |
| sea_pickle | 12 (max, 4 pickles alive) | L5003 |
| conduit | 15 | L5019 |
| smoker (lit) | 13 | L5157 |
| blast_furnace (lit) | 13 | L5167 |
| lantern | 15 | L5216 |
| soul_lantern | 10 | L5228 |
| copper_lantern | 15 | L5242 |
| campfire (lit) | 15 | L5254 |
| soul_campfire (lit) | 10 | L5266 |
| shroomlight | 15 | L5359 |
| crying_obsidian | 10 | L5605 |
| respawn_anchor | 15 (fully charged) | L5615 |
| candle_cake | 3 | L5717 |
| amethyst_cluster | 5 | L5792 |
| large_amethyst_bud | 4 | L5798 |
| medium_amethyst_bud | 2 | L5803 |
| small_amethyst_bud | 1 | L5808 |
| sculk_sensor / calibrated_sculk_sensor | 1 | L5876 |
| sculk_catalyst | 6 | L5899 |
| copper_bulb (lit) | 15 | L6158 |
| exposed_copper_bulb (lit) | 12 | L6163 |
| weathered_copper_bulb (lit) | 8 | L6168 |
| oxidized_copper_bulb (lit) | 4 | L6173 |
| cave_vines | 14 | L6343 |
| cave_vines_plant | 14 | L6354 |
| ochre_froglight | 15 | L6527 |
| verdant_froglight | 15 | L6532 |
| pearlescent_froglight | 15 | L6537 |
| trial_spawner | 14 (active) | L6566 |
| vault | 12 (active) | L6580 |
| firefly_bush | 2 | L6657 |
| candle | 12 (4 candles) | L6828 |

Engine-id mapping (`block/light_emission.rs::engine_id_to_mc_name`):

| Engine id | Engine name | MC name | lightLevel |
|---|---|---|---|
| 10 TORCH_ID | torch | torch | 14 |
| 28 GLOWSTONE_ID | glowstone | glowstone | 15 |
| 37 LAVA_ID | lava | lava | 15 |
| 49 FIRE_ID | fire | fire | 15 |
| 50 REDSTONE_TORCH_ID | redstone_torch | redstone_torch | 7 |
| 51 REDSTONE_WALL_TORCH_ID | redstone_wall_torch | redstone_wall_torch | 7 |
| 52 SOUL_TORCH_ID | soul_torch | soul_torch | 10 |
| 53 SOUL_WALL_TORCH_ID | soul_wall_torch | soul_wall_torch | 10 |
| 54 SEA_LANTERN_ID | sea_lantern | sea_lantern | 15 |
| 55 JACK_O_LANTERN_ID | jack_o_lantern | jack_o_lantern | 15 |
| 56 SOUL_FIRE_ID | soul_fire | soul_fire | 10 |
| 57 END_ROD_ID | end_rod | end_rod | 14 |
| 58 MAGMA_BLOCK_ID | magma_block | magma_block | 3 |
| 59 SHROOMLIGHT_ID | shroomlight | shroomlight | 15 |
| 60 LANTERN_ID | lantern | lantern | 15 |
| 61 SOUL_LANTERN_ID | soul_lantern | soul_lantern | 10 |
| 62 CRYING_OBSIDIAN_ID | crying_obsidian | crying_obsidian | 10 |
| 63 RESPAWN_ANCHOR_ID | respawn_anchor | respawn_anchor | 15 (max) |
| 64 ENDER_CHEST_ID | ender_chest | ender_chest | 7 |
| 65 LIT_REDSTONE_LAMP_ID | lit_redstone_lamp | redstone_lamp | 15 |
| 66 LIT_REDSTONE_ORE_ID | lit_redstone_ore | redstone_ore | 9 |
| 67 LIGHT_BLOCK_ID | light_block | light | 15 |

Checksum: `0xEF3D3956269C5D2A`.

## W11.3 - New engine block ids

Reserved range 50..=70 (W11.3 uses 50..=67; 68..=70 are spare).
Each id pairs with a BlockType row in BLOCK_TYPES + an ItemType
row in ITEM_TYPES + an entry in `engine_id_to_mc_name` so all
three layers stay in lockstep.

## W11.4 - Brigadier-shaped command autocomplete

The chat completion engine
(`examples/voxel_demo_mp/src/chat/autocomplete.rs`) mirrors MC's
`com.mojang.brigadier.CommandDispatcher` tree shape. Every
CMD_META verb is a root-level Literal node; specific commands
extend the tree with typed argument nodes:

| Verb | Argument grammar |
|---|---|
| /give | `<player> <item> [count]` |
| /tp | `<player>` |
| /tppos /goto | `<x> <y> <z>` |
| /gamemode /gm | `survival\|creative\|0\|1 [player]` |
| /setblock | `<x> <y> <z> <block>` |
| /fill | `<x1> <y1> <z1> <x2> <y2> <z2> <block>` |
| /kill | `[player]` |
| /kick | `<player> [reason]` |
| /mute /unmute | `<player>` |
| /dim | `overworld\|nether` |
| /worldgen /gen | `classic\|flat\|islands\|amplified\|realistic` |
| /weather | `clear\|rain\|storm` |
| /time | `day\|night\|noon\|midnight` |
| /summon /spawnmob | `zombie\|skeleton\|creeper\|pig\|cow\|chicken\|sheep` |
| /w /whisper /tell /msg | `<player> <message>` |
| /tool | tool block names (pickaxe_wood ... flint_and_steel) |

Selectors (@p, @a, @s, @r) are listed as completions for
`<player>` slots but NOT semantically resolved. The runtime
treats them as literal player names today (will fail dispatch
until selector resolution lands in a later wave).

## Wave 11 lock test

`examples/voxel_demo_mp/src/wave11_audit_tests.rs::wave_11_locked`
asserts:
1. MC_FLAMMABLES.len() == 207 + flammability_checksum() == 0x6B584F067ED80FE6.
2. MC_EMITTERS.len() == 65 + emission_checksum() == 0xEF3D3956269C5D2A.
3. The autocomplete root tree has >= 70 verbs (one per CMD_META row).

Any change to either MC-fidelity table must bump the row count
+ recompute the checksum here AND in the per-module test, so a
silent regression trips both layers.

## W12.1 - block/mod.rs split

`block/mod.rs` was 1181 LOC, over the 1000 hard cap. Split into
`types.rs` (TileSpec + flags + BlockType), `ids.rs` (every
`*_ID` constant), `registry.rs` (the BLOCK_TYPES static slice),
`mod.rs` (re-exports + lookup helpers). External call sites
unchanged.

## W12.2 - Slab + fence + door + stairs + pane + wall

New block ids 68..=82 (15 entries), each carrying
`F_DEDICATED_RENDER` so the chunk mesher routes through
`resource_pack::special_models::model_for_block`:

| id | name | model |
|----|------|-------|
| 68 | oak_slab | block/slab |
| 69 | stone_slab | block/slab |
| 70 | cobblestone_slab | block/slab |
| 71 | oak_slab_top | block/slab_top |
| 72 | oak_stairs | block/stairs |
| 73 | stone_stairs | block/stairs |
| 74 | cobblestone_stairs | block/stairs |
| 75 | oak_fence | block/fence_post |
| 76 | nether_brick_fence | block/fence_post |
| 77 | oak_door | block/door_lower |
| 78 | oak_door_upper | block/door_upper |
| 79 | iron_door | block/door_lower |
| 80 | glass_pane | block/glass_pane |
| 81 | iron_bars | block/glass_pane |
| 82 | cobblestone_wall | block/wall_post |

Per W12 design: slab top + bottom are separate ids, stairs
default east-facing, doors split into lower + upper.

## W12.3 - Textured particle pass plumbing

New `particle_pass`: EmitterKind (Smoke / BigSmoke / Flame /
Lava), ParticleInstance, ParticleAtlas. Frame index =
`(age / lifetime) * frame_count` clamped to last (no wrap).
Cite: SmokeParticle.pickSprite (8-frame cycle).

## W12.4 - Sun + moon position math

New `sky`: `sun_direction(phase)` / `moon_direction(phase)`
(unit + antipodal), `moon_phase_for_day` (8-day cycle),
`moon_phase_uv` (4x2 atlas). 5 deg angular diameter per spec.
Cite: DimensionSpecialEffects + LevelRenderer.renderSky.

## W12.5 - Emitter point-light halo

New `emitter_lights` picks the 8 nearest emitter voxels with
warm RGB per kind. Feeds the existing
`mesh3d.wgsl::lights: array<PointLight, 8>` uniform.

## W12.6 - Menu panorama backdrop

New `panorama`: discover_panorama_faces walks
`textures/gui/title/background/panorama_0..5.png`, 1 RPM
rotation, dark-navy fallback. Cite: PanoramaRenderer.

## W12.7 - .jar auto-extraction cache

New `jar_cache`: CacheStatus { Fresh, Stale, Missing },
`jar_cache_dir = <data>/cache/mc_assets/<ver>/`, sentinel-mtime
freshness check. Extraction itself gated on adding the zip
crate (separate decision).

## W12.8 - Mob skin discovery

New `mob_skin`: `mob_skin_path(root, MobKind)` resolves
`textures/entity/<mob>/<mob>.png`. Renderer falls back to the
hard-coded mob colour when missing.

## W12.9 - Creative tab categories

CreativeCategory { All, Blocks, Decoration, Tools, Combat,
Food, Misc } + filter helpers. Partition is total (every item
lands in exactly one non-All tab). Cite: CreativeModeTab.

## Wave 12 lock test

`examples/voxel_demo_mp/src/wave12_audit_tests.rs::wave_12_locked`
asserts:
1. W12.2 block ids 68..=82 are contiguous + present in BLOCK_TYPES.
2. Every W12.2 id has a `name_for_block` -> `model_for_block`
   routing in `resource_pack::special_models`.
3. block/mod.rs is below the 500 LOC soft cap.
4. CreativeCategory partition is total.

Any addition to the W12.2 slab/fence/door/stairs family must
bump the contiguous range AND extend the routing assertion so
a silent regression trips the lock.

## W17 - Cave / Ravine / Dungeon parameters

Wave 17 adds three underground feature passes to the `Realistic`
generator. `Classic` stays vanilla-flat (no caves, no ravines, no
dungeons).

### W17.1 - 3D-noise cave carver

Source: `examples/voxel_demo_mp/src/worldgen/caves.rs`

Algorithm: two octaves of trilinearly-interpolated hash-based noise,
multiplied together. Cave voids form where the product exceeds a
threshold. Quintic smoothstep on interpolation weights.

| Parameter | Value | Notes |
|---|---|---|
| CAVE_SCALE_A | 12.0 | Large-feature frequency (broad caves) |
| CAVE_SCALE_B | 6.0 | Fine-feature frequency (narrow passages) |
| THRESHOLD_NORMAL (wy >= 10) | 0.82 * 0.82 = 0.6724 | ~5% underground voxels carved |
| THRESHOLD_DEEP (wy < 10) | 0.70 * 0.70 = 0.49 | ~10% carved, denser lava-zone caves |
| Floor guard | wy <= 0 | Never carved |
| Surface guard | non-stone blocks | Grass/sand/water/bedrock preserved |

MC ballpark: ~5-8% of underground volume. Our normal-depth target is 5%.

### W17.2 - Ravine carver

Source: `examples/voxel_demo_mp/src/worldgen/ravines.rs`

Algorithm: per-(chunk_x, chunk_z) column hash decides if a ravine
spawns. Ravine shape is an axis-aligned ellipsoid (x-aligned or
z-aligned, chosen randomly). Carves all stone-like blocks inside.

| Parameter | Value | Notes |
|---|---|---|
| RAVINE_CHUNK_RATE | 64 | ~1/64 chunk columns spawn a ravine |
| HALF_LENGTH range | 8..24 | Long-axis half-size in blocks |
| HALF_WIDTH range | 2..6 | Short-axis half-size in blocks |
| HALF_HEIGHT range | 8..24 | Vertical half-size in blocks |
| CY range | 10..50 | World-Y centre of the ravine |
| RAVINE_FLOOR_Y | 2 | Floor guard (never carves at y <= 2) |
| Neighbourhood check | +/-2 chunk columns | Ravines may overhang neighbours |

MC ballpark: ~1/64 chunks. Our rate matches.

### W17.3 - Dungeon rooms

Source: `examples/voxel_demo_mp/src/worldgen/dungeons.rs`

Algorithm: per-(chunk_x, chunk_z) hash decides if a dungeon spawns.
Room is a 7x7x5 (outer) cobblestone box, hollow interior, with a
spawner-placeholder at the centre floor and 1-2 empty chest blocks.

| Parameter | Value | Notes |
|---|---|---|
| DUNGEON_RATE | 200 | ~1/200 chunk columns spawn a dungeon |
| Outer dimensions | 7 x 5 x 7 (W x H x D) | 1-block cobblestone shell |
| Inner dimensions | 5 x 3 x 5 | Air interior |
| Y range | 8..=32 | Outer-box floor world-Y |
| Chests | 1 or 2 | Empty (loot tables deferred) |
| Spawner | BlockId(8) cobblestone | TODO: real tile-entity spawner |
| Neighbourhood check | +/-1 chunk column | Room fits within one column |

MC ballpark: ~1/200 chunks. Our rate matches.

### W17.4 - Surface block polish

Fixed: `Mountain` biome below the snow line previously returned stone
(id 1) for all h < 26, making terrain look uniformly grey. New rules:

| Height range | Mountain surface block |
|---|---|
| h < 22 | grass (id 3) - lower slopes |
| 22 <= h < 34 | stone (id 1) - bare rocky mid-slope |
| h >= 34 | snow (id 15) - peak |

All other biomes unchanged. Integration test
`wave17_tests::realistic_gen_non_mountain_surface_is_not_grey_stone`
guards this contract.

### Deferred W17 follow-ups

- **Real spawner tile-entity**: dungeon centre uses cobblestone
  placeholder. Implementing mob spawners requires per-block entity
  state (tile entity) + mob-kind selection + activation range logic.
- **Dungeon loot tables**: chests are placed empty. Loot generation
  requires an item-pool system + RNG seeded from the dungeon position.
- **Mineshafts**: long tunnel networks with wooden supports + rails.
  Would use a similar per-column hash + corridor-segment generation.
- **Villages**: surface structure generation with multiple buildings.
  Requires a multi-chunk footprint resolver.

## W18 - Textured particle pipeline

Wave 18 replaces the legacy tinted-cube ambient particle system with
a proper GPU billboard pipeline backed by a dedicated MC particle atlas.

### W18.1 - wgpu billboard particle pass

Source: `crates/infengine_render/src/particle_pass.rs`,
        `crates/infengine_render/src/particle.wgsl`

| Parameter | Value | Notes |
|---|---|---|
| Topology | TriangleStrip | 4 verts synthesised in vertex shader |
| Quad size | 0.125 world units | matches MC close-range sprite appearance |
| Max instances | 2048 | capped in instance buffer |
| Blend mode | alpha blend, non-premultiplied | `SRC_ALPHA + (1-SRC_ALPHA)` |
| Depth write | off | particles layer over world without z-fighting |
| Depth test | off | full-screen overlay pass after mesh3d |
| Billboard | spherical | cam_right/cam_up from view_proj_inv columns |
| Atlas binding | @group(1) binding 0/1 | separate from block atlas at group 1 |

Cite: MC client `ParticleRenderType.PARTICLE_SHEET_TRANSLUCENT` -
alpha-blended, no depth write, billboard-oriented quads.

### W18.2 - Particle atlas builder

Source: `examples/voxel_demo_mp/src/resource_pack/particle_atlas.rs`

Packs MC `assets/minecraft/textures/particle/*.png` into a separate
power-of-two atlas using a row-shelf algorithm. Sprites are keyed by
file stem. Multi-frame strips (flame.png, 16x128 = 8 frames stacked
vertically) are stored as one entry with `frames > 1`; callers step V
downward by `frame_h = (v_max - v_min) / frames` to reach frame N.

| Sprite | Format | Frames | Notes |
|---|---|---|---|
| smoke_0..smoke_7 | 16x16 each | 1 | 8 wisp variants |
| flame | 16x128 | 8 | vertical strip |
| lava | 16x16 | 1 | single droplet |
| big_smoke_0..big_smoke_11 | 16x16 each | 1 | furnace plume |

Atlas field added to `ResourcePack`; fallback/placeholder pack sets
`particle_atlas = None` (no disk assets available).

Cite: MC `assets/minecraft/textures/particle/` directory layout,
`SmokeParticle.pickSprite` (8-frame cycle), `FlameParticle` (single
strip), `LavaParticle` (one frame).

### W18.3 - Textured emitters

Source: `examples/voxel_demo_mp/src/main.rs`
        (`emit_textured_flame`, `emit_textured_smoke`, `emit_textured_lava`)

`tick_ambient_particles` now dual-emits:
1. Legacy CPU `Particle` (tinted cube) - retained for debris/gravity/break effects.
2. New `TexturedParticleInstance` - GPU billboard with particle-atlas UV.

| Source | Sprite | Color | Size | Rate |
|---|---|---|---|---|
| Torch | flame strip, random frame | [1.0, 0.78, 0.40] orange | 0.125 | 5 Hz |
| Fire block | flame strip, random frame | [1.0, 0.55, 0.20] deep orange | 0.125 | 7.5 Hz |
| Fire (smoke) | smoke_0..7, random frame | [0.7, 0.7, 0.7] grey | 0.15 | ~2.5 Hz |
| Lava source | lava sprite | [1.0, 0.45, 0.15] red-orange | 0.10 | 2.5 Hz |

Fallback (no MC assets): white quad (uv_min=[0,0], uv_max=[1,1]); tint
colour still applied, giving a tinted square instead of a sprite.

### W18.4 - Chunk-mesh anim_id

Source: `examples/voxel_demo_mp/src/resource_pack/mod.rs`
        (`build_anim_index`, `anim_slot`, `anim_slot_for_ref`)

`anim_id` vertex attribute (u32 at offset 40 in `MeshVertex`) is stamped
by the chunk mesher via `anim_lookup(block, normal)`. 0 = static face,
1..=32 = animated slot from `AnimRegistry`.

| Animated texture | Typical slot | Source |
|---|---|---|
| water_still | 1 (first registered) | AnimRegistry::register |
| lava_still | 2 | AnimRegistry::register |
| fire_0 | 3 | AnimRegistry::register |

The `AnimRegistry` has a 32-slot hard cap (matches `TileAnimsUniform`
shader array size). Slot assignment is deterministic: textures are sorted
by `TextureKey` string representation before registration.

Static faces (`frames == 1` or `frametime_ms == 0`) always get slot 0.
The mesh3d shader short-circuits on `anim_id == 0` (no UV scroll).

### Wave 18 test count

| Module | Tests added | Total |
|---|---|---|
| particle_pass.rs (W18.1) | 6 | 6 |
| particle_atlas.rs (W18.2) | 11 | 17 |
| wave18_tests.rs (W18.3+W18.4) | 6 | 23 |

Total W18 tests: 23. Baseline (W17 end): 1401. New total: 1426.

### Deferred W18 follow-ups

- **Particle gravity + lifetime animation**: the GPU pass currently renders
  each `TexturedParticleInstance` as a static sprite. Adding per-particle
  lifetime tracking + frame cycling over age would require a particle
  simulation step on the CPU or a compute shader.
- **Particle depth sorting**: alpha-blended particles should ideally sort
  back-to-front vs. the camera. Currently rendered in push order.
- **Large particle atlas cap**: the packer is capped at 2048x2048. A pack
  with many animated sprite sheets could overflow; deferred until profiling
  shows real pressure.
- **Integrate `big_smoke` with furnace emitters**: furnace + smoker blocks
  could emit `big_smoke_0..11` plumes above their chimney; deferred to a
  dedicated furnace-animation wave.

## Wave 19 - Mob skins + per-body-part UV

Wave 19 introduces MC entity-texture UV mapping for all 7 mob kinds.
Each kind gets per-face UVs derived directly from the Java model sources.

### MC source class references

| Mob | Java source file |
|---|---|
| Zombie | `net/minecraft/client/model/HumanoidModel.java` |
| Skeleton | `net/minecraft/client/model/monster/skeleton/SkeletonModel.java` |
| Pig | `net/minecraft/client/model/animal/pig/PigModel.java` + `QuadrupedModel.java` |
| Cow | `net/minecraft/client/model/animal/cow/CowModel.java` + `QuadrupedModel.java` |
| Chicken | `net/minecraft/client/model/animal/chicken/AdultChickenModel.java` |
| Spider | `net/minecraft/client/model/monster/spider/SpiderModel.java` |
| Sheep | Existing `build_sheep_head_mesh` in `main.rs` (unchanged) |

All sourced from MC 26.1.2 at:
`J:/Temp/template-mod-template-26.1.2/sources/minecraft-merged-52430b475d-26.1.2-sources/`

### Mobs covered and part counts

| Mob | Parts | UV atlas size |
|---|---|---|
| Sheep | 2 (body wool-tile + head entity-texture) | 64x32 (entity) |
| Zombie | 6 (head, body, R-arm, L-arm, R-leg, L-leg) | 64x64 |
| Chicken | 4 (head, body, R-leg, L-leg) | 64x32 |
| Pig | 6 (head, body, RF-leg, LF-leg, RB-leg, LB-leg) | 64x64 |
| Cow | 6 (head, body, RF-leg, LF-leg, RB-leg, LB-leg) | 64x64 |
| Skeleton | 6 (head, body, R-arm, L-arm, R-leg, L-leg) | 64x32 |
| Spider | 4 types (head, thorax, abdomen, leg - leg drawn x8) | 64x32 |

Total upload: up to 32 GPU mesh handles (each with its own albedo).

### Resource pack file paths searched

For each mob `<mob>` in `{zombie, chicken, pig, cow, skeleton, spider}`:
1. `<texture_dir>/../entity/<mob>/<mob>.tga`
2. `<texture_dir>/../entity/<mob>/<mob>.png`
3. `<texture_dir>/../entity/<mob>.tga`  (bedrock-flat fallback)
4. `<texture_dir>/../entity/<mob>.png`  (bedrock-flat fallback)

For sheep head (existing path, unchanged):
1. `<texture_dir>/../entity/sheep/sheep.tga`
2. `<texture_dir>/../entity/sheep/sheep.png`
3. `<texture_dir>/sheep.tga`
4. `<texture_dir>/sheep.png`

### MC box-UV layout formula

For a cube of size `w x h x d` at texture origin `(u0, v0)` (y-down):

```
face     pixel rect
top      (u0+d, v0)         .. (u0+d+w,   v0+d)
bottom   (u0+d+w, v0)       .. (u0+d+w+w, v0+d)
right    (u0,     v0+d)     .. (u0+d,     v0+d+h)
front    (u0+d,   v0+d)     .. (u0+d+w,   v0+d+h)
left     (u0+d+w, v0+d)     .. (u0+d+w+d, v0+d+h)
back     (u0+d+w+d, v0+d)   .. (u0+d+w+d+w, v0+d+h)
```

Implemented in `build_box_uv_mesh()` in
`examples/voxel_demo_mp/src/mob_meshes.rs`.

### Fallback behaviour

When no resource pack is detected or the entity texture file is absent:
- Each mob kind renders as a 2-cube silhouette (body + head) using the
  existing `player_cube` mesh tinted with the kind's hard-coded colour.
- Sheep fall back to the procedural-atlas wool tile on the body cube.
- The fallback is tested implicitly by the lib test suite which runs
  without any GPU context (all `MobMeshes` handles stay `None`).

### Deferred follow-ups

- **Per-mob walk animation**: limb rotation (xRot swing) driven by
  `walk_bob` phase - currently all parts use the same rigid body_rot.
  MC `setupAnim()` per-kind logic is a W20+ candidate.
- **Skin variants**: mooshroom (cow variant), wither skeleton, cave
  spider - each needs its own subdir search path.
- **Sheep wool overlay**: the fur/wool layer (separate `sheep_fur.tga`
  texture) is not yet rendered on top of the entity-texture body.
- **Second skin layer (64x64 humanoid)**: the outer armor/clothing layer
  (rows 32-64 on zombie/skeleton 64x64 texture) is not rendered.

## Wave 20 - Inventory + recipe book + creative tab UI fixes

### W20.1 - icon_uv_for / icon_uv_front helpers

Two new methods on `ResourcePack`
(`examples/voxel_demo_mp/src/resource_pack/mod.rs`):

- `icon_uv_for(block, normal) -> [f32; 4]`: resolves the UV rect for
  a block face. Tries `icon_ref` (which checks the item-icon table for
  tools/buckets/food, then the block top face) for the `[0,1,0]` normal,
  or `block_face_ref` for other normals. Falls back to the legacy
  `atlas_uv_rect` (4x4 synthetic grid) when the pack has no entry.
- `icon_uv_front(block) -> [f32; 4]`: shorthand for
  `icon_uv_for(block, [0,1,0])`. Matches what
  `BlockRenderDispatcher.getBlockModel().getQuads(..., Direction.UP)`
  produces for MC inventory icons.

### W20.2 - UV bug fix in slot widgets

Four call sites that still used `atlas_uv_rect` for icon UVs in
the UI pass were replaced with `icon_uv_front` / `icon_uv_for`:

1. `main.rs` hotbar icon closure (`icon()` lambda) - now calls
   `resource_pack.icon_uv_front(b)`.
2. `main.rs` craft output snapshot (`craft_output_snapshot`) - replaced
   `atlas_uv_rect(b, [0,1,0])` with `resource_pack.icon_uv_front(b)`.
3. `main.rs` recipe book input slot icons - replaced per-ingredient
   `atlas_uv_rect` call.
4. `main.rs` recipe book output slot icon - replaced output
   `atlas_uv_rect` call.

The creative tab at the `8953` site (the `unwrap_or_else` fallback
inside the `icon_ref` chain) was already correct and was updated to
`icon_uv_front` in the same pass.

MC source refs:
- `net/minecraft/client/gui/screens/inventory/CreativeModeInventoryScreen.java`
- `net/minecraft/client/gui/components/spectator/RecipeBookComponent.java`

### W20.3 - Creative tab polish

Changes in `creative_tab.rs` + `main.rs`:

- **Search box auto-focus**: `CreativeTabState` gained a `just_opened`
  one-shot flag. `open_now()` sets it; the egui pass calls
  `resp.request_focus()` on the search `TextEdit` and clears the flag
  the frame after.
- **ESC closes the creative tab**: the ESC key handler in
  `update_play` now closes `creative_tab` before falling through to
  the mp-menu / pause logic, mirroring the `E` key path.
- **Right-click = count 1**: the grid slot widget now uses
  `Sense::click_and_drag()` so `secondary_clicked()` fires. RMB adds
  a stack of exactly 1 to the hotbar; LMB still gives `max_stack`.
  Matches MC `CreativeModeInventoryScreen` pick-up behaviour.
- **Sort by display name**: the filtered entries are sorted by
  `item_display_name` before rendering so items within a category
  appear in alphabetical order.

### W20.4 - Recipe book polish

Changes in `main.rs`:

- **Search box**: a new `recipe_book_query: String` field on `Voxel`
  persists the live filter. The recipe book window renders a `Hledat:`
  text-edit; recipes are shown only when their output item's display
  name contains the query (case-insensitive substring).
- **Category headers**: recipes are grouped by the `ItemCategory` of
  their output (Bloky / Dekorace / Nastroje / Zbrane / Jidlo /
  Ostatni). A bold header is emitted the first time each category is
  encountered in the (already search-filtered) table.
- **Current match indicator**: if `craft_recipe_name` is set (the
  2x2 craft grid currently matches a recipe), a `Crafting: <name>`
  line is shown at the top of the recipe book window.
- **Highlighted output slot**: the output slot widget of the matching
  recipe uses `selected=true` so it draws with the gold outline.

MC source refs:
- `net/minecraft/client/gui/components/spectator/RecipeBookComponent.java`
- `net/minecraft/client/gui/screens/recipebook/RecipeBookTabButton.java`

### Deferred follow-ups

- **Drag-to-specific-hotbar-slot from creative tab**: the current
  click path always calls `deposit_in_hotbar` (first empty / top-up).
  A true drag-to-slot would require extending `SlotKind` with a
  `Creative` variant and wiring a `dnd_drag_source` on the creative
  grid entries. Tracked for W21.
- **Recipe categories as a first-class enum**: `Recipe` has no
  `category` field; W20 derives it from `item_category(output.id)` at
  render time. A static `RecipeCategory` field would make the sort
  order deterministic without depending on `item_category`.

## Wave 21 - MC-style main menu polish

Wave 21 overhauls the voxel_demo_mp main menu to feel Minecraft-like
without copying MC's proprietary pixel-art assets.

### W21.1 - Title banner + animated splash text

Source: `examples/voxel_demo_mp/src/menu/main_screen.rs`,
        `examples/voxel_demo_mp/src/menu/mod.rs`

The plain `ui.heading` is replaced by a layered title banner:

- "InfEngine" in 56 px bold white text.
- "Voxel Demo" subtitle in 18 px muted blue-grey.
- Splash text: yellow italic line below the subtitle, size pulsed
  at 4 Hz via `(t * 4.0).sin() * 0.05 + 1.0` scale factor.

Splash text source: inspired by MC's `TitleScreen.java` splash pool.
Our pool lives in `menu::SPLASH_LINES` (30 entries). A fresh line is
picked in `Menu::enter_main()` on each Main-screen entry using a
nanosecond-hash index so successive visits get different quips.

**Rotation deferred**: egui 0.33 does not expose per-widget rotation
without dropping to `Shape`-level transforms that break the sizing
pass. The splash text is rendered upright for now; angled rotation
(MC uses ~17 deg counter-clockwise) is a follow-up item.

### W21.2 - Background panorama

Already shipped in W12.6. The panorama backdrop is a single-axis
horizontal tile scroll (1 RPM, `panorama_scroll += 1/3600` per frame)
over a discovered `panorama_0..5.png` strip, darkened 43% by an RGBA
overlay. MC uses a full 6-face skybox (`PanoramaRenderer.java`); our
implementation uses a single stitched strip, which is visually close
at low resolution. No changes in W21.

### W21.3 - Button styling

Source: `examples/voxel_demo_mp/src/menu/shared.rs`

`big_button` and `small_button` now use a custom `mc_button_sized`
painter instead of the plain `egui::Button`. Visual deltas vs MC:

| Property | MC default | InfEngine W21 |
|---|---|---|
| Background fill | #5A5A5A stone-grey | #545454 normal, #6A6A6A hover |
| Pressed fill | darker | #3C3C3C |
| Top/left border | bright highlight | 1 px #AAAAAA |
| Bottom/right border | dark shadow | 1 px #282828 |
| Label colour | white | #FFFFA0 (pale yellow) |
| Label shadow | 1 px dark | 1.5 px semi-transparent black |
| Corner rounding | 0 px (sharp) | 2 px (slight) |

Disabled buttons use `#323232` fill and `#646464` label; enabled/
disabled state toggled via the `enabled` flag on `big_button`.

### W21.4 - Footer polish

Source: `examples/voxel_demo_mp/src/menu/shared.rs` (`footer_combined`)

The separate `footer_identity` + `footer_seed` labels on the Main
screen are replaced by a single horizontal row:
"Playing as: <name> | Seed: <seed>"

A new bottom-left `egui::Area` ("menu_pack_footer") shows
"MC pack: <name> | <count> blocks" when a real MC resource pack is
loaded (`pack_block_count > 0`). The count comes from
`resource_pack.textures.len()` and the name from
`resource_pack.source.file_name()`.

The bottom-right version footer (`footer_version`) is unchanged.

### W21.5 - Pause menu unification

Source: `examples/voxel_demo_mp/src/menu/pause.rs`

The inline `AppState::Paused` egui block in `main.rs` is extracted to
`menu::pause::draw(ctx, world_seed, actions)` which returns a vec of
`PauseAction` variants (Resume / Settings / MainMenu). The caller in
`main.rs` maps these back to the existing boolean action flags so the
transition logic is unchanged.

Visual: same `big_button` widgets as the main menu, 50% black tint
over the world rendered via a background-layer `egui::Painter` before
the central panel so the 3-D world is visible but dimmed.

The "Paused" heading uses 36 px bold white text (vs the old
`ui.heading` default).

### Deferred follow-ups

- **Splash text rotation**: ~17 deg counter-clockwise as in MC's
  `TitleScreen`. Requires `egui::Shape::text` + manual affine
  transform on the `Painter`; the sizing/layout implications need
  investigation. The text currently renders upright.
- **Full 6-face skybox panorama**: MC's `PanoramaRenderer` uses 6
  cube faces stitched into a fisheye projection rotating at 1 RPM.
  Our strip-scroll is a coarse approximation; a proper cube-map
  renderer would be a dedicated graphics wave.
- **Dirt/gravel tile fallback for background**: when no panorama PNG
  is discovered, the current fallback is a solid dark-navy fill. A
  procedural tiled "dirt" pattern (drawn via `egui::Painter` quads)
  would match MC's fallback more closely.

## Wave 22 - Extreme view distance + worker streamline + misc cleanup

### W22.1 - Extreme view distance (VIEW_DISTANCE_MAX = 32)

`VIEW_DISTANCE_MAX` was already set to 32 in `main.rs` but several
gate sites still capped at 12 or 16:

- Both egui view-distance sliders (in-game settings panel + debug
  panel) now use `2..=VIEW_DISTANCE_MAX as u8` with `.step_by(1.0)`.
- The config-file parser `"view_distance"` branch clamp raised to
  `VIEW_DISTANCE_MAX`.
- `menu/mod.rs` `OptionsSnapshot::clamp` raised from `clamp(2,16)`
  to `clamp(2, VIEW_DISTANCE_MAX as u8)`.
- `menu/options.rs` slider raised from `2..=16` to
  `2..=VIEW_DISTANCE_MAX as u8`.

Performance fix for large distances: `stream_chunks` converted
`to_unload: Vec<ChunkCoord>` to a `HashSet<ChunkCoord>` so the
per-mesh-entry lookup is O(1) instead of O(n). Similarly
`rebuild_chunks` now builds a temporary `HashSet` from its `coords`
slice before draining `chunk_meshes`, removing an O(chunk_meshes * coords)
inner scan.

At `view_distance=32` the worst-case loaded chunk count is
`(2*32+1)^2 * 16 = 67600` chunks.

### W22.2 - Worker pool: drop legacy submesh computation

Workers previously ran `chunk_mesh_with_atlas` (legacy 4x4-grid
atlas) and shipped a `submeshes: Vec<ChunkSubmesh>` field in
`WorkerResult`. The main thread immediately discarded these on the
next deferred-remesh tick when `build_lit_submeshes` ran the proper
MC-atlas + special-models + lighting path.

Implemented option (a): workers now ship `chunk + side_effects` only.
The new chunk is pushed to `pending_remesh` at delivery so
`build_lit_submeshes` fires on the very next deferred-drain cycle.
This eliminates the dead `chunk_mesh_with_atlas` call and shrinks
`WorkerResult` payload.

### W22.3 - Misc cleanup

- **Autocomplete**: no open TODOs or FIXMEs found.
- **Mob persistence**: confirmed already implemented in
  `world_save/runtime/helpers/entities.rs` - `build_dimension_entities_from`
  + `apply_dimension_entities` serialise/restore the full mob list.
- **Recipe book**: W20.2 `icon_uv_front` routes through the MC atlas
  at all 4 sites; no regression found.
- **Slider step lag**: added `.step_by(1.0)` to `fov_deg` sliders in
  the debug panel (`main.rs`) and the Options screen (`menu/options.rs`).

### Deferred follow-ups

- **Mob persistence - velocity**: `vel` is stored as `[0.0,0.0,0.0]`
  in `MobSave`; mobs restart stationary on load. Tracking momentum
  across save/load would require a `vel` field in `MobSave`.
- **Parallel meshing (W22.2 option b)**: routing `build_lit_submeshes`
  into workers would unlock full parallel meshing at the cost of
  passing the `ResourcePack` + `LightingState` snapshots through the
  job queue. Deferred as a future wave.

## Wave 23 - Loading + streaming perf hotfix

### W23.1 - Splash text alpha pulse (layout stability)

The main menu "Now with chunks!" splash used a scale-pulse
(0.95..1.05x of 16 px base size). Changing `RichText::size` each
frame resizes the label bounding box, which egui's vertical layout
propagates upward - the button stack visibly shifted every frame.

Fix: alpha-pulse instead (0.6..1.0 opacity, ~1.6 s period). The
label stays at a fixed 16 px height so the surrounding layout is
stable. Italic + yellow colour preserved.

### W23.2 - Remove blocking rebuild_all_meshes at load end

After all chunks arrived (`phase == GeneratingChunks && live >= target`)
the old code called `rebuild_all_meshes()` synchronously. At
`view_distance=4` that is ~1296 chunks * several `create_mesh` +
`bind_mesh_albedo_arc` GPU calls each = 10+ s blocking on the main
thread while the loading screen showed 99%.

The chunks had ALREADY been meshed individually by `stream_chunks`
via the `pending_remesh` queue as each one arrived - `rebuild_all_meshes`
was therefore redundant (it discarded and re-created the same meshes).

Fix: on entering `BuildingMeshes` phase, push every chunk into
`pending_remesh`. The normal per-frame drain loop (16 chunks per
frame + 4 ms soft cap) spreads the re-lighting mesh across ~200 ms
while the loading screen shows "Stavim chunky..." making visible
progress. The transition to `Playing` now gates on
`pending_remesh.is_empty()` instead of happening immediately.

MC source reference: `ChunkRenderDispatcher.compile()` schedules
N compile tasks per frame asynchronously; we approximate this via
the per-frame budget approach.

### W23.3 - In-game chunk streaming stutter fixes

Three sub-fixes applied to `stream_chunks`:

**W23.3a - Raised per-frame budgets**

`RESULTS_PER_FRAME`: 2 -> 8.
`REMESH_DEFERRED_PER_FRAME`: 2 -> 16.

The old 2/2 values were tuned for a conservative worst case but
actively caused queue flooding during normal play - the complaint
was that the game stuttered badly after loading.

**W23.3b - 4 ms soft time cap on remesh drain**

The drain loop is now also bounded by `Duration::from_millis(4)`.
On fast hardware more chunks drain per frame; on a slow frame the
loop exits early. `REMESH_DEFERRED_PER_FRAME` (16) is the hard
ceiling. Matches MC's `ChunkRenderDispatcher` time-slice approach.

**W23.3c - Dedup pending_remesh via HashSet mirror**

Added `pending_remesh_set: HashSet<ChunkCoord>` as a mirror of
`pending_remesh`. Each enqueue site now guards with
`pending_remesh_set.insert(coord)` - a chunk already in the queue
is silently skipped. Pop sites call `pending_remesh_set.remove`.
All `clear()` calls drain both structures.

Without dedup: 5 chunks arriving in one frame each list the same
neighbour, pushing it 5 times into the queue. At `view_distance=32`
(67600 chunks) the queue could accumulate thousands of redundant
entries per second, keeping `REMESH_DEFERRED_PER_FRAME` permanently
maxed out and causing persistent stutter.

## Wave 24 - Parallel chunk meshing in worker threads

### Motivation

W22-W23 moved generation to workers but meshing (`build_lit_submeshes`)
stayed on the main thread - every arriving chunk triggered a full
MC-atlas + lighting + special-model pass on the main thread, causing
visible stutter during initial world load.

MC source reference: `ChunkRenderDispatcher.compile()` runs N async
compile tasks per frame. W24 mirrors this: N worker threads now each
run a full `mesh_chunk_in_worker` call concurrently. Main thread only
does GPU upload (`renderer.create_mesh`) which cannot be threaded.

### Changes

**W24.1 - LightGrid Clone + Arc<ResourcePack>**

`LightGrid` gains `#[derive(Clone)]` (4096 bytes per grid; cheap to
copy for the halo snapshot). `Voxel::resource_pack` changes from
`ResourcePack` to `Arc<ResourcePack>` so workers share a reference
without locking. All three construction sites wrap with `Arc::new`.

**W24.2 - HaloLighting + extended ChunkJob**

New type `lighting::HaloLighting` (derives Clone + Default): owns 7
`Option<LightGrid>` slots - own chunk + +X/-X/+Z/-Z/+Y/-Y neighbours.
The `lookup(own_coord, world)` method maps a world voxel coord to the
correct slot and returns `[block_light, sky_light]` in `0.0..=1.0`,
falling back to `[0.0, 1.0]` when the slot is None (no block light,
full sky - same as the pre-W24 main-thread fallback).

`ChunkJob` is extended with `pack: Arc<ResourcePack>` and
`halo: HaloLighting`. `WorkerResult` gains `submeshes: Vec<ChunkSubmesh>`.

**W24.3 - mesh_chunk_in_worker free function**

Replicates the closure setup of `build_lit_submeshes` but captures
`&ResourcePack` + `&HaloLighting` instead of `&self`. Calls
`chunk_mesh_with_atlas_light_anim_and_special` via the same path.
Called at the end of each worker iteration after generation + side-
effect scan.

**W24.4 - Main-thread integration**

`stream_chunks` dispatch path: snapshot `Arc::clone(&resource_pack)` +
pre-fetch 6-neighbour halo grids (cloned from `LightingState`), ship
with each job.

`stream_chunks` delivery path: on `WorkerResult` arrival, upload
`result.submeshes` directly via `renderer.create_mesh` +
`bind_mesh_albedo_arc` instead of enqueueing to `pending_remesh`. The
new chunk itself is NOT enqueued - it was just meshed by the worker.
The 4 horizontal neighbours ARE still enqueued into `pending_remesh`
because their boundary faces depend on the new chunk's voxels.

### Tradeoffs

- Workers cannot access the GPU. Mesh data ships as
  `Vec<MeshVertex> + Vec<u32>` (already the shape of `ChunkSubmesh`).
- Halo snapshot is taken at enqueue time. If lighting propagates
  for the neighbours AFTER enqueue, the worker's baked light will be
  stale until the next `tick_lighting` triggers a `rebuild_chunks`
  remesh. This is acceptable: initial load gets correct neighbour
  geometry, lighting update pass runs afterwards and triggers a
  boundary remesh with fresh values.
- Block edits + neighbour remeshes triggered by `pending_remesh` still
  run `build_lit_submeshes` on the main thread (W25 could parallelise
  these too via `RemeshJob` dispatched to the worker pool).

### Shipped in W25

Neighbour remeshes from `pending_remesh` and all block-edit remeshes
are now fully parallel (see below).

## Wave 25 - Parallel remesh dispatch

### Goal

Eliminate the last remaining synchronous `build_lit_submeshes` calls
on the main thread. After W24, new-chunk meshing was parallelised via
`Job::Generate`; block-edit and neighbour remeshes still blocked the
main thread. W25 moves them to the worker pool.

### Generate vs Remesh job split

`WorkerPool` now accepts a `Job` enum with two variants:

```
Job::Generate(ChunkJob)   -- full worldgen + side-effect scan + mesh
Job::Remesh(RemeshJob)    -- mesh-only; skips worldgen + scan
```

`RemeshJob` carries a clone of the chunk voxels (~8 KB), an
`Arc<ResourcePack>`, a `HaloLighting` snapshot, and a `revision: u64`
counter so stale results can be identified on delivery.

`WorkerResult` is likewise an enum:

```
WorkerResult::Generated { coord, chunk, side_effects, submeshes }
WorkerResult::Remeshed  { coord, revision, submeshes }
```

### Why block edits + neighbour remesh are now non-blocking

`rebuild_chunks` no longer calls `build_lit_submeshes`. Instead it:

1. Clones the chunk voxels from `World` (fast, ~8 KB memcpy).
2. Snapshots the 6-neighbour `HaloLighting` grids (same pre-fetch as
   Generate jobs).
3. Bumps `chunk_revisions[coord]` to the next revision.
4. Sends `Job::Remesh(RemeshJob { ... , revision })` to the pool.

The actual meshing runs on a worker thread. On delivery the main thread
pattern-matches `WorkerResult::Remeshed`, drops old GPU handles, and
uploads the new submeshes. No `build_lit_submeshes` call on the main
thread at all.

Falling back to the synchronous `rebuild_chunks_sync` path (kept for
headless tests and pre-init callers that have no worker pool).

### Chunk-revision race guard

MC reference: `LevelRenderer.setBlocksDirty()` schedules a
`ChunkRenderDispatcher::CompiledChunk` rebuild on the chunk's render
task queue - a later edit supersedes an earlier job for the same chunk.

We mirror this: every `rebuild_chunks` call increments
`chunk_revisions[coord]`. The `RemeshJob` captures the revision at
dispatch time. On `WorkerResult::Remeshed` delivery the main thread
compares `result.revision` vs `chunk_revisions[coord]`. If the result
revision is strictly less than the current counter (i.e., another edit
fired before this result arrived), the result is discarded. The second
edit's result will arrive later with the higher revision and will be
applied.

This prevents a sequence like:

```
edit A -> revision 1 -> dispatch R1
edit B -> revision 2 -> dispatch R2
R1 arrives  (stale: 1 < 2) -> dropped
R2 arrives  (current: 2 == 2) -> applied
```

without this guard, R2 would arrive and be applied, then R1 would
overwrite it with a mesh that does not reflect edit B.

### Budget constants

- `RESULTS_PER_FRAME`: raised from 8 -> 32 (Generate + Remesh results
  share the budget; time-cap on the frame keeps GPU upload cost bounded
  regardless).
- `REMESH_DEFERRED_PER_FRAME`: raised from 16 -> 64 (dispatching a
  `Job::Remesh` is O(1) Arc clones + channel send, not O(mesh_size);
  the hard ceiling is no longer the bottleneck).
- Time-budget cap on the pending_remesh drain loop removed (was 4 ms
  per frame; dispatch is now microseconds, not milliseconds).

## Wave 26 - Fix dispatch/upload overhead

### Why W25 stuttered worse than the original

W25 moved meshing off-thread but bumped per-frame budgets too high, so
dispatch + GPU upload on the main thread did MORE work per frame than
the original synchronous mesher:

1. **Dispatch cost not free**: each `rebuild_chunks` call did
   `chunk.clone()` (~8 KB malloc) plus constructing `HaloLighting` with
   7 `Option<LightGrid>` clones (~7 x 4096 bytes = ~28 KB malloc). With
   `REMESH_DEFERRED_PER_FRAME = 64` that was ~4 MB of allocator pressure
   per frame on the main thread - enough to cause stutter by itself.

2. **GPU upload churn**: `RESULTS_PER_FRAME = 32` drained up to 32
   results per frame, each with up to 4 submeshes = up to 128 wgpu
   buffer allocs + queue submits + bind group setups per frame.

3. **O(n*R) mesh removal**: `chunk_meshes` was a flat `Vec`; removing
   the old GPU handles for a chunk (Remeshed delivery) required scanning
   the entire Vec - O(n) per result, O(n*R) per frame where n can be
   tens of thousands at view_distance=8+.

### Arc<LightGrid> (W26.1)

`LightingState::grids` changed from `HashMap<ChunkCoord, LightGrid>` to
`HashMap<ChunkCoord, Arc<LightGrid>>`. `HaloLighting` fields changed
from `Option<LightGrid>` to `Option<Arc<LightGrid>>`.

Effect: snapshotting halo lighting for a job is now 7 refcount bumps
(~56 bytes) instead of 7 x 4096-byte copies (~28 KB). Mutable access
(BFS engines) goes through `Arc::make_mut()` for copy-on-write
semantics: unique arcs are mutated in place, shared arcs (held by an
in-flight worker) get a fresh copy only when actually needed.

MC reference: `ChunkLightProvider` in
`net/minecraft/world/level/lighting/` holds light data in arrays that
the chunk-rendering task accesses concurrently with the main thread; MC
uses copy-on-write snapshots for the same reason.

### Arc<Chunk> (W26.2) - deferred to W27

The `chunk.clone()` (~8 KB) in `rebuild_chunks` is the remaining malloc
per dispatch. Eliminating it requires changing `World::chunks` from
`HashMap<ChunkCoord, Chunk>` to `HashMap<ChunkCoord, Arc<Chunk>>` in
the `infengine_voxel` crate (public API change), updating all 65+
call sites in `main.rs`, and fixing `voxel_demo_server`. Deferred to
W27 to keep W26 scope manageable.

### New per-frame budgets (W26.3)

- `RESULTS_PER_FRAME`: 32 -> 4. GPU upload is expensive (wgpu buffer
  alloc + queue submit + bind group per submesh); fewer per frame keeps
  the main thread free.
- `RESULTS_BUDGET_MS = 2`: new soft time cap on the GPU upload drain
  loop. Caps wall-clock upload cost regardless of result count.
- `REMESH_DEFERRED_PER_FRAME`: 64 -> 8. At 8 remesh dispatches/frame
  and ~8 KB chunk clone per dispatch, allocator pressure drops from
  ~512 KB/frame (was ~4 MB/frame at 64). Will drop to ~0 after W27
  Arc<Chunk> lands.

MC reference: `ChunkBuilder::compile()` runs N async render tasks per
frame but caps GPU buffer uploads to ~4-8 per frame via
`ChunkRenderDispatcher.maxTotalUploadSize` to prevent frame spikes.

### O(1) chunk mesh removal (W26.4)

`chunk_meshes` changed from `Vec<(ChunkCoord, BlockId, MeshHandle)>` to
`HashMap<ChunkCoord, Vec<(BlockId, MeshHandle)>>`. Per-chunk removal is
now a single `HashMap::remove()` call instead of a full Vec scan.

Affected paths: `WorkerResult::Remeshed` delivery, `rebuild_chunks_sync`
(synchronous fallback), `stream_chunks` chunk-unload path, and all
drain-all sites (dim switch, world reset, `/seed` command).

## Wave 27 - Lighting BFS off main thread + loader counter fix

### Loader counter mismatch (W27.1)

Before W27, `build_world` enqueued `INITIAL_CHUNKS=3` radius ->
7x7x16=784 chunks as the `target_chunks`. But `stream_chunks` runs
during loading with `view_distance=4` -> 9x9x16=1296, so the
loading counter showed "1296/784 (100%)" - nonsensical.

**Fix**: `build_world` now uses `self.settings.view_distance` (clamped
the same way `stream_chunks` does) to compute the initial enqueue
radius. `target_chunks` matches the live footprint, counter reaches
100% cleanly. `INITIAL_CHUNKS` constant removed.

### Lighting BFS off main thread (W27.2 - W27.5)

**Root cause of 2-minute loading freeze**: `rebuild_all_lighting` ran
the full BFS synchronously over ALL loaded chunks (1300+ at
`view_distance=4`) before transitioning to the Playing state. For a
complex world this took 1-2 minutes on the main thread.

**Root cause of gameplay stutter**: `tick_lighting` ran block + sky BFS
over a 27-chunk halo (the dirty chunk + 26 neighbours) synchronously at
~10 Hz on the main thread. Every block edit introduced a frame spike.

**Solution**: introduced `LightingWorldAccess` trait so the BFS engines
can run against both the live `World` (main thread) and an owned
`ChunkSnapshot` (worker thread). Workers never borrow from the
main-thread World; the main thread clones the affected chunks into the
snapshot before dispatch.

- `propagate_block_light` + `propagate_sky_light` now take
  `&impl LightingWorldAccess` instead of `&World`.
- `ChunkSnapshot { chunks: HashMap<ChunkCoord, Chunk> }` implements
  `LightingWorldAccess` for the worker side.
- New `Job::Lighting(LightingJob)` / `WorkerResult::Lit` variants carry
  the owned snapshot + return the new `Arc<LightGrid>` values.
- `tick_lighting`: with worker pool -> dispatches `Job::Lighting` (~216
  KB snapshot for a 27-chunk halo); main thread returns immediately;
  remesh is triggered on `WorkerResult::Lit` delivery. Without worker
  pool (headless tests): synchronous fallback unchanged.
- `rebuild_all_lighting`: with worker pool -> dispatches ONE
  `Job::Lighting` for all loaded chunks. `pending_lighting_jobs`
  counter lets the loading phase wait for `WorkerResult::Lit` before
  advancing to `BuildingMeshes`. Without worker pool: synchronous as
  before.

**MC reference**: `LightingProvider` runs on a background thread
scheduler; chunks marked dirty enqueue light tasks via
`LightChunkGetter`. We now mirror this: dirty chunks enqueue a
`LightingJob` to the worker pool; the result arrives a few frames
later and triggers remesh.

### Arc<LightGrid> swap (W27.5)

`LightingState::set_grid(coord, Arc<LightGrid>)` atomically replaces
an existing grid. Any `HaloLighting` snapshot held by an in-flight
remesh job sees the old `Arc` via COW - no race between the lighting
worker and the mesh worker.

## Wave 28 - Full perf overhaul

Performance improvements across the chunk streaming pipeline and render path.
All items shipped in a single wave to avoid incremental regressions.

### W28.1 - Priority queue for pending chunks

`Voxel::pending_chunks` changed from `VecDeque<ChunkCoord>` to
`BinaryHeap<PendingChunk>`. `PendingChunk` stores `neg_dist_sq: i64`
(negated squared distance from player) so `BinaryHeap::pop` returns the
closest chunk first. Tie-breaking on `coord` makes the order deterministic.

### W28.2 - Per-frame dispatch cap

The chunk-dispatch loop now caps at `workers * 2` jobs per frame so the
crossbeam channel is never flooded. Counter resets each tick.

### W28.3 - Vertical chunk range

`Voxel::vertical_chunk_range(player_y)` computes `cy +/- 3` clamped to
`[0, VERTICAL_CHUNKS-1]`. `build_world` and `stream_chunks` use this so only
6-7 Y-slices around the player load at any time (was 16).

### W28.4 - Greedy meshing

`infengine_voxel::chunk_mesh_greedy` and `chunk_mesh_greedy_with_special`
merge coplanar same-material faces (same block+light+anim_id quantised to
4-bit steps) into larger quads using the Mikola Lysenko greedy algorithm.
Special-shaped blocks handled by `emit_special` are emitted first; the cube
path skips them. Typical terrain: 30-60% fewer opaque faces. Both demo
mesher call sites use `chunk_mesh_greedy_with_special`.

UV scaling: the merged quad UV rect is widened proportionally to the
rectangle's voxel extents so tiling block textures repeat correctly.

### W28.5 - Two-queue incremental block-light update

`propagate_block_light_incremental(world, state, affected, pos, old_e, new_e)`
mirrors MC's two-phase `BlockLightEngine`:

1. Unlight BFS: zero cells reachable only from the old emitter.
2. Relight BFS: propagate surviving + new emitter light into cleared region.

`LightingState` gains `dirty_changes: Vec<([i32;3], u8, u8)>` and
`mark_dirty_with_change`. `world_set_tracked` records the old/new emission
before mutating the world. `tick_lighting` uses the incremental path for
single emitter-only changes; falls back to full re-flood for opaque block
changes or multi-voxel dirty sets.

### W28.6 - Vertex format packing

`MeshVertex` packed from 44 to 28 bytes (36% reduction):

| field      | old type  | new type  | offset |
|------------|-----------|-----------|--------|
| position   | `[f32;3]` | `[f32;3]` | 0      |
| normal_pad | `[f32;3]` | `[i8;4]`  | 12     |
| uv_packed  | `[f32;2]` | `[u16;2]` | 16     |
| light_pad  | `[f32;2]` | `[u8;4]`  | 20     |
| anim_id    | `u32`     | `u32`     | 24     |

wgpu vertex attributes updated to Sint8x4, Uint16x2, Unorm8x4.
`mesh3d.wgsl` VertexInput decodes packed values in the vertex shader.
Decode helper methods `.normal()/.uv()/.light()` added for tests.

### W28.7 - Phase-adaptive budgets

`current_results_budget() -> (usize, u64)` returns `(32, 8ms)` during
`LoadingPhase` and `(4, 2ms)` during `Playing`. Keeps the loading phase
fast without starving the game loop.

### W28.8 - Diagnostic spam removal

Removed 3 `tracing::warn!` calls that fired every frame in `render_ui`
and before the renderer draw call.

### W28.9 - Icon UV table cache + stream_delivered reuse

`icon_uv_table` is rebuilt only when the animation tick counter changes
(50ms intervals). `stream_delivered: Vec<WorkerResult>` uses
`std::mem::take`/`drain`/reassign to reuse capacity across frames.

### W28.10 - crossbeam-channel MPMC

`std::sync::mpsc` replaced with `crossbeam_channel` unbounded MPMC channels.
Removes the `Arc<Mutex<Receiver>>` wrapper; workers call `req_rx.recv()`
directly. Eliminates lock contention under high worker counts.

### W28.11 - Mob frustum culling

`Frustum::contains_point(point: Vec3) -> bool` added with a 2-voxel margin.
The mob render loop builds `mob_frustum` once per frame and skips any mob
whose position is outside the frustum.

### W28.12 - Test suite

`wave28_tests.rs`: 18 tests covering PendingChunk heap ordering,
`vertical_chunk_range` boundary conditions, `MeshVertex` encode/decode
round-trips, `Frustum::contains_point`, incremental lighting torch
place/remove, and greedy mesher face-count reduction.

### W28.13 - Documentation

This section documents all Wave 28 changes in `docs/design/mc_fidelity.md`.

## Wave 29 - Creative tab + mob skin + chunk priority fixes

### W29.1 - Fix icon_uv_for fallback (rainbow stripes bug)

`ResourcePack::icon_uv_for` previously fell back to `block::atlas_uv_rect`
when neither `icon_ref` nor `block_face_ref` resolved an entry. That helper
was calibrated for the 64x64 placeholder atlas (4x4 grid, 0.25 u_step); on
a 2048x2048 MC atlas the same formula mapped the ENTIRE atlas into one slot,
producing vertical rainbow stripes in the creative tab.

Fix: fallback now returns `[0, 0, 16/atlas_w, 16/atlas_h]` - the top-left
16x16 tile of whatever atlas is currently loaded. On the placeholder pack
this is the same stone swatch. On a live MC atlas this is stone (first row).

Tests added in `wave29_tests.rs`:
- `icon_uv_for_unknown_block_returns_safe_constant`
- `icon_uv_for_fallback_does_not_span_full_atlas`
- `icon_uv_front_consistent_with_icon_uv_for`

### W29.2 - Creative tab blacklist

Added `CREATIVE_BLACKLIST: &[u16]` constant to `creative_tab.rs` and
filtered it from `available_items()`. Excluded ids:
- 36 (water source): use water bucket
- 37 (lava source): use lava bucket
- 47 (portal): spawns via flint+steel ignition only
- 49 (fire): spawns via flint+steel / lava-burn only
- 56 (soul_fire): technical world block
- 78 (oak_door_upper): internal upper-half block, never held by player

Mirrors MC's `Items.java` creative-mode tab exclusion logic.

Tests: 7 tests in `wave29_tests.rs` covering each blacklisted id plus
retained normal blocks (stone=1, torch=10, chest=41).

### W29.3 - mob_skin_path resolves .tga + .png

W19 regression: `mob_skin_path` in `mob_skin.rs` only probed `<mob>.png`.
Many Bedrock resource packs ship `<mob>.tga`; so `full_skin_set_present`
and `MobSkinSet::discover` returned `None` for every mob even when valid
TGA files existed, causing all mob textures to vanish.

Fix: `mob_skin_path` now tries `.tga` first, then `.png`.

The sheep head loader and `load_non_sheep_mob_meshes` already handled
both extensions via their local `try_open_entity` helper - this fix
brings the public discovery API into parity.

Tests in `mob_skin.rs`:
- `mob_skin_path_resolves_tga`
- `mob_skin_path_prefers_tga_over_png`
- `full_skin_set_present_with_tga_files`

### W29.4 - Deferred items

- Fog distance tied to view_distance: no fog API in the renderer yet.
  Deferred to a future wave when the renderer gains `set_fog_distance`.
- Mob persistence (`mobs.save`): mobs are not persisted across world
  reload; documented as a known limitation.

### W29.5 - Boost chunk dispatch cap during fast movement

Raised the per-frame chunk dispatch cap from `worker_count * 2` to `32`
when the player's horizontal speed exceeds 6 m/s (sprinting / flying).
Below the threshold the conservative cap applies. Addresses void-walking
artifacts where the priority queue drained too slowly at the leading edge
of a fast-moving player's view frustum.

### W29.6 - Test suite

`wave29_tests.rs`: 10 tests covering icon_uv_for safe fallback and the
creative tab blacklist. Mob skin TGA tests live in `mob_skin.rs` (3 tests).
Total test count after W29: 1495 pass, 7 pre-existing fail (MC-install
detection tests that expect no MC assets but a real install is present).

## Wave 30 - Greedy mesh re-enable + fog + shadow audit

### W30.1 - Vertex layout extension (40 bytes per vertex)

`MeshVertex` in `crates/infengine_render/src/mesh3d.rs` extended from 28 to
40 bytes by adding three new `[u16; 2]` fields:

| field       | offset | bytes | wgpu format | description |
|-------------|--------|-------|-------------|-------------|
| position    | 0      | 12    | Float32x3   | world-space XYZ |
| normal_pad  | 12     | 4     | Sint8x4     | normal * 127, 4th byte = 0 |
| uv_packed   | 16     | 4     | Uint16x2    | legacy UV (/ 65535.0) |
| light_pad   | 20     | 4     | Unorm8x4    | block/sky light |
| anim_id     | 24     | 4     | Uint32      | animation slot |
| tile_uv0    | 28     | 4     | Uint16x2    | atlas tile origin / 65535 |
| tile_size   | 32     | 4     | Uint16x2    | atlas tile extents / 65535 |
| quad_uv     | 36     | 4     | Uint16x2    | per-corner [0,16] / 65535 * 16 |

Backward compat: `with_uv(uv)` sets `tile_uv0 = uv_packed` and `tile_size = 0`
so the shader outputs `tile_uv0 + fract(0) * 0 = tile_uv0 = uv` unchanged.

### W30.2 - Shader fract approach for atlas tiling

`mesh3d.wgsl` vertex shader computes:

```
tile_uv = tile_uv0 + fract(quad_uv) * tile_size
```

where `quad_uv` carries the per-corner UV in voxel coordinates (e.g. `[0, w]
x [0, h]` for a `w x h` merged quad). `fract()` wraps the tiling so the
final UV always stays within `[tile_uv0, tile_uv0 + tile_size]` regardless
of how large the merged quad is. This fixes the W28.4 UV-bleed bug where
merged quads sampled neighbouring atlas tiles.

New vertex inputs in `VertexInput`: `@location(11) tile_uv0_packed`,
`@location(12) tile_size_packed`, `@location(13) quad_uv_packed`.

### W30.3 - Greedy meshing re-enabled (perf delta)

`chunk_mesh_greedy_with_special` is now the default mesher in both
`build_lit_submeshes` and `mesh_chunk_in_worker` in `voxel_demo_mp/main.rs`.
The W28.4 revert comment is replaced with a W30.3 note.

A new `emit_element_quad_tiled` function in `infengine_voxel/src/lib.rs`
populates the tile attrs for greedy quads. Per-voxel quads still use
`emit_element_quad` which calls `with_uv` -> compatible fallback path.

Estimated performance delta: fully uniform chunks reduce from O(N^2) faces
to 6 faces (one per axis direction). Typical mixed terrain sees 30-60%
fewer faces. At 40 bytes/vertex vs 28 bytes/vertex (43% larger) the net
upload bandwidth still wins because face count dominates vertex size.

### W30.4 - Tests

New tests in `crates/infengine_render/src/mesh3d.rs`:
- `with_tile_uv_round_trips`: verifies pack/decode round-trip within
  quantisation tolerance.
- `with_uv_sets_tile_uv0_and_zero_size`: verifies backward compat path.
- Updated `mesh_vertex_size_matches_attribute_layout` to expect 40 bytes.

### W30.5 - Fog at view_distance = 32

`Camera3DUniform` extended with `fog: [f32; 4]` (fog_start, fog_end) and
`sky_color: [f32; 4]` at offsets 160 and 176. The fragment shader applies
`smoothstep(fog_start, fog_end, dist)` to blend geometry toward sky color.

`Renderer::set_fog(fog_start, fog_end, sky_color)` exposes the API.
`voxel_demo_mp` calls it each frame with:
- `fog_start = (view_distance - 2) * 16`
- `fog_end = view_distance * 16 * 0.95`
- `sky_color = current sky tint` (same value passed to set_clear_color)

Chunks at the view edge now fade smoothly to sky colour instead of hard
pop-clipping.

### W30.6 - Shadow pass investigation outcome

Investigated the ghost-world artifact (smaller duplicate world visible in
upper-left corner of an earlier screenshot). Hypothesis was a shadow pass
leak (shadow map rendered into the surface as a color attachment).

Result: NOT the shadow pass. Evidence:
- No debug overlay code exists in the codebase (searched for
  `debug_shadow`, `show_shadow_map`, `draw_shadow_debug`, `shadow_overlay`).
- Shadow map is created with `RENDER_ATTACHMENT | TEXTURE_BINDING` but is
  only ever bound as a depth-stencil attachment (depth-only pass, zero color
  attachments) and as a `TextureSampleType::Depth` sampler input.
- Render order is correct: shadow -> mesh3d -> particle -> 2D -> overlay.

Investigation documented in `crates/infengine_render/src/shadow.rs` above
`SHADOW_MAP_SIZE`. Most likely cause: Windows/wgpu DPI artifact on resize.

### W30.7 - Deferred

UberGpuBuffer batching (W30.X.x) deferred as a separate massive refactor.
AO (ambient occlusion) deferred.

Total test count after W30: see CI output (lib + bin tests).
Pre-existing fail count stays at 7 (MC-install detection tests).

## Wave 32 - UberGpuBuffer batching

### Problem

At view_distance=8, rendering ~512 visible chunks each with 1-5 submeshes
produced 2000-5000 `draw_mesh_pbr` calls per frame. Each call triggered
three GPU state changes: `set_vertex_buffer`, `set_index_buffer`,
`draw_indexed`. CPU-side wgpu/driver overhead was the primary bottleneck;
GPU was mostly idle.

### Solution: shared vertex/index pool + multi_draw_indexed_indirect

All chunk meshes share two fixed-size GPU buffers:
- Vertex pool: 128 MB / 40 bytes per vertex = ~3.3 M vertices
  (covers ~1600 chunks of typical greedy-meshed density with 2x headroom)
- Index pool:  32 MB / 4 bytes per index = ~8 M indices

Per-frame draw submission:
1. `queue_chunk_draw(handle, model, color, ...)` pushes a
   `DrawIndexedIndirectArgs` + `MeshInstance` into a per-frame Vec. O(1).
2. At `render()` time, the Vec is uploaded to a GPU indirect command buffer
   and a separate instance buffer.
3. One `multi_draw_indexed_indirect(buf, offset, count)` call per layer
   (opaque + transparent) dispatches all chunk draws in one GPU call.

Result: 2000-5000 draw calls -> 1-4 GPU calls (two layers x two passes:
main + shadow). CPU bottleneck is effectively eliminated for chunk geometry.

### Allocator design

`UberMeshAllocator` (crates/infengine_render/src/uber_buffer.rs):
- Primary path: bump allocator. O(1) alloc, monotonic watermark.
- Free path: slot pushed to `Vec<FreeSlot>` when chunk unloads.
- Next alloc checks free list first (first-fit). O(n) worst case but
  n is small (free list size ~ # unloaded chunks in flight).
- Defragmentation / TLSF deferred to W33.

### wgpu feature requirements

`INDIRECT_FIRST_INSTANCE` (WebGPU / DX12 / Vulkan / Metal): needed so
`first_instance` in the indirect args can index the per-frame instance
buffer. Requested at adapter init; fallback to a loop of
`draw_indexed_indirect` calls (one per chunk) if unavailable. Even the
fallback avoids per-draw vertex/index rebinds.

`INDIRECT_EXECUTION` (downlevel flag): required for `draw_indexed_indirect`
itself. Supported by all wgpu primary backends.

### MC reference

Minecraft's `SectionRenderDispatcher` builds per-section vertex/index
buffers and issues `multi_draw_indirect` (or `MultiDrawIndirectFirstVertexID`
on older GL) via its equivalent of this path. The 128 MB pool size follows
the same order of magnitude as MC's `VertexBuffer` pool.

### Files changed

- `crates/infengine_render/src/uber_buffer.rs` (new): UberMeshAllocator
- `crates/infengine_render/src/lib.rs`: new fields + API (create_chunk_mesh,
  remove_chunk_mesh, queue_chunk_draw, set_chunk_mesh_transparent)
- `crates/infengine_render/src/mesh3d.rs`: Mesh3DPass::draw_uber_chunks
- `crates/infengine_render/src/shadow.rs`: ShadowPass::draw_uber
- `examples/voxel_demo_mp/src/main.rs`: all chunk create/remove/draw
  sites migrated to the uber API

### Tests

- `uber_buffer::tests::uber_alloc_constants_are_sane`
- `uber_buffer::tests::free_slot_first_fit_reuse`
- `uber_buffer::tests::uber_alloc_offsets_non_overlapping`
- infengine_render: 30 pass / 0 fail (up from 27, +3 uber tests)
- voxel_demo_mp: 1495 pass / 7 fail (pre-existing, unchanged)

### Deferred

- W32.X: TLSF allocator for better free-list utilisation (DONE W33.2)
- W32.X: separate render layers (opaque / cutout / translucent / water)
  for correct alpha ordering within each layer (DONE W33.3)
- W32.X: defragmentation pass when fragmentation exceeds threshold (backlog)
- W32.X: mob batching (small benefit - max ~30 mobs visible) (backlog)
- W32.X: per-chunk shadow indirect optimisation (currently uses same
  buffer as main pass; could use a dedicated shadow-only command list)

## Wave 33

### W33.1 - Per-face UV rotation final fix

User-visible bug: textures appeared rotated 90deg or 180deg on side/top faces
after the W31.3 per-face swap fix.

Root cause: W31.3 used `if fa == 2` to choose between two wrong swap
formulas. The Z-face branch swapped h/w so tex-u ran along the Y axis
instead of Z. The X/Y branch had "bottom" corners at integer values
(0, w, h) whose `fract() = 0` sampled the tile TOP, so the grass strip
appeared at the face bottom instead of top.

Fix: single universal `quad_uv_corners` formula for all 6 faces, no `if`
branch. The winding already handles the side distinction:

```
const E: f32 = 0.001;
let quad_uv_corners = [
    [0.0f32, w - E],   // c0 = BL: tex(0, ~1)
    [h - E,  w - E],   // c1 = BR: tex(~1, ~1)
    [h - E,  0.0f32],  // c2 = TR: tex(~1, 0)
    [0.0f32, 0.0f32],  // c3 = TL: tex(0, 0)
];
```

Derivation:
- `quad_uv.x` (tex-u) increases with `dv` (the `va`-axis offset).
  Goes from 0 at left edge to `h-E` at right edge; `fract(h-E) ~= 1`.
- `quad_uv.y` (tex-v) DECREASES with `du` (the `ua`-axis offset).
  Inverted: `w-E` at the bottom of the face (du=0), `0` at top (du=w).
  This places the grass strip at the TOP of side faces (high Y = low v).
- For both `side > 0` and `side < 0`, the greedy winding already ensures
  c0 is the "bottom-left" world corner. The UV formula is universal.

Tiling for merged quads (w>1, h>1): the GPU interpolates `quad_uv`
linearly; interior fragments get correct fractional values and `fract()`
wraps at integer tile boundaries. The epsilon only affects the single
corner pixel (masked by bilinear filter + seam clamp).

Applied identically to `chunk_mesh_greedy` and `chunk_mesh_greedy_with_special`.

New test: `greedy_mesh_quad_uv_corners_bl_br_tr_tl_all_faces` verifies
that all 6 faces have the correct BL/BR/TR/TL quad_uv fract pattern.

### W33.2 - TLSF segregated free list for UberMeshAllocator

Replaced O(n) flat first-fit `Vec<FreeSlot>` with a 32-bucket
power-of-2 segregated free list (Two-Level Segregated Fit, simplified).

Algorithm:
- `free(slot)`: push to `buckets[floor_log2(v_cap_bytes)]` - O(1).
- `alloc(v_need, i_need)`: scan upward from `buckets[floor_log2(v_need)]`;
  first bucket checks both caps, higher buckets only i_cap (v is guaranteed
  by the bucket invariant). Worst case 32 iterations = O(1) amortised.

Motivation: at view_distance=8 with constant player movement, chunks are
freed and re-allocated at ~50/s. The old flat list grew to hundreds of
entries; first-fit caused frame stalls. The new path is constant time.

5 new unit tests in `uber_buffer::tests` cover log2 correctness, exact-fit,
cross-bucket lookup, i_cap filter mismatch, and multi-slot reuse.

### W33.3 - Render layer split: opaque / cutout / translucent

Replaced `UberAlloc.transparent: bool` with `layer: u8`. Three layer constants:
- `LAYER_OPAQUE = 0`: depth-test + write ON, no blend.
- `LAYER_CUTOUT = 1`: same depth state, alpha-discard in shader (leaves, glass).
- `LAYER_TRANSLUCENT = 2`: depth-test ON, depth-write OFF, alpha blend (water).

Per-frame indirect command buffer layout: `[opaque | cutout | translucent]`.
Shadow pass covers opaque + cutout (both write depth); translucent excluded.
Cutout uses the same pipeline as opaque (shader already discards alpha < 0.5).
A dedicated `pipeline_cutout` for two-pass backface leaves is deferred.

Public API additions:
- `Renderer::set_chunk_mesh_layer(handle, layer)` - new primary API.
- `CHUNK_LAYER_OPAQUE / _CUTOUT / _TRANSLUCENT` re-exported from crate root.
- `set_chunk_mesh_transparent(h, bool)` kept as a backward-compat wrapper.

### W33.4 - Incremental two-queue lighting

Deferred to W34. Current W28.5 partial implementation (emitter-only incremental
updates) is correct and stable. Extending to blocker changes (solid <-> air
transitions requiring a two-queue decrease/increase flood) requires careful
BFS correctness work across `block_engine.rs` and `sky_engine.rs`.

### Files changed

- `crates/infengine_voxel/src/lib.rs`: W33.1 quad_uv fix + new UV test
- `crates/infengine_render/src/uber_buffer.rs`: W33.2 TLSF + W33.3 layer field
- `crates/infengine_render/src/lib.rs`: W33.3 three queues + set_chunk_mesh_layer
- `crates/infengine_render/src/mesh3d.rs`: W33.3 draw_uber_chunks with cutout_count

### Tests

- `infengine_voxel`: new test `greedy_mesh_quad_uv_corners_bl_br_tr_tl_all_faces`
- `infengine_render`: 36 pass / 0 fail (up from 30, +6 TLSF tests)
- `voxel_demo_mp`: 1495 pass / 7 fail (pre-existing, unchanged)

---

## Wave 34

Wave 34 shipped four changes: creative-tab icon name_map expansion,
greedy mesher UV fix and re-enable, incremental lighting blocker
extension, and a ghost-world DPI investigation.

### W34.1 - name_map expansion (creative tab icons)

`BLOCK_NAME_MAP` and `ITEM_NAME_MAP` in
`examples/voxel_demo_mp/src/resource_pack/name_map.rs` previously
covered only IDs 1-49. Wave 34.1 adds entries for:

- IDs 50-67 (W11.3 emitter blocks): redstone_torch, soul_torch,
  sea_lantern, jack_o_lantern, end_rod, magma, shroomlight, lantern,
  soul_lantern, crying_obsidian, respawn_anchor, ender_chest (obsidian
  fallback), redstone_lamp_on, redstone_ore, light_block_15.
- IDs 68-82 (W12.2 shape blocks): oak_planks (slabs/stairs/fence),
  stone (stone slab/stairs), cobblestone (cobblestone variants),
  nether_bricks (nether brick fence), glass (glass_pane), iron_bars,
  cobblestone (cobblestone_wall).
- ITEM_NAME_MAP additions: redstone_torch (50-51), soul_torch (52-53),
  end_rod (57), lantern (60), soul_lantern (61), oak_door (77-78),
  iron_door (79).

Aggregate test: `w34_name_map_covers_70_of_82` verifies at least 70 of
82 ITEM_TYPES entries resolve through the name_map.

### W34.2 - Greedy mesher per-face UV tables

The greedy mesher in `crates/infengine_voxel/src/lib.rs` has been
reverted twice due to UV rotation bugs. W34.2 derives explicit per-face
`quad_uv` tables by matching each greedy corner position to the OLD
(per-voxel) mesher UV assignment.

**Derivation method:**
- OLD UV pattern: `uvs = [(u0,v1),(u1,v1),(u1,v0),(u0,v0)]` indexed by corner order.
- Shader: `final_uv = tile_uv0 + fract(quad_uv) * tile_size`.
- For each face, map the greedy corner's world position to the OLD UV's texture
  fractions, then compute the `quad_uv` value that produces those fractions.

**Per-face quad_uv table (corners: side>0 = C0(0,0),C1(0,h),C2(w,h),C3(w,0);
side<0 = C0(w,0),C1(w,h),C2(0,h),C3(0,0)):**

| Face | Greedy corners = OLD | quad_uv formula |
|------|---------------------|-----------------|
| +X   | yes (Y=du, Z=dv)    | `[[0,w-E],[h-E,w-E],[h-E,0],[0,0]]` |
| +Y   | yes (Z=du, X=dv)    | same as +X |
| -Y   | yes (Z=du, X=dv)    | same as +X |
| -X   | no (corners reordered) | `[[h-E,0],[0,0],[0,w-E],[h-E,w-E]]` |
| +Z   | no (X,Y axes inverted) | `[[w-E,h-E],[w-E,0],[0,0],[0,h-E]]` |
| -Z   | no (X,Y axes inverted) | same as +Z |

where E = 0.001 (epsilon to avoid fract(integer) = 0 discontinuity).

Both `chunk_mesh_greedy` and `chunk_mesh_greedy_with_special` updated.
Both call sites in `voxel_demo_mp/main.rs` switched from
`chunk_mesh_with_atlas_light_anim_and_special` to
`chunk_mesh_greedy_with_special`.

New test: `greedy_flat_floor_produces_one_quad_not_256` verifies that
a 16x16 uniform floor produces 1 merged top quad (greedy) vs 256 (naive).

### W34.3 - Incremental block-light for blocker changes

`propagate_block_light_incremental` in
`examples/voxel_demo_mp/src/lighting/block_engine.rs` gained a
two-queue unlight BFS for the **blocker-added** case (transparent
cell becoming opaque):

1. If the changed cell holds a non-zero light value AND is now opaque,
   seed the decrease-queue with that cell.
2. Walk neighbours: cells whose stored value <= `source_level - decay`
   were lit through this cell; zero them and enqueue. Cells with higher
   stored values come from independent sources -> collect as relight seeds.
3. Run standard propagation from all relight seeds.

The **blocker-removed** case (opaque -> transparent) already worked:
the else-branch gathered neighbour light as relight seeds, and the
BFS naturally propagated through the now-transparent cell.

`tick_lighting` in main.rs was updated to use the incremental path for
single-voxel changes regardless of opacity, but still calls
`propagate_sky_light` for blocker changes (sky columns need the full
BFS engine at chunk boundaries).

**Two-queue invariant**: The decrease-queue tracks the maximum level
that can be sourced from the removed/blocked emitter at each cell.
A cell is a "decreasing candidate" if its stored value is no more than
what the source could have provided (source_level - step_decay). If it
has a higher value, it has an independent source and becomes a relight seed.
The increase-queue (relight) then re-floods the cleared region from surviving
sources, restoring correct light without touching unaffected areas.

### W34.4 - Ghost world DPI artifact (research only)

Investigation documented in `docs/design/ghost_world_investigation.md`.

Most likely cause: on Windows 11 with >100% DPI scale, `inner_size()`
called during `on_start` (which runs in `resumed()` before the window
is shown) returns the logical size (1280x720) rather than the physical
size. The wgpu swapchain is configured for the smaller logical size;
the OS compositor stretches the backbuffer to fill the physical window,
potentially showing the previous-frame content in the upper-left.

Suggested mitigation: after `Renderer::new`, immediately call
`renderer.resize(window.size())` to force a reconfigure with the
latest physical size. No code change applied (research task).

### Files changed

- `examples/voxel_demo_mp/src/resource_pack/name_map.rs`: W34.1 entries + tests
- `crates/infengine_voxel/src/lib.rs`: W34.2 per-face UV tables + flat-floor test
- `examples/voxel_demo_mp/src/main.rs`: W34.2 greedy call sites + W34.3 tick_lighting
- `examples/voxel_demo_mp/src/lighting/block_engine.rs`: W34.3 unlight BFS
- `docs/design/ghost_world_investigation.md`: W34.4 investigation

### Tests

- `infengine_voxel`: 20 pass / 0 fail (up from 19, +1 flat-floor test)
- `voxel_demo_mp`: 1499 pass / 7 fail (pre-existing unchanged, +4 W34.1 tests)

---

## Wave 35

### W35.1 - Color saturation / ambient lighting fix

**Root cause**: the hemispheric ambient term in `mesh3d.wgsl` used
cold blue-tinted values `(0.20, 0.22, 0.28)` for the sky hemi and
`(0.08, 0.08, 0.10)` for the ground hemi. This added a pervasive
blue wash that desaturated all blocks - most visibly obsidian which
appeared as a muted grey-purple instead of MC's vibrant black-purple.

The surface format (selected as sRGB in `lib.rs`) and the atlas
upload format (`Rgba8UnormSrgb` in `mesh3d.rs`) are both correct; the
sRGB pipeline was already in place.

**Fix**: raised and warmed the ambient coefficients:

- `ambient_up`:  `(0.20, 0.22, 0.28)` -> `(0.34, 0.36, 0.40)`
- `ambient_down`: `(0.08, 0.08, 0.10)` -> `(0.18, 0.17, 0.16)`

The new values are ~65% brighter overall and have a warm-neutral tint
(very slightly warm at the ground, slightly blue-neutral at the sky)
instead of a cold blue tint. This brings saturation closer to MC's
look while keeping the hemispheric sky/ground gradient.

### W35.2 - Wall-torch unification

MC has one torch item that auto-selects the wall-torch block when placed
on a side face. We now do the same for `redstone_torch` / `soul_torch`:

- `redstone_wall_torch` (id 51) and `soul_wall_torch` (id 53) added to
  `CREATIVE_BLACKLIST` - hidden from the creative inventory.
- Block placement code in `main.rs` computes `placed_block`: when
  `hit.normal[1] == 0` (side face), `REDSTONE_TORCH_ID` is mapped to
  `REDSTONE_WALL_TORCH_ID` and `SOUL_TORCH_ID` to `SOUL_WALL_TORCH_ID`
  before writing the world block and broadcast edit.
- Mining drop remap: a mined wall-torch variant drops the floor-torch
  item so the inventory always gets the holdable/placeable id.

The plain `torch` (id 10) already handled wall vs. floor via the
`torch_orient` map and specialized rendering - no change needed there.

### W35.3 - Item registry expansion (+15 blocks)

Added 15 new block+item pairs covering four categories (ids 83..=97):

| ID | Name | Category |
|----|------|----------|
| 83 | granite | BuildingBlock |
| 84 | diorite | BuildingBlock |
| 85 | andesite | BuildingBlock |
| 86 | bricks | BuildingBlock |
| 87 | smooth_stone | BuildingBlock |
| 88 | birch_log | BuildingBlock |
| 89 | birch_planks | BuildingBlock |
| 90 | birch_leaves | BuildingBlock |
| 91 | blue_wool | Decoration |
| 92 | red_wool | Decoration |
| 93 | yellow_wool | Decoration |
| 94 | green_wool | Decoration |
| 95 | tuff | BuildingBlock |
| 96 | deepslate | BuildingBlock |
| 97 | calcite | BuildingBlock |

Each entry touches three files: `block/registry.rs` (BlockType + color),
`block/ids.rs` (ID constant), `item/mod.rs` (ItemType), and
`resource_pack/name_map.rs` (MC asset name for texture lookup).

Block textures fall back gracefully to `stone.png` / `planks_oak.png` /
`leaves_oak.tga` / `white_wool.png` when the MC asset pack is absent.

Deferred to W36: oak_slab_top hidden from creative, iron_door_upper
hidden, polished variants (polished_granite etc.), orange/lime/cyan/
magenta/purple/light_blue/brown/pink/black/light_gray/gray wool, more
food crops, sea_lantern + prismarine, beacon, end_stone.

### W35.4 - Creative bucket never fills or empties

In `instabuild` (creative) mode:

- **Empty bucket aimed at water/lava source**: the source block is NOT
  removed and the inventory slot stays as an empty bucket. Mirrors MC's
  `ItemBucket.use` path which skips the fill + remove step when
  `player.getAbilities().instabuild`.
- **Filled bucket placed into the world**: the liquid source block IS
  placed, but the inventory slot stays as the filled bucket (does not
  convert to empty). Mirrors MC's same abilities gate.

Survival mode behavior is unchanged in both cases.

### Files changed

- `crates/infengine_render/src/mesh3d.wgsl`: W35.1 ambient values
- `examples/voxel_demo_mp/src/main.rs`: W35.4 instabuild bucket gates +
  W35.2 torch auto-wall-variant placement + drop remap
- `examples/voxel_demo_mp/src/creative_tab.rs`: W35.2 CREATIVE_BLACKLIST
  adds ids 51 + 53
- `examples/voxel_demo_mp/src/block/registry.rs`: W35.3 ids 83..=97
- `examples/voxel_demo_mp/src/block/ids.rs`: W35.3 ID constants
- `examples/voxel_demo_mp/src/item/mod.rs`: W35.3 ItemType rows
- `examples/voxel_demo_mp/src/resource_pack/name_map.rs`: W35.3 MC asset
  name entries

### Tests

- `infengine_voxel`: 20 pass / 0 fail (unchanged)
- `voxel_demo_mp`: 1499 pass / 7 fail (pre-existing unchanged)

## Wave 36

### W36.1 - Per-block PBR tuning

Obsidian and several other blocks received explicit PBR material parameters instead
of the old global default (metallic=0, roughness=0.85). The 0.85 roughness produced
a wide Cook-Torrance specular peak that washed out dark albedo values - obsidian
looked faded and "glassy" instead of deep purple-black.

New defaults: metallic=0.0, roughness=0.95 for terrain blocks.

Per-block overrides (see `block::block_pbr()` in `mod.rs`):

| Block(s)                      | metallic | roughness | Rationale                          |
|-------------------------------|----------|-----------|------------------------------------|
| obsidian (id 46)              | 0.0      | 1.0       | Fully matte - dark albedo pops     |
| crying_obsidian (id 62)       | 0.0      | 1.0       | Same treatment                     |
| glass (id 9), glass_pane (80) | 0.0      | 0.05      | Smooth, crisp specular edge        |
| iron_ore (id 14)              | 0.4      | 0.5       | Slight metallic ore sheen          |
| iron_door (79), iron_bars (81)| 0.6      | 0.4       | More metallic than raw ore         |
| glowstone (28), sea_lantern(54)| 0.0     | 1.0       | Matte lets emissive colour pop     |
| all others                    | 0.0      | 0.95      | Almost-matte default               |

The `BlockDef::with_pbr()` constructor was added to `infengine_voxel` to allow
explicit metallic/roughness at construction time. The registry loop in `main.rs`
now calls `block_pbr(id)` before inserting each `BlockDef`.

### W36.2 - Torch mesh visible

Root cause: the torch special model (built by `special_models::build_torch()`)
referenced texture key `"torch"` for all face UVs. When a real MC resource pack
is present, the atlas scanner registers the torch texture under key `"torch_on"`
(from filename `torch_on.png`) - not `"torch"`. The lookup `pack.block_texture("torch")`
returned `None`, defaulting to the fallback UV `[0, 0, 1, 1]` - sampling the whole
atlas as a miniature sprite on every torch face.

Fix:
- `build_torch()` and `build_wall_torch()` now use texture key `"torch_on"`.
- `fallback.rs` placeholder pack registers `"torch_on"` as an alias for the torch
  tile (block id 10) so the model resolves correctly without MC installed.
- `name_map.rs` entry for id 10 updated from `block: "torch"` to `block: "torch_on"`.

### W36.3 - Smoke rises upward

`TexturedParticleInstance` (GPU particle pass) has no velocity or lifetime fields.
The `emit_textured_smoke()` helper emitted a static billboard each tick at a fixed
`pos.y + 1.1`, creating a hovering static blob that visually appeared to sink
relative to the rising flame particles above it.

Fix: migrated torch smoke to the legacy CPU `ParticleSystem` via
`spawn_smoke_particle()`, which carries a per-particle velocity `[0, 0.3..0.5, 0]`
so smoke visibly drifts upward. Torches now emit one smoke wisp per ~4 flame bursts.
The `emit_textured_smoke()` call on fire blocks was also removed (same issue);
fire now uses only legacy rising smoke. The function is kept with
`#[allow(dead_code)]` because `wave18_tests.rs` exercises it directly.

### Files changed

- `crates/infengine_voxel/src/lib.rs`: `BlockDef::with_pbr()` + roughness default 0.95
- `examples/voxel_demo_mp/src/block/mod.rs`: `block_pbr()` function
- `examples/voxel_demo_mp/src/main.rs`: registry loop uses `block_pbr`; torch smoke;
  disable `emit_textured_smoke`
- `examples/voxel_demo_mp/src/resource_pack/fallback.rs`: "torch_on" alias for id 10
- `examples/voxel_demo_mp/src/resource_pack/name_map.rs`: id 10 block -> "torch_on"
- `examples/voxel_demo_mp/src/resource_pack/special_models.rs`: torch texture -> "torch_on"

### Tests

- `infengine_voxel`: 20 pass / 0 fail (unchanged)
- `voxel_demo_mp`: 1499 pass / 7 fail (pre-existing unchanged)
