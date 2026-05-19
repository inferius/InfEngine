//! Nether portal frame detection + ignition (Phase F.5)
//! plus cross-dim teleport orchestration (Phase F.6).
//!
//! Algorithm (MC-faithful, simplified):
//! 1. RMB hands us an air cell adjacent to obsidian.
//! 2. Flood-fill up to `MAX_FLOOD_CELLS` cells of air, walking
//!    only along the portal's two interior axes (the third axis
//!    stays pinned to the hit cell's coord). The portal plane
//!    is the axis-aligned 2D plane the obsidian frame sits in;
//!    we infer it by checking which neighbouring cells are
//!    obsidian.
//! 3. Validate the flood region is a rectangle of size 2x3 or
//!    3x2 (the legal MC sizes) and that EVERY perimeter cell
//!    around it is obsidian.
//! 4. Replace every interior air cell with `PORTAL_ID`. The
//!    portal axis is implicit in the chosen plane; render-side
//!    rotation will use it once dedicated portal rendering
//!    lands.
//!
//! The fn is pure on `(World, hit_pos)` so unit tests build a
//! tiny world with a hand-laid obsidian frame and assert the
//! ignition outcome. The main.rs integration just calls
//! `try_ignite` from the RMB handler with `&mut voxel.world`.
//!
//! Durability + cooldown are explicitly out of scope (see Phase
//! F module docs for the full deferred list).

use infengine_voxel::{BlockId, World};

use super::{OBSIDIAN_ID, PORTAL_ID};

/// Hard cap on the flood-fill cell count, matching MC's
/// portal-ignition limit (a 3x4 frame yields 12 interior cells;
/// the cap is generous so it catches accidental ignitions inside
/// large air pockets without iterating forever).
pub(crate) const MAX_FLOOD_CELLS: usize = 22;

/// Axis the portal plane runs along. The portal's "tall" side
/// is always the Y axis; the third axis (the one perpendicular
/// to the plane) is locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortalPlane {
    /// XY plane - the portal faces along +/-Z. Interior cells
    /// vary in X and Y; Z is fixed.
    Xy,
    /// ZY plane - the portal faces along +/-X. Interior cells
    /// vary in Z and Y; X is fixed.
    Zy,
}

/// Result of a portal-ignition attempt.
///
/// The fail kinds are surfaced so the chat / HUD can show a
/// helpful diagnostic ("frame is the wrong size", "perimeter
/// has gaps"). The runtime currently just no-ops on a fail;
/// callers can wire the diagnostic into chat as a follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgniteResult {
    /// Portal lit successfully. The frame interior was filled
    /// with `PORTAL_ID` blocks; the caller should run a
    /// world-edit broadcast for each cell.
    Ok { plane: PortalPlane, interior_count: u8 },
    /// The hit cell isn't adjacent to any obsidian, so there's
    /// no candidate frame plane to start a flood-fill on.
    NoAdjacentObsidian,
    /// The flood-fill exceeded `MAX_FLOOD_CELLS` - the player
    /// is trying to ignite an air pocket too big to be a portal.
    FloodTooLarge,
    /// The flood region isn't a rectangle of size 2x3 or 3x2.
    NotRectangular,
    /// The rectangle's perimeter cells aren't all obsidian -
    /// the frame has a gap.
    PerimeterIncomplete,
    /// The cell already holds a portal block (idempotent no-op).
    AlreadyLit,
}

impl IgniteResult {
    #[cfg(test)]
    pub(crate) fn is_ok(&self) -> bool {
        matches!(self, IgniteResult::Ok { .. })
    }
}

/// Attempt to ignite a portal at `hit_pos`. See module docs for
/// the algorithm.
///
/// The hit cell MUST be air (otherwise there's nothing to
/// flood-fill); the caller is expected to have run a sane
/// "RMB target" check first. Returns the outcome; on `Ok` the
/// world has been edited in place.
pub(crate) fn try_ignite(world: &mut World, hit_pos: [i32; 3]) -> IgniteResult {
    let cell = world.get(hit_pos);
    if cell.0 == PORTAL_ID {
        return IgniteResult::AlreadyLit;
    }
    if !cell.is_air() {
        // Lighting a non-air, non-portal cell is meaningless -
        // the player should target the air gap inside the frame.
        return IgniteResult::NoAdjacentObsidian;
    }
    // Pick the plane: prefer XY if obsidian sits on the +/- Z
    // sides (the frame walls run perpendicular to Z), otherwise
    // try ZY. The two cases never collide for a well-formed
    // axis-aligned frame.
    let plane = match infer_plane(world, hit_pos) {
        Some(p) => p,
        None => return IgniteResult::NoAdjacentObsidian,
    };
    // Walk +/- along each free axis from the hit cell until we
    // hit obsidian (= the frame wall). This gives us the
    // bounding rectangle; doing a hard "first obsidian wins"
    // sweep is more robust than a generic flood-fill, since the
    // air "outside" the frame doesn't poison the result.
    let rect = match scan_rect_from(world, hit_pos, plane) {
        Some(r) => r,
        None => return IgniteResult::NotRectangular,
    };
    // Accept 2x3 and 3x2 interior sizes (the two MC legal frames).
    let width = (rect.a_max - rect.a_min + 1) as usize;
    let height = (rect.b_max - rect.b_min + 1) as usize;
    if width * height > MAX_FLOOD_CELLS {
        return IgniteResult::FloodTooLarge;
    }
    let ok_dims = (width == 2 && height == 3) || (width == 3 && height == 2);
    if !ok_dims {
        return IgniteResult::NotRectangular;
    }
    // Every interior cell must be air (no obsidian protrusions).
    // Skip the corner-of-perimeter check that MC also skips;
    // the perimeter sweep below covers the four edges only.
    let fixed = rect.fixed;
    for a in rect.a_min..=rect.a_max {
        for b in rect.b_min..=rect.b_max {
            let p = make_pos(plane, a, b, fixed);
            if !world.get(p).is_air() {
                return IgniteResult::NotRectangular;
            }
        }
    }
    // Perimeter sweep: every cell one step outside the
    // rectangle (along the four edges, NOT the corners) must be
    // obsidian.
    for b in rect.b_min..=rect.b_max {
        for &edge_a in &[rect.a_min - 1, rect.a_max + 1] {
            let p = make_pos(plane, edge_a, b, fixed);
            if world.get(p).0 != OBSIDIAN_ID {
                return IgniteResult::PerimeterIncomplete;
            }
        }
    }
    for a in rect.a_min..=rect.a_max {
        for &edge_b in &[rect.b_min - 1, rect.b_max + 1] {
            let p = make_pos(plane, a, edge_b, fixed);
            if world.get(p).0 != OBSIDIAN_ID {
                return IgniteResult::PerimeterIncomplete;
            }
        }
    }
    // Edit: every interior cell becomes a portal block.
    let interior_count = (width * height) as u8;
    for a in rect.a_min..=rect.a_max {
        for b in rect.b_min..=rect.b_max {
            let p = make_pos(plane, a, b, fixed);
            world.set(p, BlockId(PORTAL_ID));
        }
    }
    IgniteResult::Ok {
        plane,
        interior_count,
    }
}

/// Plane-aligned bounding rectangle of the air interior reached
/// by sweeping +/-A and +/-B from the hit cell.
struct InteriorRect {
    a_min: i32,
    a_max: i32,
    b_min: i32,
    b_max: i32,
    /// Coord of the axis perpendicular to the plane (locked).
    fixed: i32,
}

/// Walk +/- along each free axis of `plane`. Stops on the first
/// non-air cell each direction. Returns the bounding rect of
/// the air interior. Caps the sweep at `MAX_FLOOD_CELLS` cells
/// per axis to defend against runaway air pockets.
fn scan_rect_from(world: &World, seed: [i32; 3], plane: PortalPlane) -> Option<InteriorRect> {
    let (ax_a, ax_b, ax_fixed) = match plane {
        PortalPlane::Xy => (0, 1, 2),
        PortalPlane::Zy => (2, 1, 0),
    };
    let mut a_min = seed[ax_a];
    let mut a_max = seed[ax_a];
    let mut b_min = seed[ax_b];
    let mut b_max = seed[ax_b];
    let fixed = seed[ax_fixed];

    // Sweep +A
    let mut steps = 0;
    let mut probe = seed;
    while steps < MAX_FLOOD_CELLS {
        probe[ax_a] += 1;
        if !world.get(probe).is_air() {
            break;
        }
        a_max = probe[ax_a];
        steps += 1;
    }
    // Sweep -A
    steps = 0;
    probe = seed;
    while steps < MAX_FLOOD_CELLS {
        probe[ax_a] -= 1;
        if !world.get(probe).is_air() {
            break;
        }
        a_min = probe[ax_a];
        steps += 1;
    }
    // Sweep +B
    steps = 0;
    probe = seed;
    while steps < MAX_FLOOD_CELLS {
        probe[ax_b] += 1;
        if !world.get(probe).is_air() {
            break;
        }
        b_max = probe[ax_b];
        steps += 1;
    }
    // Sweep -B
    steps = 0;
    probe = seed;
    while steps < MAX_FLOOD_CELLS {
        probe[ax_b] -= 1;
        if !world.get(probe).is_air() {
            break;
        }
        b_min = probe[ax_b];
        steps += 1;
    }
    Some(InteriorRect {
        a_min,
        a_max,
        b_min,
        b_max,
        fixed,
    })
}

/// Decide which plane the candidate frame sits in by inspecting
/// the four lateral neighbours of `hit_pos`.
///
/// A frame in the XY plane has its side walls along the X axis
/// (cells at +/-X are obsidian for at least one interior row).
/// A frame in the ZY plane has its side walls along the Z axis.
/// We pick the plane by which neighbour axis shows obsidian.
///
/// Ambiguous (both X and Z neighbours look like walls -> picks
/// XY to match MC's implicit axis pick).
fn infer_plane(world: &World, p: [i32; 3]) -> Option<PortalPlane> {
    let has_x_obsidian = world.get([p[0] - 1, p[1], p[2]]).0 == OBSIDIAN_ID
        || world.get([p[0] + 1, p[1], p[2]]).0 == OBSIDIAN_ID;
    let has_z_obsidian = world.get([p[0], p[1], p[2] - 1]).0 == OBSIDIAN_ID
        || world.get([p[0], p[1], p[2] + 1]).0 == OBSIDIAN_ID;
    // Bottom must also be obsidian for either plane (the floor
    // of the frame). Without it we can't be inside a frame.
    let has_floor = world.get([p[0], p[1] - 1, p[2]]).0 == OBSIDIAN_ID;
    if !has_floor && !has_x_obsidian && !has_z_obsidian {
        return None;
    }
    if has_x_obsidian {
        // Side walls run along X => frame's free axis is X (and
        // Y); the plane is XY.
        Some(PortalPlane::Xy)
    } else if has_z_obsidian {
        // Side walls run along Z => frame's free axis is Z (and
        // Y); the plane is ZY.
        Some(PortalPlane::Zy)
    } else if has_floor {
        // Standing on a single obsidian block with no adjacent
        // side walls means there's no frame yet - bail.
        None
    } else {
        None
    }
}

/// Reconstruct a world-space coord from plane-local axes.
fn make_pos(plane: PortalPlane, a: i32, b: i32, fixed: i32) -> [i32; 3] {
    match plane {
        PortalPlane::Xy => [a, b, fixed],
        PortalPlane::Zy => [fixed, b, a],
    }
}

// ---------------------------------------------------------------
// Phase F.6: cross-dim teleport helpers.
// ---------------------------------------------------------------

/// MC's overworld <-> nether ratio. One block in the overworld
/// covers 8 blocks in the nether (and vice-versa).
pub(crate) const COORD_SCALE: i32 = 8;

/// Number of consecutive ticks the player must stand in a
/// portal cell before triggering a teleport. MC ships 80; we
/// shorten to 30 for snappier dev / test UX.
pub(crate) const PORTAL_TRIGGER_TICKS: u32 = 30;

/// Cool-down after a teleport. While > 0 the portal-overlap
/// counter stays at 0 so the player doesn't immediately
/// re-trigger on the destination portal.
pub(crate) const PORTAL_COOLDOWN_TICKS: u32 = 30;

/// Search radius around a target coord when looking for an
/// existing portal at the destination dim. Matches MC's
/// "search a 16x16x16 cube" heuristic.
pub(crate) const PORTAL_SEARCH_RADIUS: i32 = 16;

/// Convert a position between paired dims. From the overworld
/// to the nether: divide by 8 (Euclidean). From the nether to
/// the overworld: multiply by 8.
///
/// String literals here mirror `dim::OVERWORLD` / `dim::NETHER`
/// but keep the function lib-friendly (the lib crate doesn't
/// see the bin-only `dim` module).
pub(crate) fn pair_coord(from_dim: &str, pos: [i32; 3]) -> [i32; 3] {
    if from_dim == "overworld" {
        [
            pos[0].div_euclid(COORD_SCALE),
            pos[1],
            pos[2].div_euclid(COORD_SCALE),
        ]
    } else {
        [
            pos[0] * COORD_SCALE,
            pos[1],
            pos[2] * COORD_SCALE,
        ]
    }
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
    // Make sure the chunks around the destination exist before
    // we try to set/get cells in them.
    ensure_chunks_loaded(world, target);
    if let Some(existing) = find_portal_near(world, target) {
        // Player lands one cell to the south so they aren't
        // standing IN the portal (which would immediately
        // re-trigger).
        return [existing[0], existing[1], existing[2] + 1];
    }
    // Need to mint a new frame. Clamp target Y to a sane band
    // before building so the frame doesn't end up underground
    // or above the world.
    let safe_y = target[1].clamp(4, 100);
    let safe = [target[0], safe_y, target[2]];
    ensure_chunks_loaded(world, safe);
    let new_edits = build_minimal_portal(world, safe);
    if let Some(out) = edits_out {
        out.extend(new_edits);
    }
    [safe[0], safe[1], safe[2] + 1]
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

#[cfg(test)]
mod portal_pair_tests {
    use super::*;
    use infengine_voxel::Chunk;

    fn insert_neighbour_chunks(w: &mut World, range: i32) {
        for cx in -range..=range {
            for cy in -range..=range {
                for cz in -range..=range {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
    }

    #[test]
    fn pair_coord_overworld_to_nether_scales_by_8() {
        assert_eq!(pair_coord("overworld", [80, 64, -16]), [10, 64, -2]);
        assert_eq!(pair_coord("overworld", [0, 64, 0]), [0, 64, 0]);
        // Euclidean div on negatives: -1 / 8 = -1 (NOT 0).
        assert_eq!(pair_coord("overworld", [-1, 64, -1]), [-1, 64, -1]);
    }

    #[test]
    fn pair_coord_nether_to_overworld_scales_by_8() {
        assert_eq!(pair_coord("nether", [10, 64, -2]), [80, 64, -16]);
        assert_eq!(pair_coord("nether", [0, 64, 0]), [0, 64, 0]);
        assert_eq!(pair_coord("nether", [-1, 64, -1]), [-8, 64, -8]);
    }

    #[test]
    fn pair_dim_inverts_when_called_twice() {
        let a = "overworld";
        let b = pair_dim(a);
        let c = pair_dim(b);
        assert_eq!(c, a);
    }

    #[test]
    fn find_portal_near_returns_closest() {
        let mut w = World::new();
        // Use y near 8 so a small chunk range still covers it.
        insert_neighbour_chunks(&mut w, 2);
        w.set([5, 8, 5], BlockId(PORTAL_ID));
        w.set([10, 8, 5], BlockId(PORTAL_ID));
        // Distance from [0, 8, 0]: first = 10 (5+0+5),
        // second = 15 (10+0+5). Closest = [5, 8, 5].
        let found = find_portal_near(&w, [0, 8, 0]);
        assert_eq!(found, Some([5, 8, 5]));
    }

    #[test]
    fn find_portal_near_returns_none_outside_radius() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 3);
        // Place a portal outside the search radius (>16
        // Manhattan distance from origin).
        w.set([20, 8, 20], BlockId(PORTAL_ID));
        let found = find_portal_near(&w, [0, 8, 0]);
        // 20 + 0 + 20 = 40 > 16 (radius); but the cube checks
        // each axis <= radius, so [20, 8, 20] is outside the
        // search cube too. Expect None.
        assert!(found.is_none(), "portal at 20,8,20 should be outside the search cube");
    }

    #[test]
    fn build_minimal_portal_creates_lit_frame() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 2);
        let edits = build_minimal_portal(&mut w, [0, 5, 5]);
        // Frame walls (10 obsidian) + interior (6 portal) = 16
        // cells total (corners are written too in this impl).
        assert!(edits.len() >= 16, "expected at least 16 edits, got {}", edits.len());
        // Interior is portal:
        for x in 0..=1 {
            for y in 5..=7 {
                assert_eq!(w.get([x, y, 5]).0, PORTAL_ID);
            }
        }
        // Walls are obsidian:
        assert_eq!(w.get([-1, 5, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([2, 5, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([0, 4, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([0, 8, 5]).0, OBSIDIAN_ID);
    }

    #[test]
    fn ensure_return_portal_reuses_existing() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 2);
        w.set([3, 8, 3], BlockId(PORTAL_ID));
        // Target near the existing portal: no new frame should
        // be built; the spawn coord lands next to the portal.
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let spawn = ensure_return_portal(&mut w, [0, 8, 0], Some(&mut edits));
        assert!(edits.is_empty(), "no edits expected when reusing existing portal");
        assert_eq!(spawn, [3, 8, 4]); // existing + south
    }

    #[test]
    fn ensure_return_portal_builds_new_when_far() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 5);
        // No portal nearby - target [20, 8, 20] is far from any
        // existing portal. The fn must build a fresh frame there.
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let _ = ensure_return_portal(&mut w, [20, 8, 20], Some(&mut edits));
        assert!(
            edits.len() >= 6,
            "expected portal-frame edits, got {} edits",
            edits.len()
        );
        // At least one portal block should now sit near the target.
        let found = find_portal_near(&w, [20, 8, 20]);
        assert!(found.is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::Chunk;

    /// Pre-insert every chunk that any of our test coords touches.
    /// Coords used in this test are in `[-1, ..]`; a 4-chunk grid
    /// at the 4 corners of the origin column covers them all.
    fn insert_neighbour_chunks(w: &mut World) {
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
    }

    /// Build a small world that already contains an obsidian
    /// portal frame in the XY plane at z=fixed_z. Interior is
    /// 2 wide x 3 tall.
    fn world_with_2x3_frame_xy(fixed_z: i32) -> World {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        // Frame outer rect: x in [-1..=2], y in [-1..=3], at
        // z = fixed_z. Interior: x in 0..=1, y in 0..=2.
        // Walls (lay obsidian):
        //   bottom row: y=-1, x in -1..=2
        //   top row:    y=3,  x in -1..=2
        //   left col:   x=-1, y in -1..=3
        //   right col:  x=2,  y in -1..=3
        // Corners ARE placed for this test - the algorithm
        // doesn't require them but their presence shouldn't
        // affect the outcome.
        for x in -1..=2 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 3, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=3 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([2, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        w
    }

    fn world_with_3x2_frame_zy(fixed_x: i32) -> World {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        // Interior: z in 0..=2 (3 wide), y in 0..=1 (2 tall).
        // Outer: z in -1..=3, y in -1..=2.
        for z in -1..=3 {
            w.set([fixed_x, -1, z], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, 2, z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=2 {
            w.set([fixed_x, y, -1], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, y, 3], BlockId(OBSIDIAN_ID));
        }
        w
    }

    #[test]
    fn ignites_a_valid_2x3_xy_frame() {
        let mut w = world_with_2x3_frame_xy(5);
        let res = try_ignite(&mut w, [0, 0, 5]); // bottom-left interior
        assert!(res.is_ok(), "expected Ok, got {res:?}");
        if let IgniteResult::Ok { plane, interior_count } = res {
            assert_eq!(plane, PortalPlane::Xy);
            assert_eq!(interior_count, 6);
        }
        // Every interior cell must now be PORTAL_ID.
        for x in 0..=1 {
            for y in 0..=2 {
                assert_eq!(
                    w.get([x, y, 5]).0,
                    PORTAL_ID,
                    "interior ({x},{y},5) should be portal"
                );
            }
        }
    }

    #[test]
    fn ignites_a_valid_3x2_zy_frame() {
        let mut w = world_with_3x2_frame_zy(7);
        let res = try_ignite(&mut w, [7, 0, 0]);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
        if let IgniteResult::Ok { plane, interior_count } = res {
            assert_eq!(plane, PortalPlane::Zy);
            assert_eq!(interior_count, 6);
        }
        for z in 0..=2 {
            for y in 0..=1 {
                assert_eq!(
                    w.get([7, y, z]).0,
                    PORTAL_ID,
                    "interior (7,{y},{z}) should be portal"
                );
            }
        }
    }

    #[test]
    fn rejects_frame_with_gap_in_perimeter() {
        let mut w = world_with_2x3_frame_xy(5);
        // Punch a hole in the top row.
        w.set([0, 3, 5], BlockId::AIR);
        let res = try_ignite(&mut w, [0, 0, 5]);
        // Either NotRectangular (the flood escapes through the
        // gap) or PerimeterIncomplete (the flood stayed contained
        // but a perimeter check failed). Both indicate the frame
        // is invalid.
        assert!(
            matches!(
                res,
                IgniteResult::NotRectangular | IgniteResult::PerimeterIncomplete | IgniteResult::FloodTooLarge
            ),
            "expected ignition failure, got {res:?}"
        );
        // No interior cell should have become a portal.
        for x in 0..=1 {
            for y in 0..=2 {
                assert_ne!(w.get([x, y, 5]).0, PORTAL_ID);
            }
        }
    }

    #[test]
    fn rejects_1x1_air_hole() {
        // A single air voxel surrounded by obsidian - too small
        // to be a portal interior.
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        let fixed_z = 0;
        // Frame around (0, 0, 0) interior cell.
        for x in -1..=1 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 1, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=1 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([1, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        let res = try_ignite(&mut w, [0, 0, fixed_z]);
        assert!(
            !res.is_ok(),
            "1x1 hole must not ignite, got {res:?}"
        );
    }

    #[test]
    fn already_portal_cell_is_noop() {
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        w.set([0, 0, 0], BlockId(PORTAL_ID));
        let res = try_ignite(&mut w, [0, 0, 0]);
        assert_eq!(res, IgniteResult::AlreadyLit);
    }

    #[test]
    fn rejects_hit_cell_without_adjacent_obsidian() {
        // Empty world; the hit cell has no obsidian anywhere.
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        let res = try_ignite(&mut w, [0, 0, 0]);
        assert_eq!(res, IgniteResult::NoAdjacentObsidian);
    }

    #[test]
    fn rejects_oversize_air_pocket() {
        // Build a 5x5 air pocket sealed with obsidian - more
        // than MAX_FLOOD_CELLS interior cells. Must reject.
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        let fixed_z = 0;
        // Frame: x in [-1..=5], y in [-1..=5]. Interior 5x5 = 25 cells > 22.
        for x in -1..=5 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 5, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=5 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([5, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        let res = try_ignite(&mut w, [0, 0, fixed_z]);
        // Either FloodTooLarge (cap hit during flood) or
        // NotRectangular (the 5x5 isn't a 2x3 or 3x2).
        assert!(
            matches!(
                res,
                IgniteResult::FloodTooLarge | IgniteResult::NotRectangular
            ),
            "expected size-rejection, got {res:?}"
        );
    }
}
