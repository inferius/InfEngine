//! Mob spawn loop: pick a candidate ring position around the host,
//! resolve the biome's category pools, draw a weighted pack and
//! scatter its members MC-style, each member re-validating placement.
//!
//! W475 replaces the one-mob-per-attempt day / night picker with the
//! `NaturalSpawner` shape: per-`MobCategory` pools (generated from
//! the 26.1.2 biome jsons into `pool_table.rs`), per-category live
//! caps, pack sizes rolled from `minCount..=maxCount` and the
//! member scatter walk (`x += nextInt(6) - nextInt(6)` per member).
//!
//! MC mirror:
//! - `net/minecraft/world/level/NaturalSpawner.java` -
//!   `spawnCategoryForPosition`: pack roll + scatter + per-member
//!   validation; `SpawnState` - per-category caps.
//! - `net/minecraft/world/entity/monster/Monster.java::isDarkEnoughToSpawn`
//!   the W479 light gate (block light 0 + darkened-sky sample);
//!   monsters are gated by LIGHT, not the clock, so dark caves spawn
//!   at noon (`light::is_dark_enough_for_hostile` documents the
//!   channel mapping).
//! - `net/minecraft/world/entity/animal/Animal.java::checkAnimalSpawnRules`
//!   - raw brightness > 8 + spawnable-floor gate (per-kind block tags).
//! - `net/minecraft/world/level/levelgen/structure/Structure.java`
//!   `spawnOverrides` - the W479 per-structure category replacements
//!   (`structure_overrides.rs`).
//!
//! The per-species placement predicates (slime / bat / glow squid /
//! floor tags) live in `spawn_rules.rs`.

use infengine_voxel::CHUNK_SIZE;

use crate::net::{MobMsg, NetRole};
use crate::worldgen::biome_at;
use crate::{DAY_LENGTH_S, Voxel};

use super::kinds::{KIND_MAGMA_CUBE, KIND_SLIME};
use super::light::is_dark_enough_for_hostile;
use super::pool::{
    ALL_CATEGORIES, CategoryPools, SpawnCategory, SpawnRow, category_of_kind,
    pool_for, rows_of, weighted_pick,
};
use super::pool_table;
use super::spawn_rules::{is_water_at, validate_member};
use super::state::MobState;
use super::structure_overrides::{StructureSpawnOverrides, overrides_at};

/// Period (seconds) between spawn attempts. W52 - tightened from
/// 6.0 to 2.0. Each attempt now tries SPAWN_ATTEMPTS_PER_TICK
/// columns and bails on the first that lands a pack; the per-chunk
/// cap + per-category caps still hold the density.
pub(crate) const MOB_SPAWN_PERIOD: f32 = 2.0;

/// W52: number of candidate columns the spawner probes per spawn
/// pass. MC's NaturalSpawner walks every nearby chunk and tries 3
/// "pack" attempts per chunk; we approximate with N attempts in a
/// single tick over the ring around the player. The first
/// successful pack wins; the rest are dropped without retry.
pub(crate) const SPAWN_ATTEMPTS_PER_TICK: usize = 4;

/// Per-chunk soft cap on simultaneous mobs, checked at the pack
/// ANCHOR (members of one pack may briefly push a chunk past it -
/// MC has no per-chunk mob cap at all, only the category caps, so
/// the anchor gate is the engine's stricter extra).
pub(crate) const PER_CHUNK_SOFT_CAP: usize = 4;

/// Inner radius (blocks) of the spawn ring around the player.
pub(crate) const SPAWN_RING_INNER: f32 = 12.0;
/// Outer radius (blocks) of the spawn ring around the player.
/// Cells outside this distance get the immediate-despawn pass
/// (`MOB_DESPAWN_DIST_SQ = 80^2`), so keeping the spawn ring
/// inside that envelope avoids spawn / despawn ping-ponging.
pub(crate) const SPAWN_RING_OUTER: f32 = 32.0;

/// Pluggable RNG: caller threads a counter-based xorshift through
/// the AI tick so spawn + wander share entropy.
#[derive(Debug)]
pub(crate) struct SpawnRng<'a> {
    state: &'a mut u32,
}

impl<'a> SpawnRng<'a> {
    pub(crate) fn new(state: &'a mut u32) -> Self {
        Self { state }
    }
    pub(crate) fn next_u32(&mut self) -> u32 {
        *self.state ^= *self.state << 13;
        *self.state ^= *self.state >> 17;
        *self.state ^= *self.state << 5;
        *self.state
    }
    pub(crate) fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    }
}

/// Count mobs that currently live in the same chunk as `wpos`.
/// Used to enforce the per-chunk soft cap.
pub(crate) fn mobs_in_chunk(voxel: &Voxel, wpos: [i32; 3]) -> usize {
    let cs = CHUNK_SIZE as i32;
    let cx = wpos[0].div_euclid(cs);
    let cz = wpos[2].div_euclid(cs);
    voxel
        .mobs
        .iter()
        .filter(|m| {
            (m.pos[0].floor() as i32).div_euclid(cs) == cx
                && (m.pos[2].floor() as i32).div_euclid(cs) == cz
        })
        .count()
}

/// W475: live mobs counted against one MC `MobCategory` cap. A kind
/// counts toward the category `EntityType.getCategory()` names for
/// it, regardless of which pool list drew it (the jungle ocelot is
/// drawn from the monster list but counts as a CREATURE - exactly
/// MC's bookkeeping). MISC kinds (villager, iron golem) never count.
pub(crate) fn category_count(voxel: &Voxel, cat: SpawnCategory) -> usize {
    voxel
        .mobs
        .iter()
        .filter(|m| category_of_kind(m.kind) == Some(cat))
        .count()
}

/// Probe the column at `(wx, wz)` for a placement target. Returns
/// the world voxel coord of the spawn cell (the AIR cell the mob
/// will stand in - one above the surface block) plus the surface
/// Y. `None` means the column has no exposed surface within
/// `VERTICAL_CHUNKS`.
pub(crate) fn probe_column(voxel: &Voxel, wx: i32, wz: i32) -> Option<([i32; 3], i32)> {
    // W298: ceiled dims (nether) probe from near the PLAYER, not the
    // world top - the sky-down scan always landed on the bedrock
    // roof, so nothing ever spawned inside (the dead nether).
    let dim_def =
        crate::worldgen::dim_registry::dim_def_or_overworld(&voxel.current_dim);
    let ceiling = if dim_def.has_ceiling {
        (voxel.player_pos.y as i32 + 8).min(crate::WORLD_MAX_Y)
    } else {
        crate::WORLD_MAX_Y
    };
    let sy = voxel.surface_y_below(wx, wz, ceiling)?;
    let spawn_y = sy + 1;
    Some(([wx, spawn_y, wz], sy))
}

/// W464: MC `WorldgenRandom.seedSlimeChunk` + one
/// `java.util.Random.nextInt(10)` draw (the 48-bit LCG) - exactly
/// the chunks MC marks as slime chunks for a given seed. The x
/// terms wrap in i32 like Java int arithmetic; the z^2 term
/// multiplies as a long.
pub(crate) fn is_slime_chunk(world_seed: u32, cx: i32, cz: i32) -> bool {
    let xx = cx.wrapping_mul(cx).wrapping_mul(0x4c1906);
    let xs = cx.wrapping_mul(0x5ac0db);
    let zz = (cz.wrapping_mul(cz) as i64).wrapping_mul(0x4307a7);
    let zs = cz.wrapping_mul(0x5f24f);
    let seed = (world_seed as i64)
        .wrapping_add(xx as i64)
        .wrapping_add(xs as i64)
        .wrapping_add(zz)
        .wrapping_add(zs as i64)
        ^ 987_234_911;
    let mask = (1u64 << 48) - 1;
    let mut state = (seed as u64 ^ 0x5DEE_CE66D) & mask;
    state = state.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB) & mask;
    let bits = (state >> 17) as i64; // next(31)
    bits % 10 == 0
}

/// W145: mint one mob from a spawn egg at `pos` (clicked-cell
/// centre). Host/offline only - mirrors the /spawnmob mint but the
/// mob stays a NATURAL spawn (despawnable), exactly like MC eggs.
pub(crate) fn spawn_from_egg(voxel: &mut crate::Voxel, kind: u8, pos: [f32; 3]) {
    mint_mob(voxel, kind, pos, 0.0);
}

/// W242.2: the shared natural-mob mint behind spawn eggs and the
/// monster spawner (`BaseSpawner` sets a random yaw on the fresh
/// entity; eggs keep 0). Host/offline only.
pub(crate) fn mint_mob(voxel: &mut crate::Voxel, kind: u8, pos: [f32; 3], yaw: f32) {
    use crate::net::{MobMsg, NetRole};
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    let id = voxel.next_mob_id;
    voxel.next_mob_id = voxel.next_mob_id.wrapping_add(1).max(1);
    // W429/W475: the slime FAMILY mints at a random MC size
    // (`Slime.finalizeSpawn`: 1 << rng(3) = 1/2/4; the magma cube
    // inherits it) with HP = size^2 (`Slime.setSize`). W475 extends
    // the roll to magma cubes - they minted stuck at size 1 before.
    let size: u8 = if kind == KIND_SLIME || kind == KIND_MAGMA_CUBE {
        1u8 << ((id.wrapping_mul(0x9E37_79B9) >> 9) % 3)
    } else {
        1
    };
    let hp = sized_max_health(kind, size);
    let mut mob =
        crate::mob::state::MobState::new_natural(id, kind, pos, pos, yaw, 0.0, hp);
    mob.size = size;
    let mob = mob;
    voxel.mobs.push(mob);
    voxel.remote_mobs.insert(
        id,
        MobMsg {
            id,
            kind,
            pos,
            yaw,
            health: hp.max(0.0).round() as u8,
            head_yaw: yaw,
            head_pitch: 0.0,
            baby: false,
            size,
            sheared: false,
        },
    );
}

/// W302: mint a STRUCTURE-spawned mob (villager / pen animal):
/// `mint_mob` + `PersistenceRequired` (structure inhabitants never
/// despawn in MC) + the baby growth timer when the template entity
/// carried `Age < 0`.
pub(crate) fn mint_structure_mob(
    voxel: &mut crate::Voxel,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    baby: bool,
) {
    use crate::net::NetRole;
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    mint_mob(voxel, kind, pos, yaw);
    if let Some(m) = voxel.mobs.last_mut() {
        m.persistent = true;
        if baby {
            m.baby_left = crate::mob::breed::BABY_GROW_TIME;
        }
        let baby_flag = m.baby_left > 0.0;
        let id = m.id;
        if let Some(msg) = voxel.remote_mobs.get_mut(&id) {
            msg.baby = baby_flag;
        }
    }
}

/// Initial HP for a freshly spawned mob. Pulled out so the place
/// site + the wire-init match stay in sync.
///
/// W474: reads the generated `attributes::MOB_ATTRIBUTES` table
/// (per-kind `createAttributes` MAX_HEALTH from the decompiled
/// source). The arms below are the kinds whose LIVE spawn HP
/// deviates from the registry base in MC itself:
pub(crate) fn initial_health_for(kind: u8) -> f32 {
    match kind {
        // Slime family: HP is size^2 (`Slime.setSize`). W475 sizes
        // every mint path via `sized_max_health`; these arms are the
        // no-size-context fallback only (16 = the old size-4
        // stand-in, 4 = the magma size-2 identity).
        x if x == KIND_SLIME => 16.0,
        x if x == KIND_MAGMA_CUBE => 4.0,
        // Horse family: `randomizeAttributes` rerolls MAX_HEALTH via
        // generateMaxHealth (15 + rand(8) + rand(9) = 15..30) at
        // spawn - the table's 53 base never walks. Midpoint 22.
        x if x == super::kinds::KIND_HORSE => 22.0,
        x if x == super::kinds::KIND_DONKEY => 22.0,
        x if x == super::kinds::KIND_MULE => 22.0,
        _ => super::attributes::mob_attributes(kind).max_health,
    }
}

/// W475: max HP for a `(kind, size)` pair - the slime family scales
/// as size^2 (`Slime.setSize` updates MAX_HEALTH), every other kind
/// reads the flat per-kind value.
pub(crate) fn sized_max_health(kind: u8, size: u8) -> f32 {
    if kind == KIND_SLIME || kind == KIND_MAGMA_CUBE {
        let s = size.max(1) as f32;
        return s * s;
    }
    initial_health_for(kind)
}

// ----------------------------------------------------------------
// W475 - bucket + category selection.
// ----------------------------------------------------------------

/// Resolve the category pools + context flags for a spawn column.
/// Returns `(pools, in_swamp_bucket, ceiled_dim)`.
pub(crate) fn spawn_bucket(
    voxel: &Voxel,
    wx: i32,
    wz: i32,
) -> (&'static CategoryPools, bool, bool) {
    let dim_def =
        crate::worldgen::dim_registry::dim_def_or_overworld(&voxel.current_dim);
    if dim_def.has_ceiling {
        // W300/W327/W475: the real nether_wastes pool (now incl.
        // enderman 1; strider + piglin skip-logged).
        return (&pool_table::NETHER, false, true);
    }
    let ctx = voxel.current_worldgen_ctx();
    if matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        // W383: register this thread for the probe cold-build
        // budget (worker threads stay exempt).
        crate::worldgen::mcgen::probe_budget::mark_frame_thread();
        let mc =
            crate::worldgen::mcgen::terrain::mc_full_biome_at(&ctx, wx, wz);
        let name = mc.name();
        // W382: mushroom fields have their own pool (mooshroom
        // herds, NO monster category at all - the island is safe).
        if name.contains("mushroom") {
            return (&pool_table::MUSHROOM, false, false);
        }
        // W388: oceans run the ocean.json pools.
        if name.contains("ocean") {
            return (&pool_table::OCEAN, false, false);
        }
        // W479: rivers run the river.json pools (drowned 100 in
        // water). `frozen_river` stays in the tundra bucket per the
        // snowy-collapse policy; stony_shore keeps the beach bucket -
        // its monster list matches beach.json's and its empty
        // creature list differs only by the turtle row, which the
        // sand-floor rule already rejects on stone shores.
        if name.contains("river") && !name.contains("frozen") {
            return (&pool_table::RIVER, false, false);
        }
        let bucket = mc_biome_to_pool(name);
        let in_swamp = matches!(bucket, crate::worldgen::Biome::Swamp);
        (pool_for(bucket), in_swamp, false)
    } else {
        let biome = biome_at(wx as f32, wz as f32, voxel.base_seed);
        let in_swamp = matches!(biome, crate::worldgen::Biome::Swamp);
        (pool_for(biome), in_swamp, false)
    }
}

/// The spawner rows in effect for `cat` at a spot: the structure
/// override's replacement list when one is declared (W479, MC
/// `ChunkGenerator.getMobsAt`), the biome pool otherwise.
pub(crate) fn effective_rows(
    pools: &'static CategoryPools,
    ovr: Option<&'static StructureSpawnOverrides>,
    cat: SpawnCategory,
) -> &'static [SpawnRow] {
    ovr.and_then(|o| o.rows_for(cat))
        .unwrap_or_else(|| rows_of(pools, cat))
}

/// The categories a spawn attempt may draw from right now.
///
/// W479: the MONSTER category is no longer clock-gated - MC gates
/// monsters by LIGHT at the candidate cell (`Monster.
/// isDarkEnoughToSpawn`), which `try_spawn_one`'s anchor pick + the
/// per-member validation apply. Dark caves therefore spawn monsters
/// at noon again (the R24 clock approximation is gone). Creature
/// keeps the day-clock gate so the passive cadence is unchanged
/// (their light rule needs brightness > 8 anyway); ambient + water
/// categories run any time (bats + glow squids probe underground
/// where the clock does not reach).
pub(crate) fn eligible_categories(
    voxel: &Voxel,
    pools: &'static CategoryPools,
    ovr: Option<&'static StructureSpawnOverrides>,
    is_night: bool,
) -> ([SpawnCategory; 6], usize) {
    let mut cats = [SpawnCategory::Monster; 6];
    let mut n = 0;
    for c in ALL_CATEGORIES {
        let clock_ok = match c {
            SpawnCategory::Creature => !is_night,
            _ => true,
        };
        if clock_ok
            && !effective_rows(pools, ovr, c).is_empty()
            && category_count(voxel, c) < c.cap()
        {
            cats[n] = c;
            n += 1;
        }
    }
    (cats, n)
}

/// Random y-scan for a cave cell strictly below the column surface:
/// pick a start y, descend to the first air pocket, land on its
/// floor. Serves the ambient (bat) category.
fn probe_cave(
    voxel: &Voxel,
    wx: i32,
    wz: i32,
    rng: &mut SpawnRng<'_>,
) -> Option<[i32; 3]> {
    let surface = voxel.surface_y_below(wx, wz, crate::WORLD_MAX_Y)?;
    let lo = crate::WORLD_MIN_Y + 2;
    let hi = surface - 5;
    if hi <= lo {
        return None;
    }
    let mut y = lo + (rng.next_u32() % (hi - lo) as u32) as i32;
    // Descend out of solid rock into the first air cell.
    while y > crate::WORLD_MIN_Y && !voxel.world.get([wx, y, wz]).is_air() {
        y -= 1;
    }
    if y <= crate::WORLD_MIN_Y {
        return None;
    }
    // Descend through the pocket onto its floor.
    while y > crate::WORLD_MIN_Y && voxel.world.get([wx, y - 1, wz]).is_air() {
        y -= 1;
    }
    if y <= crate::WORLD_MIN_Y {
        return None;
    }
    Some([wx, y, wz])
}

/// Random y-scan for an underground water cell at or below the glow
/// squid depth (`sea_level - 33`). Serves the
/// underground_water_creature category.
fn probe_underground_water(
    voxel: &Voxel,
    wx: i32,
    wz: i32,
    rng: &mut SpawnRng<'_>,
) -> Option<[i32; 3]> {
    let surface = voxel.surface_y_below(wx, wz, crate::WORLD_MAX_Y)?;
    let hi = (crate::worldgen::climate::SEA_LEVEL - 33).min(surface - 1);
    let lo = crate::WORLD_MIN_Y + 1;
    if hi <= lo {
        return None;
    }
    let mut y = lo + (rng.next_u32() % (hi - lo) as u32) as i32;
    while y > crate::WORLD_MIN_Y {
        if is_water_at(voxel, [wx, y, wz]) {
            return Some([wx, y, wz]);
        }
        y -= 1;
    }
    None
}

/// Mint one natural pack member at `wpos`. Rolls the slime-family
/// size (`Slime.finalizeSpawn`: 1 << rng(3); the difficulty-scaled
/// bump toward bigger sizes is left out - the engine has no local
/// difficulty) with HP = size^2, and gives the mob a random yaw the
/// way `NaturalSpawner` snaps fresh entities to `rand * 360`.
fn mint_pack_member(
    voxel: &mut Voxel,
    kind: u8,
    wpos: [i32; 3],
    day_time: f32,
    mut next_id: u32,
    rng: &mut SpawnRng<'_>,
) -> u32 {
    let id = next_id;
    next_id = next_id.wrapping_add(1).max(1);
    let pos = [
        wpos[0] as f32 + 0.5,
        wpos[1] as f32,
        wpos[2] as f32 + 0.5,
    ];
    let yaw = rng.next_f32() * std::f32::consts::TAU;
    let size: u8 = if kind == KIND_SLIME || kind == KIND_MAGMA_CUBE {
        1u8 << (rng.next_u32() % 3)
    } else {
        1
    };
    let hp = sized_max_health(kind, size);
    let mut mob = MobState::new_natural(
        id,
        kind,
        pos,
        pos,
        yaw,
        (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
        hp,
    );
    mob.size = size;
    voxel.mobs.push(mob);
    voxel.remote_mobs.insert(
        id,
        MobMsg {
            id,
            kind,
            pos,
            yaw,
            health: hp.max(0.0).round() as u8,
            head_yaw: yaw,
            head_pitch: 0.0,
            baby: false,
            size,
            sheared: false,
        },
    );
    next_id
}

/// Context one spawn pack shares across its members.
pub(crate) struct PackCtx {
    pub(crate) cat: SpawnCategory,
    pub(crate) in_swamp: bool,
    pub(crate) ceiled: bool,
    pub(crate) day_time: f32,
}

/// Scatter + validate + mint one pack of `row.kind` around `anchor`.
/// The MC member walk: every member (including the first) shifts
/// `x += nextInt(6) - nextInt(6)` / same for z from the previous
/// member's spot, keeps the anchor Y, and re-validates placement.
/// Returns `(next_id, minted)`.
pub(crate) fn spawn_pack(
    voxel: &mut Voxel,
    row: &'static SpawnRow,
    ctx: &PackCtx,
    anchor: [i32; 3],
    mut next_id: u32,
    rng: &mut SpawnRng<'_>,
) -> (u32, usize) {
    let span = (row.max_count - row.min_count) as u32 + 1;
    let pack = row.min_count as u32 + rng.next_u32() % span;
    let mut x = anchor[0];
    let mut z = anchor[2];
    let y = anchor[1];
    let mut minted = 0usize;
    for _ in 0..pack {
        x += (rng.next_u32() % 6) as i32 - (rng.next_u32() % 6) as i32;
        z += (rng.next_u32() % 6) as i32 - (rng.next_u32() % 6) as i32;
        let wpos = [x, y, z];
        if !validate_member(
            voxel, row.kind, ctx.cat, wpos, ctx.in_swamp, ctx.ceiled, rng,
        ) {
            continue;
        }
        next_id =
            mint_pack_member(voxel, row.kind, wpos, ctx.day_time, next_id, rng);
        minted += 1;
    }
    (next_id, minted)
}

/// Try to spawn one PACK this tick. Returns the new `next_mob_id`
/// (advanced if at least one member was placed, unchanged
/// otherwise).
///
/// Caller has already established: server-role authority, global
/// cap not breached, last spawn was at least `MOB_SPAWN_PERIOD`
/// ago.
pub(crate) fn try_spawn_one(
    voxel: &mut Voxel,
    day_time: f32,
    mut next_id: u32,
    rng: &mut SpawnRng<'_>,
) -> u32 {
    // Spawn in a ring 12..32 blocks around the host so the world
    // feels more populated as the player explores.
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let radius =
        SPAWN_RING_INNER + rng.next_f32() * (SPAWN_RING_OUTER - SPAWN_RING_INNER);
    let cx = voxel.player_pos.x;
    let cz = voxel.player_pos.z;
    let wx = (cx + angle.cos() * radius).round() as i32;
    let wz = (cz + angle.sin() * radius).round() as i32;
    let Some((surface_anchor, _sy)) = probe_column(voxel, wx, wz) else {
        return next_id;
    };
    // Per-chunk cap: bail if this chunk already holds N mobs.
    if mobs_in_chunk(voxel, surface_anchor) >= PER_CHUNK_SOFT_CAP {
        return next_id;
    }
    // W479: structure spawn overrides replace category POOLS now
    // (outpost / monument / fortress tables from the structure
    // jsons), pack-spawned through the same loop as biome rows.
    let ovr = overrides_at(voxel, surface_anchor);
    let (pools, in_swamp, ceiled) = spawn_bucket(voxel, wx, wz);
    // W479: night = raw sun elevation below the horizon, the SAME
    // convention the bed night-skip validates (`try_sleep_bed`).
    // The old `day_night(t).0.y < 0.0` check was DEAD - that fn
    // clamps the returned vector to a +0.95y "moon" direction below
    // the horizon, so the W475 clock gate never fired at all.
    let phase = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
    let is_night = (phase * std::f32::consts::TAU).sin() < -0.05;
    let (cats, n) = eligible_categories(voxel, pools, ovr, is_night);
    if n == 0 {
        return next_id;
    }
    let cat = cats[(rng.next_u32() as usize) % n];
    let Some(row) = weighted_pick(effective_rows(pools, ovr, cat), rng.next_f32())
    else {
        return next_id;
    };
    // Category-specific anchor: ambient (bat) + underground water
    // (glow squid) probe BELOW the surface; everyone else uses the
    // surface column cell (water_creature lands one above the ocean
    // floor = submerged, the W388 squid spot). W479: the MONSTER
    // anchor follows the light - MC rolls a random y over the whole
    // column (`NaturalSpawner.getRandomPosWithin`), which is what
    // puts daytime monsters into caves; the engine keeps its
    // surface-first probe and falls back to the cave probe when the
    // surface cell is too bright, so dark caves spawn at any hour
    // while lit surfaces never do. Structure overrides + ceiled dims
    // keep the surface cell (their interiors ARE the probe target;
    // member validation still applies each kind's own light rule).
    let anchor = match cat {
        SpawnCategory::Ambient => {
            let Some(a) = probe_cave(voxel, wx, wz, rng) else {
                return next_id;
            };
            a
        }
        SpawnCategory::UndergroundWaterCreature => {
            let Some(a) = probe_underground_water(voxel, wx, wz, rng) else {
                return next_id;
            };
            a
        }
        SpawnCategory::Monster
            if ovr.is_none()
                && !ceiled
                && !is_dark_enough_for_hostile(voxel, surface_anchor, rng) =>
        {
            let Some(a) = probe_cave(voxel, wx, wz, rng) else {
                return next_id;
            };
            a
        }
        _ => surface_anchor,
    };
    let pack_ctx = PackCtx { cat, in_swamp, ceiled, day_time };
    let (id_after, minted) = spawn_pack(voxel, row, &pack_ctx, anchor, next_id, rng);
    next_id = id_after;
    if minted > 0 {
        voxel.last_mob_spawn_time = day_time;
    }
    next_id
}

/// Wrapper used by the AI tick. Reads + writes
/// `voxel.next_mob_id` + `voxel.last_mob_spawn_time` directly.
pub(crate) fn spawn_pass(voxel: &mut Voxel, day_time: f32, rng_state: &mut u32) {
    let due = ((day_time - voxel.last_mob_spawn_time).rem_euclid(DAY_LENGTH_S))
        > MOB_SPAWN_PERIOD;
    if !(voxel.mobs.len() < voxel.mob_cap && due) {
        return;
    }
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    // W475: name the MC species the generated pool table had to
    // skip, once per process.
    super::pool::log_skipped_species_once();
    // W52: try up to SPAWN_ATTEMPTS_PER_TICK ring positions before
    // giving up this tick. Stops on the first successful pack so
    // density still respects the per-chunk + category caps.
    let mut rng = SpawnRng::new(rng_state);
    for _ in 0..SPAWN_ATTEMPTS_PER_TICK {
        if voxel.mobs.len() >= voxel.mob_cap {
            break;
        }
        let id = try_spawn_one(voxel, day_time, voxel.next_mob_id, &mut rng);
        if id != voxel.next_mob_id {
            voxel.next_mob_id = id;
            break;
        }
    }
}

/// W354: MC biome name -> spawn-pool bucket. Substring matching over
/// the vanilla biome ids; order matters (snowy_taiga is a TAIGA, not
/// a tundra). The off-`Biome`-enum buckets (mushroom / ocean / W479
/// river) are routed by `spawn_bucket` BEFORE this collapses the
/// rest; `shore` stays a beach on purpose (stony_shore.json's
/// monster list equals beach.json's, and the beach turtle row cannot
/// validate on a stone floor).
pub(crate) fn mc_biome_to_pool(name: &str) -> crate::worldgen::Biome {
    use crate::worldgen::Biome;
    let n = name;
    if n.contains("snowy_taiga") || n.contains("taiga") {
        return Biome::Taiga;
    }
    if n.contains("snowy")
        || n.contains("frozen")
        || n.contains("grove")
        || n.contains("ice")
        || n.contains("peaks")
    {
        return Biome::Tundra;
    }
    if n.contains("desert") || n.contains("badlands") {
        return Biome::Desert;
    }
    if n.contains("jungle") || n.contains("bamboo") {
        return Biome::Jungle;
    }
    if n.contains("swamp") || n.contains("mangrove") {
        return Biome::Swamp;
    }
    if n.contains("savanna") {
        return Biome::Savanna;
    }
    if n.contains("beach")
        || n.contains("shore")
        || n.contains("river")
        || n.contains("ocean")
        || n.contains("mushroom")
    {
        return Biome::Beach;
    }
    if n.contains("windswept") || n.contains("stony") {
        return Biome::Mountain;
    }
    if n.contains("forest") || n.contains("cherry") {
        return Biome::Forest;
    }
    Biome::Plains
}

#[cfg(test)]
mod tests {
    //! Live-Voxel tests for the pack / category machinery. The
    //! per-kind placement predicates are tested next door in
    //! `spawn_rules.rs`; the world-stamping helpers are shared from
    //! `spawn_rules::teststamp`.

    use super::super::kinds::{KIND_SHEEP, KIND_ZOMBIE};
    use super::super::spawn_rules::teststamp::stamp_platform;
    use super::*;
    use crate::Voxel;

    #[test]
    fn per_chunk_cap_blocks_excess_spawns() {
        let mut v = Voxel::new();
        for i in 0..PER_CHUNK_SOFT_CAP {
            v.mobs.push(MobState::new_natural(
                (i as u32) + 1,
                KIND_SHEEP,
                [0.5, 64.0, 0.5],
                [0.5, 64.0, 0.5],
                0.0,
                0.0,
                initial_health_for(KIND_SHEEP),
            ));
        }
        assert!(mobs_in_chunk(&v, [0, 64, 0]) >= PER_CHUNK_SOFT_CAP);
    }

    #[test]
    fn mobs_in_chunk_counts_only_same_chunk() {
        let mut v = Voxel::new();
        v.mobs.push(MobState::new_natural(
            1,
            KIND_SHEEP,
            [0.5, 64.0, 0.5],
            [0.5, 64.0, 0.5],
            0.0,
            0.0,
            1.0,
        ));
        v.mobs.push(MobState::new_natural(
            2,
            KIND_SHEEP,
            [200.5, 64.0, 200.5],
            [200.5, 64.0, 200.5],
            0.0,
            0.0,
            1.0,
        ));
        assert_eq!(mobs_in_chunk(&v, [0, 64, 0]), 1);
        assert_eq!(mobs_in_chunk(&v, [200, 64, 200]), 1);
    }

    /// W464: the slime-chunk lattice matches MC's seedSlimeChunk +
    /// nextInt(10) draw - pinned against reference vectors computed
    /// with the Java LCG, plus the ~10% global density.
    #[test]
    fn slime_chunks_match_java_reference() {
        // Seed 0.
        assert!(is_slime_chunk(0, 0, 10));
        assert!(is_slime_chunk(0, 2, 2));
        assert!(!is_slime_chunk(0, 0, 0));
        // The user's test-world seed.
        assert!(is_slime_chunk(3616343644, 0, 6));
        assert!(is_slime_chunk(3616343644, 1, 6));
        assert!(!is_slime_chunk(3616343644, 0, 0));
        // Global density ~1 in 10.
        let mut hits = 0u32;
        for cx in -50i32..50 {
            for cz in -50i32..50 {
                if is_slime_chunk(0, cx, cz) {
                    hits += 1;
                }
            }
        }
        assert!((800..1300).contains(&hits), "density {hits}/10000");
    }

    #[test]
    fn spawn_pack_respects_bounds_scatter_and_validation() {
        // A 4-4 sheep row on a big lit grass platform mints exactly
        // 4 members, all within the MC scatter envelope (4 steps of
        // +-5 = Chebyshev 20) and all standing on grass.
        static ROW: SpawnRow = SpawnRow {
            kind: KIND_SHEEP,
            weight: 12,
            min_count: 4,
            max_count: 4,
        };
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_platform(&mut v, 0, 0, 59, 30, 3, 15);
        let mut seed = 0xBEEFu32;
        let mut rng = SpawnRng::new(&mut seed);
        let day_ctx = PackCtx {
            cat: SpawnCategory::Creature,
            in_swamp: false,
            ceiled: false,
            day_time: 0.0,
        };
        let (next_id, minted) =
            spawn_pack(&mut v, &ROW, &day_ctx, [0, 60, 0], 1, &mut rng);
        assert_eq!(minted, 4, "full pack on a perfect platform");
        assert_eq!(next_id, 5);
        for m in &v.mobs {
            assert_eq!(m.kind, KIND_SHEEP);
            let dx = (m.pos[0] - 0.5).abs();
            let dz = (m.pos[2] - 0.5).abs();
            assert!(dx <= 20.0 && dz <= 20.0, "scatter escaped: {:?}", m.pos);
        }
        // Variable pack bounds: a 2-5 row over many seeds stays in
        // 0..=5 and hits the min..=max span when everything
        // validates.
        static TROW: SpawnRow = SpawnRow {
            kind: KIND_SHEEP,
            weight: 5,
            min_count: 2,
            max_count: 5,
        };
        for s in 1..40u32 {
            let mut v = Voxel::new();
            v.day_time = 0.0;
            stamp_platform(&mut v, 0, 0, 59, 30, 3, 15);
            let mut seed = s.wrapping_mul(0x9E37_79B9);
            let mut rng = SpawnRng::new(&mut seed);
            let (_, minted) =
                spawn_pack(&mut v, &TROW, &day_ctx, [0, 60, 0], 1, &mut rng);
            assert!(minted <= 5, "pack overshot maxCount: {minted}");
        }
        // Validation per member: the same row on STONE mints zero.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_platform(&mut v, 0, 0, 59, 30, 1, 15);
        let mut seed = 0xBEEFu32;
        let mut rng = SpawnRng::new(&mut seed);
        let (_, minted) =
            spawn_pack(&mut v, &ROW, &day_ctx, [0, 60, 0], 1, &mut rng);
        assert_eq!(minted, 0, "sheep pack on stone must fail per member");
    }

    #[test]
    fn sized_slime_packs_mint_hp_size_squared() {
        static ROW: SpawnRow = SpawnRow {
            kind: KIND_SLIME,
            weight: 100,
            min_count: 4,
            max_count: 4,
        };
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        // Swamp-band platform in the dark (slime swamp branch).
        stamp_platform(&mut v, 0, 0, 59, 30, 3, 0);
        let night_ctx = PackCtx {
            cat: SpawnCategory::Monster,
            in_swamp: true,
            ceiled: false,
            day_time: v.day_time,
        };
        let mut minted_total = 0usize;
        for s in 1..64u32 {
            let mut seed = s.wrapping_mul(0x0061_C886);
            let mut rng = SpawnRng::new(&mut seed);
            let (_, minted) = spawn_pack(
                &mut v,
                &ROW,
                &night_ctx,
                [0, 60, 0],
                s * 100 + 1,
                &mut rng,
            );
            minted_total += minted;
        }
        assert!(minted_total > 0, "no swamp slimes over 63 seeds");
        let mut sizes_seen = std::collections::HashSet::new();
        for m in &v.mobs {
            assert_eq!(m.kind, KIND_SLIME);
            assert!(matches!(m.size, 1 | 2 | 4), "bad size {}", m.size);
            assert_eq!(
                m.health,
                (m.size as f32) * (m.size as f32),
                "HP must be size^2"
            );
            sizes_seen.insert(m.size);
        }
        assert!(sizes_seen.len() >= 2, "size roll stuck: {sizes_seen:?}");
        // The wire mirror carries the size byte.
        for m in &v.mobs {
            assert_eq!(v.remote_mobs.get(&m.id).unwrap().size, m.size);
        }
    }

    #[test]
    fn category_caps_enforced_independently() {
        let mut v = Voxel::new();
        // Fill the monster cap with zombies.
        for i in 0..SpawnCategory::Monster.cap() {
            v.mobs.push(MobState::new_natural(
                (i as u32) + 1,
                KIND_ZOMBIE,
                [0.5, 64.0, 0.5],
                [0.5, 64.0, 0.5],
                0.0,
                0.0,
                20.0,
            ));
        }
        assert_eq!(
            category_count(&v, SpawnCategory::Monster),
            SpawnCategory::Monster.cap()
        );
        assert_eq!(category_count(&v, SpawnCategory::Creature), 0);
        let pools = pool_for(crate::worldgen::Biome::Plains);
        // Night: monster is capped out, but ambient (bat) + the
        // underground water category stay eligible.
        let (cats, n) = eligible_categories(&v, pools, None, true);
        assert!(!cats[..n].contains(&SpawnCategory::Monster));
        assert!(cats[..n].contains(&SpawnCategory::Ambient));
        // Day: creature spawning is NOT blocked by the full monster
        // cap.
        let (cats, n) = eligible_categories(&v, pools, None, false);
        assert!(cats[..n].contains(&SpawnCategory::Creature));
        // MISC kinds never count toward any cap.
        v.mobs.push(MobState::new_natural(
            999,
            super::super::kinds::KIND_VILLAGER,
            [0.5, 64.0, 0.5],
            [0.5, 64.0, 0.5],
            0.0,
            0.0,
            20.0,
        ));
        assert_eq!(category_count(&v, SpawnCategory::Creature), 0);
    }

    #[test]
    fn eligible_categories_monster_is_clock_free_creature_is_not() {
        // W479: the MONSTER category is eligible day AND night (the
        // light rule at the anchor / member decides, not the clock);
        // Creature keeps the day gate.
        let v = Voxel::new();
        let pools = pool_for(crate::worldgen::Biome::Plains);
        // Day: Monster present now (light decides later), Creature
        // present.
        let (cats, n) = eligible_categories(&v, pools, None, false);
        assert!(cats[..n].contains(&SpawnCategory::Monster));
        assert!(cats[..n].contains(&SpawnCategory::Creature));
        // Night: Monster present, Creature gone.
        let (cats, n) = eligible_categories(&v, pools, None, true);
        assert!(cats[..n].contains(&SpawnCategory::Monster));
        assert!(!cats[..n].contains(&SpawnCategory::Creature));
        // Nether pools: Monster active regardless of the clock.
        let (cats, n) =
            eligible_categories(&v, &pool_table::NETHER, None, false);
        assert!(cats[..n].contains(&SpawnCategory::Monster));
        // Empty pools never become eligible: mushroom fields have
        // no monster category at all.
        let (cats, n) =
            eligible_categories(&v, &pool_table::MUSHROOM, None, true);
        assert!(!cats[..n].contains(&SpawnCategory::Monster));
    }

    #[test]
    fn structure_overrides_replace_the_monster_rows() {
        // W479: inside a structure's bounds the monster category
        // draws from the override table, not the biome pool; other
        // categories fall through, and the monument's EMPTY
        // underground-water replacement silences that category.
        use super::super::structure_overrides::{FORTRESS, MONUMENT, OUTPOST};
        let pools = pool_for(crate::worldgen::Biome::Plains);
        let rows = effective_rows(pools, Some(&OUTPOST), SpawnCategory::Monster);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, super::super::kinds::KIND_PILLAGER);
        // Weighted pick over the single-row table always lands on it.
        for roll in [0.0, 0.5, 0.99] {
            assert_eq!(
                weighted_pick(rows, roll).map(|r| r.kind),
                Some(super::super::kinds::KIND_PILLAGER)
            );
        }
        // Non-overridden categories keep the biome rows.
        assert_eq!(
            effective_rows(pools, Some(&OUTPOST), SpawnCategory::Creature),
            rows_of(pools, SpawnCategory::Creature)
        );
        // Monument: monster -> guardian pack 2-4; underground water
        // silenced (empty replacement -> category not eligible).
        let m = effective_rows(pools, Some(&MONUMENT), SpawnCategory::Monster);
        assert_eq!(m[0].kind, super::super::kinds::KIND_GUARDIAN);
        assert_eq!((m[0].min_count, m[0].max_count), (2, 4));
        assert!(
            effective_rows(
                pools,
                Some(&MONUMENT),
                SpawnCategory::UndergroundWaterCreature
            )
            .is_empty()
        );
        let v = Voxel::new();
        let (cats, n) = eligible_categories(&v, pools, Some(&MONUMENT), true);
        assert!(!cats[..n].contains(&SpawnCategory::UndergroundWaterCreature));
        // Fortress: the five-row table replaces the nether pool.
        let f = effective_rows(
            &pool_table::NETHER,
            Some(&FORTRESS),
            SpawnCategory::Monster,
        );
        assert_eq!(f.len(), 5);
        let total: u32 = f.iter().map(|r| r.weight).sum();
        assert_eq!(total, 28, "fortress weights 10+5+8+2+3");
        // Outside any structure: the biome pool applies.
        assert_eq!(
            effective_rows(pools, None, SpawnCategory::Monster),
            rows_of(pools, SpawnCategory::Monster)
        );
    }

    #[test]
    fn outpost_pack_spawns_pillagers_in_daylight() {
        // W479: the override rows run through the SAME pack loop as
        // biome rows, and the pillager placement rule is block-light
        // <= 8 with NO sky gate - a full-sun outpost yard mints the
        // pack (the old single-mint arm is gone).
        use super::super::structure_overrides::OUTPOST;
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon
        stamp_platform(&mut v, 0, 0, 59, 30, 1, 15);
        let row = &OUTPOST.monster.unwrap()[0];
        let mut seed = 0xABCDu32;
        let mut rng = SpawnRng::new(&mut seed);
        let ctx = PackCtx {
            cat: SpawnCategory::Monster,
            in_swamp: false,
            ceiled: false,
            day_time: 0.0,
        };
        let (_, minted) = spawn_pack(&mut v, row, &ctx, [0, 60, 0], 1, &mut rng);
        assert_eq!(minted, 1, "pillager pack (1-1) mints at noon");
        assert_eq!(v.mobs[0].kind, super::super::kinds::KIND_PILLAGER);
        // A zombie row on the same lit platform mints nothing.
        static ZROW: SpawnRow = SpawnRow {
            kind: KIND_ZOMBIE,
            weight: 95,
            min_count: 4,
            max_count: 4,
        };
        let mut v2 = Voxel::new();
        v2.day_time = 0.0;
        stamp_platform(&mut v2, 0, 0, 59, 30, 1, 15);
        let mut seed2 = 0xABCDu32;
        let mut rng2 = SpawnRng::new(&mut seed2);
        let (_, minted2) =
            spawn_pack(&mut v2, &ZROW, &ctx, [0, 60, 0], 1, &mut rng2);
        assert_eq!(minted2, 0, "zombies never mint on a noon surface");
    }
}
