//! W479 - `PhantomSpawner` port (the insomnia mechanic).
//!
//! MC spawns phantoms through a CUSTOM spawner, never through biome
//! pools: every 60-120 s it checks each player's `TIME_SINCE_REST`
//! stat and rolls `nextInt(clamp(stat, 1, MAX)) >= 72000` - below
//! 72000 ticks (1 h of play) a spawn is impossible; past it the odds
//! grow with every sleepless minute. Passing players get 1-4
//! phantoms minted 20-34 blocks above their head with a +-10 block
//! scatter.
//!
//! Cited MC source:
//! - `net/minecraft/world/level/levelgen/PhantomSpawner.java` - the
//!   cooldown roll, per-player gates and the spawn burst.
//! - `net/minecraft/stats/Stats.TIME_SINCE_REST` - reset when the
//!   player sleeps (`ServerPlayer.startSleepInBed`); the engine
//!   resets on the bed night-skip (`try_sleep_bed` host arm + the
//!   host's `vx.sleep` handler for remote players).
//!
//! Divergences (documented per the mission):
//! - No local-difficulty system: MC's difficulty gate
//!   (`difficulty.isHarderThan(rand times 3.0)`) and the
//!   `1 + rand(difficultyId + 1)` group roll have no inputs here -
//!   the gate is omitted and the group uses the hard-difficulty
//!   roll `1 + rand(4)` = one to four phantoms per burst.
//! - MC excludes spectators; the engine's closest mode is Creative,
//!   so creative hosts are exempt.
//! - Remote players' rest is a host-side approximation: the host
//!   counts ticks per client and zeroes the counter when that
//!   client's validated `vx.sleep` skips the night. Remote counters
//!   are not persisted (a reconnect starts rested); the HOST's own
//!   counter persists in players.bin v7.

use crate::Voxel;
use crate::net::NetRole;

use super::kinds::KIND_PHANTOM;
use super::pool::SpawnCategory;
use super::spawn::{SpawnRng, category_count, mint_mob};

/// `TIME_SINCE_REST` threshold: 72000 ticks = 3 in-game days = 1 h
/// of play at 20 TPS. Below this a phantom spawn is impossible.
pub(crate) const INSOMNIA_MIN_TICKS: u32 = 72_000;

/// Runtime state of the phantom spawner. Transient (MC keeps
/// `nextTick` runtime-only too).
#[derive(Debug, Default)]
pub(crate) struct PhantomSpawnerState {
    /// Ticks until the next spawn evaluation
    /// (`PhantomSpawner.nextTick`).
    pub(crate) next_tick: i32,
}

/// Bump every insomnia counter by one game tick: the host player's
/// persisted stat plus the per-client approximations (entries are
/// minted lazily from the live pose table).
pub(crate) fn tick_rest_counters(voxel: &mut Voxel) {
    voxel.ticks_since_rest = voxel.ticks_since_rest.saturating_add(1);
    if let NetRole::Server { poses, .. } = &voxel.net {
        let ids: Vec<u32> =
            poses.values().map(|p| p.id).filter(|id| *id != 0).collect();
        for id in ids {
            let e = voxel.remote_ticks_since_rest.entry(id).or_insert(0);
            *e = e.saturating_add(1);
        }
    }
}

/// The insomnia odds roll: `random.nextInt(clamp(stat, 1, MAX)) >=
/// 72000`. Pure so the odds are testable without a `Voxel`.
pub(crate) fn insomnia_roll(ticks_since_rest: u32, rng: &mut SpawnRng<'_>) -> bool {
    let value = ticks_since_rest.max(1);
    rng.next_u32() % value >= INSOMNIA_MIN_TICKS
}

/// True when nothing solid sits above `wpos` up to the world top -
/// the engine's `canSeeSky` probe (heightmap semantics: ANY block
/// occludes).
fn can_see_sky(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    for y in (wpos[1] + 1)..=crate::WORLD_MAX_Y {
        if !voxel.world.get([wpos[0], y, wpos[2]]).is_air() {
            return false;
        }
    }
    true
}

/// One `PhantomSpawner.tick` - called once per 20 Hz mob tick on the
/// authoritative side. Rolls the 60-120 s cooldown, then walks every
/// player through the per-player gates and mints the phantom burst
/// above the ones the insomnia roll condemns.
pub(crate) fn phantom_spawner_tick(voxel: &mut Voxel, rng: &mut SpawnRng<'_>) {
    // MC attaches the custom spawners (phantom / trader / patrol /
    // cat) to the OVERWORLD ServerLevel only - other dims never tick
    // them, which is also why the `!hasSkyLight` branch below is
    // dead in vanilla (it serves modded dims).
    if voxel.current_dim != crate::dim::OVERWORLD {
        return;
    }
    voxel.phantom_spawner.next_tick -= 1;
    if voxel.phantom_spawner.next_tick > 0 {
        return;
    }
    // `nextTick += (60 + rand(60)) * 20` ticks.
    voxel.phantom_spawner.next_tick = ((60 + rng.next_u32() % 60) * 20) as i32;
    let dim_def =
        crate::worldgen::dim_registry::dim_def_or_overworld(&voxel.current_dim);
    let has_sky_light = !dim_def.has_ceiling;
    // `getSkyDarken() >= 5 || !hasSkyLight` - the engine's night
    // clock is the skyDarken analog (darken >= 5 ~= sun below the
    // horizon). Raw sun elevation, the bed night-skip convention
    // (`day_night`'s returned vector is clamped below the horizon
    // and never reads negative).
    if has_sky_light {
        let phase =
            (voxel.day_time / crate::DAY_LENGTH_S).rem_euclid(1.0);
        if (phase * std::f32::consts::TAU).sin() >= -0.05 {
            return;
        }
    }
    // Collect the player list: host (id None) + remote poses.
    let mut players: Vec<(Option<u32>, [i32; 3])> = Vec::new();
    if !voxel.game_mode.is_creative() {
        players.push((
            None,
            [
                voxel.player_pos.x.floor() as i32,
                voxel.player_pos.y.floor() as i32,
                voxel.player_pos.z.floor() as i32,
            ],
        ));
    }
    if let NetRole::Server { poses, .. } = &voxel.net {
        for p in poses.values() {
            if p.id == 0 {
                continue;
            }
            players.push((
                Some(p.id),
                [
                    p.pos[0].floor() as i32,
                    p.pos[1].floor() as i32,
                    p.pos[2].floor() as i32,
                ],
            ));
        }
    }
    for (client, ppos) in players {
        // `!hasSkyLight || (y >= seaLevel && canSeeSky)`.
        if has_sky_light
            && (ppos[1] < crate::worldgen::climate::SEA_LEVEL
                || !can_see_sky(voxel, ppos))
        {
            continue;
        }
        // Local-difficulty gate omitted (no difficulty system).
        let rest = match client {
            None => voxel.ticks_since_rest,
            Some(id) => {
                voxel.remote_ticks_since_rest.get(&id).copied().unwrap_or(0)
            }
        };
        if !insomnia_roll(rest, rng) {
            continue;
        }
        // Spawn cell: 20 + rand(15) above, +-10 in x/z.
        let spawn = [
            ppos[0] - 10 + (rng.next_u32() % 21) as i32,
            ppos[1] + 20 + (rng.next_u32() % 15) as i32,
            ppos[2] - 10 + (rng.next_u32() % 21) as i32,
        ];
        // `NaturalSpawner.isValidEmptySpawnBlock`: air, no fluid.
        if !voxel.world.get(spawn).is_air() {
            continue;
        }
        // Group of 1..4 (hard-difficulty roll, see module doc),
        // respecting the monster category cap.
        let group = 1 + rng.next_u32() % 4;
        for _ in 0..group {
            if category_count(voxel, SpawnCategory::Monster)
                >= SpawnCategory::Monster.cap()
            {
                break;
            }
            let yaw = rng.next_f32() * std::f32::consts::TAU;
            mint_mob(
                voxel,
                KIND_PHANTOM,
                [
                    spawn[0] as f32 + 0.5,
                    spawn[1] as f32,
                    spawn[2] as f32 + 0.5,
                ],
                yaw,
            );
        }
        tracing::info!(
            player = ?client,
            rest_ticks = rest,
            at = ?spawn,
            group,
            "W479: PhantomSpawner minted a phantom burst"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng_sweep(rest: u32, seeds: u32) -> u32 {
        let mut hits = 0;
        for s in 1..=seeds {
            let mut seed = s.wrapping_mul(0x9E37_79B9) | 1;
            let mut rng = SpawnRng::new(&mut seed);
            if insomnia_roll(rest, &mut rng) {
                hits += 1;
            }
        }
        hits
    }

    /// Below (and AT) the 72000-tick threshold the roll can never
    /// pass - `nextInt(value)` tops out at `value - 1`.
    #[test]
    fn insomnia_below_threshold_never_rolls() {
        for rest in [0u32, 1, 100, 71_999, 72_000] {
            assert_eq!(rng_sweep(rest, 256), 0, "rest {rest} must never pass");
        }
    }

    /// Past the threshold the odds grow with sleeplessness:
    /// (value - 72000) / value.
    #[test]
    fn insomnia_odds_grow_with_sleeplessness() {
        let some = rng_sweep(80_000, 512);
        assert!(some > 0, "80000 ticks must pass sometimes");
        let lots = rng_sweep(720_000, 512);
        assert!(
            lots > some,
            "10x the insomnia must pass more often ({lots} vs {some})"
        );
        // 720000 ticks: pass chance = 0.9 - the sweep should sit
        // near it.
        assert!(lots > 400, "720000 ticks passes ~90% ({lots}/512)");
    }

    /// The host counter ticks up once per game tick and the bed
    /// reset zeroes it (the `try_sleep_bed` / `vx.sleep` wiring).
    #[test]
    fn rest_counter_ticks_and_resets() {
        let mut v = crate::Voxel::new();
        assert_eq!(v.ticks_since_rest, 0);
        for _ in 0..5 {
            tick_rest_counters(&mut v);
        }
        assert_eq!(v.ticks_since_rest, 5);
        v.ticks_since_rest = 0; // the sleep reset
        tick_rest_counters(&mut v);
        assert_eq!(v.ticks_since_rest, 1);
    }

    /// Full spawner path: a well-rested player never gets phantoms;
    /// an insomniac under the night sky does (cooldown forced due,
    /// open world = every column reads air = sky visible).
    #[test]
    fn spawner_tick_spawns_only_for_insomniacs_at_night() {
        let mut v = crate::Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        v.player_pos.y = 80.0; // above sea level
        // Rested: sweep many cooldown rolls, no phantom may appear.
        v.ticks_since_rest = 1000;
        let mut seed = 7u32;
        let mut rng = SpawnRng::new(&mut seed);
        for _ in 0..64 {
            v.phantom_spawner.next_tick = 0;
            phantom_spawner_tick(&mut v, &mut rng);
        }
        assert!(v.mobs.is_empty(), "rested players never meet phantoms");
        // Insomniac: 100x the threshold makes the roll near-certain.
        v.ticks_since_rest = INSOMNIA_MIN_TICKS * 100;
        for _ in 0..64 {
            v.phantom_spawner.next_tick = 0;
            phantom_spawner_tick(&mut v, &mut rng);
        }
        assert!(
            !v.mobs.is_empty(),
            "insomniac under open night sky must get phantoms"
        );
        assert!(v.mobs.iter().all(|m| m.kind == KIND_PHANTOM));
        assert!(
            v.mobs.iter().all(|m| m.pos[1] >= 100.0),
            "phantoms mint 20+ above the player"
        );
        // Daytime: even a chronic insomniac is safe.
        let mut day = crate::Voxel::new();
        day.day_time = 0.0; // noon
        day.player_pos.y = 80.0;
        day.ticks_since_rest = INSOMNIA_MIN_TICKS * 100;
        let mut seed2 = 9u32;
        let mut rng2 = SpawnRng::new(&mut seed2);
        for _ in 0..64 {
            day.phantom_spawner.next_tick = 0;
            phantom_spawner_tick(&mut day, &mut rng2);
        }
        assert!(day.mobs.is_empty(), "no phantoms under the day sky");
    }

    /// Below sea level (a mine) the sky gate protects the player
    /// even with maxed insomnia.
    #[test]
    fn spawner_tick_skips_players_below_sea_level() {
        let mut v = crate::Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75;
        v.player_pos.y = 30.0; // deep under sea level 63
        v.ticks_since_rest = INSOMNIA_MIN_TICKS * 100;
        let mut seed = 11u32;
        let mut rng = SpawnRng::new(&mut seed);
        for _ in 0..64 {
            v.phantom_spawner.next_tick = 0;
            phantom_spawner_tick(&mut v, &mut rng);
        }
        assert!(v.mobs.is_empty(), "no phantoms below sea level");
    }

    /// The cooldown re-arms to the MC 60-120 s band (1200..2400
    /// ticks) after every evaluation.
    #[test]
    fn cooldown_rearms_to_the_mc_band() {
        let mut v = crate::Voxel::new();
        v.day_time = 0.0;
        let mut seed = 13u32;
        let mut rng = SpawnRng::new(&mut seed);
        for _ in 0..32 {
            v.phantom_spawner.next_tick = 0;
            phantom_spawner_tick(&mut v, &mut rng);
            assert!(
                (1200..=2380).contains(&v.phantom_spawner.next_tick),
                "cooldown out of band: {}",
                v.phantom_spawner.next_tick
            );
        }
    }
}
