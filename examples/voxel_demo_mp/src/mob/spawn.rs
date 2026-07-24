//! Mob spawn loop: pick a candidate ring position around the host,
//! probe light + standing room, decide hostile vs passive, push a
//! fresh `MobState` + broadcast a `MobMsg`.
//!
//! H.2 layers in the MC-style light + biome gates on top of the
//! H.1 refactor: hostile mobs need block_light <= 7 AND
//! effective_sky_light <= 7; passive mobs need sky_light >= 9 AND
//! a grass block under their feet. A per-chunk soft cap (4) keeps
//! the density tolerable around the player.
//!
//! H.3 will plug a biome-aware weighted pool in place of the
//! ad-hoc kind picker at the bottom.
//!
//! MC mirror:
//! - `net/minecraft/world/level/NaturalSpawner.java` - per-chunk
//!   spawn dispatch + density gates. We collapse the per-chunk
//!   loop to "one candidate per host tick" since the demo world
//!   has a single live player at most.
//! - `net/minecraft/world/entity/monster/Monster.java::checkMonsterSpawnRules`
//!   - block_light <= 7 gate (light-driven spawn).
//! - `net/minecraft/world/entity/animal/Animal.java::checkAnimalSpawnRules`
//!   - sky_light >= 9 + grass-below gate.

use infengine_voxel::CHUNK_SIZE;

use crate::net::{MobMsg, NetRole};
use crate::{DAY_LENGTH_S, Voxel, day_night};

use super::kinds::{
    KIND_CHICKEN, KIND_PIG, KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE,
};
use super::light::{
    has_solid_floor, has_standing_room, is_bright_enough_for_passive,
    is_dark_enough_for_hostile, is_grass_below,
};
use super::pool::{pool_for, weighted_pick};
use super::state::{MOB_MAX_HEALTH, MobState, ZOMBIE_MAX_HEALTH};
use crate::worldgen::biome_at;

/// Period (seconds) between spawn attempts. W52 - tightened from
/// 6.0 to 2.0. Each attempt now tries SPAWN_ATTEMPTS_PER_TICK
/// columns and bails on the first that lands; the per-chunk cap
/// + global mob_cap still hold the density.
pub(crate) const MOB_SPAWN_PERIOD: f32 = 2.0;

/// W52: number of candidate columns the spawner probes per spawn
/// pass. MC's NaturalSpawner walks every nearby chunk and tries 3
/// "pack" attempts per chunk; we approximate with N attempts in a
/// single tick over the ring around the player. The first
/// successful spawn wins; the rest are dropped without retry.
pub(crate) const SPAWN_ATTEMPTS_PER_TICK: usize = 4;

/// Per-chunk soft cap on simultaneous mobs. MC keeps a per-mob-
/// category cap; we use a single all-kinds cap of 4 to stop a
/// single grass field from monopolising the spawn budget.
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

/// True for hostile kinds in the H.3 pool. Mirrors
/// `MobKind::is_hostile` but consumes the raw u8 the pool stores.
fn is_hostile_kind(kind: u8) -> bool {
    matches!(
        kind,
        KIND_ZOMBIE
            | KIND_SKELETON
            | KIND_SPIDER
            | super::kinds::KIND_MAGMA_CUBE
            | super::kinds::KIND_ZOMBIE_VILLAGER
    )
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
    let hp = initial_health_for(kind);
    let mob = crate::mob::state::MobState::new_natural(id, kind, pos, pos, yaw, 0.0, hp);
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
pub(crate) fn initial_health_for(kind: u8) -> f32 {
    if kind == crate::mob::kinds::KIND_CREEPER {
        return 20.0; // W343: Creeper.createAttributes MAX_HEALTH 20.
    }
    match kind {
        KIND_ZOMBIE | KIND_SKELETON => ZOMBIE_MAX_HEALTH,
        // W300: ZombifiedPiglin.createAttributes MAX_HEALTH 20.
        x if x == super::kinds::KIND_ZOMBIFIED_PIGLIN => 20.0,
        // W302: Villager.createAttributes MAX_HEALTH 20.
        x if x == super::kinds::KIND_VILLAGER => 20.0,
        // W315: Cat.createAttributes MAX_HEALTH 10.
        x if x == super::kinds::KIND_CAT => 10.0,
        // W375: Bee.createAttributes MAX_HEALTH 10.
        x if x == super::kinds::KIND_BEE => 10.0,
        // W376: Witch 26; Slime (large) 16.
        x if x == super::kinds::KIND_WITCH => 26.0,
        x if x == super::kinds::KIND_SLIME => 16.0,
        // W377: zombie/skeleton variants share the base 20 HP.
        x if x == super::kinds::KIND_DROWNED => 20.0,
        x if x == super::kinds::KIND_HUSK => 20.0,
        x if x == super::kinds::KIND_STRAY => 20.0,
        // W378: Pillager/Vindicator 24, Evoker 24.
        x if x == super::kinds::KIND_PILLAGER => 24.0,
        x if x == super::kinds::KIND_VINDICATOR => 24.0,
        x if x == super::kinds::KIND_EVOKER => 24.0,
        // W379: Blaze 20, WitherSkeleton 20.
        x if x == super::kinds::KIND_BLAZE => 20.0,
        x if x == super::kinds::KIND_WITHER_SKELETON => 20.0,
        // W380: Guardian 30, ElderGuardian 80.
        x if x == super::kinds::KIND_GUARDIAN => 30.0,
        x if x == super::kinds::KIND_ELDER_GUARDIAN => 80.0,
        // W381: Enderman 40, CaveSpider 12, Silverfish 8.
        x if x == super::kinds::KIND_ENDERMAN => 40.0,
        x if x == super::kinds::KIND_CAVE_SPIDER => 12.0,
        x if x == super::kinds::KIND_SILVERFISH => 8.0,
        // W382: Phantom 20, Bat 6, Mooshroom 10.
        x if x == super::kinds::KIND_PHANTOM => 20.0,
        x if x == super::kinds::KIND_BAT => 6.0,
        x if x == super::kinds::KIND_MOOSHROOM => 10.0,
        // W322: IronGolem.createAttributes MAX_HEALTH 100.
        x if x == super::kinds::KIND_IRON_GOLEM => 100.0,
        // W322: AbstractHorse generateMaxHealth 15..30, midpoint.
        x if x == super::kinds::KIND_HORSE => 22.0,
        // W344: donkey generateMaxHealth averages ~22 like the horse.
        x if x == super::kinds::KIND_DONKEY => 22.0,
        // W327: Ghast.createAttributes MAX_HEALTH 10.
        x if x == super::kinds::KIND_GHAST => 10.0,
        // W327: magma cube size 2 -> size*size = 4 HP.
        x if x == super::kinds::KIND_MAGMA_CUBE => 4.0,
        // W330: Zombie.createAttributes MAX_HEALTH 20.
        x if x == super::kinds::KIND_ZOMBIE_VILLAGER => 20.0,
        KIND_SPIDER => 12.0,
        KIND_CHICKEN => 4.0,
        KIND_PIG => 10.0,
        // W158: MC wild wolf HP.
        x if x == super::kinds::KIND_WOLF => 8.0,
        // W161: MC polar bear HP.
        x if x == super::kinds::KIND_POLAR_BEAR => 30.0,
        // W162: MC rabbit HP (3 = 1.5 hearts).
        x if x == super::kinds::KIND_RABBIT => 3.0,
        _ => MOB_MAX_HEALTH,
    }
}

/// Decide which kind to spawn at the candidate cell. Returns
/// `None` when the light gates reject every entry in the biome's
/// active pool.
///
/// H.3: looks up the biome at `(wx, wz)` via `biome_at`, fetches
/// the day / night pool from `pool::pool_for`, draws a weighted
/// candidate, then validates the candidate against the light
/// gates. If the draw fails the gate the spawn is skipped (no
/// retries this tick - the caller will try a fresh ring position
/// on the next spawn period).
pub(crate) fn pick_kind_for(
    voxel: &Voxel,
    wpos: [i32; 3],
    day_time: f32,
    rng: &mut SpawnRng<'_>,
) -> Option<u8> {
    let (sun_dir, _sky) = day_night(day_time);
    let is_night = sun_dir.y < 0.0;
    // W378: structure spawn overrides (MC per-structure monster
    // pools). The outpost yard breeds pillagers day and night; the
    // mansion grounds their vindicators + the odd evoker.
    if matches!(
        voxel.current_worldgen_ctx().profile,
        crate::worldgen::WorldGen::Mc
    ) {
        let ctx = voxel.current_worldgen_ctx();
        if crate::worldgen::mcgen::outpost::pos_in_outpost(&ctx, wpos) {
            return Some(super::kinds::KIND_PILLAGER);
        }
        // W380: monument waters teem with guardians (1-in-20 an
        // elder) - the MC monument spawn override.
        if crate::worldgen::mcgen::monument::pos_in_monument(&ctx, wpos) {
            return Some(if rng.next_u32().is_multiple_of(20) {
                super::kinds::KIND_ELDER_GUARDIAN
            } else {
                super::kinds::KIND_GUARDIAN
            });
        }
        if crate::worldgen::mcgen::mansion::pos_in_mansion(&ctx, wpos) {
            return Some(if rng.next_u32().is_multiple_of(5) {
                super::kinds::KIND_EVOKER
            } else {
                super::kinds::KIND_VINDICATOR
            });
        }
    }
    // W298: ceiled dims use the NETHER pool - MC nether_wastes
    // weights (zombified_piglin 100 / ghast 50 / magma_cube 2) with
    // the engine's stand-in kinds until those mobs are ported (same
    // once-logged policy as the fortress spawner stand-ins). Any
    // light level, no grass gate - zombified piglins spawn in full
    // glowstone light in vanilla.
    let dim_def =
        crate::worldgen::dim_registry::dim_def_or_overworld(&voxel.current_dim);
    if dim_def.has_ceiling {
        static NETHER_STANDIN_LOGGED: std::sync::Once = std::sync::Once::new();
        NETHER_STANDIN_LOGGED.call_once(|| {
            tracing::info!(
                "W298: nether spawns use ZOMBIE/SKELETON/SPIDER as \
                 zombified-piglin/ghast/magma-cube stand-ins until \
                 those mobs are ported"
            );
        });
        let roll = rng.next_f32() * 152.0;
        // W300/W327: the real nether_wastes pool - zombified piglin
        // 100 / ghast 50 / magma cube 2, all three ported now.
        return if roll < 100.0 {
            Some(super::kinds::KIND_ZOMBIFIED_PIGLIN)
        } else if roll < 150.0 {
            Some(super::kinds::KIND_GHAST)
        } else {
            Some(super::kinds::KIND_MAGMA_CUBE)
        };
    }
    // W354: the pool biome comes from the ACTIVE worldgen. The old
    // call always sampled the legacy noise biome map - under the Mc
    // profile that map has nothing to do with the generated world,
    // so cows and chickens spawned in deserts (the R17 report).
    let biome = {
        let ctx = voxel.current_worldgen_ctx();
        if matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
            // W382: mushroom fields have their own pool (mooshroom
            // herds by day, NOTHING at night - the legacy Biome
            // enum has no variant for it, so branch before the
            // mapper).
            let mc = crate::worldgen::mcgen::terrain::mc_full_biome_at(
                &ctx, wpos[0], wpos[2],
            );
            if mc.name().contains("mushroom") {
                let active = if is_night {
                    super::pool::MUSHROOM_NIGHT
                } else {
                    super::pool::MUSHROOM_DAY
                };
                return weighted_pick(active, rng.next_f32());
            }
            mc_biome_to_pool(
                crate::worldgen::mcgen::terrain::mc_full_biome_at(
                    &ctx, wpos[0], wpos[2],
                )
                .name(),
            )
        } else {
            biome_at(wpos[0] as f32, wpos[2] as f32, voxel.base_seed)
        }
    };
    let pool = pool_for(biome);
    let active = if is_night { pool.night } else { pool.day };
    let candidate = weighted_pick(active, rng.next_f32())?;
    // Validate via the H.2 light + grass gates.
    if is_hostile_kind(candidate) {
        if !is_dark_enough_for_hostile(voxel, wpos) {
            return None;
        }
    } else {
        // Passive: bright enough AND grass below.
        if !is_bright_enough_for_passive(voxel, wpos) {
            return None;
        }
        if !is_grass_below(voxel, wpos) {
            return None;
        }
    }
    Some(candidate)
}

/// Try to spawn one mob this tick. Returns the new `next_mob_id`
/// (incremented if a mob was placed, unchanged otherwise).
///
/// Caller has already established: server-role authority, cap not
/// breached, last spawn was at least `MOB_SPAWN_PERIOD` ago.
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
    let Some((spawn_pos, _sy)) = probe_column(voxel, wx, wz) else {
        return next_id;
    };
    // Per-chunk cap: bail if this chunk already holds N mobs.
    if mobs_in_chunk(voxel, spawn_pos) >= PER_CHUNK_SOFT_CAP {
        return next_id;
    }
    // Structural gate: need a solid floor + two air cells stacked.
    if !has_solid_floor(voxel, spawn_pos) || !has_standing_room(voxel, spawn_pos) {
        return next_id;
    }
    // Decide kind via light + biome rules.
    let Some(kind) = pick_kind_for(voxel, spawn_pos, day_time, rng) else {
        return next_id;
    };
    let id = next_id;
    next_id = next_id.wrapping_add(1).max(1);
    let pos = [spawn_pos[0] as f32 + 0.5, spawn_pos[1] as f32, spawn_pos[2] as f32 + 0.5];
    let mob = MobState::new_natural(
        id,
        kind,
        pos,
        pos,
        0.0,
        (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
        initial_health_for(kind),
    );
    let initial_hp = initial_health_for(kind) as u8;
    voxel.mobs.push(mob);
    voxel.remote_mobs.insert(
        id,
        MobMsg {
            id,
            kind,
            pos,
            yaw: 0.0,
            health: initial_hp,
            head_yaw: 0.0,
            head_pitch: 0.0,
            baby: false,
        },
    );
    voxel.last_mob_spawn_time = day_time;
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
    // W52: try up to SPAWN_ATTEMPTS_PER_TICK ring positions before
    // giving up this tick. Stops on the first successful spawn so
    // density still respects the per-chunk + global caps.
    let mut rng = SpawnRng::new(rng_state);
    let before = voxel.next_mob_id;
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
    // Defensive: if every attempt bailed without spawning, leave
    // next_mob_id untouched (try_spawn_one already preserved it).
    let _ = before;
}

/// W354: MC biome name -> spawn-pool bucket. Substring matching over
/// the vanilla biome ids; order matters (snowy_taiga is a TAIGA, not
/// a tundra).
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
    //! Live-Voxel tests for the spawn-loop gates. Each test stamps
    //! the world + lighting state by hand, then calls the gate
    //! predicates + the `try_spawn_one` entry point.

    use super::*;
    use super::super::kinds::KIND_SHEEP;
    use crate::Voxel;
    use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};

    fn stamp_light(v: &mut Voxel, w: [i32; 3], block: u8, sky: u8) {
        let cs = CHUNK_SIZE as i32;
        let cc: ChunkCoord = [
            w[0].div_euclid(cs),
            w[1].div_euclid(cs),
            w[2].div_euclid(cs),
        ];
        let lc = [
            w[0].rem_euclid(cs) as u32,
            w[1].rem_euclid(cs) as u32,
            w[2].rem_euclid(cs) as u32,
        ];
        let g = v.lighting.ensure(cc);
        g.set_block(lc, block);
        g.set_sky(lc, sky);
    }

    fn stamp_grass_platform(v: &mut Voxel, w: [i32; 3]) {
        // Place a grass block one below the spawn target so passive
        // rules + standing room both pass. The voxel world is empty
        // on `Voxel::new()` so we need to mint the owning chunk
        // before writing - `World::set` is a no-op for unloaded
        // chunks.
        use infengine_voxel::{Chunk, CHUNK_SIZE};
        let cs = CHUNK_SIZE as i32;
        let floor_w = [w[0], w[1] - 1, w[2]];
        let cc = [
            floor_w[0].div_euclid(cs),
            floor_w[1].div_euclid(cs),
            floor_w[2].div_euclid(cs),
        ];
        if v.world.chunk(cc).is_none() {
            v.world.insert_chunk(cc, Chunk::empty());
        }
        v.world.set(floor_w, BlockId(3));
    }

    #[test]
    fn per_chunk_cap_blocks_excess_spawns() {
        let mut v = Voxel::new();
        // Stuff the chunk at world origin with the per-chunk cap.
        for i in 0..PER_CHUNK_SOFT_CAP {
            v.mobs.push(MobState::new_natural(
                (i as u32) + 1,
                KIND_SHEEP,
                [0.5, 64.0, 0.5],
                [0.5, 64.0, 0.5],
                0.0,
                0.0,
                MOB_MAX_HEALTH,
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

    #[test]
    fn pick_kind_chooses_hostile_in_dark() {
        // Dark cell (block + sky both 0): hostile gate passes. At
        // night with roll < 0.6 we get a zombie; with roll >= 0.6
        // we fall through to passive, which also gates on grass.
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        stamp_light(&mut v, [10, 60, 10], 0, 0);
        // No grass under the cell -> passive fallback fails too,
        // so a roll >= 0.6 returns None. We deterministically pick
        // a seed that lands < 0.6 on the first call.
        let mut seed = 0x12345u32;
        let mut rng = SpawnRng::new(&mut seed);
        let kind = pick_kind_for(&v, [10, 60, 10], v.day_time, &mut rng);
        assert!(kind == Some(KIND_ZOMBIE) || kind.is_none());
    }

    #[test]
    fn pick_kind_passes_passive_on_grass_in_daylight() {
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon - daylight kills hostile gate
        stamp_light(&mut v, [20, 60, 20], 0, 15);
        stamp_grass_platform(&mut v, [20, 60, 20]);
        let mut seed = 0xC0FFEEu32;
        let mut rng = SpawnRng::new(&mut seed);
        let kind = pick_kind_for(&v, [20, 60, 20], v.day_time, &mut rng);
        // W162: the plains day pool now also carries cow + rabbit, so
        // assert the picked kind is *passive* rather than enumerating
        // the exact set (which is brittle against pool re-weighting).
        let k = kind.expect("a passive mob should spawn on lit grass at noon");
        assert!(
            super::super::kinds::MobKind::from_u8(k).is_passive(),
            "expected a passive day spawn, got kind {k}"
        );
    }

    #[test]
    fn pick_kind_rejects_dark_non_grass_during_day() {
        // Daylight: hostile gate fails (effective sky high).
        // Passive gate fails: not enough sky_light AND no grass.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_light(&mut v, [30, 60, 30], 0, 8); // sky_light = 8 (one below floor)
        // No grass placed.
        let mut seed = 1u32;
        let mut rng = SpawnRng::new(&mut seed);
        assert_eq!(pick_kind_for(&v, [30, 60, 30], v.day_time, &mut rng), None);
    }

    #[test]
    fn pick_kind_rejects_passive_on_non_grass() {
        // Bright sky but no grass below -> passive gate fails.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_light(&mut v, [40, 60, 40], 0, 15);
        // No grass under the spawn cell.
        let mut seed = 2u32;
        let mut rng = SpawnRng::new(&mut seed);
        assert_eq!(pick_kind_for(&v, [40, 60, 40], v.day_time, &mut rng), None);
    }

    #[test]
    fn pick_kind_rejects_bright_lit_corner_at_night() {
        // Torch-lit cell at night: block_light = 14 -> hostile
        // gate fails (block_light > 7). sky_light = 0 + no grass
        // -> passive gate fails too.
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75;
        stamp_light(&mut v, [50, 60, 50], 14, 0);
        let mut seed = 3u32;
        let mut rng = SpawnRng::new(&mut seed);
        assert_eq!(pick_kind_for(&v, [50, 60, 50], v.day_time, &mut rng), None);
    }
}
