//! Light-level gates for mob spawning.
//!
//! Mirrors MC's `Monster::isDarkEnoughToSpawn` (hostile gate) +
//! `Animal::checkAnimalSpawnRules` (passive gate). Both rules
//! consult the per-voxel block + sky light layers to decide whether
//! the candidate cell is "dark enough" or "bright enough" for the
//! kind being placed.
//!
//! Cited MC source files:
//! - `net/minecraft/world/entity/monster/Monster.java::isDarkEnoughToSpawn`
//!   - the three-clause dark test ported in
//!     `is_dark_enough_for_hostile` (see the mapping comment there).
//! - `net/minecraft/world/entity/animal/Animal.java::checkAnimalSpawnRules`
//!   - `getRawBrightness(pos, 0) > 8` (the spawnable-floor half
//!     lives in `spawn_rules::creature_floor_ok`).
//!
//! All free fns over `(&Voxel, [i32; 3])` so they compose into the
//! spawn loop in `spawn.rs` without coupling to the AI tick body.

use infengine_voxel::CHUNK_SIZE;

use crate::DAY_LENGTH_S;
use crate::Voxel;
use crate::lighting::compute_day_factor;

use super::spawn::SpawnRng;

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
/// day-factor ramp. The engine's analog of MC's skyDarken-adjusted
/// sky channel; also feeds the slime / bat local-brightness rules.
pub(crate) fn effective_sky_light(voxel: &Voxel, wpos: [i32; 3]) -> f32 {
    let raw = sky_light_at(voxel, wpos) as f32;
    raw * compute_day_factor(voxel.day_time, DAY_LENGTH_S)
}

/// W479: full `Monster.isDarkEnoughToSpawn` port.
///
/// MC -> engine light-model mapping (the engine has raw sky 0-15,
/// raw block 0-15 and a multiplicative day-factor ramp 1.0 noon ->
/// 0.04 midnight, where MC has raw sky, raw block and a SUBTRACTIVE
/// skyDarken 0 day -> 11 midnight; 10 while thundering):
/// 1. `skyLight > nextInt(32)` rejects  ->  identical, raw sky
///    channel vs `rng % 32` (a sky-exposed cell is probabilistically
///    penalised even at night, exactly the MC roll);
/// 2. `blockLight > monsterSpawnBlockLightLimit` rejects  ->  the
///    overworld limit is 0, so ANY block light kills the spawn
///    (torch-lit = never). Ceiled dims never reach this fn (their
///    MC limit is 15 = no check; the callers keep the `ceiled ||`
///    bypass so nether spawning is unchanged);
/// 3. `max(blockLight, skyLight - skyDarken) <= uniform(0..7)
///    sample`  ->  darkened sky = `sky * day_factor` (thundering
///    clamps the factor to the midnight floor, MC's amount=10
///    thunder case), brightness = `max(block, round(darkened))`,
///    pass iff `brightness <= rng % 8`.
///
/// Net effects: noon surface (sky 15) fails clause 3
/// deterministically; midnight surface passes probabilistically
/// (MC-like); caves (sky 0, block 0) pass at ANY hour; block light
/// >= 1 fails clause 2 deterministically.
pub(crate) fn is_dark_enough_for_hostile(
    voxel: &Voxel,
    wpos: [i32; 3],
    rng: &mut SpawnRng<'_>,
) -> bool {
    let sky = sky_light_at(voxel, wpos);
    if sky as u32 > rng.next_u32() % 32 {
        return false;
    }
    let block = block_light_at(voxel, wpos);
    if block > 0 {
        return false;
    }
    let mut factor = compute_day_factor(voxel.day_time, DAY_LENGTH_S);
    if voxel.thundering {
        // MC thunder case: sample as if skyDarken were 10 (storm
        // light ~= night). The engine's midnight factor is the
        // equivalent floor of its multiplicative model.
        factor = factor.min(compute_day_factor(DAY_LENGTH_S * 0.75, DAY_LENGTH_S));
    }
    let darkened = (sky as f32 * factor).round().clamp(0.0, 15.0) as u32;
    let brightness = (block as u32).max(darkened);
    brightness <= rng.next_u32() % 8
}

/// MC's `Animal::checkAnimalSpawnRules` light half:
/// `getRawBrightness(pos, 0) > 8` = the brighter of the raw sky and
/// block channels must exceed 8 (a torch-lit night pen spawns
/// animals in MC too - W479 aligned the engine from sky-only to the
/// max of both channels). The floor half lives in
/// `spawn_rules::creature_floor_ok` (per-kind MC block tags).
pub(crate) fn is_bright_enough_for_passive(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    sky_light_at(voxel, wpos).max(block_light_at(voxel, wpos)) > 8
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
    //! loop - they verify the gate predicates in isolation. The
    //! hostile gate carries MC's probabilistic rolls, so positive
    //! cases sweep seeds (`any`) and negative cases assert `never`.

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

    fn hostile_over_seeds(v: &Voxel, w: [i32; 3]) -> (bool, bool) {
        let mut any = false;
        let mut all = true;
        for s in 1..128u32 {
            let mut seed = s.wrapping_mul(0x9E37_79B9);
            let mut rng = SpawnRng::new(&mut seed);
            let ok = is_dark_enough_for_hostile(v, w, &mut rng);
            any |= ok;
            all &= ok;
        }
        (any, all)
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
    fn hostile_dark_cave_passes_at_noon_deterministically() {
        // Clause-by-clause: sky 0 -> clause 1 never rejects; block 0
        // -> clause 2 passes; brightness 0 <= any sample -> clause 3
        // passes. A dark cave spawns monsters at ANY hour (the W479
        // behaviour change vs the old day/night clock).
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon
        stamp_light(&mut v, [0, 30, 0], 0, 0);
        let (_, all) = hostile_over_seeds(&v, [0, 30, 0]);
        assert!(all, "dark cave must pass for every roll");
    }

    #[test]
    fn hostile_any_block_light_fails_deterministically() {
        // MC 1.18+ rule: overworld monsterSpawnBlockLightLimit = 0,
        // so even block light 1 kills the spawn (7 passed pre-W479).
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        for bl in [1u8, 7, 8, 14] {
            stamp_light(&mut v, [0, 50, 0], bl, 0);
            let (any, _) = hostile_over_seeds(&v, [0, 50, 0]);
            assert!(!any, "block light {bl} must never spawn");
        }
    }

    #[test]
    fn hostile_full_daylight_surface_never_passes() {
        // Noon + sky 15: darkened sky = 15 > 7 -> clause 3 rejects
        // every roll (clause 1 also rejects ~half).
        let mut v = Voxel::new();
        v.day_time = 0.0;
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        let (any, _) = hostile_over_seeds(&v, [0, 50, 0]);
        assert!(!any, "noon surface must never spawn");
    }

    #[test]
    fn hostile_midnight_surface_passes_for_some_rolls() {
        // Midnight: darkened sky ~= 15 * 0.04 -> brightness ~1, so
        // clause 3 passes 7/8 and clause 1 passes 17/32 - the night
        // surface spawns, probabilistically like MC.
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75;
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        let (any, all) = hostile_over_seeds(&v, [0, 50, 0]);
        assert!(any, "midnight surface must spawn for some rolls");
        assert!(!all, "sky-exposed cells keep the MC nextInt(32) roll");
    }

    #[test]
    fn hostile_thunder_darkens_the_day_sky() {
        // Thundering at noon: the factor clamps to the midnight
        // floor (MC's amount=10 thunder sample), so a sky-15 surface
        // behaves like night instead of deterministically rejecting.
        let mut v = Voxel::new();
        v.day_time = 0.0;
        v.thundering = true;
        stamp_light(&mut v, [0, 50, 0], 0, 15);
        let (any, _) = hostile_over_seeds(&v, [0, 50, 0]);
        assert!(any, "storm-dark noon surface must spawn for some rolls");
    }

    #[test]
    fn passive_spawn_at_sky_light_9_passes() {
        // MC gate: raw brightness > 8. Boundary value via sky.
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
    fn passive_spawn_accepts_block_light_channel() {
        // W479: `getRawBrightness(pos, 0)` is the max of BOTH
        // channels - a torch-lit night pen (block 9+, sky 0) spawns
        // animals, exactly like MC.
        let mut v = Voxel::new();
        stamp_light(&mut v, [0, 50, 0], 9, 0);
        assert!(is_bright_enough_for_passive(&v, [0, 50, 0]));
        stamp_light(&mut v, [0, 50, 0], 8, 0);
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
