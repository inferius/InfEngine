# Worldgen audit vs MC 26.1.2 — 2026-07-05 (po W222)

Mechanický audit: pro všech 35 biomů, které klasifikátor umí vydat
(`McBiome`), byly z biome JSONů vyčteny **kompletní feature listy všech
11 generačních kroků** a porovnány s tím, co engine skutečně spouští.
Zdroje pravdy: `F:\Data\mc_assets\mc_data_26.1.2\` (data z jaru) +
dekompilované zdrojáky (`mc_src\26.1.2\`).

## Ověřeno jako 1:1 (algoritmus ze zdrojáku, data z jaru)

| Subsystém | Stav |
|---|---|
| final_density + cell interpolace 4×8×4 | ✓ (W213, diff vs full-res ~0.02) |
| BlendedNoise (old_blended_noise) | ✓ exaktní (W216) |
| NormalNoise škálování (valueFactor, INPUT_FACTOR) | ✓ ověřeno proti zdroji |
| weird_scaled_sampler (QuantizedSpaghettiRarity) | ✓ (W213) |
| find_top_surface / invert (26.x opy) | ✓ (W213) |
| Aquifer (NoiseBasedAquifer) | ✓ (W213) + zaplavování bočních hranic (W221) |
| Worm carvery (cave/extra/canyon) | ✓ (W214), konfigy pinnuté testem |
| Obří žíly (OreVeinifier) | ✓ (W215); iron pásmo pod engine světem |
| Placed rudy (26 vč. emerald gate) | ✓ (W202/W219) |
| Surface rules | ✓ JSON-driven; biome podmínky dle MC JMEN (W221), noise depth (W217) |
| Stromy: straight/forking/fancy/mega_jungle/**giant**, blob/spruce/pine/acacia/bush/fancy/mega_jungle/**mega_pine**, fallen | ✓ (W208/209/**222**) |
| Vegetal patche (pipeline interpret) | ✓ (W210/218/222) |
| Ocean flora (seagrass varianty, kelp) | ✓ (W201/211) |
| Disky (sand/clay/gravel/**ice_patch/grass**) | ✓ (W219/**222**) |
| Prameny (water/lava/**lava_frozen**) | ✓ (W219/**222**) |
| Lava jezera (LakeFeature) | ✓ (W220) |
| freeze_top_layer + teplotní model | ✓ (W210), 35 biome temps |
| Barvy vody + podvodní mlha per biom | ✓ (W212) |
| Biome klasifikace | OCEANS/MIDDLE/VARIANT tabulky + addValleys sub-pravidla ze zdroje (W211) |

## Doplněno tímto auditem (W222)

- **Mega smrky/borovice** (GiantTrunkPlacer + MegaPineFoliagePlacer) —
  old-growth taigy už nejsou taiga fallback.
- **birch_tall** (super birch + padlé břízy) pro old-growth birch forest.
- **trees_sparse_jungle** (keře + fancy oak + padlé jungle kmeny).
- **trees_water** — vzácné oaky na březích řek/ostrůvcích (11 biomů).
- Houby old_growth + swamp varianty; **ice_patch** (snowy_slopes),
  **disk_grass** (mangrove), **spring_lava_frozen** (snowy_slopes).

## Zbývající mezery (poctivý seznam)

**Blokované chybějícími bloky** (fičura je jinak triviální/střední):
- glow_lichen (35 biomů!) — multiface render model
- underwater_magma (35) — magma_block
- amethyst_geode (35) — amethyst rodina + smooth_basalt
- patch_firefly_bush* (32) — firefly_bush (26.x)
- patch_bush (7), patch_dry_grass_desert, patch_leaf_litter — 26.x bloky
- forest_flowers/lilac/peony/rose_bush, patch_sunflower, wildflowers_* — dvoublokové květiny
- patch_waterlily — lily_pad
- bamboo/bamboo_vegetation — bamboo
- dark_forest_vegetation — dark_oak; trees_mangrove — mangrove; vines
- sea_pickle, warm_ocean_vegetation — korály
- iceberg_blue/packed + blue_ice — blue_ice (packed_ice už je)
- ore_infested — infested_stone
- fossil_* — bone_block + NBT struktury
- spring_lava_frozen ✓ hotovo; powder_snow jen aliasem na snow_block

**Blokované strukturní mašinérií** (další epic):
- monster_room/monster_room_deep (35) — spawner blok + loot
- desert_well; všechny struktury (mineshaft, village jigsaw, stronghold…)

**Klasifikátor** (výhledově): high/peaks slice matice (meadow/grove/
peaks/badlands/stony_shore), mushroom_fields. Surface pravidla těchto
biomů jsou už připravená (matchují jménem — jen je klasifikátor zatím
nevydává).

**Drobné dokumentované odchylky**: SplitMix64 streamy (pravidla, ne
seed-identita); engine pásmo y 0..255 (deepslate/-64 vrstva
nerealizovaná); MOTION_BLOCKING vs WORLD_SURFACE heightmapy sjednocené
na terénní floor; chunk-lokální detaily (grass repair na Y-švu, sugar
cane water check na hranici chunku).
