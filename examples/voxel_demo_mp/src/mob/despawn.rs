//! MC-style despawn rules.
//!
//! Replaces the H.1 "any mob past MOB_DESPAWN_DIST_SQ disappears
//! next tick" gate with the canonical 3-tier MC check:
//!
//! 1. Persistent mobs (`MobState.persistent = true`) never despawn.
//!    Set by /spawnmob, /horde, name-tagging, breeding, leashing.
//!    Mirrors MC's `Mob.persistenceRequired`.
//! 2. Mobs outside `INSTANT_DESPAWN_DIST` (80 blocks) from every
//!    known player despawn immediately.
//! 3. Mobs between `FAR_DESPAWN_DIST` (48 blocks) and
//!    `INSTANT_DESPAWN_DIST` count down `far_despawn_timer`. When
//!    it hits zero the mob despawns. The timer resets to
//!    `FAR_DESPAWN_TIMER` (30 s) whenever a player comes back
//!    within `FAR_DESPAWN_DIST`.
//!
//! The check runs every `DESPAWN_CHECK_PERIOD` seconds rather than
//! every tick - the cadence is enough granularity to feel
//! responsive (a player crossing the boundary sees mobs phase out
//! within a few seconds) without burning per-tick CPU on the
//! squared-distance scan.
//!
//! Cited MC:
//! - `net/minecraft/world/entity/Mob.java::checkDespawn` - the
//!   canonical despawn entry point: persistent check, instant
//!   despawn distance, then the per-tick far-distance timer.
//! - `net/minecraft/world/entity/Mob.java::removeWhenFarAway` -
//!   per-subclass "should this even despawn" hook (we collapse
//!   to a single per-mob `persistent` flag).

use crate::Voxel;

use super::state::{
    DESPAWN_CHECK_PERIOD, FAR_DESPAWN_DIST, FAR_DESPAWN_TIMER, INSTANT_DESPAWN_DIST,
    MobState,
};

/// Decide what to do with one mob in the despawn pass. Returns
/// `true` when the mob should be evicted, `false` when it stays
/// (the caller may have updated `far_despawn_timer`).
///
/// `dt` is the time delta since the last despawn check (NOT the
/// per-tick dt). Pure function on the mob + the nearest player
/// position so the caller can swap in a different nearest-player
/// finder during testing.
pub(crate) fn should_despawn(
    mob: &mut MobState,
    nearest_player_distance_sq: Option<f32>,
    dt: f32,
) -> bool {
    // Persistent mobs never despawn.
    if mob.persistent {
        return false;
    }
    let Some(d2) = nearest_player_distance_sq else {
        // No live players: never despawn (player disconnected;
        // we want the world to retain its mob state until they
        // come back).
        return false;
    };
    if d2 > INSTANT_DESPAWN_DIST * INSTANT_DESPAWN_DIST {
        return true;
    }
    if d2 > FAR_DESPAWN_DIST * FAR_DESPAWN_DIST {
        // Tick the far-despawn timer.
        mob.far_despawn_timer -= dt;
        if mob.far_despawn_timer <= 0.0 {
            return true;
        }
    } else {
        // Player is close: reset the timer.
        mob.far_despawn_timer = FAR_DESPAWN_TIMER;
    }
    false
}

/// Run the despawn pass over every live mob. Appends ids to
/// `to_remove` for the caller to evict. Returns the time the next
/// check should run (caller compares against `day_time`).
///
/// `last_check` is the `day_time` value at the previous despawn
/// check; the function only runs the scan when at least
/// `DESPAWN_CHECK_PERIOD` seconds have passed.
pub(crate) fn despawn_pass(
    voxel: &mut Voxel,
    day_time: f32,
    last_check: f32,
    to_remove: &mut Vec<u32>,
) -> f32 {
    let dt = day_time - last_check;
    if dt < DESPAWN_CHECK_PERIOD {
        return last_check;
    }
    let mobs_snapshot: Vec<(u32, [f32; 3], bool)> = voxel
        .mobs
        .iter()
        .map(|m| (m.id, m.pos, m.dying > 0.0))
        .collect();
    for (id, pos, dying) in mobs_snapshot {
        if dying {
            continue;
        }
        let nearest = voxel.nearest_player_pos(pos, 1.0e6);
        let d2 = nearest.map(|p| {
            let dx = p[0] - pos[0];
            let dy = p[1] - pos[1];
            let dz = p[2] - pos[2];
            dx * dx + dy * dy + dz * dz
        });
        let Some(mob_idx) = voxel.mobs.iter().position(|m| m.id == id) else {
            continue;
        };
        if should_despawn(&mut voxel.mobs[mob_idx], d2, dt) && !to_remove.contains(&id) {
            to_remove.push(id);
        }
    }
    day_time
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::kinds::KIND_SHEEP;

    fn fresh_mob(persistent: bool) -> MobState {
        let mut m = MobState::new_natural(
            1,
            KIND_SHEEP,
            [0.0, 64.0, 0.0],
            [0.0, 64.0, 0.0],
            0.0,
            0.0,
            8.0,
        );
        m.persistent = persistent;
        m
    }

    #[test]
    fn persistent_mob_never_despawns() {
        let mut m = fresh_mob(true);
        // Way outside the instant-despawn distance.
        let d2 = (INSTANT_DESPAWN_DIST * 2.0).powi(2);
        assert!(!should_despawn(&mut m, Some(d2), 1.0));
    }

    #[test]
    fn natural_mob_instant_despawns_outside_far_dist() {
        let mut m = fresh_mob(false);
        let d2 = (INSTANT_DESPAWN_DIST + 1.0).powi(2);
        assert!(should_despawn(&mut m, Some(d2), 1.0));
    }

    #[test]
    fn natural_mob_does_not_despawn_when_close() {
        let mut m = fresh_mob(false);
        let d2 = (FAR_DESPAWN_DIST - 1.0).powi(2);
        // Should not despawn + the timer should reset.
        assert!(!should_despawn(&mut m, Some(d2), 5.0));
        assert!((m.far_despawn_timer - FAR_DESPAWN_TIMER).abs() < 0.01);
    }

    #[test]
    fn far_band_ticks_down_timer() {
        let mut m = fresh_mob(false);
        let d2 = ((FAR_DESPAWN_DIST + INSTANT_DESPAWN_DIST) / 2.0).powi(2);
        // First tick: timer goes from 30 -> 25.
        assert!(!should_despawn(&mut m, Some(d2), 5.0));
        assert!((m.far_despawn_timer - (FAR_DESPAWN_TIMER - 5.0)).abs() < 0.01);
    }

    #[test]
    fn far_band_despawns_after_full_timer() {
        let mut m = fresh_mob(false);
        let d2 = ((FAR_DESPAWN_DIST + INSTANT_DESPAWN_DIST) / 2.0).powi(2);
        // Burn the entire 30-second timer in one go.
        let despawned = should_despawn(&mut m, Some(d2), FAR_DESPAWN_TIMER + 1.0);
        assert!(despawned, "far_despawn_timer = {}", m.far_despawn_timer);
    }

    #[test]
    fn timer_resets_when_player_returns_to_close_band() {
        let mut m = fresh_mob(false);
        let d2_far = ((FAR_DESPAWN_DIST + INSTANT_DESPAWN_DIST) / 2.0).powi(2);
        // Player drifts out: timer counts down by 10.
        let _ = should_despawn(&mut m, Some(d2_far), 10.0);
        assert!((m.far_despawn_timer - (FAR_DESPAWN_TIMER - 10.0)).abs() < 0.01);
        // Player returns: timer should reset to FAR_DESPAWN_TIMER.
        let d2_close = (FAR_DESPAWN_DIST - 1.0).powi(2);
        let _ = should_despawn(&mut m, Some(d2_close), 1.0);
        assert!((m.far_despawn_timer - FAR_DESPAWN_TIMER).abs() < 0.01);
    }

    #[test]
    fn no_players_means_no_despawn() {
        let mut m = fresh_mob(false);
        // None of the distance values are populated.
        assert!(!should_despawn(&mut m, None, 100.0));
    }

    #[test]
    fn boundary_at_instant_dist_does_not_despawn() {
        // Exactly at the boundary: not strictly > INSTANT_DIST^2.
        let mut m = fresh_mob(false);
        let d2 = (INSTANT_DESPAWN_DIST).powi(2);
        // d2 == INSTANT_DESPAWN_DIST^2, so the > check fails; the
        // mob is treated as "in the far band" and only ticks the
        // timer.
        assert!(!should_despawn(&mut m, Some(d2), 1.0));
    }

    #[test]
    fn persistent_flag_survives_a_natural_check() {
        // Persistence flag is read-only - never cleared by the
        // despawn pass. Sanity guard so a future refactor doesn't
        // accidentally reset it.
        let mut m = fresh_mob(true);
        let _ = should_despawn(&mut m, Some(100.0), 1.0);
        assert!(m.persistent);
    }

    #[test]
    fn default_natural_mob_starts_with_full_timer() {
        let m = fresh_mob(false);
        assert!((m.far_despawn_timer - FAR_DESPAWN_TIMER).abs() < 0.01);
        assert!(!m.persistent);
    }
}
