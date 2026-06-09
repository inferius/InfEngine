# MC vs InfEngine — ITEM gap report (non-block items)

Scope: **non-block items only** — tools, weapons, armor, food, brewing,
dyes, ingredients, buckets, projectiles, vehicles, utility. Block-form
items (every block also gets an `ItemType`) are out of scope except where
they double as a crafting ingredient the survival loop needs.

Ground truth:
- Engine: `examples/voxel_demo_mp/src/item/mod.rs` — `ITEM_TYPES` table
  (ids 1..=652, ends at `fern`). Icon wiring:
  `src/resource_pack/name_map.rs` (`ITEM_NAME_MAP`). Recipes:
  `src/recipes/{mod,shaped,smelting,brewing}.rs`.
- MC: `F:\Data\mc_assets\mc_pack\assets\minecraft\models\item\*.json`
  (1284 models) and `...\textures\item\*.png` (792 textures). This pack
  is a **very recent** version: it ships `copper_*` tools/armor,
  `*_spear`, `mace`, `wind_charge`, `resin_*`, `pale_oak_*`,
  `firefly_bush`, `leaf_litter`, `wildflowers`, `*_nautilus_armor` —
  i.e. 1.21.5+/snapshot-era content.

---

## 1. Counts

### Minecraft (from `textures/item/*.png`, the per-item icon source)

| Bucket | Count | Notes |
|---|---:|---|
| Raw item-texture PNGs | 792 | includes animation frames + overlays |
| After dropping anim frames/overlays (`*_pulling_N`, `*_brushing_N`, `*_in_hand`, `*_standby`, `*_blocking`, `*_throwing`, `*_open_*`, `*_overlay`) | ~745 | |
| **Distinct non-block MC items** (collapsing `clock_NN`→clock, `compass_NN`→compass, `light_NN`→light, `recovery_compass_NN`→recovery_compass; counting each `_spawn_egg`/`music_disc_`/`_sherd`/`_banner_pattern`/`smithing_template`/`_dye`/`_candle`/`_harness` as one member) | **≈ 480** | this is the realistic "MC item" universe for gap purposes |
| of which: spawn eggs | 87 | one giant cosmetic family |
| music discs | 21 | |
| pottery sherds | 23 | |
| armor-trim smithing templates | 19 | + `netherite_upgrade` = 19 total templates |
| dyes | 16 | |
| candles (item form) | 16 | also block |
| boats/rafts (+chest variants) | 20 | |
| horse armor | 6 | + `wolf_armor` + `*_nautilus_armor` (copper era) |
| banner patterns | 10 | |

### Engine (`ITEM_TYPES`, non-block rows only)

The table has **652 rows**, but the overwhelming majority are block-form
(`places: Some(..)`). Counting only `places: None` non-block items:

| Engine non-block family | Count | ids |
|---|---:|---|
| Pickaxes | 6 | 16,17,18,154,155,274 |
| Swords | 6 | 23,24,25,156,157,275 |
| Shovels | 6 | 29,30,31,158,159,276 |
| Axes | 6 | 32,33,34,160,161,277 |
| Hoes | 11 (dupe set) | 274-ish + 332–336 + 633–637 (two overlapping hoe sets) |
| Ranged/combat | 5 | bow 638, crossbow 639, shield 642, arrow 643, (no trident/mace) |
| Tools misc | 3 | flint_and_steel 48, shears 640, fishing_rod 641 |
| Buckets | 4 | bucket 38, water 39, lava 40 (empty/water/lava only) |
| Food | ~30 | apple, bread, cooked_*, raw_*, fish, melon_slice, cookie, pumpkin_pie, golden_apple, baked_potato, potato, carrot, beetroot, mushroom_stew, dried_kelp |
| Ingredients / materials | ~45 | sticks, ingots (iron/gold/copper/netherite), raw ores, diamond/emerald/lapis/quartz/coal/flint, redstone/glowstone dust, blaze_rod/powder, magma_cream, string, leather, paper, book, map, compass, clock, feather, bone, ender_pearl, ghast_tear, slime_ball, gunpowder, spider_eye, sugar, seeds, ink_sac, glow_ink_sac, echo_shard, nautilus_shell, heart_of_the_sea, prismarine shard/crystals, amethyst_shard, fermented_spider_eye |
| **Non-block total** | **≈ 130** | (some duplicate ids: iron_ingot 43+324, coal 13-block + 325, two hoe sets) |

**Headline:** engine has **~130** distinct non-block items vs MC's
**~480**. But MC's count is inflated by 87 spawn eggs + 21 discs + 23
sherds + 19 templates + 16 dyes + 16 candles + 20 boats + 10 banner
patterns ≈ **210 "cosmetic/long-tail"** entries. Stripping those, the
**survival-relevant non-block gap is ≈ 270 → ~130 present, ~140 missing**,
and the missing set is heavily weighted toward **armor, dyes, potions,
nuggets, and utility items**.

---

## 2. Missing items by family (with survival-impact priority)

Priority legend: **P1** = blocks/breaks the core survival loop;
**P2** = major progression/QoL; **P3** = cosmetic / long-tail / niche.

### 2.1 Armor — **P1, entire category missing**

Engine has **zero** armor items. `mc_stack_override` already *anticipates*
`*_helmet/_chestplate/_leggings/_boots`, `elytra`, `shield`, `horse_armor`
(item/mod.rs:2335–2346) — the stack rule is wired but no rows exist.

MC armor pieces present (textures/item):
`leather_/golden_/chainmail_/iron_/diamond_/netherite_/copper_` ×
`{helmet,chestplate,leggings,boots}` = 28, plus `turtle_helmet`.

| Missing | Priority |
|---|---|
| leather/iron/diamond/netherite full sets (16 pieces) | **P1** (damage mitigation is mandatory for survival) |
| golden + chainmail sets (8 pieces) | **P2** |
| copper armor set (4 pieces) — newest tier | P3 |
| `turtle_helmet` | P3 |
| `wolf_armor`, `*_horse_armor` (6), `*_nautilus_armor` (copper era) | P3 |

### 2.2 Tools / weapons tiers — **mostly complete**

Engine covers pickaxe/sword/shovel/axe/hoe across wood→stone→iron→gold→
diamond→netherite, plus bow, crossbow, shears, fishing_rod, shield, arrow,
flint_and_steel. Gaps:

| Missing | Priority |
|---|---|
| `trident` | **P2** (ranged + Riptide; ocean loop) |
| `mace` | P2 (newest heavy weapon) |
| `spectral_arrow`, `tipped_arrow` | P3 |
| `copper_*` tools (pickaxe/sword/shovel/axe/hoe/spear) — newest | P3 |
| `*_spear` (wood→netherite) — snapshot weapon line | P3 |
| **Duplicate cleanup (engine bug, not MC gap):** two hoe sets exist — ids 332–336 (`wooden_hoe`…`diamond_hoe`) AND ids 633–637 (`hoe_wood`…`hoe_diamond`). Also `iron_ingot` at id 43 and id 324, and `coal` as block-id 13 and item-id 325. | note |

### 2.3 Food — several survival staples missing

Engine food is solid (cooked meats, all 4 fish raw+cooked, bread, apple,
golden_apple, pie, potato/baked, carrot, beetroot, cookie, melon_slice,
mushroom_stew, dried_kelp). Missing:

| Missing | Priority |
|---|---|
| `golden_carrot` | **P2** (best saturation; brewing ingredient) |
| `cooked_rabbit` / `rabbit` / `rabbit_stew` | P2 |
| `beetroot_soup`, `suspicious_stew`, `rabbit_stew` | P2 (bowls already exist) |
| `sweet_berries`, `glow_berries` | P3 |
| `chorus_fruit` / `popped_chorus_fruit` | P3 (End loop) |
| `poisonous_potato`, `pufferfish`, `cod`/`salmon` raw item names (engine uses `raw_cod`; MC stem is `cod`) | P3 (icon-name mismatch risk) |
| `glistering_melon_slice`, `cake` (block+item), `honey_bottle` | P3 |

### 2.4 Brewing — **P1 for combat survival, only placeholders today**

`recipes/brewing.rs` exists but every id is a **numeric placeholder**
(700–715: `nether_wart_item placeholder`, `water_bottle placeholder`,
`awkward_potion placeholder`…). **No real potion/bottle items in
`ITEM_TYPES`.** The brewing-block (`brewing_stand` id 566) and ingredients
(`blaze_powder`, `nether_wart`, `glowstone_dust`, `fermented_spider_eye`,
`magma_cream`, `ghast_tear`, `spider_eye`, `sugar`, `redstone_dust`) exist,
but there is nothing to brew *into*.

| Missing | Priority |
|---|---|
| `glass_bottle` | **P1** (gates all brewing) |
| `potion`, `splash_potion`, `lingering_potion` (data-driven variants) | **P1** |
| `dragon_breath` (lingering), `experience_bottle` | P2 |
| `glistering_melon_slice`, `golden_carrot`, `rabbit_foot`, `phantom_membrane`, `pufferfish`, `turtle_scute` (brewing ingredients) | P2 |
| `ominous_bottle` | P3 |

### 2.5 Dyes & color economy — **P2, entire family missing**

Engine has **zero** `*_dye` items, yet ships 16 wool + 16 stained glass +
16 concrete + 16 terracotta + 16 carpet + 16 beds — all of which MC makes
**via dyes**. Right now those colored blocks can only be obtained directly
(creative), with no crafting path.

| Missing | Priority |
|---|---|
| 16 `*_dye` items | **P2** (unlocks the entire color-block economy) |
| `bone_meal` (white dye + crop growth + the `bonemeal` mechanic) | **P2** |
| `ink_sac` exists (348) / `glow_ink_sac` exists (349); `cocoa_beans` (brown dye) **missing** | P2 |
| `lapis_lazuli` exists (315, = blue dye source) | ok |

### 2.6 Materials / ingredients — assorted P2/P3 gaps

| Missing | Priority |
|---|---|
| `charcoal` | **P2** — engine smelts logs → coal id 13 (reuses coal!), so charcoal as a distinct item/fuel doesn't exist |
| `gold_nugget`, `iron_nugget`, `copper_nugget` | **P2** (golden_carrot/glistering_melon/clock/many recipes) |
| `clay_ball` | P2 (→ brick → terracotta chain) |
| `honeycomb`, `honey_bottle` | P2 (waxing copper, candles) |
| `bone_meal` | P2 (see dyes) |
| `nether_star` | P2 (beacon; beacon block exists id 567) |
| `slime_ball` exists (298); `phantom_membrane`, `rabbit_foot`, `rabbit_hide`, `shulker_shell`, `armadillo_scute`, `turtle_scute`, `breeze_rod`, `resin_clump`/`resin_brick`, `disc_fragment_5` | P3 |
| `blaze_powder` exists, `breeze_rod` missing | P3 |
| `wind_charge`, `fire_charge`, `firework_rocket`, `firework_star`, `end_crystal` | P3 |

### 2.7 Buckets — **P2, only 3 of 11**

Engine: empty/water/lava only. MC also has:
`milk_bucket` (**P2** — clears potion effects, cake recipe),
`powder_snow_bucket` (P3), and 5 fish buckets
(`axolotl/cod/salmon/pufferfish/tropical_fish/tadpole`, P3).
`mc_stack_override` already lists all of these (item/mod.rs:2350–2359).

### 2.8 Utility / tools-of-convenience — P2/P3

| Missing | Priority |
|---|---|
| `name_tag`, `lead`, `saddle` | **P2** (mob/animal handling) |
| `spyglass` | P2 |
| `totem_of_undying` | P2 (survival panic button) |
| `elytra` | P2 (late-game traversal) |
| `clock`/`compass` exist as items (308/307) but engine maps to single sprite; MC ships 32-frame `clock_NN`/`compass_NN` + `recovery_compass` (missing) | P3 |
| `filled_map` / `map`(empty exists id 306) / `knowledge_book` / `writable_book` / `written_book` / `enchanted_book` | P3 |
| `goat_horn`, `painting`, `item_frame`/`glow_item_frame`, `armor_stand`, `flower_pot`, `bell`, `campfire`/`soul_campfire`, `cauldron`, `decorated_pot`, `brush` | P3 (mix of block-item + entity items) |

### 2.9 Vehicles & rails — **P2, entire category missing**

Engine has no minecarts, boats, or rails as items.
MC items: `minecart`, `chest_minecart`, `furnace_minecart`,
`hopper_minecart`, `tnt_minecart`, `command_block_minecart`; 20
boat/raft textures; rails are blocks (`rail`, `powered_rail`,
`detector_rail`, `activator_rail`). `mc_stack_override` already
anticipates `boat`/`minecart` (item/mod.rs:2362–2363).

| Missing | Priority |
|---|---|
| `minecart` + 5 variants | P2 |
| boats/rafts (per wood, +chest) | P2 |
| rails (block items) | P2 |

### 2.10 Spawn eggs / discs / sherds / banner patterns / smithing templates — **P3 long tail**

- 87 `*_spawn_egg` (creative only) — P3.
- 21 `music_disc_*` + `disc_fragment_5` (jukebox 592 exists) — P3.
- 23 `*_pottery_sherd` + `brush` + `decorated_pot` — P3.
- 10 `*_banner_pattern` + banners/looms (loom 565 exists) — P3.
- 19 smithing templates (`netherite_upgrade` + 18 armor trims) +
  smithing_table 557 exists — **`netherite_upgrade_smithing_template` is
  P2** if netherite gear is meant to be craftable (currently netherite
  tools exist as items but no upgrade path); the 18 trims are P3.

### 2.11 Projectiles / throwables — P3

`snowball`, `egg`, `ender_eye`, `wind_charge`, `fire_charge`,
`firework_rocket`, `spectral_arrow`, `tipped_arrow`, `experience_bottle`.
All P3 except `ender_eye` (**P2** — stronghold/End access; ender_pearl
item 295 already exists, blaze_powder 320 exists, so the craft is one row
away).

---

## 3. Crafting / smelting / brewing gaps

Current recipe coverage (`recipes/`):
- **Shapeless 2×2 + shaped 3×3:** planks, sticks, crafting table, chest,
  furnace, all tool/sword tiers (wood/stone/iron only in shaped),
  bucket, ladder, bed, book, bread, compacted mineral blocks
  (iron/gold/diamond/emerald/lapis/redstone/raw-metal/amethyst/slime),
  paper, compass, hay bale.
- **Smelting:** raw_iron/gold/copper → ingots, iron_ore→ingot,
  cobble→stone, sand→glass, logs→charcoal(=coal id 13), wheat→cooked_beef
  (stand-in), dirt→bricks (stand-in).
- **Brewing:** placeholder ids only (no real items).

Major MC recipe families the engine **lacks**:

| Recipe family | Priority | Why it matters |
|---|---|---|
| **Armor crafting** (helmet/chestplate/leggings/boots per material) | **P1** | no armor items exist to craft |
| **Gold/diamond/netherite tool & sword recipes** (shaped) | **P2** | items exist but `SHAPED_RECIPES` only defines wood/stone/iron; gold/diamond/netherite have no shaped recipe (only the table rows + creative) |
| **Netherite upgrade** (smithing: diamond gear + netherite_ingot + template) | **P2** | netherite tools exist with no obtain path |
| **Dye crafting** (flower→dye, dye+block→colored block, 16×) | **P2** | colored blocks have no survival path |
| **Brewing chains** (real potions) | **P1** | see §2.4 |
| **Nugget ↔ ingot** (9 nuggets ↔ ingot, golden_carrot, glistering_melon) | P2 | nuggets missing |
| **Charcoal as distinct fuel/item** | P2 | currently collapses into coal id 13 |
| **Bone → bone_meal → white dye / crop growth** | P2 | bone exists (294), bone_meal missing |
| **Food prep** (golden_carrot, golden_apple already exists, rabbit_stew, beetroot_soup, suspicious_stew, cake) | P2 | bowls exist; soups/stews missing |
| **Stonecutter recipes** (stonecutter 564 exists) | P3 | block-shaping shortcut |
| **Minecart/boat/rail crafting** | P2 | vehicle items missing |
| **flint_and_steel** (iron+flint), **shears** (2 iron), **bucket** variants | P2 | items exist; flint exists (327); recipes absent |

Note: existing recipes lean heavily on **stand-in inputs** (wheat→cooked
beef, dirt→bricks, 4 leaves→log, 4 coal→glowstone). These are non-faithful
and should be revisited once real ingredients land (CLAUDE.md mandate:
faithful data-driven porting, fix divergences *after* default works).

---

## 4. Biggest wins — top 10 item additions for a faithful survival loop

Ranked by impact on the core "mine → craft → fight → progress" loop:

1. **Armor sets (leather/iron/diamond/netherite, 16 pieces) + armor
   recipes.** *P1.* Damage mitigation is the #1 missing survival pillar;
   stack rules are already wired. Follow the MC-adoption checklist Item
   path + add 16 shaped recipes.
2. **Potions: `glass_bottle` + data-driven `potion`/`splash`/`lingering`
   + wire the real brewing table.** *P1.* All ingredients + brewing_stand
   already exist; only the bottle/potion items + non-placeholder
   `BREWING_RECIPES` are missing.
3. **16 `*_dye` items + `bone_meal` + `cocoa_beans` + dye→block recipes.**
   *P2.* Unlocks the entire 80+ colored-block economy that's currently
   creative-only.
4. **`charcoal` as a distinct item** (fix the smelt-log→coal-id-13 hack)
   **+ `gold_nugget`/`iron_nugget`/`copper_nugget`.** *P2.* Charcoal is the
   early-game torch fuel; nuggets gate golden_carrot/glistering_melon/clock.
5. **Gold/diamond/netherite shaped tool recipes + netherite smithing
   upgrade (`netherite_upgrade_smithing_template`).** *P2.* The high-tier
   tool items exist but cannot be obtained legitimately.
6. **`milk_bucket` + `ender_eye` + `nether_star`.** *P2.* Milk clears
   effects (combat staple); ender_eye gates the End (ender_pearl +
   blaze_powder already present); nether_star powers the existing beacon.
7. **Rabbit + soups/stews family (`rabbit`, `cooked_rabbit`, `rabbit_stew`,
   `beetroot_soup`, `suspicious_stew`) + `golden_carrot`.** *P2.* Fills the
   biggest food/saturation holes; bowls already exist.
8. **`name_tag`, `lead`, `saddle`, `totem_of_undying`, `spyglass`,
   `elytra`.** *P2.* Core mob-handling + survival-utility items with no
   block dependencies.
9. **Minecarts (6) + boats/rafts + rails.** *P2.* Entire transportation
   category; stack overrides already anticipate boat/minecart.
10. **De-dup + faithful-recipe cleanup:** collapse the two hoe sets
    (332–336 vs 633–637), the duplicate `iron_ingot` (43/324) and `coal`
    (13/325); replace stand-in recipe inputs (wheat→steak, dirt→brick,
    leaves→log) with real ingredients. *P2 hygiene* — prevents the gap
    list from drifting further and keeps the table MC-strict.

---

### Appendix — engine items already present that only need a *recipe/source*
(not a new item row): `flint` (327), `flint_and_steel` (48), `shears`
(640), all bucket variants' empties, `nether_wart` (502), `blaze_powder`
(320), `magma_cream` (321), `ghast_tear` (297), `glowstone_dust` (310),
`fermented_spider_eye` (322), `paper`/`book`/`map`/`compass`/`clock`,
`sugar` (319), `bowl` (347), `mushroom_stew` (346). Wiring recipes for
these is cheaper than the item-row work above.
