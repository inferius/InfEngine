//! Liquid simulation: data, planning, applying, and rendering
//! helpers.
//!
//! Owns:
//! - `Liquid` struct + the static `LIQUIDS` registry (water +
//!   lava parameters).
//! - Pure, side-effect-free planning / apply / merge passes
//!   (`liquid_plan_step`, `liquid_apply_plan`, `liquid_merge_step`,
//!   `liquid_get_new_state`).
//! - Rendering helpers (`liquid_surface_height_at`,
//!   `liquid_corner_height_at`, `push_quad_flow`).
//! - The behaviour test suite (`#[cfg(test)] mod water_tests`).
//!
//! `Voxel::tick_liquids` / `tick_liquid_step` / `tick_liquid_merge` /
//! `maybe_rebuild_liquid_mesh` stay in `main.rs` - they touch
//! renderer / net / edit-history / drops state. Their bodies are
//! thin and delegate to the free fns in this module.

use std::collections::{HashMap, HashSet};

use infengine_render::MeshVertex;
use infengine_voxel::{BlockId, World};

use crate::block::{LAVA_ID, WATER_ID, block_is_washable};

// ---------------------------------------------------------------
// Liquid registry.
// ---------------------------------------------------------------

/// Per-liquid physics parameters. The generic `tick_liquid_*`
/// passes read these so adding a new liquid kind = one
/// `Liquid` row + one `BlockType` entry with the matching
/// `F_SWIM | F_DEDICATED_RENDER | F_TRANSPARENT_RENDER` flags.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Liquid {
    /// `BlockId.0` for this liquid's voxel state. Source +
    /// flowing share the same id; the `liquid_level` map
    /// distinguishes them.
    pub(crate) block_id: u16,
    /// Horizontal spread distance cap from any source. MC water
    /// = 7, MC overworld lava = 3.
    pub(crate) max_range: u8,
    /// Per-step level penalty applied each cell when spreading
    /// horizontally. MC's `FlowingFluid.getDropOff` returns 1
    /// for water and 2 for overworld lava - the higher drop_off
    /// makes lava thin out faster (visible as a steeper
    /// own_height ramp). Source level (0) -> first ring level
    /// = drop_off; second ring = 2 * drop_off; etc.
    pub(crate) drop_off: u8,
    /// Net ticks between spread + merge + decay updates. MC
    /// ticks are 50 ms; a net tick is ~16 ms (60 Hz), so water
    /// (every 5 game ticks = 250 ms) maps to ~15 net ticks
    /// here. Lava overworld (every 30 game ticks = 1500 ms)
    /// maps to ~90.
    pub(crate) tick_period: u32,
    /// Legacy field from the split fall+spread scheduler. The
    /// unified `tick_liquid_step` runs both in the same pass,
    /// so this is unused today. Kept for future per-fluid
    /// tuning (e.g. lava that streams down slower than water).
    #[allow(dead_code)]
    pub(crate) fall_period: u32,
    /// Whether two adjacent sources promote a flow voxel between
    /// them to a source. MC: water = true, lava = false.
    pub(crate) infinite_source: bool,
    /// Horizontal velocity multiplier while a player is
    /// submerged in this liquid. Lower = thicker drag. Water
    /// = 0.45, lava = 0.20.
    pub(crate) swim_horiz_drag: f32,
    /// Y-velocity floor while submerged (negative number = how
    /// fast the player can sink). Water -1.8, lava -0.5.
    pub(crate) swim_fall_cap: f32,
    /// Y-velocity set when the player holds the jump key while
    /// submerged (active swim-up). Water 2.2, lava 1.0.
    pub(crate) swim_jump_vel: f32,
    /// Passive upward acceleration applied every frame (units:
    /// m/s^2) when the player is submerged without holding
    /// jump. Models buoyancy: water > gravity = drift up;
    /// lava < gravity = slow sink. Capped at +1.0 m/s.
    pub(crate) swim_buoyancy: f32,
    /// Mining-time multiplier applied to every block the
    /// player breaks while submerged. Water 5.0, lava 5.0 -
    /// MC slows underwater mining drastically.
    pub(crate) submerged_mining_mul: f32,
    /// HP damage per second the player takes while submerged.
    /// Water 0.0, lava 4.0.
    pub(crate) submerged_damage_per_sec: f32,
    /// Whether being submerged in this liquid drains the
    /// player's air meter (and triggers drowning damage when
    /// the meter hits zero). Water = true, lava = false (lava
    /// burns directly via `submerged_damage_per_sec`).
    pub(crate) drains_air: bool,
    /// RGBA tint composited as a full-screen overlay while the
    /// player is submerged. Water reads as a cool blue haze,
    /// lava as a bright orange wash. Alpha controls intensity.
    pub(crate) submerged_tint: [f32; 4],
    /// Horizontal velocity (m/s) the current pushes the player
    /// in the flow direction while standing in a flow voxel.
    /// Source cells have no flow so no push. Water 4.0,
    /// lava 1.5 (sticky).
    pub(crate) current_strength: f32,
}

/// Registry of every flowing liquid. The tick loop iterates this
/// table and runs the shared physics with each liquid's params,
/// so the code path is "this is a liquid" rather than
/// "this is water".
pub(crate) const LIQUIDS: &[Liquid] = &[
    Liquid {
        block_id: WATER_ID,
        max_range: 7,
        drop_off: 1,
        // ~167 ms per tick at 60 fps. Roughly half MC's
        // 250 ms - readable progress without burning CPU on
        // big pools.
        tick_period: 10,
        fall_period: 5,
        infinite_source: true,
        swim_horiz_drag: 0.45,
        swim_fall_cap: -1.8,
        swim_jump_vel: 2.2,
        swim_buoyancy: 12.0,
        submerged_mining_mul: 5.0,
        submerged_damage_per_sec: 0.0,
        drains_air: true,
        submerged_tint: [0.10, 0.30, 0.60, 0.40],
        current_strength: 4.0,
    },
    Liquid {
        block_id: LAVA_ID,
        // MC overworld lava: drop_off = 2, max amount = 7
        // -> 3 cells of flow before evaporation. In our level
        // convention (0 = source, level += drop_off per ring),
        // that's levels 2, 4, 6 -> 3 rings, so max_range = 7
        // and drop_off = 2 lines up with MC.
        max_range: 7,
        drop_off: 2,
        tick_period: 90,
        fall_period: 15,
        infinite_source: false,
        swim_horiz_drag: 0.20,
        swim_fall_cap: -0.5,
        swim_jump_vel: 1.0,
        swim_buoyancy: 4.0,
        submerged_mining_mul: 5.0,
        submerged_damage_per_sec: 4.0,
        drains_air: false,
        submerged_tint: [1.00, 0.35, 0.05, 0.55],
        current_strength: 1.5,
    },
];

/// Look up a `Liquid` definition for a placed block. Returns
/// `None` for non-liquid voxels (every regular solid block).
pub(crate) fn liquid_for(b: BlockId) -> Option<&'static Liquid> {
    LIQUIDS.iter().find(|l| l.block_id == b.0)
}

// ---------------------------------------------------------------
// Liquid rendering helpers as free functions so the unit tests
// can call them without constructing a full `Voxel` app. The
// `Voxel` methods below delegate to these so the production
// code path stays a single implementation.
// ---------------------------------------------------------------

pub(crate) fn liquid_surface_height_at(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid_id: u16,
) -> f32 {
    if world.get(pos).0 != liquid_id {
        return 0.0;
    }
    if world.get([pos[0], pos[1] + 1, pos[2]]).0 == liquid_id {
        return 1.0;
    }
    let level = level_map.get(&pos).copied().unwrap_or(0);
    if level == 0 {
        return 1.0;
    }
    if level >= 8 {
        return 1.0;
    }
    (8 - level as i32).max(1) as f32 / 9.0
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn liquid_corner_height_at(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    vx: i32,
    vy: i32,
    vz: i32,
    cx: i32,
    cz: i32,
    liquid_id: u16,
) -> f32 {
    let touching = [
        [vx + cx - 1, vy, vz + cz - 1],
        [vx + cx, vy, vz + cz - 1],
        [vx + cx - 1, vy, vz + cz],
        [vx + cx, vy, vz + cz],
    ];
    // Phase 1: column-above lock. If any touching cell is part
    // of a falling column (same liquid both at this y and at
    // y+1), the corner snaps to full height. Mirrors MC's
    // `hasSameAbove == true -> getHeight = 1`.
    for t in &touching {
        if world.get(*t).0 == liquid_id
            && world.get([t[0], t[1] + 1, t[2]]).0 == liquid_id
        {
            return 1.0;
        }
    }
    // Phase 2: weighted blend - matches MC's
    // `LiquidBlockRenderer.calculateAverageHeight`. Liquid
    // contributions count 10x relative to air so a pool edge
    // sits at ~0.91 (slight slope toward the bank) instead of
    // crashing down to 0.5. The 0.5 averaging was inverted on
    // screen: corners shared between two adjacent sources came
    // out lower than the outer "tip" corners, so the water
    // surface visibly sloped UP toward air instead of down.
    // The 10:1 weight keeps liquid-touching corners near full
    // height while still letting the very outermost tip taper.
    //
    // Solids are ignored entirely (a wall doesn't slope the
    // water). Air counts with weight 1 and a height of 0.
    const LIQUID_WEIGHT: f32 = 10.0;
    const AIR_WEIGHT: f32 = 1.0;
    let mut weighted_sum = 0.0_f32;
    let mut weight_total = 0.0_f32;
    for t in &touching {
        let b = world.get(*t);
        if b.0 == liquid_id {
            let h = liquid_surface_height_at(world, level_map, *t, liquid_id);
            weighted_sum += h * LIQUID_WEIGHT;
            weight_total += LIQUID_WEIGHT;
        } else if b.is_air() {
            // air contributes height 0 at weight 1
            weight_total += AIR_WEIGHT;
        }
        // solids: ignored (no slope into a wall)
    }
    if weight_total <= 0.0 {
        return 0.0;
    }
    weighted_sum / weight_total
}

pub(crate) fn distance_to_drop_at(
    world: &World,
    start: [i32; 3],
    max_steps: u32,
) -> u32 {
    use std::collections::VecDeque;
    let walkable = |b: BlockId| b.is_air() || block_is_washable(b);
    let drop_at = |p: [i32; 3]| walkable(world.get(p));
    if !walkable(world.get(start)) {
        return u32::MAX;
    }
    if drop_at([start[0], start[1] - 1, start[2]]) {
        return 0;
    }
    let mut visited: HashSet<[i32; 3]> = HashSet::new();
    visited.insert(start);
    let mut queue: VecDeque<([i32; 3], u32)> = VecDeque::new();
    queue.push_back((start, 0));
    let dirs: [[i32; 3]; 4] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
    while let Some((cell, dist)) = queue.pop_front() {
        if dist >= max_steps {
            continue;
        }
        for d in &dirs {
            let n = [cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]];
            if visited.contains(&n) {
                continue;
            }
            if !walkable(world.get(n)) {
                continue;
            }
            visited.insert(n);
            if drop_at([n[0], n[1] - 1, n[2]]) {
                return dist + 1;
            }
            queue.push_back((n, dist + 1));
        }
    }
    u32::MAX
}

/// Pure MC-style liquid planner. Reads world + level + active set
/// and returns the writes / decays / evaporations the next tick
/// should apply. No side effects - intended to be shared between
/// the live `Voxel::tick_liquid_step` path (which adds
/// broadcasting + drop spawns on top) and the offline `LiquidSim`
/// used by behaviour tests.
///
/// The algorithm mirrors `FlowingFluid.spread` from MC's
/// decompiled source:
///   1. Every active liquid cell with air-below proposes a
///      level-1 falling write straight down. Falling cells skip
///      horizontal spread that tick - MC water columns are one
///      voxel wide.
///   2. Cells resting on solid scan NSEW for a cliff (neighbour
///      open + neighbour-below open) and spill there with
///      level-1 falling. Non-source non-falling cells with a
///      cliff drop skip the regular horizontal spread.
///   3. Remaining cells propose level = (own_level + drop_off)
///      writes to NSEW neighbours, biased toward neighbours with
///      the shortest BFS distance to a cliff (Pathfinder).
///   4. Every non-source flow cell not receiving a fresh write
///      decays one step (level+1). Cells with level > max_range
///      evaporate.
///
/// Documented intentional deviations from MC's `FlowingFluid`
/// (audited against `net.minecraft.world.level.material.FlowingFluid`
/// from the 1.21.x decomp, post-`getNewLiquid`):
///   - **Decay**: MC re-evaluates every non-source cell each
///     tick via `getNewLiquid` = `max(neighbour amounts) -
///     drop_off`. We do a pure +1 decay when no write lands.
///     The two converge once spread settles; the pure decay
///     gives an off-by-N transient when sources disappear.
///   - **Falling propagation**: MC's `getNewLiquid` returns
///     `getFlowing(8, true)` when the cell above is the same
///     liquid. We don't write self - instead the cell above
///     writes us as falling when it sees same-liquid-below.
///     Equivalent outcome.
///   - **Source merge stability**: MC promotes when
///     `neighbourSources >= 2 && below is solid OR source`.
///     We add a "no open NSEW neighbour" check on top so a
///     pocket with a side drain doesn't promote. Intentional -
///     user-spec quirk that prevents surprise infinite pools.
///   - **`sourceNeighborCount >= 3` post-vertical-drop**: MC
///     spreads sides even after spreading down when surrounded
///     by 3+ sources. We never do. Edge case.
///   - **`canPassThroughWall`**: MC checks VoxelShape so water
///     can drip past stair shapes. We treat any non-air, non-
///     washable block as a solid wall. Fine for the demo's
///     block palette.
#[derive(Debug, Default, Clone)]
pub(crate) struct LiquidPlanStep {
    /// Cells the tick will set to air.
    pub(crate) evaporate: Vec<[i32; 3]>,
    /// Cells whose level steps up by one (weaker flow).
    pub(crate) decay: Vec<([i32; 3], u8)>,
    /// Proposed liquid writes. `(pos, level, falling)`.
    pub(crate) place: Vec<([i32; 3], u8, bool)>,
}

/// Side-effect-free record of what `liquid_apply_plan` did.
/// Caller (Voxel or test sim) drives mesh-dirty / broadcast /
/// drop spawns from this.
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

/// Stage a proposed write into the per-tick update map. Stronger
/// (numerically lower) levels win on conflict; the falling flag
/// sticks once any contributor sets it.
pub(crate) fn put_water_update(
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

pub(crate) fn liquid_plan_step(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    active: &HashSet<[i32; 3]>,
    liquid: &Liquid,
    bias_drops: bool,
) -> LiquidPlanStep {
    let liquid_id = liquid.block_id;
    let max_range = liquid.max_range;
    let candidates: Vec<[i32; 3]> = active
        .iter()
        .filter(|p| world.get(**p).0 == liquid_id)
        .take(512)
        .copied()
        .collect();
    let mut plan = LiquidPlanStep::default();
    if candidates.is_empty() {
        return plan;
    }
    let mut updates: HashMap<[i32; 3], (u8, bool)> = HashMap::new();
    let drop_dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    let _ = bias_drops; // MC's getSpread always bias-min; flag retained
    // for the F6 debug toggle but ignored by the new spread loop.
    for pos in &candidates {
        let level = level_map.get(pos).copied().unwrap_or(0);
        let is_source = level == 0;
        let is_falling = level == 8;
        // MC `Fluid.getAmount`: 8 for source / falling-full, else
        // `8 - my_level`. We reconstruct it here so the
        // `spreadToSides` neighbour-amount math is literal MC.
        let amount: u8 = if is_source || is_falling { 8 } else { 8 - level };

        // Step 1: try below. MC `FlowingFluid.spread`:
        //   if canMaybePassThrough(DOWN, below) {
        //       newBelowFluid = getNewLiquid(below);
        //       if below.canBeReplaced + canHoldSpecificFluid {
        //           spreadTo(below, newBelowFluid);
        //           if sourceNeighborCount(self) >= 3 {
        //               spreadToSides(...);
        //           }
        //           return;
        //       }
        //   }
        //   if isSource || !isWaterHole(below) {
        //       spreadToSides(...);
        //   }
        let below = [pos[0], pos[1] - 1, pos[2]];
        let below_b = world.get(below);
        let below_open = below_b.is_air() || block_is_washable(below_b);
        let below_same_liquid = below_b.0 == liquid_id;
        // `canMaybePassThrough` simplified: target is not a
        // same-liquid source AND can hold a liquid. Air,
        // washable, and same-liquid flow cells all pass.
        let below_passes = if below_same_liquid {
            level_map.get(&below).copied().unwrap_or(0) != 0
        } else {
            below_open
        };
        let mut placed_below = false;
        if below_passes {
            // newBelowFluid = falling amount 8 (since above is
            // same liquid). put_water_update with our level-8
            // marker + falling flag.
            put_water_update(&mut updates, below, 1, true, max_range);
            placed_below = true;
        }

        // Decide whether to spreadToSides.
        // - Cell that placed below: only when sourceNeighborCount >= 3
        // - Else (no below path): spreadToSides if source OR
        //   !waterHole. waterHole = below is open OR same liquid.
        let do_spread_sides = if placed_below {
            // Count same-liquid SOURCE neighbours horizontally.
            let mut sources = 0u8;
            for d in &drop_dirs {
                let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
                if world.get(n).0 == liquid_id
                    && level_map.get(&n).copied().unwrap_or(0) == 0
                {
                    sources += 1;
                }
            }
            sources >= 3
        } else {
            let water_hole = below_open || below_same_liquid;
            is_source || !water_hole
        };
        if !do_spread_sides {
            continue;
        }

        // spreadToSides: neighbour amount = amount - drop_off,
        // with `FALLING` override clamping to 7. Sources at
        // amount 8 also resolve to amount 7 here (8 - 1).
        let neighbor_amount: i16 = if is_falling {
            7
        } else {
            amount as i16 - liquid.drop_off as i16
        };
        if neighbor_amount <= 0 {
            continue;
        }
        // Convert back to our level convention.
        let neighbor_level = (8 - neighbor_amount).max(1) as u8;

        // getSpread: pick all NSEW dirs with the minimum
        // slope-distance to a drop. A "drop" is a cell whose
        // own below is open. Targets that are non-passable or
        // same-liquid sources don't count.
        let spread_dest = |b: BlockId, p: [i32; 3]| -> bool {
            if !(b.is_air() || block_is_washable(b) || b.0 == liquid_id) {
                return false;
            }
            // Same-liquid source neighbour: can't overwrite.
            if b.0 == liquid_id
                && level_map.get(&p).copied().unwrap_or(0) == 0
            {
                return false;
            }
            true
        };
        let mut weights: [u32; 4] = [u32::MAX; 4];
        for (i, d) in drop_dirs.iter().enumerate() {
            let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
            let nb = world.get(n);
            if !spread_dest(nb, n) {
                continue;
            }
            weights[i] = distance_to_drop_at(world, n, 4);
        }
        let min_w = *weights.iter().min().unwrap_or(&u32::MAX);
        if min_w == u32::MAX {
            // No reachable drop in 4 steps; spread to every
            // viable dir (MC: getSpread still includes all
            // ties at the sentinel distance).
            for (i, d) in drop_dirs.iter().enumerate() {
                let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
                let nb = world.get(n);
                if !spread_dest(nb, n) {
                    continue;
                }
                if weights[i] != u32::MAX {
                    continue; // someone had a real drop -> only they
                }
                put_water_update(
                    &mut updates,
                    n,
                    neighbor_level,
                    false,
                    max_range,
                );
            }
        } else {
            // Spread only to min-distance directions. MC
            // behaviour - water flows toward the nearest cliff
            // first instead of fanning radially.
            for (i, d) in drop_dirs.iter().enumerate() {
                if weights[i] != min_w {
                    continue;
                }
                let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
                put_water_update(
                    &mut updates,
                    n,
                    neighbor_level,
                    false,
                    max_range,
                );
            }
        }
    }
    // Phase 2: for every non-source flow cell not receiving a
    // fresh push write this tick, run MC's `getNewLiquid` pull
    // recompute. The cell takes max(neighbour amounts) - drop_off
    // (i.e. min(neighbour levels) + drop_off in our inverted
    // convention). If no liquid neighbour exists at all, the cell
    // evaporates immediately - faster than the old +1 decay,
    // which leaked a 7-tick tail when a source disappeared.
    for pos in &candidates {
        let level = level_map.get(pos).copied().unwrap_or(0);
        if level == 0 {
            continue;
        }
        if updates.contains_key(pos) {
            continue;
        }
        match liquid_get_new_state(world, level_map, *pos, liquid) {
            Some((new_level, _falling)) if new_level != level => {
                plan.decay.push((*pos, new_level));
            }
            Some(_) => {
                // Unchanged - skip the write so the mesh stays clean.
            }
            None => {
                plan.evaporate.push(*pos);
            }
        }
    }
    plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
    plan
}

/// MC `FlowingFluid.getNewLiquid` port. Pulls the cell's
/// "preferred" state from its surroundings: a same-liquid cell
/// directly above turns this cell into a falling column at
/// amount 8 (our level 8 marker); otherwise the level is
/// `min(neighbour_levels) + drop_off`. Returns `None` when the
/// cell should evaporate (no liquid neighbour + no column
/// above, or the new level would exceed `max_range`).
///
/// Source promotion (neighbourSources >= 2 + below solid /
/// source) lives in the separate `liquid_merge_step` with our
/// stricter stability check, so it is NOT replicated here.
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
    // (lowest level in our convention). Falling neighbours
    // contribute as level 1 (MC: FALLING override -> amount 7).
    let dirs: [[i32; 3]; 4] =
        [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    let mut min_n: Option<u8> = None;
    for d in &dirs {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        if world.get(n).0 != liquid_id {
            continue;
        }
        let nl = level_map.get(&n).copied().unwrap_or(0);
        let effective = if nl >= 8 { 1 } else { nl };
        min_n = Some(min_n.map_or(effective, |m| m.min(effective)));
    }
    let m = min_n?;
    let new_level = m.saturating_add(liquid.drop_off);
    if new_level > liquid.max_range {
        return None;
    }
    Some((new_level, false))
}

/// Apply a `LiquidPlanStep` to the world. Mutates `world`,
/// `level_map`, `active`, `positions`. Returns a `LiquidTape`
/// describing what actually changed (the apply phase enforces
/// "sources are sacred" + "no-op write" guards, so not every
/// proposed place lands).
pub(crate) fn liquid_apply_plan(
    world: &mut World,
    level_map: &mut HashMap<[i32; 3], u8>,
    active: &mut HashSet<[i32; 3]>,
    positions: &mut HashSet<[i32; 3]>,
    liquid: &Liquid,
    plan: LiquidPlanStep,
) -> LiquidTape {
    let liquid_id = liquid.block_id;
    let mut tape = LiquidTape::default();
    for pos in plan.evaporate {
        world.set(pos, BlockId::AIR);
        level_map.remove(&pos);
        active.remove(&pos);
        positions.remove(&pos);
        tape.evaporated.push(pos);
    }
    for (pos, lvl) in plan.decay {
        level_map.insert(pos, lvl);
        tape.decayed.push((pos, lvl));
    }
    for (pos, level, falling) in plan.place {
        let existing = world.get(pos);
        let existing_level = level_map.get(&pos).copied().unwrap_or(0);
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
    }
    tape
}

/// Pure source-merge pass. Returns positions promoted to source
/// this tick (caller writes `level = 0` + broadcasts).
pub(crate) fn liquid_merge_step(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    liquid: &Liquid,
) -> Vec<[i32; 3]> {
    let liquid_id = liquid.block_id;
    let mut promoted: Vec<[i32; 3]> = Vec::new();
    let candidates: Vec<[i32; 3]> = level_map
        .iter()
        .filter(|(_, d)| **d > 0)
        .take(256)
        .map(|(v, _)| *v)
        .collect();
    for pos in &candidates {
        if world.get(*pos).0 != liquid_id {
            continue;
        }
        let horiz = [
            [pos[0] + 1, pos[1], pos[2]],
            [pos[0] - 1, pos[1], pos[2]],
            [pos[0], pos[1], pos[2] + 1],
            [pos[0], pos[1], pos[2] - 1],
        ];
        let mut sources = 0u8;
        let mut stable = true;
        for n in &horiz {
            let nb = world.get(*n);
            if nb.0 == liquid_id {
                let nd = level_map.get(n).copied().unwrap_or(0);
                if nd == 0 {
                    sources += 1;
                } else {
                    stable = false;
                }
            } else if nb.is_air() || block_is_washable(nb) {
                stable = false;
            }
        }
        if sources >= 2 && stable {
            promoted.push(*pos);
        }
    }
    promoted
}

/// Append one quad to `verts` / `idx`. Corners must be CCW from
/// the `normal` side. Two triangles, four vertices, six indices.
/// Writes a per-cell flow direction into the `uv` slot - water
/// meshes don't sample the albedo texture so UV is repurposed as
/// the scroll vector the animated water shader reads.
//
// TODO(phase A2 follow-up): per the Phase A split plan
// (docs/design/phase_a_split_detailed.md), normalise quad winding
// here so every face is CCW from the outward side, then re-enable
// `cull_mode: Some(wgpu::Face::Back)` on the transparent pipeline.
// Out of scope for the pure module-move PR.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_quad_flow(
    verts: &mut Vec<MeshVertex>,
    idx: &mut Vec<u32>,
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
    v3: [f32; 3],
    normal: [f32; 3],
    flow: [f32; 2],
) {
    let base = verts.len() as u32;
    verts.push(MeshVertex::new(v0, normal).with_uv(flow));
    verts.push(MeshVertex::new(v1, normal).with_uv(flow));
    verts.push(MeshVertex::new(v2, normal).with_uv(flow));
    verts.push(MeshVertex::new(v3, normal).with_uv(flow));
    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod water_tests {
    //! Behaviour tests for the liquid simulation. The pure
    //! helpers (`liquid_surface_height_at`, `distance_to_drop_at`,
    //! `liquid_corner_height_at`) are exercised directly; the
    //! tick loop is driven through `LiquidSim`, a minimal
    //! wrapper around the same `liquid_plan_step` /
    //! `liquid_apply_plan` / `liquid_merge_step` free fns the
    //! live `Voxel::tick_liquid_step` calls. Side effects
    //! (renderer / network / drops / edit history) are skipped -
    //! we only care about world state transitions here.
    use super::*;
    use infengine_voxel::{BlockId, Chunk, World};
    use std::collections::{HashMap, HashSet};

    /// Side-effect-free liquid simulator for tests. Holds the
    /// minimal state set the pure planner/applier need, no
    /// renderer / network / mesh hookups.
    struct LiquidSim {
        world: World,
        level: HashMap<[i32; 3], u8>,
        active: HashSet<[i32; 3]>,
        positions: HashSet<[i32; 3]>,
        bias_drops: bool,
    }

    impl LiquidSim {
        fn new() -> Self {
            // 3x3x3 chunk grid around origin -> writable coords
            // [-16 .. 32) on each axis. Plenty of room for a
            // 7-radius pool + a 10-tall pillar.
            let mut w = World::new();
            for cx in -1..=1 {
                for cy in -1..=1 {
                    for cz in -1..=1 {
                        w.insert_chunk([cx, cy, cz], Chunk::empty());
                    }
                }
            }
            Self {
                world: w,
                level: HashMap::new(),
                active: HashSet::new(),
                positions: HashSet::new(),
                // Real game runs with bias=true; tests on a flat
                // floor stay deterministic because no cell can
                // see a cliff within 4 steps -> bias never fires.
                bias_drops: true,
            }
        }
        fn solid(&mut self, p: [i32; 3]) {
            // Plain non-air, non-washable block. `BlockId(1)` is
            // dirt in the demo's table - flags = F_NONE, fine
            // for "is a wall".
            self.world.set(p, BlockId(1));
        }
        fn fill_floor(
            &mut self,
            y: i32,
            x: std::ops::RangeInclusive<i32>,
            z: std::ops::RangeInclusive<i32>,
        ) {
            for xx in x {
                for zz in z.clone() {
                    self.solid([xx, y, zz]);
                }
            }
        }
        fn place_source(&mut self, p: [i32; 3], liquid: &Liquid) {
            self.world.set(p, BlockId(liquid.block_id));
            self.level.insert(p, 0);
            self.active.insert(p);
            self.positions.insert(p);
        }
        fn remove_voxel(&mut self, p: [i32; 3]) {
            self.world.set(p, BlockId::AIR);
            self.level.remove(&p);
            self.active.remove(&p);
            self.positions.remove(&p);
            // Mirror `world_set_tracked` neighbour wake-up so
            // adjacent liquid is re-queued for the next tick.
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let n = [p[0] + dx, p[1] + dy, p[2] + dz];
                        if self.positions.contains(&n) {
                            self.active.insert(n);
                        }
                    }
                }
            }
        }
        fn tick(&mut self, liquid: &Liquid) {
            let plan = liquid_plan_step(
                &self.world,
                &self.level,
                &self.active,
                liquid,
                self.bias_drops,
            );
            let _ = liquid_apply_plan(
                &mut self.world,
                &mut self.level,
                &mut self.active,
                &mut self.positions,
                liquid,
                plan,
            );
            if liquid.infinite_source {
                let promoted =
                    liquid_merge_step(&self.world, &self.level, liquid);
                for p in promoted {
                    self.level.insert(p, 0);
                }
            }
        }
        fn run(&mut self, liquid: &Liquid, ticks: u32) {
            for _ in 0..ticks {
                self.tick(liquid);
            }
        }
        fn is(&self, p: [i32; 3], liquid: &Liquid) -> bool {
            self.world.get(p).0 == liquid.block_id
        }
        fn level_at(&self, p: [i32; 3]) -> Option<u8> {
            self.level.get(&p).copied()
        }
    }

    fn water() -> &'static Liquid {
        &LIQUIDS[0]
    }
    fn mk_world() -> World {
        // Single chunk at the origin so coords [0..16) on each
        // axis are writable.
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        w
    }
    #[test]
    fn surface_source_is_full_block() {
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        let mut map = HashMap::new();
        map.insert([5, 5, 5], 0u8);
        // Level 0 (source) renders at full height per the
        // bucket-pour spec.
        assert!(
            (liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID) - 1.0)
                .abs()
                < 1e-4
        );
    }
    #[test]
    fn surface_flow_levels_ramp_down() {
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        let mut map = HashMap::new();
        for lvl in 1u8..=7 {
            map.insert([5, 5, 5], lvl);
            let h =
                liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID);
            let expected = (8 - lvl as i32).max(1) as f32 / 9.0;
            assert!(
                (h - expected).abs() < 1e-4,
                "level {} -> {:.3}, want {:.3}",
                lvl,
                h,
                expected
            );
        }
    }
    #[test]
    fn surface_falling_or_column_full_block() {
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        w.set([5, 6, 5], BlockId(WATER_ID)); // water above
        let mut map = HashMap::new();
        map.insert([5, 5, 5], 7u8); // weakest flow, but column above
        // Column above forces full height regardless of level.
        let h = liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID);
        assert!((h - 1.0).abs() < 1e-4);
        // Falling marker (level 8) without column also renders full.
        let mut w2 = mk_world();
        w2.set([5, 5, 5], BlockId(WATER_ID));
        let mut map2 = HashMap::new();
        map2.insert([5, 5, 5], 8u8);
        let h2 = liquid_surface_height_at(&w2, &map2, [5, 5, 5], WATER_ID);
        assert!((h2 - 1.0).abs() < 1e-4);
    }
    #[test]
    fn distance_to_drop_immediate() {
        // Air cell with air below -> distance 0.
        let w = mk_world();
        // [5, 5, 5] = air, below [5, 4, 5] = air (default).
        assert_eq!(distance_to_drop_at(&w, [5, 5, 5], 4), 0);
    }
    #[test]
    fn distance_to_drop_solid_floor_unreachable() {
        // Air cell on solid floor, with surrounding solid -> no drop.
        let mut w = mk_world();
        for x in 0..16 {
            for z in 0..16 {
                w.set([x, 4, z], BlockId(1));
            }
        }
        // [5, 5, 5] = air, but everywhere within 4 steps has
        // solid below.
        assert_eq!(distance_to_drop_at(&w, [5, 5, 5], 4), u32::MAX);
    }
    #[test]
    fn distance_to_drop_steps_to_cliff() {
        // Solid floor from x=0..=8, air beyond x=8.
        // Start at x=5: 3 steps to reach x=8, then cliff at x=9.
        let mut w = mk_world();
        for x in 0..=8 {
            for z in 0..16 {
                w.set([x, 4, z], BlockId(1));
            }
        }
        // From [5, 5, 5] going +X: cells 6, 7, 8 are on solid;
        // cell 9 has air below = drop. Distance = 4 (steps to
        // reach the drop cell from the start).
        let d = distance_to_drop_at(&w, [5, 5, 5], 6);
        assert!(d <= 4, "got {d}");
        assert!(d > 0);
    }
    #[test]
    fn corner_height_all_water_flat() {
        let mut w = mk_world();
        // 2x2 pool at y=5 all source.
        for x in 4..=5 {
            for z in 4..=5 {
                w.set([x, 5, z], BlockId(WATER_ID));
            }
        }
        let mut map = HashMap::new();
        for x in 4..=5 {
            for z in 4..=5 {
                map.insert([x, 5, z], 0u8);
            }
        }
        // Corner at (5, 6, 5) - shared by all 4 source cells.
        // No column above. Average of 4 source heights = 1.0.
        let h = liquid_corner_height_at(
            &w, &map, 4, 5, 4, 1, 1, WATER_ID,
        );
        assert!((h - 1.0).abs() < 1e-4, "got {h}");
    }
    #[test]
    fn corner_height_isolated_cell_tapers_slightly() {
        // Single source + 3 air corners: MC 10:1 liquid:air
        // weighting -> 10 / 13 ~ 0.77. Slight slope instead of
        // a sharp 0.25 pyramid OR a flat 1.0 cube; matches MC's
        // visible "tiny pool" look.
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        let mut map = HashMap::new();
        map.insert([5, 5, 5], 0u8);
        let h = liquid_corner_height_at(
            &w, &map, 5, 5, 5, 0, 0, WATER_ID,
        );
        assert!(
            (0.70..0.85).contains(&h),
            "isolated cell tip corner: got {h}, want ~0.77"
        );
        let h_far = liquid_corner_height_at(
            &w, &map, 5, 5, 5, 2, 2, WATER_ID,
        );
        assert!(h_far.abs() < 1e-4, "non-touching corner: got {h_far}");
    }

    #[test]
    fn corner_height_pool_joint_stays_high() {
        // Two adjacent sources form a tiny pool. The corner
        // shared between them (2 source + 2 air) must stay
        // visibly high - matches MC's 10:1 weighting (20 /
        // 22 ~ 0.91). Before the weighting fix this corner
        // came out at 0.5, lower than the outer-tip corners
        // at 0.25, so the surface visibly sloped UP toward
        // air instead of down.
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        w.set([6, 5, 5], BlockId(WATER_ID));
        let mut map = HashMap::new();
        map.insert([5, 5, 5], 0u8);
        map.insert([6, 5, 5], 0u8);
        // Corner shared by (5, 5, 5) and (6, 5, 5): 2 source + 2 air.
        let joint = liquid_corner_height_at(
            &w, &map, 5, 5, 5, 1, 0, WATER_ID,
        );
        // Outer tip of cell (5,5,5): 1 source + 3 air.
        let tip = liquid_corner_height_at(
            &w, &map, 5, 5, 5, 0, 0, WATER_ID,
        );
        assert!(
            joint > tip,
            "joint corner ({joint}) should be higher than tip ({tip}) - water slopes DOWN toward air"
        );
        assert!(
            (0.85..0.95).contains(&joint),
            "joint expected ~0.91, got {joint}"
        );
    }
    #[test]
    fn corner_height_column_above_locks_full() {
        let mut w = mk_world();
        w.set([5, 5, 5], BlockId(WATER_ID));
        w.set([5, 6, 5], BlockId(WATER_ID)); // column above the source
        let mut map = HashMap::new();
        map.insert([5, 5, 5], 7u8); // even with weakest flow
        map.insert([5, 6, 5], 0u8);
        // Phase 1 (any touching cell is a column) snaps corner to 1.0.
        let h = liquid_corner_height_at(
            &w, &map, 5, 5, 5, 0, 0, WATER_ID,
        );
        assert!((h - 1.0).abs() < 1e-4, "got {h}");
    }
    // Six behaviour scenarios from the user spec, run against
    // `LiquidSim`. Each test sets up a tiny world, places one
    // or two sources, ticks for a fixed budget, and asserts a
    // few cell-state invariants - never on the full pool
    // contents so the tests stay stable under tweaks to spread
    // ordering or candidate caps.

    #[test]
    fn pour_on_flat_spreads_seven_blocks() {
        // Flat solid floor, single source. After enough ticks
        // the pool reaches the source's NSEW at distance 7 and
        // never sets distance-8 cells (would-be level 8 =
        // beyond max_range = evaporate).
        let mut sim = LiquidSim::new();
        sim.fill_floor(0, -12..=12, -12..=12);
        sim.place_source([0, 1, 0], water());
        sim.run(water(), 20);
        assert!(sim.is([7, 1, 0], water()), "+x distance 7 missing");
        assert!(sim.is([-7, 1, 0], water()), "-x distance 7 missing");
        assert!(sim.is([0, 1, 7], water()), "+z distance 7 missing");
        assert!(sim.is([0, 1, -7], water()), "-z distance 7 missing");
        assert!(!sim.is([8, 1, 0], water()), "+x distance 8 should stay air");
        assert!(!sim.is([0, 1, 8], water()), "+z distance 8 should stay air");
        assert_eq!(sim.level_at([0, 1, 0]), Some(0), "source corrupted");
    }

    #[test]
    fn pour_off_pillar_falls_then_pools() {
        // Source on top of a 1x1 pillar surrounded by air,
        // basin floor far below. Vertical columns at the
        // pillar's NSEW must stay 1 voxel wide while falling -
        // mid-air cells beside the column stay air. Base pools
        // out radially.
        let mut sim = LiquidSim::new();
        // Basin floor.
        sim.fill_floor(-2, -8..=8, -8..=8);
        // Pillar from y=-1 to y=10 at column (0,_,0).
        for y in -1..=10 {
            sim.solid([0, y, 0]);
        }
        // Source one cell above the pillar's top.
        sim.place_source([0, 11, 0], water());
        sim.run(water(), 60);
        // Source intact.
        assert_eq!(sim.level_at([0, 11, 0]), Some(0));
        // Falling column at +x of pillar, mid-air at y=5.
        assert!(sim.is([1, 5, 0], water()), "+x column gap at y=5");
        // Column is 1 voxel wide - no horizontal spread mid-air.
        assert!(
            !sim.is([2, 5, 0], water()),
            "column fanned out mid-air at +x"
        );
        assert!(
            !sim.is([1, 5, 1], water()),
            "column fanned out mid-air at +z of +x column"
        );
        // Base pool exists at y=-1 around the drop points.
        assert!(sim.is([1, -1, 0], water()), "+x base pool missing");
        assert!(sim.is([-1, -1, 0], water()), "-x base pool missing");
        assert!(sim.is([0, -1, 1], water()), "+z base pool missing");
    }

    #[test]
    fn two_sources_closed_one_three_make_middle_source() {
        // 1x3 sealed trench, sources at both ends. The middle
        // cell becomes flow on tick 1 and then promotes to
        // source on the merge pass (stable pocket: 2 source
        // neighbours, every other neighbour solid wall).
        let mut sim = LiquidSim::new();
        for x in 0..=2 {
            sim.solid([x, -1, 0]); // floor under each trench cell
            sim.solid([x, 0, 1]); // +z wall
            sim.solid([x, 0, -1]); // -z wall
        }
        sim.solid([-1, 0, 0]); // -x cap
        sim.solid([3, 0, 0]); // +x cap
        sim.place_source([0, 0, 0], water());
        sim.place_source([2, 0, 0], water());
        sim.run(water(), 10);
        assert!(
            sim.is([1, 0, 0], water()),
            "middle cell didn't fill"
        );
        assert_eq!(
            sim.level_at([1, 0, 0]),
            Some(0),
            "middle cell didn't promote to source"
        );
    }

    #[test]
    fn two_sources_open_drain_no_middle_source() {
        // Same 1x3 setup but one z-wall has a hole the middle
        // cell can drain through. Stability check fails (open
        // air neighbour) so the merge never promotes.
        let mut sim = LiquidSim::new();
        for x in 0..=2 {
            sim.solid([x, -1, 0]);
            sim.solid([x, 0, -1]); // -z wall intact
        }
        // +z wall: hole at the middle cell so water drains.
        sim.solid([0, 0, 1]);
        // (no solid at [1, 0, 1]) - that's the drain.
        sim.solid([2, 0, 1]);
        // Drain has a deep shaft so water keeps falling and the
        // drain cell stays a falling flow neighbour of [1,0,0].
        for y in -8..=-2 {
            // 3x3 air shaft below the drain - nothing to set,
            // but we want no floor there.
            let _ = y;
        }
        // Caps.
        sim.solid([-1, 0, 0]);
        sim.solid([3, 0, 0]);
        sim.place_source([0, 0, 0], water());
        sim.place_source([2, 0, 0], water());
        sim.run(water(), 20);
        assert!(
            sim.is([1, 0, 0], water()),
            "middle cell didn't fill"
        );
        assert_ne!(
            sim.level_at([1, 0, 0]),
            Some(0),
            "middle wrongly promoted despite open drain"
        );
    }

    #[test]
    fn source_removal_drains_gradually() {
        // Fill a small pool, remove the source, tick long
        // enough for every flow cell to evaporate.
        let mut sim = LiquidSim::new();
        sim.fill_floor(0, -10..=10, -10..=10);
        sim.place_source([0, 1, 0], water());
        sim.run(water(), 20);
        // Pool exists before removal.
        assert!(
            sim.is([5, 1, 0], water()),
            "pool didn't fill before source removal"
        );
        // Remove source.
        sim.remove_voxel([0, 1, 0]);
        // Run long enough to drain (max_range=7 -> ~7 decay
        // steps in the worst case + some slack).
        sim.run(water(), 50);
        // Whole pool gone.
        for x in -8..=8 {
            for z in -8..=8 {
                let p = [x, 1, z];
                assert!(
                    !sim.is(p, water()),
                    "{p:?} still water after drain"
                );
            }
        }
    }

    #[test]
    fn source_on_cliff_no_flicker() {
        // Bug repro: source perched one voxel above an open
        // cliff. Tick 1: source's below = air -> writes below
        // as falling. Tick 2: below = water -> source must keep
        // refreshing it as falling (column propagation through
        // source). Before the fix, the orphan cell decayed +
        // evaporated, source re-placed it next tick, etc -
        // cell flickered between water and air every tick.
        //
        // Run many ticks then verify the column under the
        // source stays water continuously across a sliding
        // window of ticks - no single-tick gap.
        let mut sim = LiquidSim::new();
        // Solid bedrock far below so the column eventually
        // pools. Source perched on a 1x1 platform at y=10
        // with air around and a basin at y=-2.
        sim.fill_floor(-2, -6..=6, -6..=6);
        sim.solid([0, 9, 0]); // platform under source
        sim.place_source([0, 10, 0], water());
        // Settle.
        sim.run(water(), 30);
        // Cell directly below source must stay water across
        // every tick from here on.
        for _ in 0..10 {
            sim.tick(water());
            assert!(
                sim.is([0, 10, 0], water()),
                "source itself vanished"
            );
            // [0, 9, 0] is the solid platform; column actually
            // starts at the source's NSEW cells which fall to
            // the basin. Pick one and assert it stays water.
            assert!(
                sim.is([1, 10, 0], water()),
                "+x falling cell flickered"
            );
        }
    }

    #[test]
    fn source_above_air_no_flicker() {
        // Stricter repro: source at y=5 with NOTHING directly
        // below until y=-2 floor. The cell at y=4 should stay
        // water every tick.
        let mut sim = LiquidSim::new();
        sim.fill_floor(-2, -6..=6, -6..=6);
        sim.place_source([0, 5, 0], water());
        sim.run(water(), 15);
        for _ in 0..10 {
            sim.tick(water());
            assert!(
                sim.is([0, 4, 0], water()),
                "cell directly below source flickered (source-on-cliff bug)"
            );
        }
    }

    #[test]
    fn two_streams_meet_strongest_wins() {
        // Asymmetric source distances so the cell where the
        // streams meet has an unambiguous stronger side.
        // Source A at x=-3, source B at x=+4. Cell at x=0 is
        // 3 steps from A, 4 steps from B -> level should be
        // min(3, 4) = 3 thanks to `put_water_update`.
        let mut sim = LiquidSim::new();
        sim.fill_floor(0, -10..=10, -10..=10);
        sim.place_source([-3, 1, 0], water());
        sim.place_source([4, 1, 0], water());
        sim.run(water(), 25);
        let lvl = sim.level_at([0, 1, 0]).expect("meeting cell empty");
        assert_eq!(
            lvl, 3,
            "expected stronger (-3) side to win with level 3, got {lvl}"
        );
    }
}
