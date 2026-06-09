# MC vs InfEngine — BLOCK gap audit

Audit of InfEngine's `voxel_demo_mp` block registry against the bundled
Minecraft asset pack. Every figure below is grounded in actual file reads
(no invented block names). Sources:

- **Engine blocks:** `examples/voxel_demo_mp/src/block/registry.rs`
  (`BLOCK_TYPES` table), `block/ids.rs`, `block/mod.rs`,
  `block/special_models.rs`, plus `resource_pack/name_map.rs`
  (id → MC blockstate-name mapping that drives the JSON model pipeline).
- **MC blocks:** the blockstate files in
  `F:\Data\mc_assets\mc_pack\assets\minecraft\blockstates\*.json`
  (one `.json` per registered block).

---

## 1. Counts

| Metric | Value | Source |
|---|---:|---|
| MC blockstate files (all) | **1170** | glob of `blockstates/*.json` |
| MC "real obtainable" blocks (after removing technical/state-dup states) | **~1022** | 1170 − 148 (see note) |
| Engine `BlockType` rows total | **613** | `grep -c "BlockType {" registry.rs` |
| Engine placeable block-form items (`places: Some`) | **526** | `grep -c "places: Some" item/mod.rs` |
| Distinct MC blocks the engine covers | **469** | normalized engine names ∩ MC |
| **MC blockstates the engine lacks (raw)** | **701** | set difference |
| — of which technical / state-duplicates | **148** | see note |
| **MC "real obtainable" blocks the engine lacks** | **553** | the actual gap |

**Coverage: 469 / ~1022 ≈ 46% of obtainable MC blocks.**

> **"Technical / state-duplicate" (148)** excluded from the gap because they
> are not blocks a player obtains-and-places as a distinct item. They are:
> `air`/`cave_air`/`void_air`, fluid tech (`water`, `lava`, `bubble_column`,
> `frosted_ice`, `*_cauldron` filled states), `moving_piston`/`piston_head`,
> portal/gateway interior (`nether_portal`, `end_portal`, `end_gateway`),
> command/structure/test/jigsaw/spawner blocks, the 38 `potted_*` flower-pot
> render states, the 57 wall-mounted placement states (`*_wall_sign`,
> `*_wall_banner`, `wall_torch`/`*_wall_torch`, `*_wall_fan`, `*_wall_head`,
> `*_wall_skull`, `*_wall_hanging_sign`), crop intermediates
> (`attached_*_stem`, `*_plant`, `cocoa`, `melon_stem`, `pumpkin_stem`,
> `pitcher_crop`, `torchflower_crop`), and `bamboo_sapling`, `tripwire`,
> `tall_seagrass`, `light`, `barrier`. The engine's own state-split rows
> (`oak_slab_top`, `oak_door_upper`, immature crops, `lit_redstone_lamp`,
> `lit_redstone_ore`, `obsidian_pillar`) were likewise collapsed onto their
> canonical MC name before diffing so they don't inflate coverage.

### How the engine renders blocks (important for "divergent" reading)

`name_map.rs` maps each engine id to a **real MC blockstate name** (e.g. id
5→`oak_log`, 8→`cobblestone`, 11→`white_wool`, 13→`coal_ore`, 20→`poppy`)
and `special_models::resolve_block_model` walks the actual MC
blockstate→variant→model→parent chain to mesh it. So most engine blocks are
**data-driven from genuine MC JSON**, not ad-hoc — consistent with the
CLAUDE.md mandate. The legacy generic registry names (`wood`, `cobble`,
`coal`, `iron`, `leaves`, `planks`, `wool`, `grass`, `flower*`) are cosmetic
labels only; they resolve to the correct MC model. The remaining divergence
is therefore mostly **missing variants** and a handful of **placeholder
TileSpecs / missing special models** (Section 3).

---

## 2. Missing blocks (553) — by family, prioritized

Priority key: **P1** = core survival/building staples the world feels
incomplete without · **P2** = decorative / common build palette · **P3** =
niche / late-game / cosmetic.

### 2.1 Wood sets — per-species completeness (P1–P2) · ~115 blocks
The engine has logs/planks/leaves/slabs/stairs/fences/doors/trapdoors for
the main species, but every species is **missing its full furniture set**.
Across `oak, spruce, birch, jungle, acacia, dark_oak, mangrove, cherry,
bamboo, crimson, warped` and the **entirely-absent `pale_oak`** set:

- **Buttons** (12): `oak_button`, `spruce_button`, … `crimson_button`,
  `warped_button` *(none of these wood buttons exist; engine only has
  `oak_button`/`stone_button` generic ids)*. **P2**
- **Pressure plates** (12): `oak_pressure_plate` … `warped_pressure_plate`. **P2**
- **Fence gates** (12): `oak_fence_gate` … `warped_fence_gate` — **NONE exist**. **P1** (farms/paths)
- **Signs** (10 standing) + **hanging signs** (12) + **shelves** (12, 1.21+):
  `*_sign`, `*_hanging_sign`, `*_shelf`. **P2**
- **Trapdoors** (per species not yet added): `cherry_trapdoor`,
  `mangrove_trapdoor`, `dark_oak_trapdoor`, `acacia_trapdoor`,
  `crimson_trapdoor`, `warped_trapdoor`, `bamboo_trapdoor`,
  `pale_oak_trapdoor`. **P2**
- **All-bark "wood"/"hyphae" blocks** (`oak_wood`, `birch_wood`,
  `spruce_wood`, `jungle_wood`, `acacia_wood`, `dark_oak_wood`,
  `mangrove_wood`, `cherry_wood`, `pale_oak_wood`). **P2**
- **Stripped logs & stripped wood** (23): `stripped_oak_log`,
  `stripped_oak_wood`, … `stripped_warped_hyphae`. **P1** (basic woodworking)
- **Doors/fences still missing**: `bamboo_door`, `bamboo_fence`,
  `crimson_door`, `crimson_fence`, `warped_fence`, `mangrove_fence`,
  `cherry_fence`, `pale_oak_door`, `pale_oak_fence`. **P1/P2**
- **Bamboo wood line** (P2): `bamboo_block`, `bamboo_planks`,
  `bamboo_mosaic`, `bamboo_mosaic_slab`, `bamboo_mosaic_stairs`,
  `bamboo_slab`, `bamboo_stairs`, `stripped_bamboo_block`.
- **`pale_oak` whole tree** (1.21.4): `pale_oak_log/planks/leaves/sapling/
  slab/stairs/wood` + furniture — **0% present**. **P2**

### 2.2 Copper + oxidation + waxed (P2) · 111 blocks — biggest single family
Engine has only the 5 plain oxidation stages (`copper_block`,
`exposed/weathered/oxidized_copper`, `cut_copper`) and 4 waxed plain blocks.
**Missing the entire functional + waxed + cut-stair/slab matrix:**

- Per oxidation stage ×4 (plain/exposed/weathered/oxidized) **and** waxed ×4
  → 8 finishes × each of: `copper_bars`, `copper_bulb`, `copper_chain`,
  `copper_chest`, `copper_door`, `copper_grate`, `copper_lantern`,
  `copper_trapdoor`, `chiseled_copper`, `cut_copper_slab`,
  `cut_copper_stairs`, `lightning_rod`, `copper_golem_statue`,
  plus the `*_cut_copper` block per stage.
- 56 of the 111 are the `waxed_*` mirror set.
- **`lightning_rod`** itself (all 8 finishes) is absent → no copper utility. **P2**

### 2.3 Stone / deepslate / tuff / blackstone / quartz brick families (P1–P2) · ~120 blocks
Engine has the base blocks but **almost none of their slab/stair/wall cuts**:

- **Quartz family — 0% present** (engine has `nether_quartz_ore` only):
  `quartz_block`, `quartz_pillar`, `quartz_bricks`, `chiseled_quartz_block`,
  `smooth_quartz`, `quartz_slab/stairs`, `smooth_quartz_slab/stairs`. **P2**
- **Tuff family — 11 missing** (engine has bare `tuff` only):
  `tuff_bricks`, `chiseled_tuff`, `chiseled_tuff_bricks`, `polished_tuff`,
  `tuff_slab/stairs/wall`, `tuff_brick_slab/stairs/wall`,
  `polished_tuff_slab/stairs/wall`. **P2**
- **Deepslate brick/tile cuts — 8 missing**: `deepslate_brick_slab/stairs/
  wall`, `deepslate_tile_slab/stairs/wall`, `cracked_deepslate_bricks`,
  `cracked_deepslate_tiles`, `chiseled_deepslate`. **P2**
- **Polished blackstone suite**: `polished_blackstone_slab/stairs/wall`,
  `polished_blackstone_brick_slab/stairs/wall`, `polished_blackstone_button`,
  `polished_blackstone_pressure_plate`, `chiseled_polished_blackstone`,
  `cracked_polished_blackstone_bricks`, `blackstone_slab/stairs/wall`,
  `gilded_blackstone` cuts. **P2**
- **Missing slabs/stairs/walls of blocks the engine already has the base of**
  (44 slabs / 40 stairs / 15 walls total): `andesite_slab/stairs`,
  `diorite_slab/stairs`, `granite_slab`, `polished_granite_slab`,
  `polished_diorite_slab/stairs`, `polished_andesite_slab/stairs`,
  `cobbled_deepslate_slab/stairs/wall`, `polished_deepslate_slab/stairs/wall`,
  `mossy_cobblestone_slab/stairs/wall`, `mud_brick_slab/stairs/wall`,
  `prismarine_slab/stairs/wall` + `prismarine_brick_slab/stairs`,
  `dark_prismarine_slab/stairs`, `red_nether_brick_slab/stairs/wall`,
  `cut_sandstone_slab`, `smooth_sandstone_slab/stairs`,
  `cut_red_sandstone_slab`, `smooth_red_sandstone_slab/stairs`,
  `end_stone_brick_stairs`, `purpur_slab/stairs`,
  `petrified_oak_slab`. **P1** for stairs/slabs of common stone.
- **Chiseled / cracked variants** (decorative): `chiseled_sandstone`,
  `chiseled_red_sandstone`, `chiseled_nether_bricks`,
  `cracked_nether_bricks`. **P2**
- **`polished_basalt`** (engine has `basalt`+`smooth_basalt` but **not**
  `polished_basalt`). **P2**

### 2.4 Ores & ore/metal blocks (P1) · small but high-impact
- **`coal_block`** — missing (engine has the `coal_ore` block but no
  storage block). **P1**
- **`netherite_block`** — missing (engine has the ingot item only). **P1**
- **`reinforced_deepslate`** — missing. **P3**
- *(All other ores ARE present: coal/iron/gold/diamond/redstone/lapis/
  emerald/copper + all deepslate variants + nether_gold/nether_quartz/
  ancient_debris. raw_iron/gold/copper blocks present.)*

### 2.5 Redstone & mechanisms (P1) · functional gap
Engine ships the **block rows** for `lever`, buttons, pressure plates,
`repeater`, `comparator`, `observer`, `piston`, `sticky_piston`,
`dispenser`, `dropper`, `hopper`, `target`, `redstone_lamp`,
`daylight_detector`?(no) — but is **missing**:

- **`redstone_wire`** as a placeable dust block — the engine has a
  `redstone_dust` *item* (id 309) and a `redstone_lamp`, but no
  `redstone_wire` blockstate coverage in the missing diff → wiring visuals
  incomplete. **P1**
- **Rails — all 4 missing**: `rail`, `powered_rail`, `detector_rail`,
  `activator_rail`. **P1** (minecarts)
- **`daylight_detector`**, **`tripwire_hook`**, **`crafter`** (1.21),
  **`heavy_core`** (1.21). **P2/P3**
- **`tnt`** — missing. **P1** (core survival).
- **`note_block`**/`jukebox` rows exist; `dispenser`/`dropper`/`hopper`/
  `piston` rows exist (behaviour may be cosmetic-only — see §3).

### 2.6 Decorative building blocks (P2)
- **`bookshelf`**, **`chiseled_bookshelf`** — missing. **P2** (enchanting room)
- **`clay`** — missing (engine has terracotta but not raw clay). **P1**
  (terracotta/brick supply chain)
- **`tinted_glass`** — missing. **P2**
- **Stained-glass panes — all 16 missing** (`white_stained_glass_pane` …
  `black_stained_glass_pane`); engine has the 16 solid stained glass + a
  plain `glass_pane`, but no colored panes. **P2**
- **Glazed terracotta — all 16 missing** (`white_glazed_terracotta` …). **P2**
- **Banners — all 16 standing missing** (`white_banner` …). **P2**
- **Candles — all 16 missing** (`candle`, `white_candle` …). **P2**
- **Shulker boxes — all 17 missing** (`shulker_box` + 16 colors). **P2**
- **`decorated_pot`**, **`flower_pot`**, **`item_frame`**,
  **`glow_item_frame`**. **P2**
- **Mob heads/skulls (standing)** (7): `skeleton_skull`,
  `wither_skeleton_skull`, `zombie_head`, `creeper_head`, `player_head`,
  `dragon_head`, `piglin_head`. **P3**
- **`dried_ghast`** (1.21.6), **`creaking_heart`** (1.21.4). **P3**

### 2.7 Nature / plants / cave (P2–P3)
- **Mushrooms**: `brown_mushroom`, `red_mushroom`, `brown_mushroom_block`,
  `red_mushroom_block`, `mushroom_stem`. **P2**
- **Tall plants & grass**: `tall_grass`, `large_fern`, `vine`, `lily_pad`,
  `seagrass`, `sea_pickle`, `kelp`, `bush`, `leaf_litter`, `wildflowers`,
  `pink_petals`, `firefly_bush`, `short_dry_grass`, `tall_dry_grass`,
  `cactus_flower`, `golden_dandelion`, `closed_eyeblossom`/`open_eyeblossom`
  (1.21.4 pale-garden flowers). **P2/P3**
- **Azalea leaves**: `azalea_leaves`, `flowering_azalea_leaves` (engine has
  the bush blocks but not the leaf blocks). **P2**
- **Cave**: `dripstone_block`, `pointed_dripstone`, `rooted_dirt`,
  `small/medium/large_amethyst_bud`, `cave_vines`, `pale_moss_block`,
  `pale_moss_carpet`, `pale_hanging_moss`, `sculk_vein`. **P2/P3**
- **Froglight (3)**: `ochre_froglight`, `verdant_froglight`,
  `pearlescent_froglight`. **P3**
- **Coral (24)** — engine has the 5 live coral *blocks* + 1 dead block,
  but **missing** all coral fans/plants (`tube_coral`, `brain_coral`,
  `*_coral_fan`) and most dead variants (`dead_tube_coral`,
  `dead_*_coral_block`, `dead_*_coral_fan`). **P3**
- **`farmland`** — missing as a distinct block (crops likely render on
  generic dirt). **P1** (farming visual).

### 2.8 Nether / End decorative (P2–P3)
- `crimson_roots`, `warped_roots`. **P3**
- `chorus_plant` (engine has `chorus_flower` only). **P3**
- *(`crimson_nylium`, `warped_nylium`, fungi, vines, `warped_wart_block`,
  `nether_sprouts` are present.)*

### 2.9 Utility / functional blocks (P1–P2)
- **`barrel`** — missing (second storage block). **P1**
- **`composter`** — missing. **P2**
- **`enchanting_table`** — missing. **P1**
- **`cauldron`** (+ filled states) — missing. **P2**
- **`campfire`**, **`soul_campfire`** — missing. **P2**
- **`bell`** — missing. **P2**
- **`trapped_chest`** — missing. **P2**
- **`cobweb`** — missing. **P2**
- *(`crafting_table`, `furnace`, `smoker`, `blast_furnace`, `smithing_table`,
  `fletching_table`, `cartography_table`, `lectern`, `grindstone`,
  `stonecutter`, `loom`, `brewing_stand`, `beacon`, `anvil` family,
  `lodestone`, `conduit`, `bee_nest`, `beehive`, `scaffolding`, `ladder`,
  `end_portal_frame`, `dragon_egg`, `jukebox`, `note_block`, all present
  as rows.)*

### 2.10 Snow / ice / misc terrain (P1–P2)
- **`snow_block`** — missing (engine has the snow *layer* id 15 but not the
  full snow block). **P1**
- **`powder_snow`** — missing. **P2**
- **`muddy_mangrove_roots`** — missing (engine has `mangrove_roots`). **P3**
- **`infested_*` (7)** + **`suspicious_sand`/`suspicious_gravel`** +
  **`turtle_egg`/`sniffer_egg`/`frogspawn`** — niche. **P3**
- **Resin (1.21.4) — 7 missing**: `resin_block`, `resin_bricks`,
  `resin_clump`, `chiseled_resin_bricks`, `resin_brick_slab/stairs/wall`. **P3**
- **`cake`** (+ `candle_cake` states), **`big_dripleaf_stem`**. **P3**

---

## 3. Partial / divergent — blocks the engine HAS but that differ from MC

The JSON pipeline (`resolve_block_model` over real MC blockstates) means most
of these render correctly; the divergences are **missing variants, missing
special-model geometry, or placeholder TileSpecs**.

1. **Legacy generic names cover only one variant.** Engine ids 5/6/7/8/11/13/
   14/20/21/22 are named `wood/leaves/planks/cobble/wool/coal/iron/flower*`
   but `name_map.rs` already points them at `oak_log/oak_leaves/oak_planks/
   cobblestone/white_wool/coal_ore/iron_ore/poppy/dandelion/blue_orchid`. They
   render right — the gap is purely that the **per-species/per-color siblings**
   are listed as missing in §2. (Not a render bug, a catalog gap.)

2. **`special_models::name_for_block` only hand-codes a handful** of
   non-cube geometries: torch (+redstone/soul/copper), 5 cross-plants
   (poppy/dandelion/blue_orchid/short_grass/fern), generic slab/slab_top/
   stairs/fence_post/door_lower/door_upper/trapdoor (548–552, 568–575),
   chest, glass_pane/iron_bars, wall_post. **Every other special shape relies
   on the JSON model loader.** Blocks that are flagged passable but whose MC
   model is a non-trivial shape and may fall back to a flat cross / cube if
   the JSON model fails to resolve: `amethyst_cluster`, `big_dripleaf`,
   `small_dripleaf`, `hanging_roots`, `spore_blossom`, `glow_lichen`,
   `weeping_vines`, `twisting_vines`, `nether_sprouts`, `crimson_fungus`,
   `warped_fungus`, `chorus_flower`. Worth a visual pass to confirm they
   don't render as flat sprites. **(verify in-world)**

3. **Many later rows use `col: 0, row: 0` legacy atlas coords** (e.g.
   ids 603–652). This is dead data — the atlas-UV path is only a fallback;
   the JSON pipeline supplies real UVs. Harmless unless a block fails JSON
   resolution, in which case it would render as `stone` (atlas 0,0). Low risk
   but means **any block whose MC model/blockstate doesn't load silently
   degrades to the stone tile**, not magenta. **(verify)**

4. **`redstone_lamp` ↔ `lit_redstone_lamp`** and
   **`redstone_ore` ↔ `lit_redstone_ore`** are split into two ids each
   because the engine has partial redstone state. `redstone_ore` is present
   only as the lit id (66) + the engine ships `redstone_ore` block — fine,
   but the lit/unlit pairing is engine-specific, not MC's single block.

5. **`obsidian_pillar` (id 450) is engine-only** — no such MC block. It maps
   to a quartz-pillar-style model. Cosmetic extra, not a gap.

6. **Snow:** engine `snow` (id 15) is the **layer** only; the full
   `snow_block` is absent (listed missing §2.10). The layer's flags/render
   are a dedicated path (`snow_positions`), divergent from MC's 8-height
   layered blockstate (engine layer is single-height).

7. **Functional vs cosmetic:** rows exist for `piston/sticky_piston/
   dispenser/dropper/hopper/observer/repeater/comparator/lever/buttons/
   pressure_plates/note_block/jukebox/target`, but the audit only confirms
   the **block exists** — whether they have MC-faithful redstone behaviour
   is out of scope here (the redstone solver covers wire+lamp+sources per
   `ids.rs` W67.1, suggesting pistons/hoppers are still cosmetic). Flagged
   as behavioural divergence to verify.

8. **Doors/trapdoors flag completeness:** `block/special_models.rs` routes
   ids 548–552 (trapdoors) and 568–575 (doors) to generic `block/trapdoor`/
   `block/door_lower`. Confirm each has `F_RAY_PASS | F_AABB_PASS` so the
   player can walk through an open door (CLAUDE.md flags rule). Iron door/
   trapdoor present; many wood doors/trapdoors still missing (§2.1).

---

## 4. Biggest wins — top 10 highest-impact block additions

Ranked by how much each makes the world *read* like MC for the least work
(most are variant-family fills that reuse an existing model template):

1. **Stripped logs + all-bark wood blocks** (`stripped_*_log/_wood`,
   `oak_wood`…`pale_oak_wood`, ~30 blocks) — ubiquitous in builds; reuse the
   log model with swapped textures. **P1**
2. **Stairs + slabs for the common stones** (`andesite/diorite/granite/
   polished_*`, `cobbled_/polished_deepslate`, `mossy_cobblestone`,
   `smooth/cut sandstone`, `red_nether_brick`, ~40 blocks) — the single most
   visible building gap; all reuse `block/slab`+`block/stairs`. **P1**
3. **`tnt`, `coal_block`, `clay`, `snow_block`, `netherite_block`,
   `bookshelf`** — iconic full-cube staples, trivial cube rows. **P1**
4. **Rails (all 4) + `redstone_wire` placeable** — unlocks the
   transport/redstone read of the world. **P1**
5. **Wood fence gates (12) + missing wood doors/fences** (`crimson/warped/
   bamboo/cherry/mangrove/pale_oak`) — farms, builds, paths. **P1**
6. **`barrel`, `enchanting_table`, `composter`, `cauldron`, `bell`,
   `campfire`** — high-frequency utility blocks players expect. **P1/P2**
7. **Stained-glass panes (16) + glazed terracotta (16)** — big colorful
   build-palette boost; panes reuse `block/glass_pane`. **P2**
8. **Quartz family (9) + tuff family (11) + deepslate brick cuts (8)** —
   completes the "polished" building tiers. **P2**
9. **Shulker boxes (17) + banners (16) + candles (16)** — storage + decoration
   the world currently can't show at all. **P2**
10. **Copper functional set** (`lightning_rod`, `copper_bulb/grate/door/
    trapdoor/bars` across oxidation+waxed, the cut-copper slab/stairs matrix,
    111 blocks) — single largest family gap; high modern-MC signal. **P2**

---

*Method note:* engine names were normalized to MC blockstate names using
`resource_pack/name_map.rs` aliases before diffing, so coverage is measured
on canonical MC identifiers. Counts were produced by set operations over the
1170-file blockstate glob and the 613-row `BLOCK_TYPES` table; the
technical/state-duplicate exclusion list is enumerated in the Section 1 note.
