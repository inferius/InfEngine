# Worldgen: InfEngine vs Minecraft — Gap Report

Audit of `examples/voxel_demo_mp/src/worldgen/` against modern Minecraft (1.18+
"Caves & Cliffs" baseline). Every engine claim below is grounded in a file read;
paths are cited inline. MC side is reasoned from the well-known data-driven
roster plus the block/feature textures shipped in
`F:\Data\mc_assets\mc_pack\assets\minecraft\textures\block\` (the pack is a
*resource* pack — textures/models/blockstates only; it carries **no** worldgen
/ biome / structure JSON, so the data-pack roster is reasoned, not read).

## TL;DR

The engine's terrain *shape* is in good shape: a multi-octave fBm `Natural`
profile (continentalness + hills + detail + ridged peaks), an 11-biome climate
table, 3D-noise caves, ellipsoid ravines, cobble dungeons, and a real nether.
The decoration layer is respectable (7 tree species, flowers, ground cover,
cactus, dead bush, bamboo, sugar cane).

> **STATUS UPDATE (post-audit):** the underground headline items below
> were implemented in **W121** — `worldgen/strata.rs` now places the
> full MC-1.18 ore roster (gold/diamond/redstone/lapis/emerald/copper/
> iron/coal) with triangular sea-relative depth bands, swaps the
> deepslate-ore variants below the deepslate threshold, and lays a
> jagged bedrock floor in `gen_natural`. The ore/deepslate/bedrock
> bullets below describe the PRE-W121 state and are kept for context.
> Structures/features/ocean items remain open.

The gaps are almost entirely in **content breadth + faithfulness of
distribution**, not engine plumbing:

- ~~**Ores**: only **coal + iron** generate.~~ **DONE in W121** (strata.rs
  depth-band table).
- ~~**No deepslate layer**~~ **DONE in W121** (deepslate stratum + ore
  variant swap).
- **Structures**: only dungeons. No villages, mineshafts, strongholds, temples,
  monuments, ruined portals, shipwrecks, fossils, geodes.
- **Features**: no lakes/springs, no gravel/clay/dirt blobs, no snow layers /
  ice, no aquifers, no underwater vegetation (kelp/coral/seagrass), no
  pumpkins/melons/mushrooms/vines/lily pads/cobwebs.
- **Vertical magic numbers** in the CARVERS (cave `y<10`, dungeon `y8..32`,
  ravine `y10..50`) are still absolute, **not** rescaled relative to
  `SEA_LEVEL=63` (ores ARE sea-relative since W121). They sit in the bottom
  ~1/8 of the world and will be wrong the moment the world grows taller.

---

## 1. Biomes

### Engine roster (11)
`worldgen/biome.rs:18` — `Biome` enum: Desert, Plains, Mountain, Tundra, Taiga,
Forest, Jungle, Swamp, Savanna, SnowyMountain, Beach.

Selection: `worldgen/climate.rs` samples 3 sin/cos noises (temperature /
humidity / elevation), and `worldgen/biome_table.rs:27` `biome_from_climate` is
a decision tree over that cube. This is a sound *Whittaker-style* climate map
and conceptually mirrors MC's temperature/humidity grid — but at far lower
resolution (one biome per climate octant; no MC-style "biome variants" or
sub-biomes).

### MC roster (overworld, ~50+ biomes incl. variants)
Plains, Sunflower Plains, Snowy Plains, Ice Spikes, Desert, Savanna, Savanna
Plateau, Windswept Savanna, Forest, Flower Forest, Birch Forest, Old-Growth
Birch Forest, Dark Forest, Taiga, Snowy Taiga, Old-Growth Pine/Spruce Taiga,
Jungle, Sparse Jungle, Bamboo Jungle, Swamp, Mangrove Swamp, Beach, Snowy Beach,
Stony Shore, Meadow, Grove, Snowy Slopes, Frozen/Jagged/Stony Peaks, Windswept
Hills/Forest/Gravelly Hills, Mushroom Fields; **oceans**: Ocean, Deep Ocean,
Warm/Lukewarm/Cold/Frozen + Deep variants; **rivers**: River, Frozen River;
**cave biomes**: Dripstone Caves, Lush Caves, Deep Dark; **nether**: Nether
Wastes, Soul Sand Valley, Crimson/Warped Forest, Basalt Deltas; **end**:
The End + 4 island variants.

### Gap

| MC biome group | Engine? | Priority | Notes |
|---|---|---|---|
| Plains/Forest/Desert/Taiga/Jungle/Swamp/Savanna/Beach | ✅ approximated | — | one variant each, no flavor variants |
| Mountain family (Windswept Hills, Snowy Slopes, Peaks, Meadow, Grove) | ⚠️ partial (Mountain + SnowyMountain only) | **P1** | no peak-block identity (snow/ice/packed ice/stone variants); surface is grass + bare-rock cap above sea+40 (`climate.rs:155`) |
| **Ocean biomes** (warm/cold/deep/frozen) | ❌ none | **P1** | water exists as a fill below sea level (`natural.rs:163`), but there is no Ocean *biome*, no ocean-floor surface (gravel/sand/clay), no temperature-tinted water, no kelp/coral/seagrass |
| **Rivers** | ❌ none | **P2** | continentalness produces basins, but no dedicated river carver/biome |
| **Cave biomes** (Dripstone, Lush, **Deep Dark**) | ❌ none | **P1** | blocks exist (dripstone, moss `615`, azalea, sculk `628`, sculk_catalyst/shrieker/sensor `629-631`) but no biome places them |
| **Mushroom Fields** | ❌ none | **P3** | mycelium/mushroom textures present in pack |
| Snowy variants (Snowy Plains, Ice Spikes, Snowy Taiga/Beach, Frozen River/Ocean) | ❌ none | **P2** | Tundra exists but lacks snow-layer/ice surface treatment |
| Flavor variants (Sunflower Plains, Flower Forest, Dark Forest, Old-Growth, Bamboo Jungle, Mangrove Swamp, Cherry Grove) | ❌ none | **P3** | cherry + mangrove textures present in pack |
| Nether sub-biomes (Soul Sand Valley, Crimson/Warped Forest, Basalt Deltas) | ❌ none (single nether) | **P2** | nylium/warped/crimson/soul/basalt/blackstone textures all present |
| The End | ❌ none | **P3** | no end dimension at all |

**Biome-coloring gap**: MC tints grass/foliage/water per-biome (swamp green,
desert yellow, etc.). Engine surface block is a flat `BlockId(3)` grass for
nearly every biome (`climate.rs:139` `surface_block`) with no biome tint — so
biomes read as identical green except desert/beach sand. Listed under Features.

---

## 2. Structures

### Engine (1 structure type)
`worldgen/dungeons.rs` — a 7×7×5 cobblestone box (`ROOM_INNER 5×5×3`), 1–2 chests
on the floor, a **placeholder** "spawner" that is just another cobblestone block
(`dungeons.rs:48` `SPAWNER_PLACEHOLDER = COBBLE_ID`, with a `TODO: replace with a
real tile-entity spawner block`). Chests are placed as blocks but their loot is
explicitly deferred (`dungeons.rs:9` "loot tables are a deferred feature").
Spawn rate ~1/200 columns, Y range 8..32.

That is the **only** structure. `ravines.rs` is a carver (terrain), not a
structure.

### MC structures — gap

| Structure | Engine? | Priority | Notes |
|---|---|---|---|
| Dungeon (monster room) | ⚠️ shell only | **P1** | no real spawner (no mob-spawner block/tile-entity), no loot tables, walls are plain cobble (MC mixes mossy cobble — block `109` exists, unused) |
| **Mineshaft** (corridors, rails, supports, cobwebs, chest minecarts) | ❌ | **P1** | needs corridor/branch generator; rails + cobweb blocks needed |
| **Village** (per-biome building set, paths, farms, villagers) | ❌ | **P1** | largest single content gap; requires template/jigsaw system + villager mob |
| **Stronghold** (portal room, libraries, silverfish) | ❌ | **P2** | needed for end-game loop; bookshelf exists |
| **Desert/Jungle temple** | ❌ | **P2** | jungle temple needs trap/dispenser/lever logic |
| **Ocean monument / Ocean ruins / Shipwreck** | ❌ | **P3** | prismarine/sea_lantern/sponge textures present; gated on ocean biomes (P1) |
| **Ruined portal** | ❌ | **P2** | obsidian + crying obsidian + netherrack blob; cheap win, high "modern MC" signal |
| **Pillager outpost / Mansion** | ❌ | **P3** | |
| **Witch hut / Igloo** | ❌ | **P3** | |
| **Nether fortress / Bastion** | ❌ | **P2** | nether exists but is structureless (`nether.rs` is bulk + lava + glowstone only) |
| **Geode (amethyst)** | ❌ | **P2** | budding_amethyst `272/452`, amethyst_block, amethyst_cluster `273`, calcite `97`, smooth basalt all exist — this is a pure *gen* gap, blocks are 100% ready |
| **Fossil** | ❌ | **P3** | bone_block textures present |
| **Well / boulder / fallen tree / forest rock** | ❌ | **P3** | small decorative MC features |

**Architectural note (mandate-relevant):** the engine has no jigsaw/template
structure system at all. MC structures are data-driven NBT templates assembled
by jigsaw connectors. Per CLAUDE.md's "data-driven, no magic numbers" mandate,
villages/mineshafts/strongholds should be built on a small template-placement
engine (load structure piece → rotate → stamp → connect), not hand-rolled
per-structure loops like `dungeons.rs`. This is the prerequisite that unblocks
half the P1/P2 structure list.

---

## 3. Features / decoration

### Engine decoration passes
Driver: `worldgen/decor.rs:20` `place_realistic` runs, in order: trees →
flowers → biome_features (dead bush / bamboo / sugar cane) → ground_cover (short
grass / fern) → cactus.

- **Trees** (`worldgen/trees/mod.rs:161` `pick_species`): Oak, Birch, Pine,
  Sequoia, Jungle, Acacia, MegaSpruce — biome-weighted. Solid coverage of MC's
  common species. **Missing**: dark-oak (2×2), spruce-as-MC-spruce vs the
  engine "pine", swamp oak with vines, mangrove, cherry, azalea tree, fallen/
  large-fungus (nether), huge mushrooms, jungle bushes. Trees never carry vines,
  bee nests, cocoa, or snow.
- **Flowers** (`worldgen/flowers.rs`): 3 colors (red/yellow/blue ids 20/21/22)
  on Plains/Forest only. MC has ~20 flower types + 2-tall flowers
  (sunflower/lilac/rose bush/peony) placed in patches; biome-specific (flower
  forest, meadow). **Big breadth gap.**
- **Ground cover** (`worldgen/ground_cover.rs`): short grass + fern. Good. No
  tall grass (2-block), no large fern, no dead bush outside desert, no moss
  carpet, no glow lichen.
- **Biome features** (`worldgen/biome_features.rs`): dead bush, bamboo, sugar
  cane (water-edge). Reasonable. No lily pads, no pumpkins/melons, no
  berry/sweet-berry bushes, no nether vegetation.
- **Cactus** (`worldgen/cactus.rs`): desert, 2–3 tall. Good.

### Ores — the headline distribution gap

`worldgen/ore.rs:43` `pick_stone_or_ore` is the **entire** ore system. It rolls
exactly **two** ores:

```
coal_ore (13): threshold 0.94 (0.90 in mountains), any depth
iron_ore (14): threshold 0.985 (0.965 in mtns), oy < 14 (24 in mountains)
else -> stone (1)
```

The block registry **already defines** every other ore (`block/registry.rs`):
gold_ore `117`, diamond_ore `118`, redstone_ore `119`, lapis_ore `120`,
emerald_ore `121`, copper_ore `122`, plus the full deepslate set
deepslate_coal/iron/diamond/gold `123-126` and
deepslate_redstone/lapis/emerald/copper `246-249`, and raw_*_block `388-390`.
**None of them are ever generated.** This is a pure worldgen gap — zero new
blocks required for gold/diamond/redstone/lapis/copper/emerald.

MC 1.18 ore distribution is **depth-banded with triangular/uniform
distributions** (not flat thresholds):

| Ore | MC distribution (approx) | Engine |
|---|---|---|
| Coal | y0..256, peak ~y96, none below 0 (above-surface exposed too) | flat threshold, all depths ✅-ish |
| Copper | triangle y-16..112, peak ~48 (dripstone-rich) | ❌ not generated |
| Iron | triangle y-24..56 (peak 16) **and** y128..320 + small batch low | flat, `y<14` only ⚠️ |
| Gold | triangle y-64..32 (peak -16); extra in badlands | ❌ |
| Redstone | y-64..15 uniform + below | ❌ |
| Lapis | triangle y-64..64 (peak 0) | ❌ |
| Diamond | triangle y-64..16, **more the deeper**, none above 16 | ❌ |
| Emerald | mountains only, triangle y-16..320 (peak ~232) | ❌ |
| **Deepslate variants** below y0 | every ore swaps to deepslate form under y0 | ❌ (no deepslate at all) |

**Recommendation (mandate-faithful):** replace the 2-ore hardcode with a small
data table `[(ore_id, deepslate_id, min_y, max_y, peak_y, size, rarity,
biome_filter)]` driven by Y relative to `SEA_LEVEL`, and a triangular Y
distribution like MC's `trapezoid`/`triangle` placement. This kills the magic
thresholds and the absolute Y caps in one move.

### Other feature gaps

| MC feature | Engine? | Priority | Notes |
|---|---|---|---|
| **Deepslate transition** (stone→deepslate around y0/y8, MC: y0 with gradient) | ❌ | **P1** | block `96` ready; surface-relative threshold needed. Without it the whole underground is one gray stone tone |
| **Bedrock floor layer** (jagged y0..4) | ❌ | **P2** | overworld gen emits a flat `BlockId(1)` stone at y=0 (`natural.rs:158`), **not** bedrock; bedrock `45` only used in nether (`nether.rs:33`) |
| **Gravel / dirt / clay / granite-diorite-andesite blobs** in stone | ❌ | **P1** | gravel `644`, granite `83`/diorite `84`/andesite `85`, clay all exist as blocks; MC scatters them as ore-style blobs. Pure gen gap |
| **Water lakes + lava lakes** (surface/underground) | ❌ overworld | **P2** | only nether has lava lakes (`nether.rs:130`). No overworld lakes/springs |
| **Springs** (single water/lava sources dripping in cave walls) | ❌ | **P2** | classic MC cave ambiance |
| **Aquifers** (1.18 local water tables that flood caves below a level) | ❌ | **P2** | engine caves are always air; MC floods deep caves with water/lava aquifers |
| **Underwater vegetation** (kelp, seagrass, coral reefs, sea pickles) | ❌ | **P2** | coral (30 textures), kelp (5), seagrass (3) all in pack; gated on ocean biomes |
| **Dripstone** (pointed dripstone, stalactites/stalagmites) | ❌ | **P2** | dripstone textures present (11) |
| **Lush cave features** (moss, azalea, glow berries, spore blossom, dripleaf) | ❌ | **P2** | moss/azalea/spore textures present |
| **Sculk / Deep Dark** (sculk vein/catalyst/shrieker/sensor, ancient city) | ❌ | **P2** | all sculk blocks exist `628-631`; spreading-sim + ancient-city structure missing |
| **Snow layers + ice** on cold biomes; **packed/blue ice** | ❌ | **P2** | snow block `15` used as old Classic mtn cap only; no thin snow_layer, no frozen water surfaces |
| **Pumpkins / melons / mushrooms / vines / lily pads / cobwebs / sweet berries** | ❌ | **P3** | scattered small features; mushroom + lily textures present |
| **Per-biome grass/water/foliage tint** | ❌ | **P2** | engine surface is flat green grass for all land biomes (`climate.rs:150`); no biome color grading |
| **Ore-vein system** (1.18 large copper/iron veins deep down) | ❌ | **P3** | |
| **Disk features** (sand/gravel/clay disks under water) | ❌ | **P3** | |

---

## 4. Terrain / vertical

### What's good
`worldgen/natural.rs:77` `natural_height` is a genuine fBm stack:
`cont (5 oct, freq .0035)` + `hills (4 oct)` + `detail (3 oct)` + a ridged peak
term, with a continentalness spline (`base = cont*9 + sign*cont²*7`,
`natural.rs:107`) producing oceans/coasts/inland — conceptually the right shape,
and the module doc (`natural.rs:13`) correctly mirrors MC's density-function
layering. Caves are 3D value-noise (`caves.rs`, multiplicative two-octave →
tunnel shapes). Ravines are ellipsoid carves (`ravines.rs`). Nether is a real
roofed dimension (`nether.rs`).

### Vertical gaps vs MC's -64..320 world

1. **World is 256 tall, floor at y=0, sea at 63** (`crates/infengine_voxel/src/
   lib.rs:127` CHUNK_SIZE=16 × `main.rs:285` VERTICAL_CHUNKS=16 = 256 blocks;
   `climate.rs:47` SEA_LEVEL=63). MC 1.18 is **384 tall, floor y=-64, top y=320**
   with sea at 63. So the engine matches MC's *sea line* but has **only ~63
   blocks of underground** vs MC's **127** (y63 down to y-64). The memory note
   ("planning a LARGER vertical range later") is the intended fix — this report
   treats sea-63/256 as the *baseline*, and the items below are what must move
   when it grows.

2. **No deepslate stratum** (covered above) — the defining visual of the
   modern underground. P1.

3. **Cave types** — MC 1.18 has three cave families:
   - **Cheese caves** (large open caverns) — engine has none; caves are
     uniformly thin tunnels (`caves.rs:6` "elongated, tunnel-like voids").
   - **Spaghetti caves** (long winding tunnels) — engine approximates these. ✅
   - **Noodle caves** (thin connectors) — partially via the 2nd octave
     (`CAVE_SCALE_B=6`). ⚠️
   So large caverns + the cheese/noodle separation are missing. P2.

4. **Aquifers** (local water/lava tables flooding deep caves) — absent; engine
   caves are always dry air. P2.

5. **Surface-relative magic numbers NOT rescaled (mandate violation).** Despite
   `SEA_LEVEL=63` and the `climate.rs:41` comment claiming "*every vertical
   feature depth is expressed RELATIVE to this constant*", the feature carvers
   still use **absolute** Y constants pinned to the old sea-12 world:

   | Constant | File:line | Value | Should be (relative) |
   |---|---|---|---|
   | Cave deep-layer threshold switch | `caves.rs:130` | `wy < 10.0` | `SEA_LEVEL - 53` or a fraction of depth |
   | Cave floor guard | `caves.rs:122` | `wy <= 0` | `WORLD_FLOOR` const |
   | Iron Y ceiling | `ore.rs:51` | `14` / `24` | band relative to SEA_LEVEL |
   | Ravine Y centre range | `ravines.rs:142-143` | `10 + (… % 40)` → y10..50 | relative to surface/sea |
   | Ravine floor | `ravines.rs:55` | `RAVINE_FLOOR_Y = 2` | `WORLD_FLOOR + n` |
   | Dungeon Y range | `dungeons.rs:159-160` | `8 + (… % 25)` → y8..32 | relative to surface |
   | Classic mtn snow line | `chunk.rs:174` | `h >= 26` | relative (legacy profile) |

   Net effect: caves, ores, ravines, and dungeons are all crammed into roughly
   **y2..50** — the bottom ~1/5 of the world and *entirely below sea level* —
   while the ~63→107 band where most terrain actually sits has **no caves or
   ores near the surface**. A player digging straight down from a hilltop hits
   solid stone for 50+ blocks before any cave/ore. **This is the most
   actionable correctness gap** and a direct violation of the no-magic-number
   mandate. Fix: introduce `WORLD_FLOOR`/`WORLD_TOP` consts and express every
   carver depth as `SEA_LEVEL ± n` (or a 0..1 depth fraction), so the single
   `SEA_LEVEL` edit (and the future taller-world edit) moves them all together.

6. **Height amplitude is deliberately clipped** (`natural.rs:92`: mountain
   amp 18→14, ridge 8→6) to keep peaks inside the ±3-chunk vertical streaming
   window — peaks top out ~y107 (sea+44). MC peaks reach y150-256. This is a
   conscious tradeoff (commented), to be relaxed when the vertical load range
   grows. P2, tied to the taller-world work.

---

## 5. Biggest wins (ranked)

1. **Full ore suite + MC 1.18 depth distribution** (`ore.rs`). Gold, diamond,
   redstone, lapis, copper, emerald all have blocks and never generate. Replace
   the 2-ore hardcode with a Y-banded, triangular-distribution data table.
   Highest gameplay impact, near-zero new-block cost. **P1.**

2. **Deepslate stratum + bedrock floor.** Emit deepslate (`96`) below a
   surface-relative depth (and its deepslate-ore variants), and a jagged bedrock
   (`45`) floor. Instantly makes the underground read as modern MC. **P1.**

3. **Rescale the magic Y numbers relative to SEA_LEVEL** (caves/ores/ravines/
   dungeons). Pulls caves + ores up into the band where players actually dig,
   fixes the "solid stone under every hill" bug, and satisfies the no-magic
   mandate. Small, mechanical, high correctness value. **P1.**

4. **Ocean biomes + water world** (warm/cold/deep), with ocean-floor surfaces
   (gravel/sand/clay) and underwater vegetation (kelp/coral/seagrass — all
   textures present). Unlocks shipwrecks/ruins/monuments later. **P1.**

5. **A template/jigsaw structure engine, then villages + mineshafts.** One
   reusable placement system (load piece → rotate → stamp → connect) unblocks
   villages, mineshafts, strongholds, temples, ruined portals. Villages are the
   single most-felt missing structure. **P1 (engine) → P1/P2 (content).**

6. **Surface/underground blob features**: gravel/dirt/clay/granite/diorite/
   andesite ore-style blobs (all blocks exist) + overworld water/lava lakes +
   springs. Cheap, big "lived-in" payoff. **P1-P2.**

7. **Cave biomes**: Lush Caves (moss/azalea/dripleaf) + Dripstone + Deep Dark
   (sculk + ancient city). All blocks already in the registry; needs cave-biome
   placement + the sculk spread/ancient-city structure. **P2.**

8. **Per-biome grass/water/foliage tint + cold-biome snow/ice.** Makes the 11
   biomes visually distinct instead of uniform green. **P2.**
