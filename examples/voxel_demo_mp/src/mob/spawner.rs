//! W242.2 - monster spawner ticking: `BaseSpawner.java` (26.1.2) port.
//!
//! MC mirror:
//! - `net/minecraft/world/level/BaseSpawner.java::serverTick` - the
//!   whole cycle: 16-block player gate, 200..800-tick delay, 4 spawn
//!   attempts at `(rd-rd)*4` horizontal / `y-1..y+1` vertical offsets,
//!   same-kind crowding cap 6 in `box(pos).inflate(4)`.
//! - `net/minecraft/world/entity/SpawnPlacements.java` - zombie /
//!   skeleton / spider all gate through `Monster::checkMonsterSpawnRules`.
//!   For `EntitySpawnReason.SPAWNER` the below-block `isValidSpawn`
//!   rule auto-passes (`Mob.checkMobSpawnRules`), so spawner mobs CAN
//!   mint in mid-air; the light rule does NOT auto-pass (only
//!   TRIAL_SPAWNER `ignoresLightRequirements`) - torching the room
//!   shuts the spawner down.
//! - `net/minecraft/world/level/block/entity/SpawnerBlockEntity.java` -
//!   block-entity glue (we keep state in `Voxel.spawner_states` keyed
//!   by position instead).
//!
//! The spawned entity kind is a pure function of (effective world
//! seed, position) over `MonsterRoomFeature.MOBS` = [skeleton, zombie,
//! zombie, spider] - the W242 design: worldgen never persisted a roll,
//! so ticking re-derives it on demand and every regeneration agrees.
//!
//! Honest gaps vs MC:
//! - `Delay`/`SpawnPotentials` NBT persistence skipped: delay state is
//!   runtime-only, an app restart or chunk unload re-inits to the
//!   20-tick initial delay (MC saves the counter in the block entity).
//! - Light gate reuses the engine-wide hostile rule
//!   (`is_dark_enough_for_hostile`: block <= 7 AND effective sky <= 7)
//!   so spawners and natural spawns stay consistent; 26.1.2's
//!   `isDarkEnoughToSpawn` (sky > rand(32) reject, overworld block
//!   limit 0, brightness <= rand(0..=7)) is a global light-rule
//!   upgrade for a future wave.
//! - `finalizeSpawn` extras (gear rolls, baby-zombie chance) don't
//!   exist in the engine's mob model; `isUnobstructed` entity-vs-
//!   entity overlap isn't checked (engine mobs don't hard-collide).
//! - The `levelEvent(2004)` smoke+flame burst renders host-side only
//!   (no level-event broadcast channel yet); remote peers still see
//!   the mob appear via the normal mob broadcast.

use crate::block::SPAWNER_ID;
use crate::net::NetRole;
use crate::worldgen::mcgen::noise::SplitMix64;
use crate::Voxel;

use super::kinds::{hitbox_for, KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE, MobKind};
use super::light::is_dark_enough_for_hostile;
use super::spawn::mint_mob;

/// `BaseSpawner.DEFAULT_REQUIRED_PLAYER_RANGE` - a player within this
/// range (block centre, spherical) keeps the spawner active.
pub(crate) const SPAWNER_PLAYER_RANGE: f32 = 16.0;
/// `DEFAULT_MIN_SPAWN_DELAY` / `DEFAULT_MAX_SPAWN_DELAY`: after a
/// cycle the delay re-rolls to `min + rand(max - min)` ticks.
pub(crate) const SPAWNER_MIN_DELAY: i32 = 200;
pub(crate) const SPAWNER_MAX_DELAY: i32 = 800;
/// `DEFAULT_SPAWN_DELAY` - the fresh block entity's first countdown.
pub(crate) const SPAWNER_INITIAL_DELAY: i32 = 20;
/// `DEFAULT_SPAWN_COUNT` - attempts (and max mints) per cycle.
pub(crate) const SPAWNER_SPAWN_COUNT: u32 = 4;
/// `DEFAULT_MAX_NEARBY_ENTITIES` - same-kind cap in the +-4 box.
pub(crate) const SPAWNER_MAX_NEARBY: usize = 6;
/// `DEFAULT_SPAWN_RANGE` - horizontal half-extent of the spawn area.
pub(crate) const SPAWNER_SPAWN_RANGE: f32 = 4.0;

/// `MonsterRoomFeature.MOBS` - the dungeon spawner pool: zombie at
/// double weight.
const SPAWNER_MOBS: [u8; 4] = [KIND_SKELETON, KIND_ZOMBIE, KIND_ZOMBIE, KIND_SPIDER];

/// Extra mix over the shared position-seed so the spawner-kind stream
/// never correlates with the chest-loot stream at the same position.
const SALT_SPAWNER_KIND: u64 = 0xD4E6_0001;

/// Runtime tick state for one spawner block. Keyed by world position
/// in `Voxel.spawner_states`; minted lazily the first time a player
/// comes in range (mirrors the fresh block entity's 20-tick delay).
#[derive(Debug)]
pub(crate) struct SpawnerState {
    /// Ticks until the next spawn cycle (`BaseSpawner.spawnDelay`).
    pub(crate) delay: i32,
    /// Runtime RNG stream: delay re-rolls + spawn-offset rolls. Seeded
    /// per spawner from (net_tick, pos) - non-replayed runtime state,
    /// so determinism isn't required (MC uses the shared level RNG).
    pub(crate) rng: SplitMix64,
}

/// The spawned kind for the spawner at `pos`: deterministic in
/// (effective world seed, position), stable across regeneration and
/// re-derivation sites - the persisted-SpawnData replacement.
pub(crate) fn spawner_entity_kind(eff_seed: u32, pos: [i32; 3]) -> u8 {
    let mut rng = SplitMix64::new(
        crate::loot::chest_pos_seed(eff_seed, pos) ^ SALT_SPAWNER_KIND,
    );
    SPAWNER_MOBS[(rng.next_u64() % SPAWNER_MOBS.len() as u64) as usize]
}

/// Structure-aware kind: W243 mineshaft corridors hold CAVE SPIDER
/// spawners in MC; until that mob is ported, corridor spawners map to
/// the regular spider (once-logged stand-in). Everything else rolls
/// the dungeon table.
///
/// W250: WORKER-ONLY. The corridor membership probe folds the mcgen
/// cache stack (100+ ms cold), so gen workers resolve the kind during
/// the side-effect scan and ship it in `ChunkSideEffect::Spawner`;
/// the main thread reads `Voxel.spawner_kinds` and falls back to the
/// pure dungeon table, never to this probe.
pub(crate) fn spawner_entity_kind_at(ctx: &crate::worldgen::WorldgenCtx, pos: [i32; 3]) -> u8 {
    if crate::worldgen::mcgen::mineshaft::pos_in_mineshaft_corridor(ctx, pos) {
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            tracing::info!(
                "W243: mineshaft spawners use SPIDER as the cave-spider \
                 stand-in until the mob is ported"
            );
        });
        return KIND_SPIDER;
    }
    // W256: the stronghold portal room holds a SILVERFISH spawner in
    // MC - the spider stands in until the mob exists.
    if crate::worldgen::mcgen::stronghold::is_portal_spawner(ctx, pos) {
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            tracing::info!(
                "W256: stronghold portal spawners use SPIDER as the \
                 silverfish stand-in until the mob is ported"
            );
        });
        return KIND_SPIDER;
    }
    // W271/W379: the fortress monster throne holds a BLAZE spawner
    // - the real mob now (the skeleton stand-in retired with the
    // W379 port).
    if crate::worldgen::mcgen::fortress::is_throne_spawner(ctx, pos) {
        return crate::mob::kinds::KIND_BLAZE;
    }
    spawner_entity_kind(ctx.effective_seed(), pos)
}

/// Main-thread kind resolve: the worker-shipped `spawner_kinds` map
/// first (it knows about mineshaft corridors), the pure dungeon table
/// as fallback (player-placed cages + stale-save entries that never
/// went through a gen scan). Shared by the tick and the W250 cage
/// display render.
pub(crate) fn spawner_display_kind(vx: &Voxel, pos: [i32; 3]) -> u8 {
    vx.spawner_kinds.get(&pos).copied().unwrap_or_else(|| {
        spawner_entity_kind(vx.current_worldgen_ctx().effective_seed(), pos)
    })
}

/// `nextInt(n)` over the SplitMix64 stream (same shape the worldgen
/// feature streams use).
fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// Uniform f32 in [0, 1).
fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// `Level.hasNearbyAlivePlayer` for the spawner gate: host pose +
/// every server-tracked remote pose, strict `< range` from the block
/// centre. MC only filters SPECTATORS here - creative players DO
/// activate spawners (unlike the AI chase-target picker, which skips
/// creative).
fn any_player_within(vx: &Voxel, pos: [i32; 3], range: f32) -> bool {
    let c = [
        pos[0] as f32 + 0.5,
        pos[1] as f32 + 0.5,
        pos[2] as f32 + 0.5,
    ];
    let r2 = range * range;
    let near = |p: [f32; 3]| -> bool {
        let dx = p[0] - c[0];
        let dy = p[1] - c[1];
        let dz = p[2] - c[2];
        dx * dx + dy * dy + dz * dz < r2
    };
    if near([vx.player_pos.x, vx.player_pos.y, vx.player_pos.z]) {
        return true;
    }
    if let NetRole::Server { poses, .. } = &vx.net {
        for p in poses.values() {
            if p.id == 0 {
                continue;
            }
            if near(p.pos) {
                return true;
            }
        }
    }
    false
}

/// `level.noCollision(getSpawnAABB(...))` + the liquid half of
/// `checkSpawnObstruction`: every voxel cell the mob's AABB overlaps
/// must be passable and dry.
fn spawn_volume_clear(vx: &Voxel, kind: u8, feet: [f32; 3]) -> bool {
    let (hx, hy, hz, cy_off) = hitbox_for(kind);
    let min = [feet[0] - hx, feet[1] + cy_off - hy, feet[2] - hz];
    let max = [feet[0] + hx, feet[1] + cy_off + hy, feet[2] + hz];
    // Epsilon keeps a box edge that lands exactly on a cell boundary
    // from counting the next cell over.
    let lo = [
        min[0].floor() as i32,
        min[1].floor() as i32,
        min[2].floor() as i32,
    ];
    let hi = [
        (max[0] - 1e-4).floor() as i32,
        (max[1] - 1e-4).floor() as i32,
        (max[2] - 1e-4).floor() as i32,
    ];
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                let b = vx.world.get([x, y, z]);
                if !(b.is_air() || crate::block::block_is_aabb_passable(b)) {
                    return false;
                }
                // Water carries F_AABB_PASS, so the liquid rule is a
                // separate reject (MC `containsAnyLiquid`).
                if crate::block::block_is_watery(b.0) || b.0 == crate::LAVA_ID {
                    return false;
                }
            }
        }
    }
    true
}

/// Same-kind crowding count: alive mobs whose AABB intersects
/// `box(spawner block).inflate(4)` (MC counts by exact entity class
/// over that box).
fn count_same_kind_near(vx: &Voxel, pos: [i32; 3], kind: u8) -> usize {
    let min = [
        pos[0] as f32 - SPAWNER_SPAWN_RANGE,
        pos[1] as f32 - SPAWNER_SPAWN_RANGE,
        pos[2] as f32 - SPAWNER_SPAWN_RANGE,
    ];
    let max = [
        pos[0] as f32 + 1.0 + SPAWNER_SPAWN_RANGE,
        pos[1] as f32 + 1.0 + SPAWNER_SPAWN_RANGE,
        pos[2] as f32 + 1.0 + SPAWNER_SPAWN_RANGE,
    ];
    let (hx, hy, hz, cy_off) = hitbox_for(kind);
    vx.mobs
        .iter()
        .filter(|m| {
            if m.kind != kind || m.dying > 0.0 {
                return false;
            }
            let c = [m.pos[0], m.pos[1] + cy_off, m.pos[2]];
            c[0] + hx > min[0]
                && c[0] - hx < max[0]
                && c[1] + hy > min[1]
                && c[1] - hy < max[1]
                && c[2] + hz > min[2]
                && c[2] - hz < max[2]
        })
        .count()
}

/// `BaseSpawner.delay`: re-roll the countdown to `min + rand(range)`.
/// (MC also re-picks from `spawnPotentials` here; our kind is a fixed
/// function of position, so there's nothing to re-pick.)
fn reset_delay(st: &mut SpawnerState) {
    st.delay =
        SPAWNER_MIN_DELAY + next_int(&mut st.rng, SPAWNER_MAX_DELAY - SPAWNER_MIN_DELAY);
}

/// One spawn cycle (`spawnDelay == 0` branch of `serverTick`): up to 4
/// attempts, each rolling a position in the +-4 x [y-1, y+1] envelope;
/// a crowding-cap hit resets the delay and aborts the whole cycle
/// (matching the Java early return).
fn run_spawn_cycle(vx: &mut Voxel, pos: [i32; 3], st: &mut SpawnerState) {
    let kind = spawner_display_kind(vx, pos);
    let mut spawned_any = false;
    for _ in 0..SPAWNER_SPAWN_COUNT {
        let sx = pos[0] as f32
            + (next_f32(&mut st.rng) - next_f32(&mut st.rng)) * SPAWNER_SPAWN_RANGE
            + 0.5;
        let sy = (pos[1] + next_int(&mut st.rng, 3) - 1) as f32;
        let sz = pos[2] as f32
            + (next_f32(&mut st.rng) - next_f32(&mut st.rng)) * SPAWNER_SPAWN_RANGE
            + 0.5;
        let feet = [sx, sy, sz];
        if !spawn_volume_clear(vx, kind, feet) {
            continue;
        }
        // SpawnPlacements.checkSpawnRules(SPAWNER): the monster light
        // gate at the target block (ground rule auto-passes).
        let cell = [
            sx.floor() as i32,
            sy.floor() as i32,
            sz.floor() as i32,
        ];
        if !is_dark_enough_for_hostile(vx, cell) {
            continue;
        }
        if count_same_kind_near(vx, pos, kind) >= SPAWNER_MAX_NEARBY {
            reset_delay(st);
            return;
        }
        let yaw = next_f32(&mut st.rng) * std::f32::consts::TAU;
        mint_mob(vx, kind, feet, yaw);
        // `level.levelEvent(2004, pos, 0)` - smoke + flame burst at
        // the spawner block (host-local; see module gaps).
        let mut burst_seed =
            (vx.net_tick as u32) ^ (pos[0] as u32).rotate_left(9) ^ (pos[2] as u32);
        let tint =
            crate::resource_pack::particles::smoke_tint_at(&vx.resource_pack, 0.0);
        crate::ambient::spawn_spawner_burst(&mut vx.particles, pos, &mut burst_seed, tint);
        tracing::info!(
            pos = ?pos,
            kind = MobKind::from_u8(kind).name(),
            at = ?feet,
            "W242.2: spawner spawned mob",
        );
        spawned_any = true;
    }
    if spawned_any {
        reset_delay(st);
    }
}

/// Per-tick spawner pass (`serverTick` over every indexed spawner).
/// Runs at the fixed 20 Hz mob tick, host/offline only (the caller
/// guards `NetRole::Client`). Out-of-range spawners freeze their
/// countdown exactly like MC's `isNearPlayer` gate.
pub(crate) fn spawner_pass(vx: &mut Voxel) {
    if vx.spawner_positions.is_empty() {
        return;
    }
    let positions: Vec<[i32; 3]> = vx.spawner_positions.iter().copied().collect();
    for pos in positions {
        if !any_player_within(vx, pos, SPAWNER_PLAYER_RANGE) {
            continue;
        }
        // Self-heal: the index tolerates edit paths that bypass
        // `world_set_tracked` (stale save entries etc.).
        if vx.world.get(pos).0 != SPAWNER_ID {
            vx.spawner_positions.remove(&pos);
            vx.spawner_states.remove(&pos);
            continue;
        }
        let mut st = vx.spawner_states.remove(&pos).unwrap_or_else(|| SpawnerState {
            delay: SPAWNER_INITIAL_DELAY,
            rng: SplitMix64::new(
                (vx.net_tick ^ 0xA5A5_5A5A_0F0F_F0F0)
                    .wrapping_add(crate::loot::chest_pos_seed(0, pos)),
            ),
        });
        if st.delay > 0 {
            st.delay -= 1;
        } else {
            run_spawn_cycle(vx, pos, &mut st);
        }
        vx.spawner_states.insert(pos, st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{BlockId, Chunk, ChunkCoord, CHUNK_SIZE};

    /// Stamp the chunk holding `w` (minting it if needed) + set the
    /// block. `World::set` is a no-op on unloaded chunks.
    fn set_block(v: &mut Voxel, w: [i32; 3], id: u16) {
        let cs = CHUNK_SIZE as i32;
        let cc = [
            w[0].div_euclid(cs),
            w[1].div_euclid(cs),
            w[2].div_euclid(cs),
        ];
        if v.world.chunk(cc).is_none() {
            v.world.insert_chunk(cc, Chunk::empty());
        }
        v.world.set(w, BlockId(id));
    }

    fn stamp_dark(v: &mut Voxel, w: [i32; 3]) {
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
        g.set_block(lc, 0);
        g.set_sky(lc, 0);
    }

    /// A dungeon-ish rig: spawner at `pos`, cobble floor one below
    /// across the spawn envelope, everything else air, all dark.
    fn spawner_rig(pos: [i32; 3]) -> Voxel {
        let mut v = Voxel::new();
        set_block(&mut v, pos, SPAWNER_ID);
        v.spawner_positions.insert(pos);
        for dx in -6..=6 {
            for dz in -6..=6 {
                set_block(
                    &mut v,
                    [pos[0] + dx, pos[1] - 2, pos[2] + dz],
                    crate::block::COBBLE_ID,
                );
                for dy in -1..=3 {
                    stamp_dark(&mut v, [pos[0] + dx, pos[1] + dy, pos[2] + dz]);
                }
            }
        }
        // Player right next to the cage.
        v.player_pos = glam::Vec3::new(
            pos[0] as f32 + 2.5,
            pos[1] as f32,
            pos[2] as f32 + 0.5,
        );
        v
    }

    #[test]
    fn entity_kind_is_deterministic_and_weighted() {
        let k1 = spawner_entity_kind(1234, [10, 20, 30]);
        let k2 = spawner_entity_kind(1234, [10, 20, 30]);
        assert_eq!(k1, k2, "same seed + pos must derive the same kind");
        // Across many positions the pool must cover exactly the
        // dungeon trio, zombie ~2x the others.
        let mut counts = std::collections::HashMap::new();
        for i in 0..4000 {
            let k = spawner_entity_kind(99, [i, i * 7 % 253, -i * 3]);
            *counts.entry(k).or_insert(0usize) += 1;
        }
        assert_eq!(counts.len(), 3, "skeleton, zombie, spider only");
        let z = counts[&KIND_ZOMBIE] as f64;
        let s = counts[&KIND_SKELETON] as f64;
        let p = counts[&KIND_SPIDER] as f64;
        assert!(z > s * 1.5 && z > p * 1.5, "zombie double weight: z={z} s={s} p={p}");
    }

    #[test]
    fn initial_delay_counts_down_before_first_spawn() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        // 20 initial-delay ticks: no mobs yet.
        for _ in 0..SPAWNER_INITIAL_DELAY {
            spawner_pass(&mut v);
            assert!(v.mobs.is_empty(), "no spawn during the initial delay");
        }
        // Tick 21 runs the first cycle.
        spawner_pass(&mut v);
        assert!(
            !v.mobs.is_empty(),
            "first cycle after the 20-tick delay should mint in an open dark room"
        );
        let st = v.spawner_states.get(&pos).expect("state exists");
        assert!(
            (SPAWNER_MIN_DELAY..SPAWNER_MAX_DELAY).contains(&st.delay),
            "delay re-rolled into [200, 800): {}",
            st.delay
        );
        // Every minted mob is the derived kind, inside the envelope.
        let eff = v.current_worldgen_ctx().effective_seed();
        let want = spawner_entity_kind(eff, pos);
        for m in &v.mobs {
            assert_eq!(m.kind, want);
            assert!((m.pos[0] - (pos[0] as f32 + 0.5)).abs() <= 4.5);
            assert!((m.pos[2] - (pos[2] as f32 + 0.5)).abs() <= 4.5);
            let dy = m.pos[1] - pos[1] as f32;
            assert!((-1.0..=1.0).contains(&dy), "y offset in -1..=1: {dy}");
        }
    }

    #[test]
    fn out_of_range_player_freezes_the_countdown() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        v.player_pos = glam::Vec3::new(
            pos[0] as f32 + 40.0,
            pos[1] as f32,
            pos[2] as f32,
        );
        for _ in 0..100 {
            spawner_pass(&mut v);
        }
        assert!(v.mobs.is_empty());
        assert!(
            !v.spawner_states.contains_key(&pos),
            "state stays unminted while nobody is in range"
        );
    }

    #[test]
    fn lit_room_blocks_the_cycle() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        // Torch-lit: every candidate cell over the hostile ceiling.
        let cs = CHUNK_SIZE as i32;
        for dx in -6..=6 {
            for dz in -6..=6 {
                for dy in -1..=3 {
                    let w = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                    let cc = [
                        w[0].div_euclid(cs),
                        w[1].div_euclid(cs),
                        w[2].div_euclid(cs),
                    ];
                    let lc = [
                        w[0].rem_euclid(cs) as u32,
                        w[1].rem_euclid(cs) as u32,
                        w[2].rem_euclid(cs) as u32,
                    ];
                    v.lighting.ensure(cc).set_block(lc, 14);
                }
            }
        }
        for _ in 0..(SPAWNER_INITIAL_DELAY + 50) {
            spawner_pass(&mut v);
        }
        assert!(v.mobs.is_empty(), "block light 14 must gate every attempt");
        // MC keeps delay at 0 retrying every tick after a silent
        // (no-spawn, no-cap) cycle - it never re-rolls.
        assert_eq!(v.spawner_states.get(&pos).expect("state").delay, 0);
    }

    #[test]
    fn crowding_cap_aborts_and_rerolls() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        let eff = v.current_worldgen_ctx().effective_seed();
        let kind = spawner_entity_kind(eff, pos);
        for i in 0..SPAWNER_MAX_NEARBY {
            v.mobs.push(crate::mob::state::MobState::new_natural(
                (i as u32) + 1,
                kind,
                [pos[0] as f32 + 0.5, pos[1] as f32, pos[2] as f32 + 2.5],
                [pos[0] as f32 + 0.5, pos[1] as f32, pos[2] as f32 + 2.5],
                0.0,
                0.0,
                10.0,
            ));
        }
        for _ in 0..(SPAWNER_INITIAL_DELAY + 1) {
            spawner_pass(&mut v);
        }
        assert_eq!(v.mobs.len(), SPAWNER_MAX_NEARBY, "cap blocks new mints");
        let st = v.spawner_states.get(&pos).expect("state");
        assert!(
            (SPAWNER_MIN_DELAY..SPAWNER_MAX_DELAY).contains(&st.delay),
            "cap hit re-rolls the delay: {}",
            st.delay
        );
    }

    #[test]
    fn broken_spawner_self_heals_out_of_the_index() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        set_block(&mut v, pos, 0);
        spawner_pass(&mut v);
        assert!(!v.spawner_positions.contains(&pos));
        assert!(!v.spawner_states.contains_key(&pos));
    }

    /// Full-pipeline probe (the in-game-verify stand-in while screen
    /// control is unavailable): generate the REAL chunks around a
    /// known monster room with the live worldgen pipeline, index the
    /// spawner exactly like the stream delivery arm, park the player
    /// by the cage and run the 20 Hz pass. Proves worldgen -> index
    /// -> tick -> mint end-to-end on real dungeon geometry, plus the
    /// crowding cap on long runs. Defaults target the test-mc-9 room
    /// at (4743, 24, 4733), seed 3592732596.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_SPAWNER_X/Y/Z=<pos>
    /// cargo test -p voxel_demo_mp --release --bin voxel_demo_mp
    /// spawner_live_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn spawner_live_probe() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let spos = [
            env_i32("INFENGINE_SPAWNER_X", 4743),
            env_i32("INFENGINE_SPAWNER_Y", 24),
            env_i32("INFENGINE_SPAWNER_Z", 4733),
        ];
        let mut v = Voxel::new();
        v.base_seed = seed;
        v.dim_worldgens.insert(
            crate::dim::OVERWORLD.to_string(),
            crate::worldgen::WorldGen::Mc,
        );
        // Generate the 3x3 chunk-column neighbourhood on the room's
        // vertical slice with the LIVE pipeline and index spawners
        // the way the stream delivery arm does.
        let cs = CHUNK_SIZE as i32;
        let ctx = v.current_worldgen_ctx();
        let (ccx, ccy, ccz) = (
            spos[0].div_euclid(cs),
            spos[1].div_euclid(cs),
            spos[2].div_euclid(cs),
        );
        for dx in -1..=1 {
            for dz in -1..=1 {
                let coord = [ccx + dx, ccy, ccz + dz];
                let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
                for lx in 0..CHUNK_SIZE {
                    for ly in 0..CHUNK_SIZE {
                        for lz in 0..CHUNK_SIZE {
                            if chunk.get(lx, ly, lz).0 == SPAWNER_ID {
                                v.spawner_positions.insert([
                                    coord[0] * cs + lx as i32,
                                    coord[1] * cs + ly as i32,
                                    coord[2] * cs + lz as i32,
                                ]);
                            }
                        }
                    }
                }
                v.world.insert_chunk(coord, chunk);
            }
        }
        assert!(
            v.spawner_positions.contains(&spos),
            "the full worldgen pipeline must place the spawner at {spos:?} \
             (found: {:?})",
            v.spawner_positions
        );
        // Midnight: unlit probe grids default to sky 15, and the
        // effective-sky ramp at night matches the cave's real 0 -
        // the light gate itself is unit-tested above.
        v.day_time = crate::DAY_LENGTH_S * 0.75;
        v.player_pos = glam::Vec3::new(
            spos[0] as f32 + 2.5,
            spos[1] as f32 + 1.0,
            spos[2] as f32 + 0.5,
        );
        let kind = spawner_entity_kind_at(&v.current_worldgen_ctx(), spos);
        let mut first_spawn_tick = None;
        let mut cycles = 0u32;
        for tick in 0..6000u32 {
            let before = v.mobs.len();
            spawner_pass(&mut v);
            if v.mobs.len() > before {
                cycles += 1;
                if first_spawn_tick.is_none() {
                    first_spawn_tick = Some(tick);
                }
                println!(
                    "tick {tick}: +{} mob(s) -> {} total (kind {})",
                    v.mobs.len() - before,
                    v.mobs.len(),
                    MobKind::from_u8(kind).name(),
                );
            }
        }
        let first = first_spawn_tick.expect("the real dungeon must spawn mobs");
        assert_eq!(
            first, SPAWNER_INITIAL_DELAY as u32,
            "first cycle fires right after the 20-tick initial delay"
        );
        assert!(cycles >= 2, "the 200..800 re-roll must fire more cycles");
        for m in &v.mobs {
            assert_eq!(m.kind, kind, "every mint uses the derived kind");
            assert!(
                (m.pos[0] - (spos[0] as f32 + 0.5)).abs() <= 4.5
                    && (m.pos[2] - (spos[2] as f32 + 0.5)).abs() <= 4.5,
                "mob at {:?} outside the spawn envelope",
                m.pos
            );
        }
        assert!(
            v.mobs.len() <= SPAWNER_MAX_NEARBY,
            "static mobs never leave the box, so the crowding cap must \
             hold the population at {SPAWNER_MAX_NEARBY}: {}",
            v.mobs.len()
        );
        println!(
            "spawner at {spos:?}: first spawn at tick {first}, {cycles} \
             spawning cycles over 6000 ticks (5 min), {} mob(s) alive \
             (cap {SPAWNER_MAX_NEARBY}), kind {}",
            v.mobs.len(),
            MobKind::from_u8(kind).name(),
        );
    }

    /// W250 diagnosis probe: read the SAVED chunks around a spawner
    /// and print ASCII slices with light emitters marked - answers
    /// "is the spawner still there, and is the room lit (the MC
    /// light gate correctly holding spawns back)?" headlessly.
    /// `INFENGINE_DUMP_WORLD=test-mc-9 INFENGINE_SPAWNER_X/Y/Z=<pos>
    /// cargo test -p voxel_demo_mp --release --bin voxel_demo_mp
    /// dump_spawner_room_save -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_spawner_room_save() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let slug = std::env::var("INFENGINE_DUMP_WORLD")
            .unwrap_or_else(|_| "test-mc-9".into());
        let dim = std::env::var("INFENGINE_DUMP_DIM")
            .unwrap_or_else(|_| "overworld".into());
        let spos = [
            env_i32("INFENGINE_SPAWNER_X", 4743),
            env_i32("INFENGINE_SPAWNER_Y", 24),
            env_i32("INFENGINE_SPAWNER_Z", 4733),
        ];
        let cs = CHUNK_SIZE as i32;
        let read = |w: [i32; 3]| -> Option<u16> {
            let cc = [
                w[0].div_euclid(cs),
                w[1].div_euclid(cs),
                w[2].div_euclid(cs),
            ];
            let payload = crate::world_save::chunks::read_chunk(&slug, &dim, cc)
                .ok()
                .flatten()?;
            let chunk = payload.voxels.into_chunk().ok()?;
            Some(
                chunk
                    .get(
                        w[0].rem_euclid(cs) as usize,
                        w[1].rem_euclid(cs) as usize,
                        w[2].rem_euclid(cs) as usize,
                    )
                    .0,
            )
        };
        let mut emitters = 0usize;
        for y in (spos[1] - 1)..=(spos[1] + 3) {
            println!("--- y = {y} ---");
            for z in (spos[2] - 6)..=(spos[2] + 6) {
                let mut line = String::new();
                for x in (spos[0] - 6)..=(spos[0] + 6) {
                    let ch = match read([x, y, z]) {
                        None => '?',
                        Some(0) => '.',
                        Some(id) if id == SPAWNER_ID => 'S',
                        Some(id) if id == crate::block::CHEST_ID => 'C',
                        Some(id)
                            if crate::block::light_emission::light_emission_for_id(
                                id,
                            ) > 0 =>
                        {
                            emitters += 1;
                            'T'
                        }
                        Some(_) => '#',
                    };
                    line.push(ch);
                }
                println!("{line}");
            }
        }
        println!(
            "spawner block present: {:?}, light emitters near the room: {emitters}",
            read(spos).map(|id| id == SPAWNER_ID)
        );
    }

    #[test]
    fn spawn_volume_rejects_solid_and_water() {
        let pos = [8, 40, 8];
        let mut v = spawner_rig(pos);
        // Solid at the feet cell.
        set_block(&mut v, [12, 40, 8], crate::block::COBBLE_ID);
        assert!(!spawn_volume_clear(&v, KIND_ZOMBIE, [12.5, 40.0, 8.5]));
        // Water floods a cell: AABB-passable but wet.
        set_block(&mut v, [4, 40, 8], crate::block::WATER_ID);
        assert!(!spawn_volume_clear(&v, KIND_ZOMBIE, [4.5, 40.0, 8.5]));
        // Open dark air passes.
        assert!(spawn_volume_clear(&v, KIND_ZOMBIE, [8.5, 40.0, 10.5]));
    }
}
