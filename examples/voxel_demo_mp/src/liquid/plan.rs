//! MC-faithful liquid planner. Each tick:
//!
//!   1. **Pull recompute** every active non-source cell via
//!      `liquid_get_new_state` (port of MC's `FlowingFluid.getNewLiquid`).
//!      If the cell's snapshot neighbours can't sustain it, stage an
//!      evaporate / decay. This is what drives drainage when a source
//!      disappears - every cell weakens one drop_off step per tick
//!      until it crosses `max_range` and disappears.
//!
//!   2. **Spread** (port of MC's `FlowingFluid.spread`): (a) if
//!      `below` can hold liquid (air or washable, not a same-liquid
//!      source), push a falling-amount-8 fluid onto it - skip
//!      horizontal spread unless `sourceNeighborCount >= 3`; (b)
//!      else if `is_source || !waterHole(below)`: spreadToSides.
//!
//!   3. **spreadToSides** picks the NSEW dirs with the minimum BFS
//!      slope distance to a cliff drop (within 4 steps) and stages a
//!      push of `getNewLiquid(target)` onto each. Target's level is
//!      recomputed from its own neighbours - that's what lets the
//!      back-fill of a just-removed source happen at level 2, not at
//!      level 1.
//!
//! All staged writes go into a per-tick `updates` map with min-level
//! wins on conflict. The applier (`liquid_apply_plan`) merges them
//! into the world and wakes neighbours so the next tick continues
//! the cascade.
//!
//! ## Behaviour diff vs MC
//!
//! - `canPassThroughWall` is simplified to "non-solid neighbour"
//!   instead of checking VoxelShape merge occlusion. Fine for the
//!   demo palette: every solid block here is a full cube.
//! - Source promotion (`neighbourSources >= 2`) lives in
//!   `liquid_merge_step` with a stricter stability check (no
//!   open neighbour). Intentional deviation - prevents surprise
//!   infinite pools next to small drains.
//! - `getSpread` uses a coarser BFS (distance 4, no per-direction
//!   wall-merge cache) but the resulting tie-break set matches MC
//!   for every layout the unit tests cover.

use std::collections::{HashMap, HashSet};

use infengine_voxel::{BlockId, World};

use crate::block::block_is_washable;

use super::Liquid;
use super::boundary::lava_boundary_solid_id;
use super::mesh::distance_to_drop_at;

/// Output of one liquid planning pass. The applier consumes this and
/// emits a `LiquidTape`.
#[derive(Debug, Default, Clone)]
pub(crate) struct LiquidPlanStep {
    /// Cells the tick will set to air.
    pub(crate) evaporate: Vec<[i32; 3]>,
    /// Cells whose level changes without changing block. `(pos, level)`.
    pub(crate) decay: Vec<([i32; 3], u8)>,
    /// Proposed liquid writes. `(pos, level, falling)`.
    pub(crate) place: Vec<([i32; 3], u8, bool)>,
    /// Cells whose lava/water boundary converts them to a solid
    /// block (MC `LiquidBlock.shouldSpreadLiquid`):
    /// `(pos, solid_block_id)`. Today only lava cells produce
    /// solidify entries (obsidian / cobblestone).
    pub(crate) solidify: Vec<([i32; 3], u16)>,
}

/// MC `FlowingFluid.getNewLiquid` port. Returns the level a flow cell
/// would have if recomputed from its current surroundings:
///   - same liquid directly above -> (8, falling)
///   - else level = min(NSEW neighbour level) + drop_off
///   - if the resulting level exceeds `max_range`, returns `None`
///     (= evaporate).
///
/// Falling neighbours count with effective level 0 (amount 8) - same
/// pull strength as a source, matching MC's `getAmount() == 8` for
/// falling fluid states.
pub(crate) fn liquid_get_new_state(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid: &Liquid,
) -> Option<(u8, bool)> {
    let liquid_id = liquid.block_id;
    // Above same liquid -> full-height falling stream.
    if world.get([pos[0], pos[1] + 1, pos[2]]).0 == liquid_id {
        return Some((8, true));
    }
    // Horizontal pull: find the strongest same-liquid neighbour
    // (lowest level in our convention). Source neighbours (level 0)
    // give effective 0. Falling neighbours (level 8) count as
    // effective 0 too because MC's getAmount returns 8 for falling
    // fluid states - same pull strength as a source.
    let dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    let mut min_n: Option<u8> = None;
    for d in &dirs {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        if world.get(n).0 != liquid_id {
            continue;
        }
        let nl = level_map.get(&n).copied().unwrap_or(0);
        let effective = if nl >= 8 { 0 } else { nl };
        min_n = Some(min_n.map_or(effective, |m| m.min(effective)));
    }
    let m = min_n?;
    let new_level = m.saturating_add(liquid.drop_off);
    if new_level > liquid.max_range {
        return None;
    }
    Some((new_level, false))
}

/// Stage a proposed write into the per-tick update map. Stronger
/// (numerically lower) levels win on conflict; the falling flag
/// sticks once any contributor sets it.
fn put_water_update(
    updates: &mut HashMap<[i32; 3], (u8, bool)>,
    pos: [i32; 3],
    level: u8,
    falling: bool,
    max_range: u8,
) {
    if level > max_range && !falling {
        return;
    }
    match updates.get(&pos) {
        Some((prev_level, prev_falling)) => {
            let new_lvl = level.min(*prev_level);
            let new_falling = falling || *prev_falling;
            updates.insert(pos, (new_lvl, new_falling));
        }
        None => {
            updates.insert(pos, (level, falling));
        }
    }
}

/// Is the cell at `pos` a same-liquid source? Used by `canMaybePassThrough`
/// to keep flow from overwriting sources during spread.
fn is_same_liquid_source(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid_id: u16,
) -> bool {
    world.get(pos).0 == liquid_id
        && level_map.get(&pos).copied().unwrap_or(0) == 0
}

/// "Can hold a liquid": air or washable (decoration), or empty.
/// Same-liquid flow cells also count - the spread will overwrite
/// them with a fresh `getNewLiquid` result, possibly stronger.
fn can_hold_liquid(b: BlockId, liquid_id: u16) -> bool {
    b.is_air() || block_is_washable(b) || b.0 == liquid_id
}

/// MC's `canMaybePassThrough`: target is not a same-liquid source AND
/// can hold a liquid AND the wall between source and target is not
/// occluding. We simplify the wall check to "non-solid target" since
/// every solid block in the demo palette is a full cube.
fn can_maybe_pass_through(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    target: [i32; 3],
    liquid_id: u16,
) -> bool {
    let b = world.get(target);
    if is_same_liquid_source(world, level_map, target, liquid_id) {
        return false;
    }
    can_hold_liquid(b, liquid_id)
}

/// Count same-liquid sources horizontally adjacent to `pos`.
fn source_neighbour_count(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid_id: u16,
) -> u8 {
    let dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    let mut count = 0u8;
    for d in &dirs {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        if is_same_liquid_source(world, level_map, n, liquid_id) {
            count += 1;
        }
    }
    count
}

/// MC's `getSpread`: pick the NSEW directions with the minimum BFS
/// distance to a cliff drop (within `max_steps`). Returns `(dir,
/// new_state)` pairs where `new_state` is the `getNewLiquid` result
/// computed at the target cell.
///
/// If no direction can reach a drop within range, all viable
/// candidates tie at the sentinel distance and every one of them is
/// returned (matches MC's tie-on-INFINITY behaviour).
fn get_spread(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid: &Liquid,
    bias_drops: bool,
) -> Vec<([i32; 3], u8, bool)> {
    let liquid_id = liquid.block_id;
    let dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    // First gather candidates with their slope distances.
    let mut cands: Vec<([i32; 3], u32)> = Vec::with_capacity(4);
    for d in &dirs {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        if !can_maybe_pass_through(world, level_map, n, liquid_id) {
            continue;
        }
        // Compute spread state via getNewLiquid for the target. The
        // strongest neighbour at the target's NSEW determines its
        // level - that's what makes back-fill of a removed source
        // slot land at level 2 (not level 1) on the first tick after
        // the slot is wiped.
        let _new = liquid_get_new_state(world, level_map, n, liquid);
        // Even if getNewLiquid returns None (would evaporate), we
        // still consider the direction here because MC stores the
        // empty-fluid result and the candidate filter below will
        // gate on it. We re-call inside the build below.
        let dist = if bias_drops {
            distance_to_drop_at(world, level_map, liquid_id, n, 4)
        } else {
            // bias disabled (debug toggle): treat every viable dir
            // as equal distance.
            0
        };
        cands.push((n, dist));
    }
    if cands.is_empty() {
        return Vec::new();
    }
    let min_d = cands.iter().map(|(_, d)| *d).min().unwrap_or(u32::MAX);
    let mut out: Vec<([i32; 3], u8, bool)> = Vec::with_capacity(cands.len());
    for (n, d) in cands {
        if d != min_d {
            continue;
        }
        // Compute the level the target would take. If getNewLiquid
        // returns None, the target would evaporate on its own next
        // tick - we still write our spread proposal so the level
        // can stabilise; the apply guard handles the "no-op"
        // collapse if our level == its current level.
        let (lvl, falling) = match liquid_get_new_state(world, level_map, n, liquid)
        {
            Some(s) => s,
            None => continue,
        };
        out.push((n, lvl, falling));
    }
    out
}

/// Per-cell planner. Direct port of MC's `FlowingFluid.tick(pos)`.
/// Runs pull-recompute and spread for a single cell using the
/// world and level snapshot it sees, stages writes into
/// `updates`. The caller applies the merged plan via
/// `liquid_apply_plan`.
///
/// Each call only proposes writes for `pos` and its NSEW and
/// below neighbours; the caller is expected to schedule the next
/// tick for `pos` (if it stays active) and for every neighbour
/// the plan touches.
pub(crate) fn liquid_plan_cell(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid: &Liquid,
    bias_drops: bool,
    plan: &mut LiquidPlanStep,
    updates: &mut HashMap<[i32; 3], (u8, bool)>,
) {
    let liquid_id = liquid.block_id;
    let max_range = liquid.max_range;
    // Defensive: cell may have been overwritten since it was
    // scheduled.
    if world.get(pos).0 != liquid_id {
        return;
    }
    // Q.4a: lava-water boundary. If this is a lava cell with a
    // water neighbour, convert it to obsidian (source) or
    // cobblestone (flow) and skip the rest of the tick. MC's
    // `LiquidBlock.shouldSpreadLiquid` returns false in the same
    // shape - no spread is staged when a fizz triggers.
    if let Some(solid_id) = lava_boundary_solid_id(world, level_map, pos, liquid) {
        plan.solidify.push((pos, solid_id));
        return;
    }
    let level = level_map.get(&pos).copied().unwrap_or(0);
    let is_source = level == 0;

    // Phase 1: pull-recompute non-source cells (see bulk planner
    // for the rationale on snapshot semantics + steady-state
    // oscillation).
    if !is_source {
        match liquid_get_new_state(world, level_map, pos, liquid) {
            None => {
                plan.evaporate.push(pos);
                return;
            }
            Some((new_level, _falling)) if new_level != level => {
                plan.decay.push((pos, new_level));
            }
            Some(_) => {}
        }
    }

    // Phase 2: spread. Uses snapshot state, not the decayed level
    // - matches MC's behaviour where tick() reads fluidState
    // before applying writes.
    let is_falling = level == 8;
    let below = [pos[0], pos[1] - 1, pos[2]];
    let below_passes = can_maybe_pass_through(world, level_map, below, liquid_id);
    let mut placed_below = false;
    if below_passes {
        put_water_update(updates, below, 1, true, max_range);
        placed_below = true;
    }
    let do_spread_sides = if placed_below {
        source_neighbour_count(world, level_map, pos, liquid_id) >= 3
    } else {
        let below_b = world.get(below);
        let below_blocks = !(below_b.is_air()
            || block_is_washable(below_b)
            || below_b.0 == liquid_id);
        is_source || below_blocks
    };
    if !do_spread_sides {
        return;
    }
    let amount: i16 = if is_source || is_falling { 8 } else { 8 - level as i16 };
    let neighbor_amount: i16 = if is_falling {
        7
    } else {
        amount - liquid.drop_off as i16
    };
    if neighbor_amount <= 0 {
        return;
    }
    for (target, lvl, falling) in get_spread(world, level_map, pos, liquid, bias_drops)
    {
        put_water_update(updates, target, lvl, falling, max_range);
    }
}

/// MC-faithful planner. See module docs for the algorithm. Bulk
/// variant used by the test sim (`LiquidSim`); the live Voxel
/// runs per-cell ticks via `liquid_plan_cell` + the schedule.
#[allow(dead_code)]
pub(crate) fn liquid_plan_step(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    active: &HashSet<[i32; 3]>,
    liquid: &Liquid,
    bias_drops: bool,
) -> LiquidPlanStep {
    let liquid_id = liquid.block_id;
    // No more 8192 cap on the bulk variant either - the schedule
    // path enforces its budget per frame, and the test sim runs on
    // small worlds (no risk of huge candidate counts).
    let candidates: Vec<[i32; 3]> = active
        .iter()
        .filter(|p| world.get(**p).0 == liquid_id)
        .copied()
        .collect();
    let mut plan = LiquidPlanStep::default();
    if candidates.is_empty() {
        return plan;
    }
    let mut updates: HashMap<[i32; 3], (u8, bool)> = HashMap::new();
    for pos in &candidates {
        liquid_plan_cell(
            world, level_map, *pos, liquid, bias_drops, &mut plan, &mut updates,
        );
    }
    plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
    plan
}
