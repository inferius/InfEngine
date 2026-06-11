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
| Worldgen | shape + underground solid (W121) | — | full ore roster + deepslate + bedrock ✅; no structures bar dungeons |
| Rendering | AO ✅ (W122), colormap tint ✅ (W135, plains-only) | — | 5/125 particles; no per-biome tint variation |

Detailed reports: [blocks](blocks.md) · [items](items.md) · [mobs](mobs.md) · [worldgen](worldgen.md) · [rendering-effects](rendering-effects.md)

## Cross-cutting P1 "biggest wins" (ranked by impact ÷ effort)
1. ~~**Vertex AO / smooth lighting**~~ — **DONE (W122)**: per-corner
   AO + smooth lighting in the greedy mesher. Residual: a geometric
   seam see-through case remains as an open polish task.
2. ~~**Worldgen underground = modern MC**~~ — **DONE (W121)**:
   `worldgen/strata.rs` places the full MC-1.18 ore roster with
   triangular sea-relative depth bands, swaps deepslate variants
   below the stratum threshold, and lays a jagged bedrock floor.
   Residual from this item: caves/ravines/dungeons still use a few
   absolute Y consts (see worldgen.md §4.5) and there are no
   gravel/dirt/clay/granite blobs yet.
3. ~~**Biome colormaps**~~ — **DONE (W135)** at ONE climate:
   grass/leaf/plant tiles are tinted with the plains colormap sample
   at atlas-build time. Per-biome VARIATION (desert olive, swamp,
   blending) still needs a per-vertex tint channel — that part stays
   open.
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
~~AO (#1)~~ → ~~worldgen underground (#2)~~ → ~~colormaps (#3)~~ all
landed (W121/W122/W135). Next up: the mob visual trio (#4) → bubbles
(#5) → ocean biomes / structure engine (worldgen.md wins 4-5) → then
breadth (items/blocks) in P1-first batches. Code-only changes (worldgen,
items, recipes, id-dedup) verify via build + 636 lib tests + a dump test;
visual changes (lighting, mobs, particles) verify in-game — see the
mc-parity-handoff memory for the chat-driven /fill+/tppos verification
rig (synthetic clicks are unreliable; chat commands are not).
