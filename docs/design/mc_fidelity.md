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
