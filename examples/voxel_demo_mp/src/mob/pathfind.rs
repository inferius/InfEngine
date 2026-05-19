//! A* pathfinding on the voxel grid.
//!
//! Mirrors MC's `net/minecraft/world/level/pathfinder/PathFinder.java`
//! at a coarse level: A* over a 1-block grid with `BlockPathTypes`
//! cost weights. We simplify to a fixed cost table:
//!
//! - 1.0  per orthogonal step
//! - 1.4  per diagonal step
//! - +0.5 step-up by 1 block
//! - +inf step-up by 2+ blocks (mob can't jump that high)
//! - +0.3 fall 1-3 blocks
//! - +inf fall 4+ blocks (would damage)
//!
//! Heuristic: Manhattan distance (octile is overkill for a demo).
//! Optimisation: caller bounds `max_steps` (default 256) so a
//! single mob can't blow the AI tick budget when chasing across a
//! continent.
//!
//! Returns: `Vec<IVec3>` from `start` (exclusive) to `goal`
//! (inclusive). `None` when no path exists within `max_steps`.
//!
//! Wired into the live mob tick by H.5's `ChasePlayerGoal` - until
//! then `find_path` is exercised by the test module only.

#![allow(dead_code)]

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use infengine_voxel::World;

/// Direction set: 8 horizontal neighbours (orth + diag). Vertical
/// changes are inferred from the `step_up` / `fall` cost rules - a
/// move from `(x, y, z)` to `(x+1, ?, z)` may step up 1 or fall by
/// up to 3 depending on the column profile.
const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Cost for a step-up of one block. Matches MC's
/// `WalkNodeEvaluator::JUMP_PENALTY` flavour - small bias against
/// jumpy paths so the mob prefers flat terrain when both are
/// equally short.
const STEP_UP_COST: f32 = 0.5;
/// Bonus cost per block fallen (0-3 blocks). Falling 4+ blocks is
/// rejected outright by `try_move`.
const FALL_COST_PER_BLOCK: f32 = 0.3;
/// Hard ceiling on the fall depth a mob will accept. Past this the
/// fall would damage the mob, so the path planner refuses the
/// edge.
const MAX_FALL: i32 = 3;

/// Per-move cost. Returned by `try_move` along with the resolved
/// destination Y. The Y may differ from `(start_y +/- step)`
/// because the mob slides down a slope or steps up onto a 1-block
/// rise.
#[derive(Debug, Clone, Copy)]
struct Move {
    pos: [i32; 3],
    cost: f32,
}

/// Heap entry: (f_score, ordered_key, idx) where smaller f wins.
/// We negate the float for the max-heap so the smallest f comes
/// out first.
#[derive(Debug, Clone, Copy)]
struct HeapEntry {
    neg_f: f32,
    seq: u32,
    pos: [i32; 3],
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.neg_f == other.neg_f && self.seq == other.seq
    }
}
impl Eq for HeapEntry {}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.neg_f
            .partial_cmp(&other.neg_f)
            .unwrap_or(Ordering::Equal)
            .then(self.seq.cmp(&other.seq))
    }
}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Manhattan distance + diagonal correction. Matches MC's
/// `PathFinder::distance` shape.
fn heuristic(from: [i32; 3], to: [i32; 3]) -> f32 {
    let dx = (from[0] - to[0]).abs();
    let dy = (from[1] - to[1]).abs();
    let dz = (from[2] - to[2]).abs();
    // Octile-ish: walk diagonals where possible, then straight.
    let min = dx.min(dz);
    let max = dx.max(dz);
    1.4 * min as f32 + (max - min) as f32 + dy as f32
}

/// True when a cell at `wpos` has the structural shape a mob can
/// stand in: the cell + the cell above are air, the cell below is
/// solid. We don't gate on biome or light here - the path is
/// purely structural.
fn is_walkable_cell(world: &World, wpos: [i32; 3]) -> bool {
    let here = world.get(wpos).is_air();
    let head = world.get([wpos[0], wpos[1] + 1, wpos[2]]).is_air();
    let floor = !world.get([wpos[0], wpos[1] - 1, wpos[2]]).is_air();
    here && head && floor
}

/// Try moving from `from` toward `(from.x + dx, ?, from.z + dz)`.
/// Resolves the destination Y by trying `from.y + 1` (step-up),
/// `from.y` (flat), then `from.y - 1..from.y - MAX_FALL` (fall).
/// Returns `None` when no walkable destination exists in the
/// allowed Y range.
fn try_move(world: &World, from: [i32; 3], dx: i32, dz: i32) -> Option<Move> {
    let nx = from[0] + dx;
    let nz = from[2] + dz;
    // Step up by 1.
    {
        let candidate = [nx, from[1] + 1, nz];
        if is_walkable_cell(world, candidate) {
            let base = if dx.abs() + dz.abs() == 2 { 1.4 } else { 1.0 };
            return Some(Move {
                pos: candidate,
                cost: base + STEP_UP_COST,
            });
        }
    }
    // Flat.
    {
        let candidate = [nx, from[1], nz];
        if is_walkable_cell(world, candidate) {
            let base = if dx.abs() + dz.abs() == 2 { 1.4 } else { 1.0 };
            return Some(Move {
                pos: candidate,
                cost: base,
            });
        }
    }
    // Fall by 1..MAX_FALL.
    for fall in 1..=MAX_FALL {
        let candidate = [nx, from[1] - fall, nz];
        if is_walkable_cell(world, candidate) {
            let base = if dx.abs() + dz.abs() == 2 { 1.4 } else { 1.0 };
            return Some(Move {
                pos: candidate,
                cost: base + FALL_COST_PER_BLOCK * fall as f32,
            });
        }
    }
    None
}

/// Reconstruct the path from the `came_from` chain. `goal` is
/// included, `start` is omitted (caller already stands there).
fn reconstruct(
    came_from: &HashMap<[i32; 3], [i32; 3]>,
    goal: [i32; 3],
    start: [i32; 3],
) -> Vec<[i32; 3]> {
    let mut path = Vec::new();
    let mut cur = goal;
    while cur != start {
        path.push(cur);
        match came_from.get(&cur) {
            Some(prev) => cur = *prev,
            None => break,
        }
    }
    path.reverse();
    path
}

/// A* search from `start` to `goal` over walkable voxel cells.
/// Returns the path (exclusive of `start`, inclusive of `goal`)
/// when reachable in <= `max_steps` open-list pops, `None`
/// otherwise.
pub(crate) fn find_path(
    world: &World,
    start: [i32; 3],
    goal: [i32; 3],
    max_steps: usize,
) -> Option<Vec<[i32; 3]>> {
    if start == goal {
        return Some(Vec::new());
    }
    let mut open = BinaryHeap::new();
    let mut g_score: HashMap<[i32; 3], f32> = HashMap::new();
    let mut came_from: HashMap<[i32; 3], [i32; 3]> = HashMap::new();
    let mut seq: u32 = 0;
    g_score.insert(start, 0.0);
    open.push(HeapEntry {
        neg_f: -heuristic(start, goal),
        seq,
        pos: start,
    });
    let mut popped = 0usize;
    while let Some(e) = open.pop() {
        popped += 1;
        if popped > max_steps {
            return None;
        }
        let here = e.pos;
        if here == goal {
            return Some(reconstruct(&came_from, goal, start));
        }
        let here_g = *g_score.get(&here).unwrap_or(&f32::INFINITY);
        for (dx, dz) in DIRS {
            let Some(m) = try_move(world, here, dx, dz) else {
                continue;
            };
            let tentative = here_g + m.cost;
            let cur = g_score.get(&m.pos).copied().unwrap_or(f32::INFINITY);
            if tentative < cur {
                g_score.insert(m.pos, tentative);
                came_from.insert(m.pos, here);
                let f = tentative + heuristic(m.pos, goal);
                seq = seq.wrapping_add(1);
                open.push(HeapEntry {
                    neg_f: -f,
                    seq,
                    pos: m.pos,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! World-driven A* tests. We build a small `World` by hand
    //! (one chunk's worth of voxels) so the planner sees a real
    //! 3D environment without leaning on the worldgen.

    use super::*;
    use infengine_voxel::{BlockId, Chunk, ChunkCoord};

    /// Build a fresh world with one chunk of stone at y=0 + air above.
    fn flat_world() -> World {
        let mut w = World::new();
        // Mint one chunk at (0,0,0) so set() can write to it. The
        // chunks at other Y coords are minted on demand.
        for cc in [
            [0, 0, 0],
            [-1, 0, 0],
            [0, 0, -1],
            [-1, 0, -1],
            [1, 0, 0],
            [0, 0, 1],
            [1, 0, 1],
            [1, 0, -1],
            [-1, 0, 1],
        ] {
            w.insert_chunk(cc as ChunkCoord, Chunk::empty());
        }
        // Fill y=0 with stone over a 32x32 area, leave y>=1 as air.
        for x in -16..16 {
            for z in -16..16 {
                w.set([x, 0, z], BlockId(1));
            }
        }
        w
    }

    #[test]
    fn straight_line_path_on_flat_terrain() {
        let w = flat_world();
        let start = [0, 1, 0];
        let goal = [5, 1, 0];
        let path = find_path(&w, start, goal, 256).expect("path should exist");
        // First waypoint is one step toward the goal; last is the
        // goal cell itself.
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), goal);
        // No step should jump more than 1.5 blocks of XZ distance.
        let mut prev = start;
        for w in &path {
            let dx = (w[0] - prev[0]).abs();
            let dz = (w[2] - prev[2]).abs();
            assert!(dx <= 1 && dz <= 1);
            prev = *w;
        }
    }

    #[test]
    fn diagonal_step_is_one_cell_per_axis() {
        let w = flat_world();
        let start = [0, 1, 0];
        let goal = [3, 1, 3];
        let path = find_path(&w, start, goal, 256).expect("path should exist");
        // Pure diagonal on flat ground: length is exactly 3 (one
        // step per (dx=1, dz=1)).
        assert_eq!(path.len(), 3);
        assert_eq!(*path.last().unwrap(), goal);
    }

    #[test]
    fn obstacle_wall_forces_detour() {
        let mut w = flat_world();
        // Build a 3-block-tall wall between start (-2,1,0) and goal
        // (+2,1,0): stone at (0, 1..4, -1..2) ish. Leaves an
        // opening south of the wall so the mob can go around.
        for y in 1..=3 {
            for z in -2..=1 {
                w.set([0, y, z], BlockId(1));
            }
        }
        let start = [-2, 1, 0];
        let goal = [2, 1, 0];
        let path = find_path(&w, start, goal, 1024).expect("path should exist");
        // Path goes through column z >= 2 to dodge the wall. No
        // step lands at (0, 1, z) where z in -2..=1.
        for w in &path {
            if w[0] == 0 && w[1] == 1 {
                assert!(w[2] >= 2 || w[2] <= -3, "stepped into wall: {:?}", w);
            }
        }
    }

    #[test]
    fn step_up_one_block_is_allowed() {
        let mut w = flat_world();
        // Stair step at x=1 + x=2: stone fills both columns to
        // y=1, so the mob has to step up onto the platform at y=2
        // to walk across.
        w.set([1, 1, 0], BlockId(1));
        w.set([2, 1, 0], BlockId(1));
        let start = [0, 1, 0];
        let goal = [2, 2, 0];
        let path = find_path(&w, start, goal, 256).expect("stair step should pass");
        // The first waypoint should be at y=2 (stepped up onto the
        // stone) since x=1 is now solid at y=1 and the mob can't
        // walk through it.
        assert!(path.iter().any(|w| w[1] == 2));
        assert_eq!(*path.last().unwrap(), goal);
    }

    #[test]
    fn step_up_two_blocks_rejected() {
        let mut w = flat_world();
        // Two-block-tall pillar at x=1 blocks the only line of
        // approach. From (0,1,0) toward (2,1,0) the only option
        // would be to step over a 2-block wall.
        // Build a long, tall wall the planner can't go around
        // within max_steps.
        for z in -32..32 {
            for y in 1..=2 {
                w.set([1, y, z], BlockId(1));
            }
        }
        let start = [0, 1, 0];
        let goal = [2, 1, 0];
        // Strict budget so the planner can't go around: with
        // max_steps=256 the planner can still navigate a 32-cell
        // detour. With the wall extending the full 64-cell width
        // here, the detour is way longer than max_steps allows.
        let path = find_path(&w, start, goal, 64);
        assert!(path.is_none(), "should not climb 2-block wall");
    }

    #[test]
    fn unreachable_goal_returns_none_within_max_steps() {
        let w = flat_world();
        // Goal at the same Y but far enough that 32 expansions
        // can't reach it: 64 cells away.
        let start = [0, 1, 0];
        let goal = [200, 1, 0];
        // Even on flat ground the planner will run out of budget
        // before expanding 200 cells.
        let path = find_path(&w, start, goal, 32);
        assert!(path.is_none());
    }

    #[test]
    fn start_equals_goal_returns_empty_path() {
        let w = flat_world();
        let path = find_path(&w, [0, 1, 0], [0, 1, 0], 256).unwrap();
        assert!(path.is_empty());
    }

    #[test]
    fn heuristic_matches_diagonal_cost() {
        // 3-diag step + 2 orth step = 3 * 1.4 + 2 = 6.2.
        let h = heuristic([0, 0, 0], [3, 0, 5]);
        assert!((h - 6.2).abs() < 0.01, "{}", h);
    }

    #[test]
    fn fall_one_block_is_cheap() {
        let mut w = flat_world();
        // Dig a pit at (1, 0, 0): remove the stone so the floor
        // is now at y=-1.
        w.set([1, 0, 0], BlockId(0));
        // Mint the chunk below so the y=-1 floor block lives in
        // a real chunk.
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let cc = [1i32.div_euclid(cs), (-1i32).div_euclid(cs), 0i32.div_euclid(cs)];
        if w.chunk(cc).is_none() {
            w.insert_chunk(cc, Chunk::empty());
        }
        w.set([1, -1, 0], BlockId(1));
        // Now (1, 0, 0) has stone-floor at y=-1 + two air cells
        // stacked. The path planner should fall into it.
        let start = [0, 1, 0];
        let goal = [1, 0, 0];
        let path = find_path(&w, start, goal, 256).expect("should fall");
        assert_eq!(*path.last().unwrap(), goal);
    }

    #[test]
    fn very_short_max_steps_returns_none() {
        let w = flat_world();
        let path = find_path(&w, [0, 1, 0], [10, 1, 10], 1);
        assert!(path.is_none());
    }
}
