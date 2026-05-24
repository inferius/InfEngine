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
    matches!(id, 77..=79) || (568..=575).contains(&id)
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
