# MC Comparison — Mobs / Entities

Audit of `voxel_demo_mp`'s mob/entity system against real Minecraft (asset pack at
`F:\Data\mc_assets\mc_pack\assets\minecraft\`). Everything below is grounded in file
reads; paths are cited inline. Engine is at parity-tag **W92** for models (universal
MC `ModelPart` port) with later fixes through ~W99.

Date: 2026-06-08.

---

## 0. How the engine models mobs (the "universal entity model")

`examples\voxel_demo_mp\src\entity_model\mod.rs` is a faithful, data-driven port of
MC's `ModelPart`/`PartPose` pipeline:

- `McCube { origin, size, uv, inflate, mirror }` == MC `CubeListBuilder.addBox(...).texOffs(...)`.
- `Bone { name, pivot, rotation, cubes }` == MC `ModelPart` + `PartPose`.
- `bake()` builds one mesh per bone, normalizes UVs by the **actual** loaded texture
  size (so a 64×32 or 64×64 sheet both work), and `bone_matrix()` applies the MC→engine
  flip (`scale(-1,-1,1)` * 1/16) exactly once. This matches the project mandate
  (faithful MC data, no eyeballed magic numbers — see `CLAUDE.md`).

Per-mob model files exist for all 7 kinds: `cow.rs`, `sheep.rs`, `pig.rs`, `chicken.rs`,
`zombie.rs`, `skeleton.rs`, `spider.rs`. Each is a small declarative table of MC cubes.
This infra is **good** and is the right foundation to add the missing mobs onto.

Texture binding: `examples\voxel_demo_mp\src\mob_meshes.rs::load_non_sheep_mob_meshes`
discovers `<entity>/<mob>.png` (with `_temperate/_cold/_warm` biome variants for
chicken/pig/cow via `entity_has_biome_variants`), bakes the model, and stores a
`BakedModel` per kind in `MobMeshes`. Render dispatch is one match arm per kind in
`examples\voxel_demo_mp\src\render\mobs.rs::draw_remote_mobs`.

---

## 1. Roster: MC mobs vs the 7 engine mobs

### Engine has 7 mobs

Source: `examples\voxel_demo_mp\src\mob\kinds.rs` (`MobKind` enum, discriminants 0–6).

| # | MobKind | Class | Model file | Texture loaded as |
|---|---------|-------|-----------|-------------------|
| 0 | Sheep   | passive | `entity_model\sheep.rs`   | `sheep/sheep.png` |
| 1 | Zombie  | hostile | `entity_model\zombie.rs`  | `zombie/zombie.png` |
| 2 | Chicken | passive | `entity_model\chicken.rs` | `chicken/chicken_temperate.png` |
| 3 | Pig     | passive | `entity_model\pig.rs`     | `pig/pig_temperate.png` |
| 4 | Cow     | passive | `entity_model\cow.rs`     | `cow/cow_temperate.png`* |
| 5 | Skeleton| hostile | `entity_model\skeleton.rs`| `skeleton/skeleton.png` |
| 6 | Spider  | hostile | `entity_model\spider.rs`  | `spider/spider.png` |

\* **Texture-name risk (verify in-game):** the cow dir now ships
`cow_temperate.png / cow_cold.png / cow_warm.png` (confirmed by
`ls .../textures/entity/cow/`), and the loader DOES request the biome-variant name first
for cow/pig/chicken, so cow resolves. But there is **no bare `cow.png`** anymore — if any
future code path (e.g. `mob_skin.rs::entity_texture_subpath` which still returns
`("cow","cow")`) is used, it will miss. `mob_skin.rs` is a legacy W12 discovery helper and
appears superseded by `mob_meshes.rs`; confirm it is not on the live load path.

### MC's full living-mob roster

Source: `F:\Data\mc_assets\mc_pack\assets\minecraft\textures\entity\` (one subdir per
entity). Filtering out **non-mob** entity dirs (banner, bed, boat, chest, chest_boat,
decorated_pot, sign, shield, minecart, conduit, beacon, bell, lead_knot, end_crystal,
end_portal, enchantment, experience, fishing, projectiles, equipment, armorstand) leaves
the living-entity roster below. "Engine?" = whether we have it.

#### Passive

| MC mob | Engine? | Notes |
|--------|:------:|-------|
| sheep | ✅ | wool overlay missing (see §2) |
| cow | ✅ | udder on wrong face (see §2) |
| pig | ✅ | no saddle/ride |
| chicken | ✅ | no egg-laying, no fleeing |
| mooshroom | ❌ | cow variant; `mooshroom_red/brown.png` present |
| rabbit | ❌ | 16 color/biome variants on disk |
| bat | ❌ | ambient flyer |
| squid | ❌ | aquatic |
| glow_squid | ❌ | aquatic (under `squid`/`fish`) |
| cod / salmon / tropical_fish / pufferfish | ❌ | `fish/` dir, 17 pngs (aquatic schools) |
| turtle | ❌ | aquatic/amphibious, lays eggs |
| tadpole / frog | ❌ | `frog/` 3 variants + `tadpole` |
| allay | ❌ | item-fetch flyer |
| sniffer | ❌ | large passive |
| armadillo | ❌ | has baby texture |
| camel | ❌ | rideable (2-seat) |

#### Neutral (provoked-hostile / tameable)

| MC mob | Engine? | Notes |
|--------|:------:|-------|
| wolf | ❌ | 59 pngs (variants + collar); tameable |
| cat | ❌ | 26 pngs; tameable |
| parrot | ❌ | tameable flyer |
| horse / donkey / mule | ❌ | `horse/` 30 pngs; rideable+breedable |
| llama | ❌ | 9 pngs; caravan/spit |
| fox | ❌ | 8 pngs; sleeps, holds items |
| panda | ❌ | 14 pngs (genes) |
| bee | ❌ | 9 pngs (angry/nectar/baby); pollination |
| goat | ❌ | ramming |
| polar bear (`bear/`) | ❌ | neutral, baby texture |
| dolphin | ❌ | aquatic neutral |
| iron_golem | ❌ | player-built defender |
| snow_golem | ❌ | player-built |
| enderman | ❌ | block-pickup, teleport |
| piglin | ❌ | Nether neutral (barter) |
| zombified_piglin (likely under piglin/zombie set) | ❌ | Nether neutral |

#### Hostile

| MC mob | Engine? | Notes |
|--------|:------:|-------|
| zombie | ✅ | static arms (no walk/attack anim) |
| skeleton | ✅ | **melee only — no bow/arrows** (see §2) |
| spider | ✅ | no wall-climb, no day-passive, undersized? (see §2) |
| creeper | ❌ | **iconic; explosion** — highest-priority missing hostile |
| zombie_villager | ❌ | zombie variant |
| husk / drowned / stray | ❌ | zombie/skeleton biome variants |
| cave_spider | ❌ | spider variant (poison) |
| slime | ❌ | size-split on death |
| witch | ❌ | potion thrower |
| enderman | ❌ | (also neutral) |
| silverfish / endermite | ❌ | small swarmers |
| blaze | ❌ | Nether ranged |
| ghast | ❌ | Nether fireball flyer |
| hoglin | ❌ | Nether |
| breeze | ❌ | wind-charge |
| phantom | ❌ | night flyer |
| guardian / elder (in `guardian/`) | ❌ | aquatic ranged |
| shulker | ❌ | End turret |
| illager set: pillager / vindicator / evoker / illusioner (`illager/` 8 pngs) | ❌ | raids |
| vex | ❌ | evoker summon |
| warden | ❌ | deep-dark boss-tier (5 pngs) |
| creaking | ❌ | pale-garden (2 pngs) |
| magma_cube (under `slime`) | ❌ | Nether slime |

#### Boss

| MC mob | Engine? | Notes |
|--------|:------:|-------|
| ender_dragon (`enderdragon/`) | ❌ | multi-part, End |
| wither (`wither/`) | ❌ | 3-head, player-summoned |

#### Utility / villager economy

| MC mob | Engine? | Notes |
|--------|:------:|-------|
| villager | ❌ | trading + professions (huge subsystem) |
| wandering_trader | ❌ | mobile trader |

### Missing-mob priority

**P1 — defines the "MC feel"; build on existing infra:**
- **creeper** (the single most recognizable MC mob; needs explosion + fuse + creeper-flash render).
- **skeleton bow + arrows** (we have the *mob* but not its defining ranged attack — see §2; this is a fidelity fix, not a new mob, but it is the biggest hostile gap).
- **slime** (size-split death, bounce; simple cube model, good first new-AI mob).
- **creeper-adjacent zombie variants** are cheap: **zombie_villager / husk / drowned** are texture+stat swaps on the existing zombie model.
- **mooshroom** (cow texture+model reuse; trivial once cow is fixed).

**P2 — high player value, moderate new infra:**
- **iron_golem / snow_golem** (player-built defenders; needs build-pattern detection).
- **wolf / cat / parrot** (tameable + sit/follow; needs tame/owner state — see §3).
- **horse family + camel + llama** (rideable + breedable; needs mount/ride infra — see §3).
- **enderman** (teleport + block carry + look-aggro).
- **creeper companions**: **witch, pillager/illager raid set** (ranged + group AI).
- **bee** (pollination loop; baby + anger state).

**P3 — large subsystems or niche:**
- **villager + wandering_trader** (trading UI, professions, gossip — a whole subsystem).
- **aquatic family**: squid, glow_squid, dolphin, fish schools, turtle, axolotl, guardian — needs **swimming AI + water spawn + buoyancy** (engine mobs are strictly ground-walkers today; see §2 AI gaps).
- **Nether set**: blaze, ghast, hoglin, piglin, strider, magma_cube (needs a Nether dimension's biomes/spawn rules).
- **bosses**: ender_dragon, wither (multi-part entities + bossbar + arena logic).
- **misc**: bat, allay, sniffer, armadillo, frog/tadpole, goat, panda, fox, rabbit, phantom, warden, creaking, breeze, shulker, silverfish, endermite, vex.

---

## 2. Per-existing-mob fidelity gaps

### 2.1 Sheep — **missing wool overlay (CONFIRMED)**

`entity_model\sheep.rs` renders the sheep as a **plain quadruped using the single woolly
`sheep.png`** (its own module doc says: *"no separate sheared-body + fur-overlay split"*).

MC renders the sheep in **two layers**: a body model (`sheep.png`) + a separate
**`SheepFurModel`** overlay (`sheep_wool.png`, inflated, **tinted by the dyed wool color**),
plus `sheep_wool_undercoat.png`. Both overlay textures exist on disk:
`ls .../textures/entity/sheep/` → `sheep.png, sheep_baby.png, sheep_wool.png,
sheep_wool_baby.png, sheep_wool_undercoat.png`.

Consequences in the engine:
- No **per-sheep wool color** (MC's 16 dye colors + the natural white/black/gray/brown/pink
  weights). A `DyeColor`/wool-color field does not exist on `MobState`
  (`mob\state.rs` — no color field).
- No **shearing** (drop wool, switch to sheared body texture) — there is no shear item path.
- The `EatGrassGoal` in `mob\goal.rs` (which in MC regrows sheared wool) is **dead code**
  (whole file is `#![allow(dead_code)]`, `can_start` returns `false`, and the goal selector
  is not wired into the live AI tick — see §2.7).

Fix needs the **2-layer / inflated-overlay infra** (see §3.1) + a wool-color field +
shear interaction. This is the project's flagged known issue.

### 2.2 Cow — **udder on the wrong face (CONFIRMED, non-trivial)**

`entity_model\cow.rs` is a faithful MC `CowModel` port: head box + the two horns
(texOffs 22,0) + body (texOffs 18,4, size 12×18×10) with `rotation: [PI/2,0,0]` laying
the body horizontal — matching MC's `QuadrupedModel` body. The data is correct.

The udder is **a texture region on the body cube's belly (down) face**, not a separate
bone. The bug is therefore in how `entity_model\mod.rs::append_cube` maps a **UV face to a
geometric face under the body's `xRot = PI/2` rotation**: with the body rotated 90°, the
texel strip that MC unwraps onto the belly is landing on the **rear** face in the engine.

There are unit tests in `entity_model\mod.rs` (`append_cube_uv_matches_mc_unwrap`,
`append_cube_face_geometry`) that pin the **un-rotated** box's six faces to the MC tile
layout and pass — so the static unwrap is right. The defect is the **rotation×UV-face
interaction**, which those tests don't cover. As the prompt notes, this needs a
**labelled-test-cube pass**: bake a cube whose 6 faces carry 6 distinguishable colors,
render it under `xRot=PI/2`, and assert which world face shows which texel — *not* a blind
rotation tweak (a naive `zRot += PI` or face-order swap would move the udder but break the
top/side textures).

Note the same body-`xRot=PI/2` pattern is used by **sheep, pig, chicken** bodies, so any
fix here must be validated across all four quadruped bodies (their back/belly textures are
less obvious than the cow udder, which is why only the cow exposes it).

### 2.3 Skeleton — **no bow / no ranged attack (major)**

`entity_model\skeleton.rs` builds the model correctly (thin 2×12×2 limbs, mirrored left
side). But behaviorally the skeleton is **just another melee zombie**:
- `mob\ai.rs::kind_is_hostile` lumps zombie+skeleton+spider into one `step_one` path that
  walks to the player and deals `ZOMBIE_TOUCH_DAMAGE` contact damage
  (`mob\state.rs`). The module doc literally says *"H.8 splits skeleton off into a
  ranged-bow goal"* — that split never happened.
- No **arrow projectile** entity exists anywhere (grep for projectiles: none in `mob/`).
- No **bow held-item render** on the arm.
- Loot table (`mob\damage.rs::mob_loot`) already drops bones (294) + arrows (643), so the
  drop side anticipates a bow skeleton that isn't implemented.

This is the **biggest single behavioral gap among existing mobs**.

### 2.4 Spider — climb / size / day-neutral

`entity_model\spider.rs` ports the 2 body segments + 8 legs with MC's rest-pose splay
angles faithfully. Behavior gaps:
- **No wall climbing** (MC spiders climb vertical surfaces; engine has only the 1-block
  step-up in `mob\ai.rs`).
- **Always hostile**: MC spiders are **neutral in daylight** (light ≥ some threshold) and
  only aggro at night/when attacked. Engine treats spider as unconditionally hostile via
  `kind_is_hostile`.
- **Hitbox/scale**: spawn HP is 12 (`mob\spawn.rs::initial_health_for`, but no width — the
  engine has no per-kind hitbox; collision uses a single body cell in `ai.rs`). MC spider
  is 1.4 wide × 0.9 tall; the engine's uniform mob collision will read wrong.
- No **cave_spider** variant (smaller, poison).

### 2.5 Zombie — animation + variants

Model + the raised-arm pose (`render\mobs.rs` sets `zombie_arm_r/l` extra_rotation
`-PI/2`) are present. Gaps:
- **No walk cycle** for arms/legs (legs are static; only a whole-body `walk_bob`
  sine in `render\mobs.rs`). MC swings limbs via `setupAnim` driven by `limbSwing`.
- No **burning in daylight**.
- No **baby zombie** (fast, small), no **zombie_villager / husk / drowned** variants
  (all cheap texture+stat swaps on this model).
- No **door-breaking** behavior.

### 2.6 Animations across all mobs (systemic)

`entity_model\mod.rs::BonePose { extra_rotation }` supports per-bone animation, and
`render\mobs.rs::draw` accepts a `pose_for` closure — but the **only** non-default poses
wired are: zombie raised arms, and a global `walk_bob` translation. **Missing for every
mob**:
- **Limb-swing walk cycle** (legs/arms counter-swinging with movement speed) — MC's
  universal `AgeableMob`/`QuadrupedModel.setupAnim`. The data plumbing exists; the
  per-kind `setupAnim` math is absent.
- **Head-look tracking** the target/player (MC `head.yRot/xRot = netHeadYaw/headPitch`).
- **Idle ambient** (chicken wing flap, sheep head bob, spider leg idle).
- **Death animation** is a generic z-tilt + sink in `render\mobs.rs` (not MC's
  fall-over), shared by all kinds.

### 2.7 AI behaviors — what exists vs MC

Live AI is `mob\ai.rs::tick_mobs` → `step_one`, a **single hand-rolled walk** for all
kinds. A proper **goal system exists but is NOT wired**: `mob\goal.rs` defines
`Goal`/`GoalSelector`, `WanderGoal`, `ChasePlayerGoal`, `FleeFromPlayerGoal`,
`EatGrassGoal` — the whole file is `#![allow(dead_code)]` and nothing in `ai.rs`
constructs or ticks these goals. So:

| Behavior | MC | Engine status |
|----------|----|---------------|
| Wander (random stroll) | yes | yes (inline in `step_one`) |
| Chase + melee | yes | yes (zombie/skel/spider share it) |
| **Flee when hit** (passive panic) | yes | **`FleeFromPlayerGoal` is dead code** — `is_passive` doc claims "flee from the player when hit" but `step_one` never makes passives flee |
| **Chicken flees player** | yes | not wired |
| **Sheep eat grass / regrow wool** | yes | `EatGrassGoal` dead code |
| **Breeding** | yes | yes — `mob\breed.rs` (wheat → love → adult offspring); MVP: wheat breeds *everything*, no per-species food, **no baby** |
| **Baby growth stage** | yes | **none** — offspring spawn as adults (`breed.rs::spawn_offspring`); no baby scale/texture/grow-up |
| **Avoid water / drown** | yes | **none** — mobs walk on the surface via `surface_y_below` snap; no swimming, no drowning, no water-avoidance. Aquatic mobs are impossible without this. |
| **Avoid cliffs / fall damage** | yes | none (surface-snap teleports feet to ground) |
| **Panic on fire** | yes | none |
| **Hostile target acquisition** | line-of-sight + range | range-only (`nearest_player_pos`), no LOS check |
| **Pathfinding (A*)** | yes | `mob\pathfind.rs` exists but `step_one` uses straight-line + step-up, not A* (module reserved "H.4") |
| **Despawn (3-tier)** | yes | yes — `mob\despawn.rs` faithfully (persistent / 48-block timer / 80-block instant) |
| **Spawn light+biome gates** | yes | yes — `mob\spawn.rs` + `mob\pool.rs` (day passive on grass+sky≥9, night hostile dark≤7), weighted per-biome pools |
| **Pack/group spawn** | yes | single mob per attempt (no pack spawn) |

### 2.8 Sounds — per-mob mapping present, but hurt path is wrong

`resource_pack\sounds.rs::ENGINE_TO_MC` has correct per-mob `ambient/step/hurt/death`
rows for all 7 kinds (`entity.cow.ambient`, etc.), and `mob\audio.rs::mob_sound_stem`
routes ambient + step per kind. **But:**
- `mob\damage.rs` (the hurt/death path) hardcodes `voxel.sfx_at("mob/sheep/say", ...)`
  **for every mob** and plays a synth `play_beep(280Hz)` — so a damaged cow/zombie/spider
  emits a *sheep* event + a beep, ignoring the per-kind `hurt`/`death` keys that exist in
  the map. This is a real fidelity bug.
- Ambient + step deliberately have **no synth fallback** (W56.1) and are **silent unless
  the MC `.ogg` pack ships sound files** — and in this asset tree the OGGs are
  content-addressable via `mc_asset_store` (no plain `sounds.json` on disk under
  `assets/minecraft/`; `resource_pack\sounds.rs::load_sounds_via_store` reads them from the
  store). So whether mob audio plays at all depends on the store being populated — **verify
  in-game** that `entity.*` events resolve.

---

## 3. Infra needed for the big missing categories

These are the cross-cutting systems the missing mobs depend on; building them unlocks
many mobs at once.

### 3.1 Two-layer / inflated-overlay textures (sheep wool, player skin, many mobs)

Needed by: **sheep wool** (immediate fix), **player** model (skin + hat/jacket overlay
layer), zombie/skeleton outer layer, wolf collar, many tinted overlays.

Today `entity_model::bake` binds **one** albedo per model and `draw` draws every bone with
one mesh. Add:
- A second **overlay model** per entity (separate `BakedModel` from `sheep_wool.png`) baked
  with `McCube::inflated(...)` (the `inflate` field already exists and is plumbed through
  `append_cube`/ground-offset, just unused by the current models).
- A **per-bone or per-layer tint** so wool takes the dyed color (the `draw` tint is
  currently a single model-wide color). Sheep wool color → tint the overlay only.
- Render both passes (base then overlay) with cutout alpha. Note `bake()`'s doc says it
  *keeps* texture alpha (for skeleton rib gaps), but `mob_meshes.rs::try_open_entity`
  **force-sets `px[3]=255`** before bake — so transparent overlay texels would currently
  go opaque. **Fix the alpha flush** (or special-case overlay textures) before doing wool.

### 3.2 Baby scaling + age (breeding payoff, ~15 mobs have baby textures)

Needed by: every breedable animal + baby-only variants (armadillo, polar bear, etc., which
ship `_baby.png`).

Add an `age`/`baby` field on `MobState` (none today), render with a head-scaled-up,
body-scaled-down transform (MC `AgeableListModel` scales the head bone 2× and shrinks the
body), grow-up timer, and `breed.rs::spawn_offspring` should spawn a **baby** not an adult.
Baby textures already exist on disk (`sheep_baby.png`, `cow_*_baby.png`, etc.) and the
loader's biome-variant logic can be extended to a `_baby` suffix.

### 3.3 Swimming / aquatic AI + buoyancy (whole aquatic family)

Needed by: squid, glow_squid, dolphin, fish, turtle, axolotl, guardian; also drowned,
and "avoid water" for land mobs.

`mob\ai.rs::step_one` hard-snaps feet to the solid surface (`surface_y_below`) and never
enters water — there is no Y movement model, no buoyancy, no swim navigation, no
water-breath/drown. This is a **new movement mode**, plus water-spawn rules in
`mob\spawn.rs` (today passive spawn *requires grass below*, which excludes water).

### 3.4 Tameable / owner state (wolf, cat, parrot, horse)

Add owner-UUID + `sitting`/`tame` flags on `MobState`, a feed-to-tame interaction
(`world\place_break` RMB route, like the existing breed feed in `breed.rs::try_feed`),
follow-owner + sit goals, and a collar/variant texture layer (wolf has 59 textures for
variants + collar colors). No tame concept exists today.

### 3.5 Rideable / mount (horse, pig+saddle, camel, llama, boat/minecart)

Add a `passenger`/`vehicle` link + drive player input to the mob's movement + a
dismount path + saddle item gating. Pig saddle, horse riding, camel 2-seat all need this.
None exists.

### 3.6 Projectile entities (skeleton arrow, ghast/blaze fireball, witch potion, snowball)

There is **no projectile entity type** at all (the mob system only has `MobState`). Needed
for skeleton (the #1 fidelity gap), blaze, ghast, witch, snow_golem, and player bow.
Requires a lightweight moving-entity + collision + on-hit damage/effect, separate from the
mob list, plus a render path.

### 3.7 Special-shape / multi-part entities (creeper explosion, slime split, golems, bosses)

- **Creeper**: fuse timer + AOE block-destroying explosion + creeper-flash render scale.
  No explosion system exists in the mob layer (block break exists for mining; an
  entity-triggered radial explosion does not).
- **Slime/magma_cube**: on-death spawn N smaller copies (size field + scaled model + size
  loot) — needs a per-mob `size` field.
- **Iron/snow golem**: detect a player-built block pattern (pumpkin + iron/snow) and
  convert to an entity — a new "build golem" check.
- **Bosses (dragon/wither)**: multi-part hitboxes, bossbar UI, flight/arena logic — large.

### 3.8 Per-kind hitbox + collision width

`mob\ai.rs` collides every mob as a single ~1-block column and stores no width/height.
MC entities have per-type bounding boxes (spider 1.4 wide, slime scales with size, etc.).
Add a hitbox to the per-kind stats table (`kinds.rs` already has the right home — its doc
mentions "hitbox half-extents" but the struct shipped only HP/speed). This is needed for
correct spider/slime/golem/large-mob behavior and for melee reach.

### 3.9 Variant / texture-selection layer

MC has heavy per-mob variants (wolf 59, horse 30, cat 26, rabbit 16, fish 17, panda 14
genes). The engine has a **partial** version (`BiomeVariant` Temperate/Cold/Warm for
cow/pig/chicken in `mob_meshes.rs`) but no general per-entity variant field. A
`MobState.variant: u8` + a variant→texture resolver generalizes this (and feeds
mooshroom red/brown, zombie_villager, husk/drowned, cave_spider, etc.).

---

## 4. Biggest wins (prioritized)

1. **Fix the cow udder UV-face bug** (`entity_model\mod.rs`) via a labelled-test-cube pass.
   Small, high-visibility, validates the whole rotated-body UV path for all 4 quadrupeds.
   *(Known issue, project-flagged.)*
2. **Sheep wool overlay + color** — build the 2-layer/inflated-overlay infra (§3.1) and a
   wool-color field. Also fixes the `px[3]=255` alpha flush so overlay transparency works.
   This is the most-noticed cosmetic gap and unlocks player-skin overlays too.
3. **Skeleton bow + arrow projectile** (§3.6) — turns the skeleton from "reskinned zombie"
   into a real MC skeleton; loot already anticipates it. Also unblocks player bow.
4. **Add the creeper** (§3.7) — the single most iconic missing mob; needs a fuse +
   explosion. Highest roster-credibility-per-unit-effort hostile.
5. **Wire the existing GoalSelector + add flee/eat-grass** (`mob\goal.rs` is written but
   dead) — gives passives real panic-flee, chickens flee, sheep eat grass; removes the
   "everything walks the same" feel with code that already exists.
6. **Baby mobs + real breeding payoff** (§3.2) — baby textures already on disk; makes the
   breeding feature (already shipped) actually rewarding.
7. **Per-mob limb-swing walk + head-look animation** (§2.6) — `BonePose` plumbing exists;
   adding `setupAnim` math makes *all 7* current mobs read as alive.
8. **Cheap variant mobs on existing models** — mooshroom (cow), zombie_villager/husk/
   drowned (zombie), cave_spider (spider), stray (skeleton). Each is a texture+stat swap
   once a variant field (§3.9) lands. Fast roster growth, near-zero new model work.
9. **Fix per-mob hurt/death SFX** (`mob\damage.rs` hardcodes sheep-beep) — one-line-ish
   correctness fix that makes all 7 mobs sound right (the map already has the rows).

---

## Appendix — key file map

| Concern | File |
|---------|------|
| Kind enum + stats | `examples\voxel_demo_mp\src\mob\kinds.rs` |
| Per-mob state record + balance | `examples\voxel_demo_mp\src\mob\state.rs` |
| Universal model core | `examples\voxel_demo_mp\src\entity_model\mod.rs` |
| Per-mob models | `examples\voxel_demo_mp\src\entity_model\{cow,sheep,pig,chicken,zombie,skeleton,spider}.rs` |
| Texture bind + legacy BoxUv | `examples\voxel_demo_mp\src\mob_meshes.rs` |
| Render dispatch | `examples\voxel_demo_mp\src\render\mobs.rs` |
| Live AI tick | `examples\voxel_demo_mp\src\mob\ai.rs` |
| Goal system (UNWIRED) | `examples\voxel_demo_mp\src\mob\goal.rs` |
| Spawn loop | `examples\voxel_demo_mp\src\mob\spawn.rs` |
| Biome spawn pools | `examples\voxel_demo_mp\src\mob\pool.rs` |
| Despawn (3-tier) | `examples\voxel_demo_mp\src\mob\despawn.rs` |
| Damage + loot | `examples\voxel_demo_mp\src\mob\damage.rs` |
| Breeding | `examples\voxel_demo_mp\src\mob\breed.rs` |
| Audio timers | `examples\voxel_demo_mp\src\mob\audio.rs` |
| Sound event map | `examples\voxel_demo_mp\src\resource_pack\sounds.rs` |
| Legacy skin discovery | `examples\voxel_demo_mp\src\mob_skin.rs` |
| MC mob textures (roster source) | `F:\Data\mc_assets\mc_pack\assets\minecraft\textures\entity\` |
