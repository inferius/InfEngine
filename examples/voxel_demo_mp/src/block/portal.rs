//! Nether portal frame detection + ignition (W273: the full
//! `PortalShape.java` port) plus cross-dim teleport orchestration.
//!
//! Ignition mirrors MC exactly:
//! 1. Drop from the hit cell to the frame floor (<= 21 empties).
//! 2. Walk left over frame-floored empties to the BOTTOM-LEFT
//!    corner, then measure the width to the right edge (2..21;
//!    every interior column must sit on obsidian).
//! 3. Measure the height (3..21): both side walls obsidian, all
//!    interior cells empty, a full obsidian top row.
//! 4. An "empty" cell is air, fire or an existing portal block -
//!    so re-igniting through fire works like MC.
//! 5. Fill the interior with `PORTAL_ID` and hand the caller the
//!    exact cell list for the edit broadcast.
//!
//! The X axis is preferred, then Z (`findEmptyPortalShape`'s
//! order when fire lands). The fn is pure on `(World, hit_pos)`
//! so unit tests build tiny hand-laid frames.

use infengine_voxel::{BlockId, World};

use super::{OBSIDIAN_ID, PORTAL_ID};

/// `PortalShape` frame limits.
pub(crate) const MIN_WIDTH: i32 = 2;
pub(crate) const MAX_WIDTH: i32 = 21;
pub(crate) const MIN_HEIGHT: i32 = 3;
pub(crate) const MAX_HEIGHT: i32 = 21;

/// Axis the portal plane runs along (MC `Direction.Axis`): `Xy`
/// = the frame spans X (faces +/-Z), `Zy` spans Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortalPlane {
    Xy,
    Zy,
}

/// Result of a portal-ignition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IgniteResult {
    /// Portal lit; `cells` is the exact interior (already edited
    /// in place) for the caller's edit broadcast.
    Ok { plane: PortalPlane, cells: Vec<[i32; 3]> },
    /// No obsidian near the hit cell - nothing that could be a
    /// frame (the caller falls through to plain fire placement).
    NoAdjacentObsidian,
    /// A frame-like anchor exists but the shape is invalid
    /// (wrong size, missing wall/top, interior blocked - the
    /// PortalShape walk folds perimeter gaps in here too).
    NotRectangular,
    /// The cell already holds a portal block (idempotent no-op).
    AlreadyLit,
}

impl IgniteResult {
    #[cfg(test)]
    pub(crate) fn is_ok(&self) -> bool {
        matches!(self, IgniteResult::Ok { .. })
    }
}

/// `PortalShape.isEmpty`: air, fire (both kinds) or portal.
fn is_empty_cell(id: u16) -> bool {
    // 49 = fire, 56 = soul_fire (BlockTags.FIRE).
    id == 0 || id == 49 || id == 56 || id == PORTAL_ID
}

fn is_frame(id: u16) -> bool {
    id == OBSIDIAN_ID
}

/// A validated (or failed) portal shape - `PortalShape.java`.
pub(crate) struct PortalShape {
    pub(crate) plane: PortalPlane,
    /// Interior bottom-left cell.
    pub(crate) bottom_left: [i32; 3],
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) portal_blocks: i32,
}

impl PortalShape {
    pub(crate) fn is_valid(&self) -> bool {
        (MIN_WIDTH..=MAX_WIDTH).contains(&self.width)
            && (MIN_HEIGHT..=MAX_HEIGHT).contains(&self.height)
    }

    /// The interior cell list (width x height in the plane).
    pub(crate) fn cells(&self) -> Vec<[i32; 3]> {
        let (dx, dz) = right_step(self.plane);
        let mut out = Vec::with_capacity((self.width * self.height) as usize);
        for w in 0..self.width {
            for h in 0..self.height {
                out.push([
                    self.bottom_left[0] + dx * w,
                    self.bottom_left[1] + h,
                    self.bottom_left[2] + dz * w,
                ]);
            }
        }
        out
    }
}

/// The plane's "right" direction (MC: axis X -> WEST, axis Z ->
/// SOUTH).
fn right_step(plane: PortalPlane) -> (i32, i32) {
    match plane {
        PortalPlane::Xy => (-1, 0),
        PortalPlane::Zy => (0, 1),
    }
}

/// `PortalShape.findAnyShape` for one axis.
pub(crate) fn find_any_shape(world: &World, pos: [i32; 3], plane: PortalPlane) -> PortalShape {
    let (rx, rz) = right_step(plane);
    let fail = |anchor: [i32; 3]| PortalShape {
        plane,
        bottom_left: anchor,
        width: 0,
        height: 0,
        portal_blocks: 0,
    };
    // calculateBottomLeft: sink to the frame floor (<= 21 down,
    // clamped to the world floor), then walk left over
    // frame-floored empties.
    let mut p = pos;
    let min_y = (pos[1] - 21).max(-64);
    while p[1] > min_y && is_empty_cell(world.get([p[0], p[1] - 1, p[2]]).0) {
        p[1] -= 1;
    }
    let (lx, lz) = (-rx, -rz);
    let edge = distance_until_edge(world, p, lx, lz) - 1;
    if edge < 0 {
        return fail(p);
    }
    let bottom_left = [p[0] + lx * edge, p[1], p[2] + lz * edge];
    // calculateWidth.
    let width = distance_until_edge(world, bottom_left, rx, rz);
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return fail(bottom_left);
    }
    // calculateHeight: rows of wall/interior checks, then the top
    // frame row.
    let mut portal_blocks = 0;
    let mut height = MAX_HEIGHT;
    'rows: for h in 0..MAX_HEIGHT {
        let y = bottom_left[1] + h;
        // Left wall (right * -1) and right wall (right * width).
        let lw = [bottom_left[0] - rx, y, bottom_left[2] - rz];
        if !is_frame(world.get(lw).0) {
            height = h;
            break;
        }
        let rw = [bottom_left[0] + rx * width, y, bottom_left[2] + rz * width];
        if !is_frame(world.get(rw).0) {
            height = h;
            break;
        }
        for w in 0..width {
            let c = [bottom_left[0] + rx * w, y, bottom_left[2] + rz * w];
            let id = world.get(c).0;
            if !is_empty_cell(id) {
                height = h;
                break 'rows;
            }
            if id == PORTAL_ID {
                portal_blocks += 1;
            }
        }
    }
    let has_top = (0..width).all(|w| {
        let c = [
            bottom_left[0] + rx * w,
            bottom_left[1] + height,
            bottom_left[2] + rz * w,
        ];
        is_frame(world.get(c).0)
    });
    let height = if (MIN_HEIGHT..=MAX_HEIGHT).contains(&height) && has_top {
        height
    } else {
        0
    };
    PortalShape { plane, bottom_left, width, height, portal_blocks }
}

/// `getDistanceUntilEdgeAboveFrame`: steps until the obsidian
/// edge, requiring an obsidian floor under every empty cell.
fn distance_until_edge(world: &World, pos: [i32; 3], dx: i32, dz: i32) -> i32 {
    for step in 0..=MAX_WIDTH {
        let c = [pos[0] + dx * step, pos[1], pos[2] + dz * step];
        let id = world.get(c).0;
        if !is_empty_cell(id) {
            if is_frame(id) {
                return step;
            }
            break;
        }
        let below = [c[0], c[1] - 1, c[2]];
        if !is_frame(world.get(below).0) {
            break;
        }
    }
    0
}

/// `findEmptyPortalShape`: X axis first, then Z; valid + zero
/// existing portal blocks.
pub(crate) fn find_empty_shape(world: &World, pos: [i32; 3]) -> Option<PortalShape> {
    let x = find_any_shape(world, pos, PortalPlane::Xy);
    if x.is_valid() && x.portal_blocks == 0 {
        return Some(x);
    }
    let z = find_any_shape(world, pos, PortalPlane::Zy);
    if z.is_valid() && z.portal_blocks == 0 {
        return Some(z);
    }
    None
}

/// Attempt to ignite a portal at `hit_pos` (the fire cell inside
/// the frame). On `Ok` the interior has been filled in place.
pub(crate) fn try_ignite(world: &mut World, hit_pos: [i32; 3]) -> IgniteResult {
    let cell = world.get(hit_pos);
    if cell.0 == PORTAL_ID {
        return IgniteResult::AlreadyLit;
    }
    if !cell.is_air() {
        return IgniteResult::NoAdjacentObsidian;
    }
    if let Some(shape) = find_empty_shape(world, hit_pos) {
        let cells = shape.cells();
        for &p in &cells {
            world.set(p, BlockId(PORTAL_ID));
        }
        return IgniteResult::Ok { plane: shape.plane, cells };
    }
    // Distinguish "no frame at all" (fall through to plain fire)
    // from "there is obsidian here but the shape is wrong" (chat
    // diagnostic), like the old ignition did.
    let near_obsidian = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ]
    .iter()
    .any(|d| {
        world
            .get([hit_pos[0] + d[0], hit_pos[1] + d[1], hit_pos[2] + d[2]])
            .0
            == OBSIDIAN_ID
    });
    if near_obsidian {
        IgniteResult::NotRectangular
    } else {
        IgniteResult::NoAdjacentObsidian
    }
}

// ---------------------------------------------------------------
// Phase F.6: cross-dim teleport helpers.
// ---------------------------------------------------------------


/// Number of consecutive ticks the player must stand in a
/// portal cell before triggering a teleport. W156: back to MC's
/// 80 ticks (4 s) - the 30-tick dev shortcut made the crossing
/// feel like an instant cut (the user's "prechod byl moc rychly").
pub(crate) const PORTAL_TRIGGER_TICKS: u32 = 80;

/// Cool-down after a teleport. While > 0 the portal-overlap
/// counter stays at 0 so the player doesn't immediately
/// re-trigger on the destination portal. MC uses 300 game ticks
/// for players; we keep it shorter but long enough to step out.
pub(crate) const PORTAL_COOLDOWN_TICKS: u32 = 60;

/// Search radius around a target coord when looking for an
/// existing portal at the destination dim. Matches MC's
/// "search a 16x16x16 cube" heuristic.
pub(crate) const PORTAL_SEARCH_RADIUS: i32 = 16;

/// Convert a position between paired dims. W273: the scale comes
/// from the dimension registry (`DimensionType.getTeleportationScale`
/// = from.coordinate_scale / to.coordinate_scale - nether 8, so
/// overworld -> nether divides by 8, back multiplies), and the exit
/// Y clamps into the TARGET's logical height (an overworld mountain
/// portal at y 200 must not exit above the nether's bedrock roof).
pub(crate) fn pair_coord(from_dim: &str, pos: [i32; 3]) -> [i32; 3] {
    use crate::worldgen::dim_registry::dim_def_or_overworld;
    let from = dim_def_or_overworld(from_dim);
    let to = dim_def_or_overworld(pair_dim(from_dim));
    let (x, z) = if from.coordinate_scale >= to.coordinate_scale {
        let s = from.coordinate_scale / to.coordinate_scale.max(1);
        (pos[0] * s, pos[2] * s)
    } else {
        let s = to.coordinate_scale / from.coordinate_scale.max(1);
        (pos[0].div_euclid(s), pos[2].div_euclid(s))
    };
    let min_y = to.min_chunk_y * 16;
    // Leave room for the frame + platform under the roof.
    let max_y = min_y + to.logical_height - 6;
    [x, pos[1].clamp(min_y + 1, max_y), z]
}

/// Resolve the paired dim name for `dim`. Today the pairing is
/// fixed: overworld <-> nether. Other dim pairs would land here.
pub(crate) fn pair_dim(dim: &str) -> &'static str {
    if dim == "overworld" {
        "nether"
    } else {
        "overworld"
    }
}

/// Find the closest portal block within `PORTAL_SEARCH_RADIUS`
/// of `target`. Returns `None` when no portal lives in range.
///
/// Manhattan-distance search over the cube; cheap enough since
/// the radius is fixed at 16.
pub(crate) fn find_portal_near(world: &World, target: [i32; 3]) -> Option<[i32; 3]> {
    let r = PORTAL_SEARCH_RADIUS;
    let mut best: Option<([i32; 3], i32)> = None;
    for dx in -r..=r {
        for dy in -r..=r {
            for dz in -r..=r {
                let p = [target[0] + dx, target[1] + dy, target[2] + dz];
                if world.get(p).0 == PORTAL_ID {
                    let d = dx.abs() + dy.abs() + dz.abs();
                    match best {
                        None => best = Some((p, d)),
                        Some((_, bd)) if d < bd => best = Some((p, d)),
                        _ => {}
                    }
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

/// Build a minimal 2x3 (interior) portal at `target` in the
/// XY plane. Lays 10 obsidian frame blocks + 6 portal interior
/// blocks. The bottom-left interior cell ends up at
/// `[target[0], target[1], target[2]]` so callers can use the
/// same coord as the player's "spawn after teleport" target.
///
/// Returns the list of cells the function edited so the
/// caller can broadcast them as world edits to remote peers.
pub(crate) fn build_minimal_portal(world: &mut World, target: [i32; 3]) -> Vec<[i32; 3]> {
    let mut edits: Vec<[i32; 3]> = Vec::with_capacity(16);
    let z = target[2];
    // Frame outer: x in [-1..=2], y in [-1..=3], relative to
    // target. Lay obsidian on the four edges.
    for x in -1..=2 {
        let bot = [target[0] + x, target[1] - 1, z];
        let top = [target[0] + x, target[1] + 3, z];
        if world.get(bot).0 != OBSIDIAN_ID {
            world.set(bot, BlockId(OBSIDIAN_ID));
            edits.push(bot);
        }
        if world.get(top).0 != OBSIDIAN_ID {
            world.set(top, BlockId(OBSIDIAN_ID));
            edits.push(top);
        }
    }
    for y in -1..=3 {
        let left = [target[0] - 1, target[1] + y, z];
        let right = [target[0] + 2, target[1] + y, z];
        if world.get(left).0 != OBSIDIAN_ID {
            world.set(left, BlockId(OBSIDIAN_ID));
            edits.push(left);
        }
        if world.get(right).0 != OBSIDIAN_ID {
            world.set(right, BlockId(OBSIDIAN_ID));
            edits.push(right);
        }
    }
    // Interior: 2 wide x 3 tall.
    for x in 0..=1 {
        for y in 0..=2 {
            let p = [target[0] + x, target[1] + y, z];
            world.set(p, BlockId(PORTAL_ID));
            edits.push(p);
        }
    }
    edits
}

/// Find or build a return portal at `target` in the active
/// dim's world. Returns a safe "stand-here" world coord for
/// the player.
///
/// Idempotent: if a portal already lives within
/// `PORTAL_SEARCH_RADIUS` of `target`, the player drops onto
/// the cell next to it instead of stamping a new frame.
///
/// `edits_out` (if `Some`) gets filled with the cells the call
/// site edited so the caller can broadcast them as world
/// edits to remote peers.
///
/// Auto-creates chunks around `target` if they aren't loaded -
/// the destination dim may have never been visited, so the
/// chunk-streamer hasn't dispatched anything yet. The caller
/// is responsible for rebuilding meshes once chunks are in
/// place.
pub(crate) fn ensure_return_portal(
    world: &mut World,
    target: [i32; 3],
    edits_out: Option<&mut Vec<[i32; 3]>>,
) -> [i32; 3] {
    // Defensive: callers pre-generate REAL terrain around the
    // destination (W155); this only backstops headless tests
    // that drive the fn against a hand-built world.
    ensure_chunks_loaded(world, target);
    if let Some(existing) = find_portal_near(world, target) {
        // W155: arrive INSIDE the portal like MC - the teleport
        // cool-down prevents an instant re-trigger, and stepping
        // out leaves the player right at the frame.
        return existing;
    }
    // Mint a new frame at the caller-resolved ground spot (the
    // old y-clamp built frames mid-air; the caller now ground-
    // snaps the target before handing it over).
    let new_edits = build_minimal_portal(world, target);
    if let Some(out) = edits_out {
        out.extend(new_edits);
    }
    target
}

/// W156: collect every portal block 6-connected to `start` - the
/// curtain that must extinguish when its frame (or any curtain
/// cell) breaks. Capped so a corrupt world can't scan forever.
pub(crate) fn connected_portal_cells(world: &World, start: [i32; 3]) -> Vec<[i32; 3]> {
    let mut out: Vec<[i32; 3]> = Vec::new();
    let mut queue: Vec<[i32; 3]> = vec![start];
    let mut seen: std::collections::HashSet<[i32; 3]> = std::collections::HashSet::new();
    while let Some(p) = queue.pop() {
        // W273: 21x21 frames hold up to 441 curtain cells.
        if out.len() >= 512 || !seen.insert(p) {
            continue;
        }
        if world.get(p).0 != PORTAL_ID {
            continue;
        }
        out.push(p);
        for d in [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            queue.push([p[0] + d[0], p[1] + d[1], p[2] + d[2]]);
        }
    }
    out
}

/// Ensure the chunk containing `pos` AND its 26 neighbours are
/// present in `world`. `World::set` is a no-op when the chunk
/// is missing, so the caller has to pre-create chunks before
/// the frame stamp can land.
fn ensure_chunks_loaded(world: &mut World, pos: [i32; 3]) {
    use infengine_voxel::{CHUNK_SIZE, Chunk};
    let cs = CHUNK_SIZE as i32;
    let cc = [
        pos[0].div_euclid(cs),
        pos[1].div_euclid(cs),
        pos[2].div_euclid(cs),
    ];
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let n: [i32; 3] = [cc[0] + dx, cc[1] + dy, cc[2] + dz];
                if world.chunk(n).is_none() {
                    world.insert_chunk(n, Chunk::empty());
                }
            }
        }
    }
}

// W5.2c: tests live in a sibling file (portal_tests.rs) so each
// file stays under the 500-LOC soft cap. Mounted via `#[path]` so
// the file lays out as a flat sibling rather than the more verbose
// `tests/` subfolder.
#[cfg(test)]
#[path = "portal_tests.rs"]
mod tests;
