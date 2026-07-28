//! W61.2 - door state + open/close toggle.
//!
//! MC door behavior simplified for the engine:
//! - Door place puts a lower half at the clicked cell + an upper
//!   half at +Y above it (in world::place_break).
//! - State is just `open: bool` per voxel pos. Facing + hinge
//!   directional rendering is W61.2b.
//! - RMB on either half toggles open on both halves.
//! - When open, F_RAY_PASS + F_AABB_PASS via state lookup so the
//!   player walks straight through.
//!
//! MC mirror: `net/minecraft/world/level/block/DoorBlock.java`.

#![allow(dead_code)]

use std::collections::HashSet;

/// Per-voxel door open state. `true` = door is open, player + ray
/// pass through.
pub(crate) type DoorStates = HashSet<[i32; 3]>;

/// True when `id` is a wooden / iron door block (lower OR upper
/// half). Walks the W12 + W48 door id range.
pub(crate) fn is_door_block(id: u16) -> bool {
    // Wooden + iron doors (W12: 77 oak_door, 78 oak_door_upper,
    // 79 iron_door). W48.5 added more variants but only the
    // lower-half voxel sits in 568..=575; the upper halves use
    // the SAME id today since we don't ship per-variant upper
    // ids yet.
    matches!(id, 77..=79)
        || (568..=575).contains(&id)
        // W440: copper doors (hand-open like wood in MC).
        || (826..=827).contains(&id)
}

/// W125: true when `id` is a trapdoor block (W48.4 ids 548..=552).
/// W166: + acacia / dark_oak / mangrove / cherry (722..=725).
/// Trapdoors share the door open-state set + toggle + emit_trapdoor
/// render + collision/light gating, so this one predicate covers them.
pub(crate) fn is_trapdoor_block(id: u16) -> bool {
    (548..=552).contains(&id)
        || (722..=725).contains(&id)
        // W174: crimson / warped nether-wood trapdoors.
        || (750..=751).contains(&id)
        // W440: copper trapdoors.
        || (828..=829).contains(&id)
}

/// W167: true when `id` is a fence gate (726..=731). Fence gates share
/// the door open-state set + facing + RMB toggle; closed = a solid
/// barrier, open = walk-through.
pub(crate) fn is_fence_gate_block(id: u16) -> bool {
    (726..=731).contains(&id)
}

/// W279: true when `id` is any stairs block (registry name suffix -
/// the family spans legacy ids + every W-wave stone/wood variant).
pub(crate) fn is_stairs_block(id: u16) -> bool {
    use std::sync::OnceLock;
    static SET: OnceLock<std::collections::HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        crate::block::BLOCK_TYPES
            .iter()
            .filter(|b| b.name.ends_with("_stairs"))
            .map(|b| b.id)
            .collect()
    })
    .contains(&id)
}

/// W125: per-door facing, keyed by the LOWER half voxel.
/// 0 = +X (east), 1 = +Z (south), 2 = -X (west), 3 = -Z (north) -
/// the horizontal direction the PLACER was looking.
pub(crate) type DoorFacings = std::collections::HashMap<[i32; 3], u8>;

/// W125: facing code from a look vector (horizontal projection).
pub(crate) fn facing_from_look(dx: f32, dz: f32) -> u8 {
    if dx.abs() >= dz.abs() {
        if dx >= 0.0 { 0 } else { 2 }
    } else if dz >= 0.0 {
        1
    } else {
        3
    }
}

/// W125: which cell edge the CLOSED panel rests against. MC places
/// the panel at the edge nearest the placer = opposite the facing.
/// Returns the facing code of that edge's outward direction.
pub(crate) fn closed_edge(facing: u8) -> u8 {
    (facing + 2) % 4
}

/// W125: which cell edge the OPEN panel swings to (hinge = left).
/// East-facing door opens onto the north edge, etc.
pub(crate) fn open_edge(facing: u8) -> u8 {
    (facing + 3) % 4
}

/// W125: door panel quads for one half. The panel is a 3/16-thick
/// slab standing against the cell edge `edge` (a facing code).
/// Emits 6 faces via `emit` (positions relative to the voxel origin,
/// `[min, max]` box corners + the face normal); the caller owns UV +
/// light resolution. Box coords are in voxel-local 0..1 space.
pub(crate) fn panel_box(edge: u8) -> ([f32; 3], [f32; 3]) {
    const T: f32 = 3.0 / 16.0;
    match edge % 4 {
        0 => ([1.0 - T, 0.0, 0.0], [1.0, 1.0, 1.0]), // +X edge
        1 => ([0.0, 0.0, 1.0 - T], [1.0, 1.0, 1.0]), // +Z edge
        2 => ([0.0, 0.0, 0.0], [T, 1.0, 1.0]),       // -X edge
        _ => ([0.0, 0.0, 0.0], [1.0, 1.0, T]),       // -Z edge
    }
}

/// W133: bit 2 of the trapdoor state byte = TOP half (closed slab
/// against the ceiling, the MC `half=top` state). Bits 0-1 stay the
/// facing code, so door-state plumbing (snapshot, net, toggle)
/// carries the half for free.
pub(crate) const TRAPDOOR_TOP_BIT: u8 = 4;

/// W411: bit 3 of a slab's `stair_states` byte = DOUBLE slab (both
/// halves filled; meshes as the full cube, drops two items). The
/// TOP half reuses TRAPDOOR_TOP_BIT so the state plumbing (snapshot,
/// save v6 pos-state rows) carries slabs for free.
pub(crate) const SLAB_DOUBLE_BIT: u8 = 8;

/// W133: port of MC `TrapDoorBlock::getStateForPlacement`. Returns
/// the trapdoor state byte: facing (bits 0-1) | TRAPDOOR_TOP_BIT.
/// - Clicking a block's TOP face -> bottom half; BOTTOM face -> top
///   half; facing = horizontal look reversed (panel hinges on the
///   far edge as seen by the placer).
/// - Clicking a SIDE face -> facing = that face's outward normal,
///   half from the exact click height within the face (above the
///   midline = top). The click point is recovered by intersecting
///   the eye ray with the clicked face plane.
pub(crate) fn trapdoor_place_state(
    normal: [i32; 3],
    place_at: [i32; 3],
    eye: [f32; 3],
    look: [f32; 3],
) -> u8 {
    if normal[1] != 0 {
        let f = closed_edge(facing_from_look(look[0], look[2]));
        return if normal[1] > 0 { f } else { f | TRAPDOOR_TOP_BIT };
    }
    let f = match (normal[0], normal[2]) {
        (1, _) => 0u8,
        (_, 1) => 1,
        (-1, _) => 2,
        _ => 3,
    };
    if side_click_is_top(normal, place_at, eye, look) {
        f | TRAPDOOR_TOP_BIT
    } else {
        f
    }
}

/// W411 (extracted from `trapdoor_place_state`): for a SIDE-face
/// click, was the exact click point in the upper half of the face?
/// The click point is recovered by intersecting the eye ray with
/// the clicked face plane - the shared plane between the support
/// block and the place cell: at the place cell's low edge when the
/// normal points +, high edge when -.
fn side_click_is_top(
    normal: [i32; 3],
    place_at: [i32; 3],
    eye: [f32; 3],
    look: [f32; 3],
) -> bool {
    let (axis, plane) = if normal[0] != 0 {
        let x = place_at[0] as f32;
        (0usize, if normal[0] > 0 { x } else { x + 1.0 })
    } else {
        let z = place_at[2] as f32;
        (2usize, if normal[2] > 0 { z } else { z + 1.0 })
    };
    let d = look[axis];
    if d.abs() > 1e-6 {
        let t = (plane - eye[axis]) / d;
        if t > 0.0 {
            let y_hit = eye[1] + look[1] * t;
            return y_hit - place_at[1] as f32 > 0.5;
        }
    }
    false
}

/// W413: port of MC `StairBlock.getStairsShape`. `state` is this
/// stair's byte (facing bits 0-1 + TRAPDOOR_TOP_BIT); `neighbor`
/// returns the STAIRS state at a relative XZ offset, or `None`
/// when that cell is not a stairs block. Returns the MC `shape`
/// prop value for the blockstate variant lookup.
///
/// Facing codes: 0=+X east, 1=+Z south, 2=-X west, 3=-Z north;
/// counter-clockwise (seen from above) = east->north->west->south,
/// i.e. `(f + 3) & 3`.
pub(crate) fn stairs_shape(
    state: u8,
    neighbor: impl Fn([i32; 2]) -> Option<u8>,
) -> &'static str {
    let f = state & 3;
    let half = state & TRAPDOOR_TOP_BIT;
    let dxz = |f: u8| -> [i32; 2] {
        match f & 3 {
            0 => [1, 0],
            1 => [0, 1],
            2 => [-1, 0],
            _ => [0, -1],
        }
    };
    let ccw = (f + 3) & 3;
    let same_axis = |a: u8, b: u8| (a & 1) == (b & 1);
    // MC canTakeShape: the cell on `face` must NOT be an identical
    // stairs (same facing + half) - identical rows stay straight.
    let can_take = |face: u8| -> bool {
        match neighbor(dxz(face)) {
            Some(n) => (n & 3) != f || (n & TRAPDOOR_TOP_BIT) != half,
            None => true,
        }
    };
    // Behind (toward facing): outer corners.
    if let Some(b) = neighbor(dxz(f))
        && (b & TRAPDOOR_TOP_BIT) == half
    {
        let bf = b & 3;
        if !same_axis(bf, f) && can_take((bf + 2) & 3) {
            return if bf == ccw { "outer_left" } else { "outer_right" };
        }
    }
    // In front (away from facing): inner corners.
    if let Some(fr) = neighbor(dxz((f + 2) & 3))
        && (fr & TRAPDOOR_TOP_BIT) == half
    {
        let ff = fr & 3;
        if !same_axis(ff, f) && can_take(ff) {
            return if ff == ccw { "inner_left" } else { "inner_right" };
        }
    }
    "straight"
}

/// W411: port of MC `SlabBlock::getStateForPlacement`. Returns the
/// slab state byte: 0 = bottom, TRAPDOOR_TOP_BIT = top. Clicking a
/// block's TOP face -> bottom slab; BOTTOM face -> top slab; SIDE
/// face -> the half from the exact click height within the face.
/// (The DOUBLE merge is the placement pipeline's job - it happens
/// in the CLICKED cell, not the place cell.)
pub(crate) fn slab_place_state(
    normal: [i32; 3],
    place_at: [i32; 3],
    eye: [f32; 3],
    look: [f32; 3],
) -> u8 {
    if normal[1] > 0 {
        return 0;
    }
    if normal[1] < 0 {
        return TRAPDOOR_TOP_BIT;
    }
    if side_click_is_top(normal, place_at, eye, look) {
        TRAPDOOR_TOP_BIT
    } else {
        0
    }
}

/// True when the cell directly above `pos` is the upper half of
/// the door whose lower half lives at `pos`. Used by the place
/// path to decide whether to auto-place an upper half.
pub(crate) fn upper_id_for_lower(_lower_id: u16) -> u16 {
    // W12 stores oak_door_upper as a separate block id (78); the
    // W48 variants use the same lower id for both halves. Keep
    // 78 as the canonical upper so the existing renderer/
    // collision paths still match.
    78
}

/// Toggle the door at `pos` (and its partner half above / below).
/// Returns the new open state. No-op when the cell isn't a door.
pub(crate) fn toggle_door(
    states: &mut DoorStates,
    world: &infengine_voxel::World,
    pos: [i32; 3],
) -> bool {
    // Resolve the LOWER half of the door so the open flag is
    // stored once per door, not duplicated across the two halves.
    let lower = lower_pos(world, pos);
    let was_open = states.contains(&lower);
    if was_open {
        states.remove(&lower);
    } else {
        states.insert(lower);
    }
    !was_open
}

/// True when the door at `pos` (or its sibling half) is open.
pub(crate) fn is_open(
    states: &DoorStates,
    world: &infengine_voxel::World,
    pos: [i32; 3],
) -> bool {
    let lower = lower_pos(world, pos);
    states.contains(&lower)
}

/// Walk down from `pos` until a door's lower half (or non-door
/// block) is found. Returns the position of the lower half.
/// Upper halves are at +Y from the lower; the door state is keyed
/// by the lower half pos so the two halves share one entry.
pub(crate) fn lower_pos(
    world: &infengine_voxel::World,
    pos: [i32; 3],
) -> [i32; 3] {
    // If the cell below is also a door block, this cell is the
    // upper half - step down once. Anything else and `pos` is
    // already the lower (or a stand-alone door).
    let below = [pos[0], pos[1] - 1, pos[2]];
    if is_door_block(world.get(below).0) {
        return below;
    }
    pos
}

#[cfg(test)]
mod tests {

    #[test]
    fn stairs_shape_corners() {
        // W413: StairBlock.getStairsShape. An east-facing stair
        // (f=0) with a north-facing stair (f=3 = its CCW) BEHIND it
        // (toward +X) turns outer_left; south behind -> outer_right.
        let east = 0u8;
        let behind = |n: u8| move |d: [i32; 2]| {
            if d == [1, 0] { Some(n) } else { None }
        };
        assert_eq!(stairs_shape(east, behind(3)), "outer_left");
        assert_eq!(stairs_shape(east, behind(1)), "outer_right");
        // Same-axis behind (east/west): straight.
        assert_eq!(stairs_shape(east, behind(0)), "straight");
        assert_eq!(stairs_shape(east, behind(2)), "straight");
        // A crossing stair IN FRONT (toward -X) makes inner corners.
        let front = |n: u8| move |d: [i32; 2]| {
            if d == [-1, 0] { Some(n) } else { None }
        };
        assert_eq!(stairs_shape(east, front(3)), "inner_left");
        assert_eq!(stairs_shape(east, front(1)), "inner_right");
        // Different half never connects.
        let top_behind = |d: [i32; 2]| {
            if d == [1, 0] { Some(3 | TRAPDOOR_TOP_BIT) } else { None }
        };
        assert_eq!(stairs_shape(east, top_behind), "straight");
        // canTakeShape veto: an identical stair on the flank keeps
        // the row straight (long L-corridor walls in MC).
        let veto = |d: [i32; 2]| match d {
            [1, 0] => Some(3u8), // north behind (would be outer_left)
            [0, 1] => Some(0u8), // identical east stair on +Z flank
            _ => None,
        };
        assert_eq!(stairs_shape(east, veto), "straight");
        // No neighbours: straight.
        assert_eq!(stairs_shape(east, |_| None), "straight");
    }

    #[test]
    fn slab_place_state_faces() {
        // W411: MC SlabBlock.getStateForPlacement. Top face click ->
        // bottom slab; bottom face -> top slab.
        assert_eq!(slab_place_state([0, 1, 0], [0, 5, 0], [0.5, 7.0, 0.5], [0.0, -1.0, 0.0]), 0);
        assert_eq!(
            slab_place_state([0, -1, 0], [0, 5, 0], [0.5, 4.0, 0.5], [0.0, 1.0, 0.0]),
            TRAPDOOR_TOP_BIT
        );
        // Side click low on the face (eye level with the lower half,
        // looking flat +X onto the cell's -X face plane at x=0).
        assert_eq!(
            slab_place_state([1, 0, 0], [0, 5, 0], [-1.0, 5.25, 0.5], [1.0, 0.0, 0.0]),
            0
        );
        // Side click high on the face.
        assert_eq!(
            slab_place_state([1, 0, 0], [0, 5, 0], [-1.0, 5.75, 0.5], [1.0, 0.0, 0.0]),
            TRAPDOOR_TOP_BIT
        );
    }
    use super::*;

    #[test]
    fn toggle_flips_state() {
        let mut s: DoorStates = HashSet::new();
        let w = infengine_voxel::World::default();
        let p = [0, 64, 0];
        // Without any neighbour info, lower_pos returns pos as-is.
        assert!(!is_open(&s, &w, p));
        assert!(toggle_door(&mut s, &w, p));
        assert!(is_open(&s, &w, p));
        assert!(!toggle_door(&mut s, &w, p));
        assert!(!is_open(&s, &w, p));
    }

    #[test]
    fn door_block_range_matches_registry() {
        // Spot-check the legacy + W48 ids the registry calls doors.
        assert!(is_door_block(77));
        assert!(is_door_block(78));
        assert!(is_door_block(79));
        assert!(is_door_block(568));
        assert!(is_door_block(575));
        assert!(!is_door_block(76));
        assert!(!is_door_block(576));
        assert!(!is_door_block(1)); // stone
    }

    /// W166: the original 5 trapdoors plus the 4 new wood ones all
    /// register as trapdoors (so they toggle + render + collide via
    /// the shared path); neighbours are not trapdoors.
    #[test]
    fn trapdoor_block_range_matches_registry() {
        for id in [548, 549, 550, 551, 552, 722, 723, 724, 725, 750, 751] {
            assert!(is_trapdoor_block(id), "id {id} should be a trapdoor");
        }
        assert!(!is_trapdoor_block(547)); // fence below the range
        assert!(!is_trapdoor_block(553)); // gap after the first run
        assert!(!is_trapdoor_block(721)); // rabbit egg, just below
        assert!(!is_trapdoor_block(726)); // just above (a fence gate)
    }

    /// W167: fence gates are 726..=731 and are NOT trapdoors/doors.
    #[test]
    fn fence_gate_block_range() {
        for id in 726..=731 {
            assert!(is_fence_gate_block(id), "id {id} should be a fence gate");
            assert!(!is_trapdoor_block(id));
            assert!(!is_door_block(id));
        }
        assert!(!is_fence_gate_block(725)); // cherry trapdoor
        assert!(!is_fence_gate_block(732));
    }

    #[test]
    fn trapdoor_place_side_click_height_picks_half() {
        // Clicking the +X face of a support: facing = +X (code 0);
        // the half follows the exact click height on that face.
        // Place cell [5, 10, 5] -> face plane x = 5.0, midline 10.5.
        let eye = [3.5, 11.4, 5.5];
        let look = [1.0, -0.3, 0.0];
        // t = 1.5 -> y_hit = 11.4 - 0.45 = 10.95 > 10.5 -> TOP half.
        let s = trapdoor_place_state([1, 0, 0], [5, 10, 5], eye, look);
        assert_eq!(s & 3, 0);
        assert_ne!(s & TRAPDOOR_TOP_BIT, 0, "high click = top half");
        // Aim straight at the lower part of the face -> bottom half.
        let eye2 = [3.5, 10.2, 5.5];
        let s2 = trapdoor_place_state(
            [1, 0, 0],
            [5, 10, 5],
            eye2,
            [1.0, 0.0, 0.0],
        );
        assert_eq!(s2 & 3, 0);
        assert_eq!(s2 & TRAPDOOR_TOP_BIT, 0, "low click = bottom half");
    }

    #[test]
    fn trapdoor_place_top_bottom_faces() {
        // Top-face click -> BOTTOM half; facing = look reversed
        // (looking +X -> facing -X = code 2).
        let s = trapdoor_place_state(
            [0, 1, 0],
            [5, 10, 5],
            [5.5, 12.0, 5.5],
            [1.0, -1.0, 0.0],
        );
        assert_eq!(s & TRAPDOOR_TOP_BIT, 0, "floor placement = bottom");
        assert_eq!(s & 3, 2);
        // Bottom-face (ceiling) click -> TOP half.
        let s2 = trapdoor_place_state(
            [0, -1, 0],
            [5, 10, 5],
            [5.5, 9.0, 5.5],
            [1.0, 1.0, 0.0],
        );
        assert_ne!(s2 & TRAPDOOR_TOP_BIT, 0, "ceiling placement = top");
    }

    #[test]
    fn upper_half_resolves_to_lower() {
        use infengine_voxel::{BlockId, Chunk, ChunkCoord};
        let mut w = infengine_voxel::World::default();
        let cc: ChunkCoord = [0, 4, 0];
        w.insert_chunk(cc, Chunk::empty());
        // Place a fake lower door at y=64; upper at y=65.
        w.set([3, 64, 3], BlockId(77));
        w.set([3, 65, 3], BlockId(78));
        assert_eq!(lower_pos(&w, [3, 65, 3]), [3, 64, 3]);
        assert_eq!(lower_pos(&w, [3, 64, 3]), [3, 64, 3]);
    }
}
