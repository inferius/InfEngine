# InfEngine vs Minecraft — gap audit (master index)

Five grounded audits (each = engine code vs the real MC asset pack at
`F:\Data\mc_assets\mc_pack\assets\minecraft\`, version 26.x / 1.21.5+).
This is the roadmap for the **full MC-parity push**. Port gaps FAITHFULLY
per the CLAUDE.md MC-adoption checklist (data-driven, no magic numbers).

## Coverage snapshot
| Area | Engine | MC | Coverage |
|---|---|---|---|
| Blocks | 613 rows / 526 placeable (469 distinct MC) | ~1022 obtainable | **~46%** (553 missing) |
| Items (non-block) | ~130 | ~480 (~270 core) | **~27%** |
| Mobs | 7 | ~70 living | **~10%** |
| Worldgen | sound bones, thin content | — | ores 2/12, no structures bar dungeons |
| Rendering | functional, flat | — | no AO, no colormaps, 5/125 particles |

Detailed reports: [blocks](blocks.md) · [items](items.md) · [mobs](mobs.md) · [worldgen](worldgen.md) · [rendering-effects](rendering-effects.md)

## Cross-cutting P1 "biggest wins" (ranked by impact ÷ effort)
1. **Vertex AO / smooth lighting** (rendering) — bakes one light value per
   face across all 4 corners now → flat. Per-corner AO adds contact
   shading AND **fixes the block-seam see-through** the user reported
   (it's a flat-light discontinuity at greedy-quad/chunk edges, not atlas
   bleed). GPU-free, transforms the whole look. **Top win.**
2. **Worldgen underground = modern MC** — gold/diamond/redstone/lapis/
   emerald/copper + all deepslate ores ALREADY EXIST as blocks but
   worldgen never places them (only coal+iron); no deepslate stratum, no
   bedrock floor; and the W112 sea-level raise left cave/ore/ravine/
   dungeon depths as ABSOLUTE magic numbers → everything crammed below
   y50 (under sea), 50+ blocks of empty stone under hills. Fix = rescale
   depths relative to SEA_LEVEL + place the missing ores (MC-1.18 depth
   bands) + deepslate below a depth + bedrock at the floor.
3. **Biome colormaps** (rendering) — `grass.png`/`foliage.png` colormaps
   never sampled → every biome's grass is one hardcoded green. Sample
   them per biome → instant biome variety.
4. **Mob visuals the user keeps asking for** — sheep wool overlay
   (SheepFurModel), cow udder→belly (labelled-test-cube UV pass),
   skeleton bow+arrows. Plus wire the DEAD `mob/goal.rs` (flee, eat-grass)
   into the live tick.
5. **Underwater bubbles + fullscreen overlay** (rendering, user request) —
   sprites already in the atlas; needs a velocity field on
   TexturedParticleInstance (also un-breaks textured smoke).
6. **Survival loop items** — all armor (0 pieces) + real potions
   (glass_bottle/potion missing, brewing is numeric placeholders) + 16
   dyes + bone_meal + charcoal/nuggets.
7. **Block breadth** — copper/oxidation/waxed (111), per-wood furniture
   sets + stripped logs (~115), stone/deepslate/tuff/quartz/blackstone
   slab-stair-wall cuts (~120). P1 singles: tnt, coal_block, clay,
   bookshelf, rails, farmland, barrel, enchanting_table.

## Bugs found along the way (cheap, high-signal)
- **Duplicate item ids**: hoes 332-336 AND 633-637; `iron_ingot` 43/324;
  `coal` 13/325 (item/mod.rs).
- **Non-faithful recipes** (mandate violation): wheat→steak, dirt→brick,
  leaves→log stand-ins in recipes/.
- **Worldgen feature-depth magic numbers not rescaled** after W112 (see #2).
- **`mob/goal.rs` GoalSelector is dead code** — never wired into ai.rs.
- **Mob hurt/death SFX hardcoded to the sheep beep** (damage.rs) despite
  correct per-kind rows existing.
- Special-block models: only ~15 hand-coded; the rest rely on JSON-model
  resolution and may silently degrade to the stone tile / a flat sprite —
  verify in-world.

## Suggested order
Smooth-lighting/AO (#1, biggest look jump + fixes the seam) → worldgen
underground (#2, makes the world feel like MC + fixes the W112 bug) →
biome colormaps (#3) → the mob visual trio (#4) → bubbles (#5) → then
breadth (items/blocks) in P1-first batches. Code-only changes (worldgen,
items, recipes, id-dedup) verify via build + 619 lib tests + a dump test;
visual changes (lighting, mobs, particles) verify in-game.
