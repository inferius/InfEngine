# InfEngine - TODO pro pokracovani

## Posledni stav (W47.6 commit `9...`)

main.rs: **15148 LOC** (z puvodnich 17207 = -2059 / -12%).

Layout:
```
voxel_demo_mp/src/
- main.rs              15148 LOC
- ambient.rs             500   (particle spawn loop + helpers)
- emitter_scan.rs        108   (rebuild emitter maps)
- mob_meshes.rs         1106   (mob mesh loaders)
- net/
  - mod.rs              1180   (wire types)
  - dispatch.rs          964   (tick_net body)
- player.rs              394   (collision + aabb)
- render/
  - mod.rs                16
  - mobs.rs              516   (mob render loop)
```

## Hotove tento session (W46.13 - W47.6)

Render / svetlo:
- W46.13 odstranen legacy torch render path (-135 LOC)
- W46.14 particle pass depth attachment fix
- W46.15 F_DEDICATED_RENDER drop torch family
- W46.16 torch white tint (no double color)
- W46.17 wire Auto mode = Break pro tools/decor
- W46.18 torch lights 3.0->5.5, radius 8->14
- W46.19 emissive material slot (torch/lava/glowstone glow)
- W46.27 MAX_POINT_LIGHTS 8 -> 32
- W46.29 double-sided planar elements (redstone halo geom)
- W46.35 prefer lit=true variant (redstone resolves to lit template)
- W46.37 redstone halo to cutout layer

Save/load:
- W46.20 emitter maps populated on world load
- W46.22 emitter maps in apply_chunk_payload (drop redundant rebuild)
- W46.23 guard phantom torches from save inconsistency
- W46.24 per-chunk save on unload (W46.28 = async)
- W46.25 disk-first chunk reload in worker
- W46.26 prune side-maps on chunk unload
- W46.28 async chunk unload save
- W46.30 lighting BFS timing + remesh cap 8->64
- W46.31 persist day_time across load/save
- W46.32+34 lighting BFS cap by N closest chunks (500)
- W46.33 results-per-frame 4->32 / 6 ms budget

Mobs:
- W46.4 / W46.9 mob biome variant + MC pack textures
- W46.21 copper_torch block family

Refaktor (W47.x):
- W47.1 mob mesh loaders -> mob_meshes.rs
- W47.2 ray/sweep aabb -> player.rs
- W47.3 mob render loop -> render/mobs.rs
- W47.4 tick_ambient_particles -> ambient.rs
- W47.5 particle helpers -> ambient.rs
- W47.6 tick_net (940 LOC) -> net/dispatch.rs

## Aktivni / open

### Vizualni
- **Redstone torch glow** porad nejde tak jak v MC. W46.37 (cutout) nestaci.
  Mozne: refactor na per-element layer split - halo planes do
  separatniho submesh routovaneho na cutout, stick na opaque.
  Vyzaduje rozsiritm chunk_mesh_greedy_with_special o per-element
  layer hint.
- **Flame stroboskop** stale je problem (W46.8 pending). MC pouziva
  16x512 fire_0.png + .mcmeta s frame order. Particle musi cyklit
  fram es lifetime + smooth alpha fade. Treba pridat Particle
  fields sprite_frames + frame_idx + frametime + atlas slot.
- **UI vs crosshair** - overlay panely (inventory, settings) nejsou
  centrovany na crosshair. Mensi fix.
- **Selection wireframe match shape** (W46.10 pending) - torch ma
  cube wireframe misto thin-stick outline.

### Gameplay
- **Mobove nevylezou z 1 bloku** - Mob AI tick pro climb / pathing.
  Cilem prevzit MC chovani (breed, panic, follow, ...).
- **Creative hotbar prazdny slot** - placeni do leveho slotu misto
  vybraneho slot.

### Perf
- **Sky-light BFS** je porad 1-3s pro 500 chunks. MC dela paralelni
  per-chunk dispatch. Mozne urychleni rozdelenim Job::Lighting do
  N podlitku.
- **Light propagation pri rotaci** - mensi sekani porad. Async chunk
  load by mohla pomoct.

## Dalsi refaktor splity (next sessions)

Pokracovat dal po W47.6 navrh:

| Wave | Cil | LOC odhad | Riziko |
|------|-----|-----------|--------|
| W47.7 | egui inventory + hotbar -> ui/inventory.rs | ~1500 | high (lot of egui state) |
| W47.8 | place/break logic -> world/place_break.rs | ~1200 | medium |
| W47.9 | mob AI tick (uz partial extract z mob/) | - | low |
| W47.10 | sky/celestial render -> render/sky.rs | ~400 | low |
| W47.11 | chunk streamer -> world/stream.rs | ~600 | medium |

Recommend W47.10 (smallest, safest start). Pak W47.8 (clean
boundaries) nez se pustit do W47.7 UI.

## Pravidla (z CLAUDE.md)

- **Nikdy nove kodu do main.rs.** Vsechno nove jde do submodulu.
- **Modularita pred psanim.** Kdyz subsystem > 1500 LOC nebo > 100
  LOC v jedne funkci - split BEFORE.
- **MC clone first.** Inspirovat z cache MC assets +
  decompiled Java. Konzultovat s uzivatelem drive nez improvizovat.
- **Tests + clippy mezi commity.** 590 lib tests musi byt green.
  Pre-existing 7 bin tests muzou selhat (z W42/W43).
- **Conventional commits.** `feat/fix/refactor/perf(crate): Wxx.y - <short>`
  Anglicky. Co-author tag.

## Build prikazy

```
cargo build -p voxel_demo_mp --release
cargo clippy --workspace --release --all-targets -- -D warnings
cargo test -p voxel_demo_mp --release --lib
```

## Aktualni save state user
- Slug: `test3`
- Chunks: 1587 on disk, 486 stream radius load
- Test scenarios: 4 torch (regular + redstone + soul + copper),
  pridava bloky pro test save-on-unload + disk-first reload
