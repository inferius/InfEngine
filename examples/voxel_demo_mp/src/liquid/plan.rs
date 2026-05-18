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

/// MC-faithful planner. See module docs for the algorithm.
pub(crate) fn liquid_plan_step(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    active: &HashSet<[i32; 3]>,
    liquid: &Liquid,
    bias_drops: bool,
) -> LiquidPlanStep {
    let liquid_id = liquid.block_id;
    let max_range = liquid.max_range;
    // Per-tick candidate cap. Previously 512 - too small for a
    // tall waterfall + sprawling pool: a column of >500 cells
    // exceeded the cap, HashSet iteration order randomly picked
    // which cells got processed each tick, and the column
    // visually "froze" partway down because the bottom cells
    // were skipped. 8192 covers practically any single-pool
    // scenario the demo produces without blowing per-frame
    // CPU budget (8192 * 4 NSEW reads ~ 32k world.get calls
    // every tick_period frames = ~6 fired/sec for water).
    let candidates: Vec<[i32; 3]> = active
        .iter()
        .filter(|p| world.get(**p).0 == liquid_id)
        .take(8192)
        .copied()
        .collect();
    let mut plan = LiquidPlanStep::default();
    if candidates.is_empty() {
        return plan;
    }
    // Updates merged from every spread proposal. Min-level wins on
    // conflict; falling flag sticks if any contributor sets it.
    let mut updates: HashMap<[i32; 3], (u8, bool)> = HashMap::new();

    for pos in &candidates {
        let level = level_map.get(pos).copied().unwrap_or(0);
        let is_source = level == 0;

        // ----- Phase 1: pull-recompute non-source cells. -----
        //
        // Use snapshot state (the planner is pure - no mid-loop
        // writes to world/level_map). If the recompute differs from
        // the current level, stage decay; if it empties, stage
        // evaporate.
        //
        // This is the change that fixes the steady-state oscillation
        // on flat terrain. Previously the planner only decayed cells
        // that received NO spread write that tick - on a settled
        // pool every cell got pushed, so nothing decayed and a
        // removed source's pool persisted forever.
        if !is_source {
            match liquid_get_new_state(world, level_map, *pos, liquid) {
                None => {
                    plan.evaporate.push(*pos);
                    // Skip spread for an evaporating cell - it
                    // shouldn't push water on its way out. (MC's
                    // tick() also runs spread after setBlock to air,
                    // but the resulting fluidState.isEmpty() makes
                    // spread() a no-op anyway.)
                    continue;
                }
                Some((new_level, _falling)) if new_level != level => {
                    plan.decay.push((*pos, new_level));
                    // Fall through to spread - MC keeps spreading
                    // even after a recompute landed.
                }
                Some(_) => {
                    // Unchanged.
                }
            }
        }

        // ----- Phase 2: spread (MC's FlowingFluid.spread). -----
        //
        // Use the snapshot fluidState for amount/falling decisions.
        // Decayed cells will see the new state on the next tick.
        let is_falling = level == 8;

        let below = [pos[0], pos[1] - 1, pos[2]];
        let below_passes = can_maybe_pass_through(world, level_map, below, liquid_id);
        let mut placed_below = false;

        if below_passes {
            // newBelowFluid = flowing(8, true). MC always writes
            // falling fluid below a same-liquid spreader because
            // getNewLiquid(below) sees us at below.above() and
            // returns flowing(8, true) unconditionally.
            put_water_update(&mut updates, below, 1, true, max_range);
            placed_below = true;
        }

        let do_spread_sides = if placed_below {
            // MC: only spread sides after a vertical drop when
            // surrounded by 3+ horizontal sources (rare - sustains
            // an infinite well from a 4-source spring).
            source_neighbour_count(world, level_map, *pos, liquid_id) >= 3
        } else {
            // MC: spread sides if source OR !isWaterHole(below).
            // waterHole = below already holds liquid OR is open.
            // -> equivalent to "below is a non-passable wall".
            let below_b = world.get(below);
            let below_blocks = !(below_b.is_air()
                || block_is_washable(below_b)
                || below_b.0 == liquid_id);
            is_source || below_blocks
        };
        if !do_spread_sides {
            continue;
        }

        // spreadToSides amount gate. MC:
        //   neighbor = amount - dropOff (or 7 if FALLING)
        //   if neighbor > 0: spread to getSpread dirs.
        let amount: i16 = if is_source || is_falling { 8 } else { 8 - level as i16 };
        let neighbor_amount: i16 = if is_falling {
            7
        } else {
            amount - liquid.drop_off as i16
        };
        if neighbor_amount <= 0 {
            continue;
        }

        // getSpread (slope-distance bias) + push.
        for (target, lvl, falling) in
            get_spread(world, level_map, *pos, liquid, bias_drops)
        {
            put_water_update(&mut updates, target, lvl, falling, max_range);
        }
    }

    plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
    plan
}
