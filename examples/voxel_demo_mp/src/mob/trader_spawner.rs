//! W479 - `WanderingTraderSpawner` port.
//!
//! MC spawns the wandering trader through a CUSTOM spawner, never
//! through biome pools: every 1200 ticks (60 s) it burns 1200 off a
//! 24000-tick spawn delay; when the delay expires it rolls
//! `rand(100) <= spawnChance` (25 -> 50 -> 75 on successive misses,
//! reset to 25 on a success), then a 1-in-10 sub-roll, then tries 10
//! random surface spots within +-48 blocks of a random player. The
//! minted trader carries `DespawnDelay = 48000` ticks (40 min) which
//! his entity tick counts down - at 0 he vanishes.
//!
//! Cited MC source:
//! - `net/minecraft/world/entity/npc/wanderingtrader/
//!   WanderingTraderSpawner.java` - the timer / chance ladder + the
//!   spawn placement walk.
//! - `WanderingTrader.tickDespawnDelay` - the countdown pauses while
//!   the trader is trading and discards him at 0.
//! - `net/minecraft/world/level/saveddata/WanderingTraderData.java` -
//!   spawn_delay default 24000 / spawn_chance default 25.
//!
//! Divergences (documented per the mission):
//! - The two leashed trader llamas are SKIPPED: the llama MobKind
//!   does not exist yet (queued; skip-logged per spawn).
//! - MC persists spawnDelay / spawnChance in saved data; the engine
//!   keeps them runtime-only (a fresh boot restarts the ladder at
//!   the defaults). The DESPAWN timer does persist (entities v9).
//! - No village-POI index: MC prefers a meeting point within 48
//!   blocks as the spawn/wander anchor; the engine anchors on the
//!   player position (`poiPos.orElse(playerPos)` fallback branch).
//! - `hasEnoughSpace` checks a 2x3x2 collision-free box in MC; the
//!   engine's standing-room probe covers the 1x2 core column.
//! - The `#without_wandering_trader_spawns` biome tag only lists
//!   `the_void` - no engine equivalent, check skipped.

use crate::Voxel;
use crate::net::NetRole;

use super::kinds::KIND_WANDERING_TRADER;
use super::light::{has_solid_floor, has_standing_room};
use super::spawn::{SpawnRng, mint_mob, probe_column};

/// `DEFAULT_TICK_DELAY` - one evaluation per 1200 ticks (60 s).
pub(crate) const TRADER_TICK_DELAY: i32 = 1200;
/// `DEFAULT_SPAWN_DELAY` - 24000 ticks (20 min) between spawn rolls.
pub(crate) const TRADER_SPAWN_DELAY: i32 = 24_000;
/// `MIN_SPAWN_CHANCE` / `MAX_SPAWN_CHANCE` / `SPAWN_CHANCE_INCREASE`.
pub(crate) const TRADER_MIN_CHANCE: i32 = 25;
pub(crate) const TRADER_MAX_CHANCE: i32 = 75;
pub(crate) const TRADER_CHANCE_STEP: i32 = 25;
/// `WanderingTrader` despawn budget: 48000 ticks = 40 min.
pub(crate) const TRADER_DESPAWN_DELAY: u32 = 48_000;
/// Spawn placement search: rim radius + attempt count.
const SPAWN_RADIUS: i32 = 48;
const SPAWN_ATTEMPTS: usize = 10;

/// Runtime state of the trader spawner (the engine's stand-in for
/// MC's `WanderingTraderData` saved data - see the module doc).
#[derive(Debug)]
pub(crate) struct TraderSpawnerState {
    /// Ticks until the next evaluation (`tickDelay`).
    pub(crate) tick_delay: i32,
    /// Ticks until the next spawn roll (`spawnDelay`).
    pub(crate) spawn_delay: i32,
    /// Current spawn chance percent (`spawnChance`, 25..=75).
    pub(crate) spawn_chance: i32,
}

impl Default for TraderSpawnerState {
    fn default() -> Self {
        Self {
            tick_delay: TRADER_TICK_DELAY,
            spawn_delay: TRADER_SPAWN_DELAY,
            spawn_chance: TRADER_MIN_CHANCE,
        }
    }
}

/// One `WanderingTraderSpawner.tick` - called once per 20 Hz mob
/// tick on the authoritative side (overworld only, like every MC
/// custom spawner).
pub(crate) fn trader_spawner_tick(voxel: &mut Voxel, rng: &mut SpawnRng<'_>) {
    if voxel.current_dim != crate::dim::OVERWORLD {
        return;
    }
    voxel.trader_spawner.tick_delay -= 1;
    if voxel.trader_spawner.tick_delay > 0 {
        return;
    }
    voxel.trader_spawner.tick_delay = TRADER_TICK_DELAY;
    voxel.trader_spawner.spawn_delay -= TRADER_TICK_DELAY;
    if voxel.trader_spawner.spawn_delay > 0 {
        return;
    }
    voxel.trader_spawner.spawn_delay = TRADER_SPAWN_DELAY;
    let chance = voxel.trader_spawner.spawn_chance;
    voxel.trader_spawner.spawn_chance =
        (chance + TRADER_CHANCE_STEP).clamp(TRADER_MIN_CHANCE, TRADER_MAX_CHANCE);
    if (rng.next_u32() % 100) as i32 <= chance && try_spawn_trader(voxel, rng) {
        voxel.trader_spawner.spawn_chance = TRADER_MIN_CHANCE;
    }
}

/// `WanderingTraderSpawner.spawn`: random player, 1-in-10 sub-roll,
/// placement walk, mint. Returns true when the chance ladder should
/// reset (a trader spawned, or there was no player to serve).
fn try_spawn_trader(voxel: &mut Voxel, rng: &mut SpawnRng<'_>) -> bool {
    let Some(ppos) = random_player_pos(voxel, rng) else {
        return true;
    };
    if !rng.next_u32().is_multiple_of(10) {
        return false;
    }
    let anchor = [
        ppos[0].floor() as i32,
        ppos[1].floor() as i32,
        ppos[2].floor() as i32,
    ];
    let Some(spot) = find_spawn_position_near(voxel, anchor, SPAWN_RADIUS, rng)
    else {
        return false;
    };
    let yaw = rng.next_f32() * std::f32::consts::TAU;
    mint_mob(
        voxel,
        KIND_WANDERING_TRADER,
        [spot[0] as f32 + 0.5, spot[1] as f32, spot[2] as f32 + 0.5],
        yaw,
    );
    if let Some(m) = voxel.mobs.last_mut() {
        // The trader never distance-despawns (`removeWhenFarAway`
        // false) - only his 40-minute budget removes him.
        m.persistent = true;
        m.despawn_delay = TRADER_DESPAWN_DELAY;
    }
    // MC leashes 2 trader llamas to him here - no llama kind yet.
    tracing::info!(
        at = ?spot,
        "W479: WanderingTraderSpawner minted a trader (2 trader llamas \
         skipped - no llama MobKind yet, queued)"
    );
    true
}

/// A uniformly random live player position: host + remote poses.
fn random_player_pos(voxel: &Voxel, rng: &mut SpawnRng<'_>) -> Option<[f32; 3]> {
    let mut players: Vec<[f32; 3]> = vec![[
        voxel.player_pos.x,
        voxel.player_pos.y,
        voxel.player_pos.z,
    ]];
    if let NetRole::Server { poses, .. } = &voxel.net {
        for p in poses.values() {
            if p.id != 0 {
                players.push(p.pos);
            }
        }
    }
    let idx = (rng.next_u32() as usize) % players.len();
    players.get(idx).copied()
}

/// `findSpawnPositionNear` + `hasEnoughSpace`: up to 10 random spots
/// within `+-radius` of `center`, each resolved to the column
/// surface and validated ON_GROUND (solid floor + standing room).
pub(crate) fn find_spawn_position_near(
    voxel: &Voxel,
    center: [i32; 3],
    radius: i32,
    rng: &mut SpawnRng<'_>,
) -> Option<[i32; 3]> {
    let span = (radius * 2) as u32;
    for _ in 0..SPAWN_ATTEMPTS {
        let x = center[0] + (rng.next_u32() % span) as i32 - radius;
        let z = center[2] + (rng.next_u32() % span) as i32 - radius;
        let Some((spot, _)) = probe_column(voxel, x, z) else {
            continue;
        };
        if has_solid_floor(voxel, spot) && has_standing_room(voxel, spot) {
            return Some(spot);
        }
    }
    None
}

/// W479: the entity-side `tickDespawnDelay` - every mob carrying a
/// positive `despawn_delay` counts it down one game tick, PAUSED
/// while a trade session is open against that mob; at 0 the mob is
/// queued for removal. Only the wandering trader mints with a
/// budget today (48000 ticks); 0 = no timer.
pub(crate) fn despawn_delay_pass(voxel: &mut Voxel, to_remove: &mut Vec<u32>) {
    let trading_with = voxel.trade_open;
    for m in voxel.mobs.iter_mut() {
        if m.despawn_delay == 0 || m.dying > 0.0 {
            continue;
        }
        if trading_with == Some(m.id) {
            continue;
        }
        m.despawn_delay -= 1;
        if m.despawn_delay == 0 && !to_remove.contains(&m.id) {
            tracing::info!(
                id = m.id,
                "W479: wandering trader despawn budget expired"
            );
            to_remove.push(m.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::spawn_rules::teststamp::stamp_platform;
    use super::*;

    /// Drive the spawner clock straight to the next spawn roll:
    /// burns tick_delay + spawn_delay in TRADER_TICK_DELAY strides.
    fn force_due(v: &mut Voxel) {
        v.trader_spawner.tick_delay = 1;
        v.trader_spawner.spawn_delay = TRADER_TICK_DELAY;
    }

    /// The 25 -> 50 -> 75 ladder: every failed roll-cycle bumps the
    /// chance by 25 up to the 75 cap (a bare-world spawn can never
    /// succeed - no surface - so every cycle is a miss).
    #[test]
    fn chance_ladder_climbs_25_50_75() {
        let mut v = crate::Voxel::new();
        assert_eq!(v.trader_spawner.spawn_chance, 25);
        let mut seed = 3u32;
        let mut rng = SpawnRng::new(&mut seed);
        force_due(&mut v);
        trader_spawner_tick(&mut v, &mut rng);
        assert_eq!(v.trader_spawner.spawn_chance, 50);
        assert_eq!(v.trader_spawner.spawn_delay, TRADER_SPAWN_DELAY);
        force_due(&mut v);
        trader_spawner_tick(&mut v, &mut rng);
        assert_eq!(v.trader_spawner.spawn_chance, 75);
        force_due(&mut v);
        trader_spawner_tick(&mut v, &mut rng);
        assert_eq!(v.trader_spawner.spawn_chance, 75, "capped at 75");
        assert!(v.mobs.is_empty(), "no surface = no trader");
    }

    /// With a real surface under the player the spawner eventually
    /// mints a trader carrying the 48000-tick budget + persistence,
    /// and the ladder resets to 25.
    #[test]
    fn trader_mints_with_despawn_budget_and_resets_ladder() {
        let mut v = crate::Voxel::new();
        // Big grass platform around the origin (radius covers the
        // +-48 rim walk), player standing on it.
        stamp_platform(&mut v, 0, 0, 63, 50, 3, 15);
        v.player_pos = glam::Vec3::new(0.5, 65.0, 0.5);
        let mut seed = 1u32;
        let mut rng = SpawnRng::new(&mut seed);
        let mut minted = false;
        for _ in 0..256 {
            force_due(&mut v);
            trader_spawner_tick(&mut v, &mut rng);
            if !v.mobs.is_empty() {
                minted = true;
                break;
            }
        }
        assert!(minted, "256 due cycles over a flat world must mint");
        let t = &v.mobs[0];
        assert_eq!(t.kind, KIND_WANDERING_TRADER);
        assert_eq!(t.despawn_delay, TRADER_DESPAWN_DELAY);
        assert!(t.persistent, "traders never distance-despawn");
        assert_eq!(
            v.trader_spawner.spawn_chance, 25,
            "success resets the ladder"
        );
        let dx = (t.pos[0] - 0.5).abs();
        let dz = (t.pos[2] - 0.5).abs();
        assert!(
            dx <= 48.5 && dz <= 48.5,
            "trader spawns on the +-48 rim: {:?}",
            t.pos
        );
    }

    /// The countdown removes the trader at 0, and an open trade
    /// session freezes it.
    #[test]
    fn despawn_countdown_pauses_while_trading_and_removes_at_zero() {
        let mut v = crate::Voxel::new();
        let mut m = crate::mob::MobState::new_natural(
            9,
            KIND_WANDERING_TRADER,
            [0.5, 64.0, 0.5],
            [0.5, 64.0, 0.5],
            0.0,
            0.0,
            20.0,
        );
        m.persistent = true;
        m.despawn_delay = 3;
        v.mobs.push(m);
        let mut out = Vec::new();
        // Trading: the budget must not move.
        v.trade_open = Some(9);
        for _ in 0..10 {
            despawn_delay_pass(&mut v, &mut out);
        }
        assert_eq!(v.mobs[0].despawn_delay, 3, "trading pauses the clock");
        assert!(out.is_empty());
        // Session closed: 3 ticks to expiry.
        v.trade_open = None;
        despawn_delay_pass(&mut v, &mut out);
        despawn_delay_pass(&mut v, &mut out);
        assert!(out.is_empty(), "still 1 tick left");
        despawn_delay_pass(&mut v, &mut out);
        assert_eq!(out, vec![9], "expired trader queued for removal");
        // A mob without a budget is never touched.
        let plain = crate::mob::MobState::new_natural(
            10,
            crate::mob::kinds::KIND_SHEEP,
            [2.5, 64.0, 2.5],
            [2.5, 64.0, 2.5],
            0.0,
            0.0,
            8.0,
        );
        v.mobs.push(plain);
        let mut out2 = Vec::new();
        despawn_delay_pass(&mut v, &mut out2);
        assert!(out2.is_empty());
        assert_eq!(v.mobs[1].despawn_delay, 0);
    }

    /// The spawner only serves the overworld (MC wires custom
    /// spawners onto the overworld level alone).
    #[test]
    fn trader_spawner_is_overworld_only() {
        let mut v = crate::Voxel::new();
        stamp_platform(&mut v, 0, 0, 63, 50, 3, 15);
        v.player_pos = glam::Vec3::new(0.5, 65.0, 0.5);
        v.current_dim = "nether".to_string();
        let mut seed = 1u32;
        let mut rng = SpawnRng::new(&mut seed);
        for _ in 0..256 {
            force_due(&mut v);
            trader_spawner_tick(&mut v, &mut rng);
        }
        assert!(v.mobs.is_empty(), "no traders outside the overworld");
        assert_eq!(
            v.trader_spawner.spawn_chance, 25,
            "the ladder does not even tick there"
        );
    }
}
