# InfEngine - TODO pro pokracovani

## Posledni stav

main.rs: **12193 LOC** (z puvodnich 17207 = -5014 / -29%).
Session W47.11..W47.20 delta: 14720 -> 12193 = -2527 LOC.

place_break/ submodule (W47.14): 7 files, max 377 LOC v mod.rs.
world/stream.rs (W47.18): 416 LOC.
menu/apply.rs (W47.19): 210 LOC.
bootstrap.rs (W47.20): 255 LOC.

Layout `examples/voxel_demo_mp/src/`:

```
- main.rs              14325 LOC (cilovy stav < 500)
- ambient.rs             ~500   (particle spawn loop + helpers)
- emitter_scan.rs         108
- mob_meshes.rs          1106
- fire_runtime.rs         206   (WIP, neuncommitnuto)
- net/
  - mod.rs               1180
  - dispatch.rs           964
  - container_apply.rs    (W47.8 chest+furnace)
- player.rs               394   (collision + aabb)
- render/
  - mod.rs                 13
  - mobs.rs               516
  - celestial.rs          (W47.9 sky/sun/moon)
  - point_lights.rs       (W47.10 light upload)
  - decor.rs              237   (WIP, neuncommitnuto)
```

## Hotove (W46.13 - W47.10)

Render / svetlo:
- W46.13 legacy torch path removed (-135 LOC)
- W46.14 particle pass depth attachment fix
- W46.15 F_DEDICATED_RENDER drop torch family
- W46.16 torch white tint (no double color)
- W46.17 wire Auto mode = Break pro tools/decor
- W46.18 torch lights 3.0 -> 5.5, radius 8 -> 14
- W46.19 emissive material slot (torch/lava/glowstone)
- W46.27 MAX_POINT_LIGHTS 8 -> 32
- W46.29 double-sided planar elements
- W46.35 prefer lit=true variant (redstone -> lit template)
- W46.37 redstone halo to cutout layer

Save/load:
- W46.20 emitter maps populated on world load
- W46.22 emitter maps in apply_chunk_payload
- W46.23 guard phantom torches from save inconsistency
- W46.24 per-chunk save on unload (W46.28 = async)
- W46.25 disk-first chunk reload in worker
- W46.26 prune side-maps on chunk unload
- W46.28 async chunk unload save
- W46.30 lighting BFS timing + remesh cap 8 -> 64
- W46.31 persist day_time across load/save
- W46.32 + W46.34 lighting BFS cap by N closest chunks (500)
- W46.33 results-per-frame 4 -> 32 / 6 ms budget

Mobs:
- W46.4 / W46.9 mob biome variant + pack textures
- W46.21 copper_torch block family

Refaktor (W47.x):
- W47.1  mob mesh loaders     -> mob_meshes.rs
- W47.2  ray/sweep aabb       -> player.rs
- W47.3  mob render loop      -> render/mobs.rs
- W47.4  tick_ambient_particles -> ambient.rs
- W47.5  particle helpers     -> ambient.rs
- W47.6  tick_net (940 LOC)   -> net/dispatch.rs
- W47.7  spawn_*_particles    -> ambient.rs
- W47.8  chest+furnace net    -> net/container_apply.rs
- W47.9  celestial render     -> render/celestial.rs
- W47.10 point-light upload   -> render/point_lights.rs
- W47.11 fire_tick + apply_fire_step + tick_lava_fire_emit -> fire_runtime.rs
- W47.12 snow / portal / fire / liquid decor render -> render/decor.rs
  (W47.11+12 commitnute spolu - combo commit kvuli sdilenemu main.rs diff)
- W47.13a attack target resolver -> world/place_break.rs (pure decision)
- W47.13b mining timer step -> mining::step_mining_session (pure clock)
- W47.13c RMB action classifier -> world/place_break::classify_rmb_action
- W47.13d block-break pipeline -> world/place_break::apply_block_break
- W47.13e block-place pipeline -> world/place_break::apply_block_place
- W47.13f shovel grass path -> world/place_break::apply_shovel_grass_path
- W47.13g flint+steel ignite -> world/place_break::apply_flint_and_steel_ignite
- W47.13h bucket scoop/pour -> world/place_break::apply_bucket_use
- W47.13i open furnace/chest + sleep bed -> world/place_break
- W47.13j mining damage tick -> world/place_break::tick_mining
- W47.13k PvP/mob combat dispatch -> world/place_break::apply_combat
- W47.14 place_break.rs (1261 LOC) split do 7-file submodulu
- W47.15 sky/weather/fog tint -> render/sky.rs
- W47.16 dropped items render -> render/drops.rs
- W47.17 remote/local player cube render -> render/players.rs
- W47.18 stream_chunks (509 LOC) -> world/stream.rs
- W47.19 apply_menu_action (252 LOC) -> menu/apply.rs
- W47.20 on_start (362 LOC) -> bootstrap.rs

## Aktivni / open

### Vizualni
- **Redstone torch glow** - W46.37 (cutout) nestaci. Mozne reseni:
  per-element layer split - halo planes do separatniho submesh
  routovaneho na cutout, stick na opaque. Vyzaduje rozsirit
  `chunk_mesh_greedy_with_special` o per-element layer hint.
- **Flame anim frame timing** (W46.8 pending) - reference pack pouziva
  16x512 `fire_0.png` + `.mcmeta` s frame order. Particle musi
  cyklit frames per lifetime + smooth alpha fade. Treba pridat
  Particle fields `sprite_frames` + `frame_idx` + `frametime`
  + atlas slot.
- **UI vs crosshair** - overlay panely (inventory, settings) nejsou
  centrovany na crosshair. Maly fix.
- **Selection wireframe match shape** (W46.10 pending) - torch ma
  cube wireframe misto thin-stick outline.

### Gameplay
- **Mobove nevylezou z 1 bloku** - mob AI tick pro climb / pathing.
- **Creative hotbar prazdny slot** - placeni do leveho slotu misto
  vybraneho slotu.

### Perf
- **Sky-light BFS** porad 1-3s pro 500 chunks. Mozne urychleni
  rozdelenim `Job::Lighting` do N podlitku (paralelni per-chunk
  dispatch).
- **Light propagation pri rotaci** - mensi sekani. Async chunk
  load by mohla pomoct.

## Dalsi refaktor splity

W47.13 cely hotovy (a-k). Place/break in on_update redukovany na
helper calls. Dalsi wave:

| Wave  | Cil                                                | LOC | Riziko |
|-------|----------------------------------------------------|-----|--------|
| W47.21 | render_ui (3261 LOC) split do ui/* submoduly      | ~1500 | high   |
| W47.22 | update_play (884 LOC) - intencionalni inline       | -    | n/a    |

render_ui detaily: 6073-9333 = 3261 LOC. Pre-egui setup 6073-6753
(~680 LOC state snapshots), egui closure 6757-9333 (~2576 LOC) s
AppState match (Paused 62 LOC, Settings 40 LOC, Playing 2364 LOC).
Playing branch obsahuje HUD area, debug panel, chest UI, furnace UI,
inventory UI, chat overlay, command palette, etc. Extract vyzaduje
hands-on session - mnoho state borrows uvnitr closure.

Recommend: W47.14 (split place_break.rs do 4 podsouboru: combat,
mining, rmb_actions, break_place_helpers) jako prevence pred 1500
limit. Pak W47.15+ wave podle TODO.

## Pravidla (z CLAUDE.md)

- **Nikdy nove kodu do main.rs.** Vsechno nove jde do submodulu.
- **Modularita pred psanim.** Subsystem > 1500 LOC nebo > 100 LOC
  v jedne funkci - split BEFORE.
- **Reference pack first.** Inspirovat z local cache assets v
  `data/cache/` (block models, blockstate JSON, particle textures).
  Konzultovat s uzivatelem driv nez improvizovat.
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
- Test scenarios: 4 torch varianty (regular + redstone + soul + copper),
  pridava bloky pro test save-on-unload + disk-first reload
