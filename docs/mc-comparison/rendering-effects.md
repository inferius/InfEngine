# InfEngine vs Minecraft — Rendering / Visual + Audio Effects gap report

Audit date: 2026-06-08. Scope: particles, animated textures, lighting/colour,
environment (sky/sun/moon/weather/water), audio, UI overlays. Everything below
is grounded in file reads; paths cited inline.

MC reference pack: `F:\Data\mc_assets\mc_pack\assets\minecraft\` (textures +
models + particle defs extracted; **`sounds.json` and `sounds/` are NOT
extracted in this pack** — audio roster reasoned from MC's known structure +
the engine's own mapping table).

Engine render crate: `F:\Projects\InfEngine\crates\infengine_render\src\`
Demo render glue: `F:\Projects\InfEngine\examples\voxel_demo_mp\src\render\` +
`ambient.rs`.

---

## 0. Executive summary

The engine has a solid *foundation* — a textured billboard particle pass, a
GPU tile-animation system (vertical-strip UV scroll), per-vertex baked
block+sky light, distance fog, translucent water, day/night sky tint, sun/moon
discs, and a broad audio event map. The gaps are almost all **content
breadth** and **two structural items**:

1. ~~**No ambient occlusion / smooth lighting.**~~ **DONE (W122)** —
   per-corner AO + smooth lighting landed in the greedy mesher
   (FaceKey carries the 4 corners' quantised block/sky/ao). A
   residual geometric seam-see-through case remains an open polish
   task; the original text below describes the pre-W122 state.
2. ~~**No biome colormap.**~~ **DONE at ONE climate (W135)** —
   `apply_biome_tints` samples the pack's `colormap/{grass,foliage}.png`
   at the plains climate during atlas build and tints the grayscale
   grass/leaf/plant tiles (spruce/birch/lily use the FoliageColor.java
   constants). Per-biome VARIATION (desert olive, swamp murk, blending)
   still needs a per-vertex tint channel and stays open.

Most of the particle roster and several animated-texture/environment features
are "wired but unused" — the plumbing exists (atlas packs all particle PNGs;
anim registry supports 32 slots), only the emit/lookup call sites are missing.

---

## 1. Particles

### 1.1 Engine particle infrastructure (what exists)

- **GPU textured billboard pass** — `crates/infengine_render/src/particle_pass.rs`
  + `particle.wgsl`. Camera-facing quads, alpha-blend, depth-test-read /
  depth-write-off (so flames z-reject behind walls but don't halo each other).
  Samples a dedicated particle atlas (Nearest, ClampToEdge). Instance =
  `center_world, size, uv_min, uv_max, color`.
- **Legacy CPU particle system** (`infengine_particles`) — cube particles with
  velocity + gravity, used for break/place debris and the rising flame/smoke
  (because the GPU billboards are static — see note below).
- **Particle atlas** — `resource_pack/particle_atlas.rs` packs **every** PNG in
  `textures/particle/` and exposes `sprite_uv(name, frame)` / `frame_count`.
  So the full MC sprite set (bubble, splash, crit, heart, note, portal,
  enchant, etc.) is *already in GPU memory* — the emitters just never reference
  them.

### 1.2 What the engine actually EMITS (`ambient.rs`, `app/particle_runtime.rs`)

| Engine effect | Trigger | Sprite/look | Notes |
|---|---|---|---|
| Flame | torch / fire / lava cells near player | `flame` strip, frame = `tick % n` (animates in place) | GPU billboard + a CPU rising wisp for collision |
| Smoke | torch (¼ rate), fire (⅓ rate) | CPU cube, tint sampled from `smoke_0..7` avg color | GPU smoke path exists but is **disabled** (static billboard had no velocity, looked like it sank — `ambient.rs:388`) |
| Lava drip | lava source cells | `lava` single sprite | GPU billboard, slight XZ jitter |
| Break debris | block broken | 12 CPU cubes, tinted to block albedo, gravity on | `spawn_break_particles_tinted` |
| Place dust | block placed | 6 CPU cubes, block-albedo tint | `spawn_place_particles` |

That is the **entire** emitted set: flame, smoke, lava, break-debris,
place-dust. Everything else MC emits is missing.

### 1.3 MC particle roster (from `textures/particle/` + `particles/*.json`) — MISSING

The pack ships **~125 particle definitions** (`particles/*.json`) and the
matching sprite PNGs. High-value missing emitters, roughly by how often a
player sees them:

- **`bubble` / `underwater` / `bubble_column_up` / `bubble_pop`** — underwater
  ambient + bubble columns. **User explicitly wants these** (see §4.6). Sprites
  `bubble.png`, `bubble_pop_0..4.png`, `generic_0..7.png` all packed already.
- **`splash` / `fishing` / `falling_water` / `dripping_water`** — water entry
  splash (4 frames `splash_0..3`), drip from leaves/overhangs. None emitted.
- **`crit` / `enchanted_hit` / `damage_indicator` / `sweep_attack`** — combat
  feedback (white crit stars, magic-crit, the held-attack sweep arc). The
  engine has a mob *hurt flash* (good, §4) but no hit particles.
- **`heart`** — breeding / healing hearts. Not emitted.
- **`happy_villager` (green sparkle)** — bonemeal / villager trade success.
- **`smoke` / `large_smoke` / `campfire_cosy_smoke`** — only torch/fire smoke
  exists; campfire column smoke, large_smoke (TNT/explosion) missing.
- **`flame` extras: `lava` pop, `explosion`/`explosion_emitter`, `poof`** —
  mob-death poof cloud, TNT explosion (16-frame `explosion_0..15`), gunpowder.
- **`portal` / `reverse_portal`** — the nether-portal purple star drift toward
  the block. The engine draws portal *blocks* as procedural swirl quads
  (§3) but emits no portal particles.
- **`note`** — note-block coloured note. Note-block *sounds* are mapped (§5)
  but no note particle.
- **`dust` / `dust_color_transition`** — redstone dust spark + colored redstone.
- **`falling_dust`** — gravity-block (sand/gravel) falling dust trail.
- **`item` break / `block` (block-dust)** — MC's generic "block broken /
  landed-on / sprint cloud" terrain dust. The engine's break-debris cubes are a
  rough stand-in; there is **no sprint/landing/fall dust** under the player.
- **Status-effect / mob-specific:** `effect`/`entity_effect` (potion swirls),
  `witch`, `angry_villager`, `totem_of_undying`, `dragon_breath`, `squid_ink`,
  `sculk_*`, `sonic_boom`, `warden` set, `cherry_leaves`/`pale_oak_leaves`/
  `falling_spore_blossom` (ambient biome leaf-fall), `firefly`, `snowflake`,
  `white_ash`, `crimson_spore`/`warped_spore`. All un-emitted (most are
  lower-priority / not-yet-implemented content).

**Note on the GPU smoke regression**: because the billboard pass has no
per-particle velocity, any "rising" particle (smoke, bubbles) currently has to
go through the *legacy CPU cube* path. If you add bubbles/underwater, either
(a) reuse the CPU path (cube tinted, alpha) or (b) add a velocity/age field to
`TexturedParticleInstance` so the GPU billboards can drift. Option (b) is the
better long-term fix and unlocks textured smoke too.

---

## 2. Animated textures (`.mcmeta`)

### 2.1 Engine animation system (what exists — and it's good)

- **Parser**: `resource_pack/mcmeta.rs` reads `<png>.mcmeta`
  (`frametime` ticks → ms, `frames` list or full-strip default, object-form
  `{index,time}` flattened to equal frametime). `interpolate` is parsed but
  **not honoured** (no cross-frame blending; MC interpolates water/lava).
- **Registry**: `resource_pack/anim_registry.rs` — assigns each animated
  texture a slot 1..32, dedupes by (v0, v_step, frames, frametime).
- **GPU**: `tile_anims.rs` uploads a 32-entry uniform; `mesh3d.wgsl`
  `animated_uv()` scrolls `UV.v += frame * v_step` per fragment using
  `camera.time`. Vertical-strip frames only (matches MC's stacked-PNG layout).

So **any block whose atlas tile is a vertical frame-strip with a `.mcmeta`
animates automatically** through the standard atlas path (water_still,
lava_still, fire_0/1, sea_lantern, magma, prismarine, nether_portal,
sculk, leaves-with-meta, lanterns, command blocks, kelp, seagrass, etc.).

### 2.2 MC `.mcmeta` roster (≈115 animated block textures) — coverage

The pack has ~115 `block/*.png.mcmeta` files. Coverage depends entirely on
whether the block routes its faces through the **standard atlas tile path**
(animated) vs a **special procedural/dedicated render** (not animated):

| MC animated texture | Engine status |
|---|---|
| `water_still` (frametime 2) | **Not animated as a texture.** Water is drawn by the dedicated liquid mesh through the *water-marker shader* (`render/decor.rs` → `draw_mesh_water`), which uses procedural value-noise ripple in `mesh3d.wgsl water_ripple()`, **not** the `water_still` 32-frame strip. Looks shimmery but is not MC's water. |
| `lava_still` / `lava_flow` | **Not animated as a texture.** Same path as water (`draw_mesh_water` reused for lava), procedural ripple. MC's bright animated lava strip is unused. |
| `fire_0` / `fire_1` | **Not the strip.** Fire blocks render as two procedural orange-tinted crossed quads with a sine flicker (`render/decor.rs` fires loop). The animated billboard flame *particle* exists (§1) but the fire *block* texture animation does not. |
| `nether_portal` | **Not the strip.** Portal blocks are procedural purple swirl quads (`render/decor.rs`), phase-offset per cell. MC's animated portal texture unused. |
| `sea_lantern`, `magma`, `prismarine`, `prismarine_bricks` | **Animated IF** they go through the standard atlas tile path (full-cube blocks with a `.mcmeta` strip). These are normal cube blocks, so they should pick up the anim slot via `anim_lookup` in `lighting_runtime.rs:77`. Verify visually — most likely working. |
| Leaves (`oak/birch/.../cherry/pale_oak`), `kelp`, `seagrass`, `tall_seagrass`, lanterns, `campfire_fire`, `sculk*`, `command_block_*`, `redstone_dust_*`, `tripwire`, `stonecutter_saw`, amethyst buds, flowers w/ meta | **Animated via standard path** as long as the block exists and uses the atlas tile (no dedicated render). Many of these are subtle 2-frame loops. |
| `glass.png.mcmeta` | MC ships a 1-frame meta (no visible animation) — irrelevant. |

**Bottom line for §2:** the *engine animation machinery is correct and
sufficient*, but the **four most visible animated blocks in the game — water,
lava, fire, portal — bypass it** and use procedural approximations. Routing
water_still/lava_still through the real frame strip (or at minimum honouring
`interpolate`) is the single biggest texture-fidelity win here. The `interpolate`
flag is also dropped, so even animated tiles "pop" between frames instead of
MC's smooth crossfade.

---

## 3. Lighting / colour

### 3.1 Smooth lighting / AO — **MISSING (structural)**

- `crates/infengine_voxel/src/lib.rs:1091` — quad emit: *"All 4 corners share
  the same values - the 4-corner AO smoothing pass is a follow-up."*
- Light is fetched once per face from the air-side neighbour cell
  (`app/lighting_runtime.rs:53 light_lookup`) as `(block/15, sky/15)` and
  written identically to all 4 vertices.
- `mesh3d.wgsl:447` folds it in as `light_mult = 0.05 + 0.95 * max(block,
  sky*day_factor)`.

Consequences vs MC:
- **No vertex AO** — MC darkens the corner where 2–3 neighbours are solid
  (the signature "contact shadow" in concave corners). The engine has none, so
  corners/edges look flat and plasticky.
- **No smooth light gradient** — MC averages the 4 surrounding light values per
  vertex and interpolates across the face. The engine's per-face flat light
  means adjacent faces/quads show hard light steps.
- This is also a **performance-free** win on modern GPUs and is the highest-
  impact lighting change available.

### 3.2 Biome colormaps — **MISSING**

- MC ships `textures/colormap/grass.png`, `foliage.png`, `dry_foliage.png`
  (256×256 temperature×downfall gradients). The engine **never loads them**
  (grep: no colormap sampling anywhere in `examples/voxel_demo_mp/src`).
- `models.rs:96` parses `tintindex` from block models and the comment calls it a
  *"biome tint hook"*, but nothing consumes it. Grass/leaves/water tint is the
  static `color` field in `block/registry.rs` (e.g. `short_grass` =
  `[0.36,0.62,0.28]`, grass top a fixed green).
- Result: **every biome's grass/leaves is the same green**; no swamp brown-green,
  jungle vivid, savanna olive, badlands tan, or the per-position temperature
  gradient. Water has no biome tint variation (swamp/mangrove green water).

### 3.3 What lighting DOES have (credit where due)

- Per-vertex baked block + sky light via BFS flood (block + sky engines,
  `lighting_runtime.rs:299`), day_factor-modulated sky light, 0.05 black floor.
- Hemispheric ambient (`mesh3d.wgsl:434`, warm-neutral, raised in W35.1).
- Emissive boost (`material.z`) so torch/glowstone/lava read full-bright when
  unlit.
- Up to 32 dynamic point lights (torch/fire/lava glow) with Frostbite-style
  falloff (`mesh3d.wgsl:399`).
- Directional sun with PCF-3×3 shadow map (`shadow.wgsl`, 2048²) and
  intensity tied to sun elevation (`celestial.rs:41`).

---

## 4. Environment (sky / sun / moon / clouds / weather / water) + the seam bug

### 4.1 Sky gradient

- `render/sky.rs apply_tint_and_fog` computes a single day/night clear colour
  (from `day_night(day_time)`), then mixes rain (stormy grey), snow (pale blue),
  and lightning-flash (toward white). Distance fog blends geometry to that same
  sky colour near the view edge (`mesh3d.wgsl:466`).
- vs MC: it's a **flat clear colour**, not MC's vertical **sky gradient**
  (lighter near horizon → deeper blue at zenith) plus the warm **sunrise/sunset
  horizon band**. No void fog gradient, no horizon glow at dawn/dusk.

### 4.2 Sun & moon — textures unused

- `render/celestial.rs` draws sun + moon as **over-bright untextured cubes**
  (`draw_mesh_pbr` on `player_cube`, albedo >1 to clamp bright). The sun is a
  white-ish cube; the moon a cool cube whose **size/brightness fakes the phase**
  (it shrinks for new moon) because *"we don't have phase atlas sampling yet"*
  (`celestial.rs:90`).
- MC ships `textures/environment/sun.png` and `environment/moon/` (the
  `moon_phases` 4×2 atlas). `sky.rs:32/67` even has a `moon_phases.png` UV
  helper, but `celestial.rs` doesn't use it. So: **no sun texture, no real moon
  phase texture** — both are tinted cubes.

### 4.3 Clouds — **MISSING**

- No cloud rendering at all (grep: no `clouds` consumer). MC's
  `environment/clouds.png` (the scrolling cloud layer at y≈192) is absent. The
  sky above the fog is just flat clear colour.

### 4.4 Weather — partial

- Weather drives only **sky tint** (rain grey / snow blue) + lightning flash
  (`render/sky.rs`). There are **no falling rain/snow particles**, no rain
  streaks (`environment/rain.png` / `snow.png` unused), no ground splash on
  rain, no fog-density change, and no rain *audio* loop. So "it's raining" is
  communicated purely by a greyer sky.

### 4.5 The surface-seam see-through bug (block faces, e.g. tree trunk)

**Root cause is geometry/lighting at quad boundaries, NOT atlas filter bleed.**
Confirmed: the atlas sampler is `Nearest` with no mipmaps
(`crates/infengine_render/src/...` mesh sampler), and `mesh3d.wgsl:326-339`
already clamps tile UV to `[tile_uv0 + half_texel, tile_uv0 + tile_size -
half_texel]` to kill atlas bleed. So the hairline is one (or both) of:

1. **Flat per-face light discontinuity (§3.1).** Greedy-meshed quads each carry
   a single light value; where the trunk meets air/leaves or crosses a
   chunk/greedy-quad boundary, two adjacent quads can have different light →
   a visible bright/dark hairline that reads as a "see-through" sliver. Smooth
   per-vertex light (the AO follow-up) would dissolve it because the gradient
   would match across the seam.
2. **Sub-texel clamp gap / T-junction.** The half-texel inset (W31.3) keeps the
   sampler inside the tile but means the very edge row of texels is never
   sampled — at a boundary between a merged greedy quad and a smaller
   neighbouring quad this can leave a 1px crack, especially with the clear/sky
   colour showing through. Greedy meshing without T-junction welding can also
   leave a literal pinhole where a large quad meets two smaller ones.

**Recommended fix order:** (a) implement vertex AO/smooth light so the lighting
seam disappears and the surface gains MC's corner shading anyway; (b) if a
literal geometric pinhole remains, address greedy-mesh T-junctions (weld or
overlap by an epsilon along shared edges) rather than widening the UV inset.

### 4.6 Underwater (W114) + what the user wants on top

- Current (`render/sky.rs:68`, gated by `underwater`): washes the clear colour
  to deep blue-green `[0.06,0.18,0.36]` and pulls fog in tight (0.5 → 22 world
  units). That's the *only* underwater effect. The code comment itself notes
  *"A dedicated fullscreen blue overlay … can layer on later if wanted."*
- **Missing vs MC underwater:**
  - **Bubble particles** — MC constantly emits `bubble`/`underwater` ambient
    particles around the submerged camera, plus `bubble_column_up` over
    soul-sand/magma. **User explicitly requested this.** Sprites
    (`bubble.png`, `bubble_pop_0..4`, `generic_0..7`) are already in the atlas.
  - **Fullscreen `textures/misc/underwater.png` overlay** — MC's blue
    vignette/distortion over the whole screen. The engine has no post-process
    fullscreen pass for it (only the clear-colour + fog tint).
  - **Oxygen/bubble HUD** — the depleting bubble bar above the hotbar. (Drowning
    logic may exist game-side; the HUD render isn't part of this.)
  - **Water-surface-from-below** look and `falling_water`/splash on entry/exit.

  **Concrete next step for the user's request:** add an underwater bubble emitter
  — when the camera (eye) voxel is water, every few ticks spawn a few small
  rising `bubble` billboards (or CPU cube particles with +Y velocity) in a small
  radius around the player, popping (`bubble_pop`) near the surface. This mirrors
  the existing ambient-particle throttle pattern in `ambient.rs` and reuses the
  already-packed bubble sprites. A `bubble_column_up` over magma/soul-sand and a
  one-shot `splash` on water entry are natural follow-ups.

### 4.7 Water rendering (translucent, W108/W111/W113/W114)

Working and reasonably faithful: per-chunk liquid mesh, alpha-blend transparent
pipeline (cull_mode=None) drawn after opaque terrain (`render/decor.rs:194`),
alpha 0.65, procedural caustic ripple. Gaps: not MC's `water_still` texture
(§2), no biome water tint (§3.2), no surface foam/edge, no `interpolate` smooth
flow, no reflections.

---

## 5. Audio

**Caveat:** this pack has **no `sounds.json` and no `sounds/` directory**
extracted, so exact event-count coverage can't be diffed file-by-file. The
engine *does* parse `sounds.json` from the asset store at runtime
(`resource_pack/sounds.rs load_sounds_via_store`) — assessment below is of the
**engine→MC event mapping** (`ENGINE_TO_MC`, ~120 rows) vs MC's known roster.

### 5.1 Covered well (engine `ENGINE_TO_MC`)

- **Block dig/step per material** — stone, wood, grass, dirt/gravel, sand,
  glass, wool, snow, metal, slime, honey, coral, sculk, nether_wart, nylium,
  netherrack, basalt, bone, nether_bricks. Solid breadth.
- **Mobs** — ambient/step/hurt/death for sheep, zombie, chicken, pig, cow,
  skeleton, spider (`mob/<kind>/{say,step,hurt,death}`).
- **Redstone** — buttons (stone+wood), lever, piston extend/contract, dispenser
  dispense/fail, note-block (8 instruments).
- **Workstations** — furnace/smoker/blast-furnace crackle, anvil use/land/break,
  grindstone, smithing table, brewing stand, beacon (activate/deactivate/
  ambient), conduit, bell.
- **Doors / trapdoors / containers** — wood+iron doors & trapdoors, chest/
  barrel/ender-chest/shulker open+close.
- **Fire/lava** — ignite, ambient, extinguish, lava pop/ambient.
- **Liquids** — splash in/out, water ambient. **UI** — button click.
- **Portal** — ambient/travel/trigger; dragon-egg jump.

### 5.2 Gaps / missing audio

- **Player events** — `entity.player.hurt`, `.death`, `.levelup`,
  `.burp` (eat finish), `.breath` (under/after water), fall-damage `.big_fall`
  /`.small_fall`. Not in the map.
- **Item/equip** — `item.armor.equip_*`, tool break (`entity.item.break`), bow
  shoot / arrow hit, fishing bob/retrieve, eat/drink loops
  (`entity.generic.eat`/`drink`), bucket fill/empty (water+lava+fish).
- **XP orb pickup** (`entity.experience_orb.pickup`), **item pickup**
  (`entity.item.pickup`) — frequent, satisfying feedback; absent.
- **Ambient world** — cave ambience (`ambient.cave`), underwater ambient loops
  (`ambient.underwater.*`), weather: **rain/thunder loop** (`weather.rain`,
  `entity.lightning_bolt.thunder`/`.impact`). No rain audio at all.
- **Music** — `music.*` / `music_disc.*` (background music + jukebox discs).
  Not referenced.
- **TNT/explosion** (`entity.tnt.primed`, `entity.generic.explode`),
  **bonemeal** (`item.bone_meal.use`), **mob extras** (creeper hiss/`primed`,
  villager trade/yes/no, enderman, etc. — only the 7 mobs above are mapped).
- **Synth fallback caution** (per CLAUDE.md W56.1): keys without a real OGG must
  pass a no-op `|_,_| {}` to `sfx_at`, else they emit the annoying beep. Any new
  mapping rows should follow that rule.

---

## 6. Biggest wins (priority order)

1. **Vertex AO + smooth lighting** (`infengine_voxel` mesher — implement the
   "4-corner AO smoothing follow-up"). Fixes the surface-seam see-through (§4.5),
   adds MC's contact shading, biggest single visual upgrade. GPU-free.
2. **Biome colormaps** — load `colormap/grass.png`+`foliage.png`, consume the
   already-parsed `tintindex`, tint by biome (temp×downfall). Kills the
   single-green-grass look across all biomes; also unlocks biome water tint.
3. **Underwater bubbles + fullscreen overlay** (user request) — bubble emitter
   around the submerged camera reusing packed `bubble`/`bubble_pop` sprites, a
   one-shot water-entry `splash`, and a fullscreen `misc/underwater.png` post
   pass on top of the W114 blue fog. (Needs a velocity field on
   `TexturedParticleInstance` or reuse of the CPU cube path.)
4. **Real water/lava/fire/portal animated textures** — route water_still /
   lava_still / fire_0-1 / nether_portal through the existing anim registry
   (frame strip) instead of the procedural shaders, and honour the `interpolate`
   flag for MC's smooth crossfade.
5. **Core particle pack** — break/place *block-dust* using the actual block
   texture, splash, crit/enchanted_hit/sweep (combat), heart (breeding/heal),
   note (note-block), poof (mob death), and falling_dust (gravity blocks).
   The atlas already holds every sprite.
6. **Textured sun + real moon-phase atlas** — sample `environment/sun.png` and
   the `moon_phases` atlas (UV helper already in `sky.rs`) instead of bright
   cubes; add a vertical **sky gradient + sunset horizon band**.
7. **Weather particles + audio** — falling rain/snow billboards using
   `environment/rain.png`/`snow.png`, rain-splash on ground, a rain/thunder
   audio loop. Currently weather is sky-tint-only.
8. **Clouds** — a scrolling `environment/clouds.png` layer; entirely absent
   today.
9. **Block-breaking crack overlay** — render `destroy_stage_0..9` over the
   targeted block as mining progresses (the 10 textures are in the pack); today
   mining shows only a colored shrinking wireframe box
   (`app_on_update.rs:1503`), not MC's crack overlay.
