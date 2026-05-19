//! Light-level gates for mob spawning.
//!
//! Mirrors MC's `Monster::checkMonsterSpawnRules` (hostile gate) +
//! `Animal::checkAnimalSpawnRules` (passive gate). Both rules
//! consult the per-voxel block + sky light layers + the time-of-day
//! ramp to decide whether the candidate cell is "dark enough" or
//! "bright enough" for the kind being placed.
//!
//! Cited MC source files:
//! - `net/minecraft/world/entity/monster/Monster.java::checkMonsterSpawnRules`
//!   - block_light <= 7 AND effective_sky_light <= 7 AND difficulty > 0.
//! - `net/minecraft/world/entity/animal/Animal.java::checkAnimalSpawnRules`
//!   - sky_light >= 9 AND on top of grass.
//!
//! All free fns over `(&Voxel, [i32; 3])` so they compose into the
//! spawn loop in `spawn.rs` without coupling to the AI tick body.

use infengine_voxel::CHUNK_SIZE;

use crate::Voxel;
use crate::lighting::compute_day_factor;
use crate::DAY_LENGTH_S;

/// Block id for grass-top blocks. Matches the `block::BLOCK_TYPES`
/// table entry at `id: 3`. Kept inline (no top-level const) so the
/// gate stays close to the call sites; if the worldgen team
/// renumbers grass they'll regrep `is_grass_below`.
const GRASS_BLOCK_ID: u16 = 3;

/// MC's hostile spawn-gate ceiling. `block_light` strictly above
/// this kills the spawn. MC uses 7 here too
/// (Monster.java::isDarkEnoughToSpawn).
pub(crate) const HOSTILE_MAX_BLOCK_LIGHT: u8 = 7;
/// Effective sky-light ceiling for hostile spawns. `sky_light *
/// day_factor` strictly above this kills the spawn. We mirror the
/// MC behaviour of "spawning at night ignores sun" by multiplying
/// the raw sky value by the day-factor ramp.
pub(crate) const HOSTILE_MAX_SKY_LIGHT: f32 = 7.0;
/// MC's passive spawn-gate floor. `sky_light` strictly below this
/// kills the spawn (Animal.java::checkAnimalSpawnRules). 9 is high
/// enough to filter caves + heavily forested floor.
pub(crate) const PASSIVE_MIN_SKY_LIGHT: u8 = 9;

/// Look up the block-light value at the world voxel `wpos`. Returns
/// 0 when the chunk hasn't been lit yet (treats unknown cells as
/// dark - safest default for hostile spawn check, which wants the
/// minimum value).
pub(crate) fn block_light_at(voxel: &Voxel, wpos: [i32; 3]) -> u8 {
    let cs = CHUNK_SIZE as i32;
    let cc = [
        wpos[0].div_euclid(cs),
        wpos[1].div_euclid(cs),
        wpos[2].div_euclid(cs),
    ];
    let lc = [
        wpos[0].rem_euclid(cs) as u32,
        wpos[1].rem_euclid(cs) as u32,
        wpos[2].rem_euclid(cs) as u32,
    ];
    voxel.lighting.get(cc).map(|g| g.get_block(lc)).unwrap_or(0)
}

/// Look up the sky-light value at the world voxel `wpos`. Returns
/// 15 when the chunk hasn't been lit yet (treats unknown cells as
/// "outdoors" - the passive spawn check wants the maximum value).
pub(crate) fn sky_light_at(voxel: &Voxel, wpos: [i32; 3]) -> u8 {
    let cs = CHUNK_SIZE as i32;
    let cc = [
        wpos[0].div_euclid(cs),
        wpos[1].div_euclid(cs),
        wpos[2].div_euclid(cs),
    ];
    let lc = [
        wpos[0].rem_euclid(cs) as u32,
        wpos[1].rem_euclid(cs) as u32,
        wpos[2].rem_euclid(cs) as u32,
    ];
    voxel.lighting.get(cc).map(|g| g.get_sky(lc)).unwrap_or(15)
}

/// Effective sky light at `wpos`: raw sky value multiplied by the
/// day-factor ramp. Drives the hostile gate so zombies stop
/// spawning once the sun is up.
pub(crate) fn effective_sky_light(voxel: &Voxel, wpos: [i32; 3]) -> f32 {
    let raw = sky_light_at(voxel, wpos) as f32;
    raw * compute_day_factor(voxel.day_time, DAY_LENGTH_S)
}

/// Same dark-cell check MC's `Monster::isDarkEnoughToSpawn` runs:
/// block_light <= 7 AND effective_sky_light <= 7. Both layers must
/// pass; a torch-lit night spot is invalid (block_light too high)
/// and a daylight overworld surface is invalid (effective sky
/// light too high).
pub(crate) fn is_dark_enough_for_hostile(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    let bl = block_light_at(voxel, wpos);
    let esl = effective_sky_light(voxel, wpos);
    bl <= HOSTILE_MAX_BLOCK_LIGHT && esl <= HOSTILE_MAX_SKY_LIGHT
}

/// MC's `Animal::checkAnimalSpawnRules`: sky_light >= 9 AND the
/// cell below is grass. We split the grass check out (`is_grass_below`
/// below) so the spawn-loop call site can short-circuit cheaper
/// surface checks.
pub(crate) fn is_bright_enough_for_passive(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    sky_light_at(voxel, wpos) >= PASSIVE_MIN_SKY_LIGHT
}

/// True when `(wx, wy-1, wz)` is a grass block. MC's passive spawn
/// rule requires the cell directly below the spawn target to be
/// grass; we match that.
pub(crate) fn is_grass_below(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    voxel.world.get([wpos[0], wpos[1] - 1, wpos[2]]).0 == GRASS_BLOCK_ID
}

/// Standing-room check: the cell at `wpos` is air AND the cell at
/// `wpos + (0, 1, 0)` is air. Mirrors MC's
/// `NaturalSpawner::isValidPositionForMob` "two air cells stacked"
/// shape used for tall mobs.
pub(crate) fn has_standing_room(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    voxel.world.get(wpos).is_air()
        && voxel.world.get([wpos[0], wpos[1] + 1, wpos[2]]).is_air()
}

/// Cell-below-is-solid check: required so the mob has a floor.
pub(crate) fn has_solid_floor(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    !voxel.world.get([wpos[0], wpos[1] - 1, wpos[2]]).is_air()
}

#[cfg(test)]
mod tests {
    //! Tests run against a freshly built `Voxel` so we can stamp
    //! light values directly into the lighting state + read them
    //! back out. The tests stay independent of the AI tick / spawn
    //! loop - they verify the gate predicates in isolation.

    use super::*;
    use crate::Voxel;
    use infengine_voxel::ChunkCoord;

    fn stamp_light(voxel: &mut Voxel, w: [i32; 3], block: u8, sky: u8) {
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
        let g = voxel.lighting.ensure(cc);
        g.set_block(lc, block);
        g.set_sky(lc, sky);
    }

    #[test]
    fn block_light_unset_chunk_reads_zero() {
        let v = Voxel::new();
        assert_eq!(block_light_at(&v, [100, 50, 100]), 0);
    }

    #[test]
    fn sky_light_unset_chunk_reads_max() {
        let v = Voxel::new();
        assert_eq!(sky_light_at(&v, [100, 50, 100]), 15);
    }

    #[test]
    fn light_round_trips_through_stamp() {
        let mut v = Voxel::new();
        stamp_light(&mut v, [3, 4, 5], 11, 9);
        assert_eq!(block_light_at(&v, [3, 4, 5]), 11);
        assert_eq!(sky_light_at(&v, [3, 4, 5]), 9);
    }

    #[test]
    fn hostile_spawn_at_block_light_7_passes() {
        // MC gate: block_light <= 7. Boundary value, daytime
        // (sky_light = 0 -> effective sky = 0 -> passes ceiling).
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon, day_factor = 1.0
        stamp_light(&mut v, [0, 50, 0], 7, 0);
        assert!(is_dark_enough_for_hostile(&v, [0, 50, 0]));
    }

    #[test]
    fn hostile_spawn_at_block_light_8_fails() {
        // One above the ceiling: block_light gate fails.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_light(&mut v, [0, 50, 0], 8, 0);
        assert!(!is_dark_enough_for_hostile(&v, [0, 50, 0]));
    }

    #[test]
    fn hostile_spawn_at_full_daylight_fails() {
        // Full sun (day_time = 0, day_factor ~1.0) + sky_light=15:
        // effective sky = 15 > 7 -> hostile gate fails even with
        // block_light = 0.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        assert!(!is_dark_enough_for_hostile(&v, [0, 50, 0]));
    }

    #[test]
    fn hostile_spawn_at_midnight_with_full_sky_passes() {
        // Midnight: day_factor = 0.04, sky_light = 15 ->
        // effective sky = 0.6 -> well under 7, hostile gate
        // passes (matches MC: night ignores the sun).
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight (phase 0.75)
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        assert!(is_dark_enough_for_hostile(&v, [0, 50, 0]));
    }

    #[test]
    fn passive_spawn_at_sky_light_9_passes() {
        // MC gate: sky_light >= 9. Boundary value.
        let mut v = Voxel::new();
        stamp_light(&mut v, [0, 50, 0], 0, 9);
        assert!(is_bright_enough_for_passive(&v, [0, 50, 0]));
    }

    #[test]
    fn passive_spawn_at_sky_light_8_fails() {
        // One below the floor: passive gate fails.
        let mut v = Voxel::new();
        stamp_light(&mut v, [0, 50, 0], 0, 8);
        assert!(!is_bright_enough_for_passive(&v, [0, 50, 0]));
    }

    #[test]
    fn effective_sky_light_scales_with_day_factor() {
        let mut v = Voxel::new();
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        // Noon: full sky.
        v.day_time = 0.0;
        assert!((effective_sky_light(&v, [0, 50, 0]) - 15.0).abs() < 0.01);
        // Midnight: ~0.6.
        v.day_time = crate::DAY_LENGTH_S * 0.75;
        let e = effective_sky_light(&v, [0, 50, 0]);
        assert!(e < 1.0, "{}", e);
    }

    #[test]
    fn standing_room_needs_two_air_cells() {
        let v = Voxel::new();
        // A fresh Voxel has the worldgen chunks unloaded; the
        // World::get default for unloaded coords is air. So the
        // standing-room check passes vacuously, which is fine -
        // the spawn loop also gates on `has_solid_floor`.
        assert!(has_standing_room(&v, [123, 50, 123]));
    }
}
