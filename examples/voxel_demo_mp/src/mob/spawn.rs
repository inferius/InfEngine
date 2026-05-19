//! Mob spawn loop: pick a candidate column near the host, check
//! basic standing room, pick a kind, push a fresh `MobState` +
//! broadcast a `MobMsg`.
//!
//! H.1 (this commit) is a pure refactor: the original 6-second
//! grass-block spawn logic moves here verbatim. H.2 layers in the
//! light-level + biome-aware spawn rules without touching the
//! caller signature.
//!
//! MC mirror:
//! - `net/minecraft/world/level/NaturalSpawner.java` - per-chunk
//!   spawn dispatch + density gates. We collapse the per-chunk
//!   loop to "one candidate per host tick" since the demo world
//!   has a single live player at most.
//! - `net/minecraft/world/entity/monster/Monster.java::checkMonsterSpawnRules`
//!   - block-light <= 7 gate (light-driven spawn) - landed in H.2.

use infengine_voxel::CHUNK_SIZE;

use crate::net::{MobMsg, NetRole};
use crate::{DAY_LENGTH_S, VERTICAL_CHUNKS, Voxel, day_night};

use super::kinds::{KIND_CHICKEN, KIND_PIG, KIND_SHEEP, KIND_ZOMBIE};
use super::state::{
    MOB_MAX_HEALTH, MobState, ZOMBIE_MAX_HEALTH,
};

/// Period (seconds) between spawn attempts. Keeps the world from
/// flooding when the cap is high.
pub(crate) const MOB_SPAWN_PERIOD: f32 = 6.0;

/// Pluggable RNG: caller threads a counter-based xorshift through
/// the AI tick so spawn + wander share entropy.
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
    // Spawn in a ring 12..32 blocks around the host so
    // the world feels more populated as the player
    // explores. Pure origin-anchored spawns were
    // confusing once they wandered far from spawn.
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let radius = 12.0 + rng.next_f32() * 20.0;
    let cx = voxel.player_pos.x;
    let cz = voxel.player_pos.z;
    let wx = (cx + angle.cos() * radius).round() as i32;
    let wz = (cz + angle.sin() * radius).round() as i32;
    // Spawn scan: probe the column ceiling so the sheep
    // lands on real ground (canopy is fine here too since
    // mobs spawn from above).
    let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
    let Some(sy) = voxel.surface_y_below(wx, wz, ceiling) else {
        return next_id;
    };
    let top = voxel.world.get([wx, sy, wz]);
    if top.0 != 3 {
        return next_id;
    }
    // Pick kind: zombies only spawn at night
    // (sun_dir.y < 0) and crowd out sheep with a
    // 60% chance during the dark hours. Daytime is
    // sheep-only.
    let (sun_dir, _sky) = day_night(day_time);
    let is_night = sun_dir.y < 0.0;
    let roll = rng.next_f32();
    let kind = if is_night && roll < 0.6 {
        KIND_ZOMBIE
    } else if !is_night && roll < 0.25 {
        KIND_CHICKEN
    } else if !is_night && roll < 0.45 {
        // Daytime mixes in pigs alongside
        // chickens + sheep.
        KIND_PIG
    } else {
        KIND_SHEEP
    };
    let id = next_id;
    next_id = next_id.wrapping_add(1).max(1);
    let pos = [wx as f32 + 0.5, sy as f32 + 1.0, wz as f32 + 0.5];
    let mob = MobState {
        id,
        kind,
        pos,
        target: pos,
        yaw: 0.0,
        retarget_at: (day_time + 1.0).rem_euclid(DAY_LENGTH_S),
        health: match kind {
            KIND_ZOMBIE => ZOMBIE_MAX_HEALTH,
            KIND_CHICKEN => 4.0,
            KIND_PIG => 10.0,
            _ => MOB_MAX_HEALTH,
        },
        dying: 0.0,
        attack_cd: 0.0,
    };
    let initial_hp = match kind {
        KIND_ZOMBIE => ZOMBIE_MAX_HEALTH,
        KIND_CHICKEN => 4.0,
        KIND_PIG => 10.0,
        _ => MOB_MAX_HEALTH,
    } as u8;
    voxel.mobs.push(mob);
    voxel.remote_mobs.insert(
        id,
        MobMsg {
            id,
            kind,
            pos,
            yaw: 0.0,
            health: initial_hp,
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
    let mut rng = SpawnRng::new(rng_state);
    voxel.next_mob_id = try_spawn_one(voxel, day_time, voxel.next_mob_id, &mut rng);
}
