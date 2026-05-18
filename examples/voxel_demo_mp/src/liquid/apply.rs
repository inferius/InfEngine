//! Apply a `LiquidPlanStep` to the world. Mutates the world / level
//! map / active / positions sets and returns a `LiquidTape` describing
//! every concrete change (placed / evaporated / decayed / washed). The
//! caller drives mesh-dirty / broadcasts / drop spawns from the tape.

use std::collections::HashSet;

use infengine_voxel::{BlockId, World};

use crate::block::block_is_washable;

use super::Liquid;
use super::plan::LiquidPlanStep;

/// Side-effect-free record of what `liquid_apply_plan` did. Caller
/// (Voxel or test sim) drives mesh-dirty / broadcast / drop spawns
/// from this.
#[derive(Debug, Default, Clone)]
pub(crate) struct LiquidTape {
    /// `(replaced_block, pos)` for washable cells overwritten by
    /// liquid. The applier already cleared them from `World`; the
    /// caller is expected to spawn drops + clean up registries.
    pub(crate) washed: Vec<(BlockId, [i32; 3])>,
    /// Cells now holding `liquid_id` at the listed level.
    /// `(pos, liquid_id, level)`.
    pub(crate) placed: Vec<([i32; 3], u16, u8)>,
    /// Cells now air. The applier removed them from level/active.
    pub(crate) evaporated: Vec<[i32; 3]>,
    /// Cells whose level changed without changing block.
    pub(crate) decayed: Vec<([i32; 3], u8)>,
}

/// Apply the plan. Wakes up NSEW + Y neighbours of every changed
/// cell so the next tick re-evaluates them - critical for the
/// drainage cascade after a source removal, where the level wave
/// has to propagate outward without the player retouching the world.
pub(crate) fn liquid_apply_plan(
    world: &mut World,
    level_map: &mut std::collections::HashMap<[i32; 3], u8>,
    active: &mut HashSet<[i32; 3]>,
    positions: &mut HashSet<[i32; 3]>,
    liquid: &Liquid,
    plan: LiquidPlanStep,
) -> LiquidTape {
    let liquid_id = liquid.block_id;
    let mut tape = LiquidTape::default();
    let mut touched: Vec<[i32; 3]> = Vec::new();
    for pos in plan.evaporate {
        // Defensive: don't evaporate a cell that's no longer the
        // expected liquid (could've been overwritten by a push that
        // beat us in apply order on a future tick).
        if world.get(pos).0 != liquid_id {
            continue;
        }
        // Sources are immune: an explicit source removal lands via
        // `world_set_tracked` from the player edit, not through the
        // tick planner.
        if level_map.get(&pos).copied().unwrap_or(0) == 0 {
            continue;
        }
        world.set(pos, BlockId::AIR);
        level_map.remove(&pos);
        active.remove(&pos);
        positions.remove(&pos);
        tape.evaporated.push(pos);
        touched.push(pos);
    }
    for (pos, lvl) in plan.decay {
        if world.get(pos).0 != liquid_id {
            continue;
        }
        if level_map.get(&pos).copied().unwrap_or(0) == 0 {
            // Source cells never decay.
            continue;
        }
        level_map.insert(pos, lvl);
        tape.decayed.push((pos, lvl));
        touched.push(pos);
    }
    for (pos, level, falling) in plan.place {
        let existing = world.get(pos);
        let existing_level = level_map.get(&pos).copied().unwrap_or(0);
        // Don't downgrade a source.
        if existing.0 == liquid_id && existing_level == 0 {
            continue;
        }
        let washable = block_is_washable(existing);
        if !existing.is_air() && existing.0 != liquid_id && !washable {
            continue;
        }
        let final_level = if falling { 8 } else { level };
        if existing.0 == liquid_id && existing_level == final_level {
            active.insert(pos);
            continue;
        }
        if !existing.is_air() && washable {
            tape.washed.push((existing, pos));
        }
        world.set(pos, BlockId(liquid_id));
        level_map.insert(pos, final_level);
        active.insert(pos);
        positions.insert(pos);
        tape.placed.push((pos, liquid_id, final_level));
        touched.push(pos);
    }
    // Wake the NSEW + above + below of every changed cell so the
    // next tick re-evaluates them via `getNewLiquid`. Without this
    // wake-up the drainage wave stalls a few cells out from the
    // removed source - the test `source_removal_drains_with_cliff`
    // catches that regression.
    let dirs: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
        [0, 1, 0],
        [0, -1, 0],
    ];
    for p in touched {
        for d in &dirs {
            let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
            if positions.contains(&n) {
                active.insert(n);
            }
        }
    }
    tape
}
