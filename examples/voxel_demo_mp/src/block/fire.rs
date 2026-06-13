//! Fire random-tick: spread + age + extinguish (Q.4c).
//!
//! Direct port of MC `FireBlock.java::tick` + the helpers
//! `getIgniteOdds`, `getBurnOdds`, `checkBurnOut`, and
//! `isValidFireLocation` from the 26.1.2 decompile (lines
//! 134..=277 of `FireBlock.java`).
//!
//! ## Determinism
//!
//! The tick path uses a hash-based gate, NOT `rand::*`. Every
//! random decision feeds `hash3_for(pos, fire_clock, salt)` so
//! two peers running the same world produce identical fire
//! behaviour at the same tick. Mirrors the `worldgen::hash2_for`
//! pattern already in the codebase.
//!
//! ## Rate
//!
//! MC's per-chunk-section random-tick rate is `1/4096` per game
//! tick. That's too sparse for our smaller demo worlds; we use
//! `1/30` per net-tick frame per fire cell. With a ~60Hz net
//! tick that lands a tick every ~0.5s of wall time - readable
//! spread that visibly progresses over ~10..30s.
//!
//! ## Algorithm overview
//!
//! For each fire cell whose gate fires:
//!   1. Bump age by `0..=1` (random nibble of the hash).
//!   2. If no flammable cell within the 1-block neighbourhood
//!      and no solid support directly below, extinguish (set to
//!      air).
//!   3. If `age >= 15` AND no flammable below, 25% chance to
//!      extinguish.
//!   4. Run `check_burn_out` on the 6 face neighbours: each
//!      flammable neighbour rolls `getBurnOdds / 300` and either
//!      replaces with fire (catches) or air (consumed).
//!   5. Run the 3x3x6 spread scan: for each cell in the
//!      neighbourhood (xx,zz in -1..=1, yy in -1..=4 minus self),
//!      if it's air with a flammable face neighbour, roll
//!      `(igniteOdds + 40) / (age + 30)` against `rate`. Successful
//!      rolls set the cell to fire at a derived age.
//!
//! ## Flammability table
//!
//! Per-block `(ignite_odds, burn_odds)` derived from MC
//! `FireBlock.bootStrap` (lines 299..=454). Our palette only
//! contains a handful of MC's blocks; everything missing
//! returns `(0, 0)` and stays unburnable.

use std::collections::HashMap;

use infengine_voxel::World;

use super::{F_AABB_PASS, F_RAY_PASS, FIRE_ID, block_has_flag};

#[cfg(test)]
mod tests;

/// Random-tick rate: 1 in `RATE_DENOM` net-tick frames per fire
/// cell. MC uses 1/4096 per chunk section; we trade fidelity for
/// readability in a smaller world. W159: 30 -> 12 so the MC-faithful
/// catch odds (logs ~1.7%/tick) compound fast enough that a lit tree
/// visibly burns down + spreads in seconds instead of guttering out
/// after one voxel (the user's "shori kousek a hned zhasne" report).
pub(crate) const RATE_DENOM: u32 = 12;

/// Hash three i32s + a u64 + a salt into a u32. Mirror of the
/// 2D `worldgen::hash2_for` shape, extended for fire ticks.
/// Caller picks `salt` to separate independent random decisions
/// at the same cell + tick (age bump vs catch-burn vs spread).
pub(crate) fn hash3_for(pos: [i32; 3], clock: u64, salt: u32) -> u32 {
    let mut h = (pos[0] as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (pos[1] as u32).wrapping_mul(0x1656_67B1);
    h ^= (pos[2] as u32).wrapping_mul(0x85EB_CA6B);
    h ^= clock as u32;
    h ^= (clock >> 32) as u32;
    h ^= salt;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h.wrapping_mul(0xC2B2_AE35)
}

/// 1-in-N gate for lava random-tick ignite checks (Q.4d). MC
/// reference: `LavaFluid.randomTick` runs at the random-tick
/// rate (1/4096 per chunk section per game tick). MC's per-section
/// rate translates to ~3 attempts per source per second; our
/// smaller worlds keep the gate per-cell. W8.4 dropped the gate
/// from 60 -> 20 so a lava bucket dumped on flat grass visibly
/// ignites at least one neighbour within ~10..20 seconds of wall
/// time (the previous 60 left lava cells sitting on grass without
/// any visible reaction).
pub(crate) const LAVA_IGNITE_RATE_DENOM: u32 = 20;

/// Per-block flammability lookup. Returns `(ignite_odds,
/// burn_odds)` matching MC's `FireBlock::bootStrap()` setup.
///
/// W11.1: the source-of-truth table moved to
/// [`super::flammability::MC_FLAMMABLES`] - a 1:1 transcription
/// of MC `FireBlock.java::bootStrap()` (lines 299..=508).
/// This wrapper just forwards to the engine-id resolver.
pub(crate) fn block_flammability(id: u16) -> (u32, u32) {
    super::flammability::flammability_for_id(id)
}

/// Max `ignite_odds` across the 6 face neighbours of `pos`.
/// Mirrors MC's `getIgniteOdds(level, pos)` (line 259..=271 of
/// `FireBlock.java`): the strongest flammable face determines
/// whether a spread roll fires at this cell.
fn neighbour_max_ignite_odds(world: &World, pos: [i32; 3]) -> u32 {
    let dirs: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [0, 1, 0],
        [0, -1, 0],
    ];
    let mut max = 0u32;
    for d in &dirs {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        let nb = world.get(n);
        let (ig, _) = block_flammability(nb.0);
        if ig > max {
            max = ig;
        }
    }
    max
}

/// True if at least one face neighbour of `pos` is flammable.
/// MC's `isValidFireLocation`.
pub(crate) fn has_flammable_neighbour(world: &World, pos: [i32; 3]) -> bool {
    neighbour_max_ignite_odds(world, pos) > 0
}

/// True if `pos` is supported: either a solid block directly
/// below, or at least one flammable face neighbour. Mirrors
/// MC's `canSurvive` (line 129..=132).
pub(crate) fn has_fire_support(world: &World, pos: [i32; 3]) -> bool {
    let below = world.get([pos[0], pos[1] - 1, pos[2]]);
    // Solid support: any non-air, non-aabb-passable, non-ray-passable
    // block beneath. Decorations + air don't count.
    let below_solid = !below.is_air()
        && !block_has_flag(below, F_AABB_PASS)
        && !block_has_flag(below, F_RAY_PASS);
    if below_solid {
        return true;
    }
    has_flammable_neighbour(world, pos)
}

/// Result of one fire cell's tick. The caller (Voxel) commits
/// the diff against `fires` + `world` + broadcast paths.
#[derive(Debug, Default, Clone)]
pub(crate) struct FireStep {
    /// Fire cells that go out this tick (no support / aged out).
    /// Caller sets the world block to air + drops `fires[pos]`.
    pub(crate) extinguish: Vec<[i32; 3]>,
    /// Existing fire cells whose age changed. `(pos, new_age)`.
    pub(crate) age_bump: Vec<([i32; 3], u8)>,
    /// New fire cells to place. `(pos, age)`. Caller sets the
    /// world block to FIRE_ID + inserts into `fires`. Also used
    /// when a check_burn_out "catches" - the formerly-flammable
    /// block is replaced by fire.
    pub(crate) ignite: Vec<([i32; 3], u8)>,
    /// Flammable cells that burned to air (check_burn_out's
    /// consume branch). Caller sets the world block to air.
    pub(crate) consume: Vec<[i32; 3]>,
}

/// Try to ignite `pos` as a fire cell. Returns the starting age
/// (0) if the cell is air, has support, and isn't already on
/// fire; returns `None` otherwise. Pure on the world; caller
/// commits.
#[allow(dead_code)] // wired up by Q.4e flint+steel + Q.4d lava ignite
pub(crate) fn try_ignite_block(world: &World, pos: [i32; 3]) -> Option<u8> {
    let here = world.get(pos);
    if here.0 == FIRE_ID {
        return None;
    }
    if !here.is_air() {
        return None;
    }
    if !has_fire_support(world, pos) {
        return None;
    }
    Some(0)
}

/// Plan a single fire cell's tick. Pure on the world + fire age;
/// the caller commits `FireStep`. Mirrors MC's
/// `FireBlock.tick(state, level, pos, random)`; every
/// `RandomSource` call is replaced with a `hash3_for(pos,
/// fire_clock, salt)` so the result is deterministic per-tick.
pub(crate) fn plan_fire_cell(
    world: &World,
    fires: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    fire_clock: u64,
    step: &mut FireStep,
) {
    // Defensive: cell may have been overwritten since it was
    // queued for a tick.
    if world.get(pos).0 != FIRE_ID {
        return;
    }
    let age = fires.get(&pos).copied().unwrap_or(0);
    let supports = has_fire_support(world, pos);
    let has_flammable = has_flammable_neighbour(world, pos);
    if !supports {
        // No solid support AND no flammable neighbour -> die.
        // MC's `canSurvive` returns false for the same case.
        step.extinguish.push(pos);
        return;
    }
    // Bump age by 0..=1 (cheap hash).
    let h_age = hash3_for(pos, fire_clock, 0xAA);
    let new_age = (age as u32 + (h_age & 1)).min(15) as u8;
    if new_age != age {
        step.age_bump.push((pos, new_age));
    }
    // `age >= 15` + no flammable below -> 25% extinguish chance.
    if new_age >= 15 {
        let below = world.get([pos[0], pos[1] - 1, pos[2]]);
        let (below_ig, _) = block_flammability(below.0);
        if below_ig == 0 {
            let h_die = hash3_for(pos, fire_clock, 0xBB);
            if h_die & 0x3 == 0 {
                step.extinguish.push(pos);
                return;
            }
        }
    }
    // No flammable neighbour at all -> 33% chance to die per
    // tick. MC's `canSurvive` covers no-support; this handles
    // "support OK but isolated".
    if !has_flammable {
        let h_iso = hash3_for(pos, fire_clock, 0xCC);
        if h_iso.is_multiple_of(3) {
            step.extinguish.push(pos);
        }
        return;
    }
    // Burn-out the 6 face neighbours. Each flammable face
    // neighbour rolls `burn / 300`; on success, catch OR consume.
    let face_dirs: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [0, 1, 0],
        [0, -1, 0],
    ];
    for (i, d) in face_dirs.iter().enumerate() {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        check_burn_out(world, n, fire_clock, new_age, i as u32, step);
    }
    // Spread to nearby air cells. 3x3 horizontal x (-1..=4) Y.
    // MC weights upward cells (`yy > 1` increases the rate).
    for xx in -1..=1 {
        for zz in -1..=1 {
            for yy in -1..=4 {
                if xx == 0 && yy == 0 && zz == 0 {
                    continue;
                }
                let n = [pos[0] + xx, pos[1] + yy, pos[2] + zz];
                let h = world.get(n);
                if !h.is_air() {
                    continue;
                }
                let ig = neighbour_max_ignite_odds(world, n);
                if ig == 0 {
                    continue;
                }
                let rate: u32 = if yy > 1 {
                    100 + (yy as u32 - 1) * 100
                } else {
                    100
                };
                let odds = (ig + 40) / (new_age as u32 + 30);
                if odds == 0 {
                    continue;
                }
                let salt = 0xD000
                    ^ ((xx + 1) as u32 * 9 + (yy + 1) as u32 * 3 + (zz + 1) as u32);
                let h_spread = hash3_for(pos, fire_clock, salt);
                if (h_spread % rate) <= odds {
                    let spread_age =
                        (new_age as u32 + ((h_spread >> 8) % 5) / 4).min(15) as u8;
                    step.ignite.push((n, spread_age));
                }
            }
        }
    }
}

/// MC `LavaFluid.randomTick(level, pos, fluidState, random)`
/// (lines 78..=114 of `LavaFluid.java`). Direct port of the
/// two-branch algorithm:
///
///   passes = rand[0..3];
///   if passes > 0:
///     for pass in 0..passes:
///       testPos.offset(rand[-1,0,1], 1, rand[-1,0,1])
///       if air AND has flammable neighbour:
///         set FIRE; return
///   else:
///     for i in 0..3:
///       offset(rand[-1,0,1], 0, rand[-1,0,1])
///       if pos.above is air AND pos is flammable:
///         set pos.above to FIRE
///
/// `lava_pos` must be a lava source cell; the caller is expected
/// to gate the call on `1/LAVA_IGNITE_RATE_DENOM` first. `step`
/// accumulates the resulting `ignite` entries.
pub(crate) fn plan_lava_random_tick(
    world: &World,
    lava_pos: [i32; 3],
    fire_clock: u64,
    step: &mut FireStep,
) {
    // Pass count: 0, 1, or 2. Salt 0x100 separates from the
    // fire-tick salts.
    let h_passes = hash3_for(lava_pos, fire_clock, 0x100);
    let passes = h_passes % 3;
    if passes > 0 {
        // Upward drift branch. Each pass jitters the test pos by
        // (rand[-1,0,1], 1, rand[-1,0,1]) and ignites the first
        // air cell with a flammable neighbour.
        let mut test_pos = lava_pos;
        for pass in 0..passes {
            let h_x = hash3_for(lava_pos, fire_clock, 0x110 + pass);
            let h_z = hash3_for(lava_pos, fire_clock, 0x120 + pass);
            let dx = (h_x % 3) as i32 - 1;
            let dz = (h_z % 3) as i32 - 1;
            test_pos = [test_pos[0] + dx, test_pos[1] + 1, test_pos[2] + dz];
            let here = world.get(test_pos);
            if !here.is_air() {
                // MC: if the cell blocksMotion, return (the lava
                // can't punch through). Solid block - bail.
                if !block_has_flag(here, F_AABB_PASS)
                    && !block_has_flag(here, F_RAY_PASS)
                {
                    return;
                }
                continue;
            }
            if has_flammable_neighbour(world, test_pos) {
                step.ignite.push((test_pos, 0));
                return;
            }
        }
    } else {
        // Horizontal sweep: 3 attempts to find a flammable block
        // with empty air above; ignite the air cell.
        for i in 0..3u32 {
            let h_x = hash3_for(lava_pos, fire_clock, 0x130 + i);
            let h_z = hash3_for(lava_pos, fire_clock, 0x140 + i);
            let dx = (h_x % 3) as i32 - 1;
            let dz = (h_z % 3) as i32 - 1;
            let test_pos = [lava_pos[0] + dx, lava_pos[1], lava_pos[2] + dz];
            let above = [test_pos[0], test_pos[1] + 1, test_pos[2]];
            if !world.get(above).is_air() {
                continue;
            }
            let (ig, _) = block_flammability(world.get(test_pos).0);
            if ig > 0 {
                step.ignite.push((above, 0));
            }
        }
    }
}

/// MC `FireBlock.checkBurnOut(level, pos, chance, random, age)`.
/// A flammable face neighbour rolls `burn_odds / chance`; on
/// success, either ignites (replaces with fire) or turns to air.
fn check_burn_out(
    world: &World,
    pos: [i32; 3],
    fire_clock: u64,
    age: u8,
    salt_extra: u32,
    step: &mut FireStep,
) {
    let nb = world.get(pos);
    let (_, burn) = block_flammability(nb.0);
    if burn == 0 {
        return;
    }
    // MC default per face: 300; we keep it (the
    // INCREASED_FIRE_BURNOUT env attribute is not in our world
    // model so we skip the -50 reduction).
    let chance: u32 = 300;
    let h_burn = hash3_for(pos, fire_clock, 0xE000 ^ salt_extra);
    if (h_burn % chance) >= burn {
        return;
    }
    // Successful burn check. MC: `random.nextInt(age + 10) < 5`
    // catches fire; else burns to air.
    let h_role = hash3_for(pos, fire_clock, 0xF000 ^ salt_extra);
    let ten_plus_age = age as u32 + 10;
    if (h_role % ten_plus_age) < 5 {
        // Catch: replace with fire.
        let new_age = (age as u32 + ((h_burn >> 16) % 5) / 4).min(15) as u8;
        step.ignite.push((pos, new_age));
    } else {
        // Consume to air.
        step.consume.push(pos);
    }
}
