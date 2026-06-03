//! W65.2 - runtime sapling -> tree growth.
//!
//! The worldgen tree generators (`worldgen::trees::*::place_at`)
//! write into a single chunk during generation and clip to that
//! chunk's bounds, so they can't grow a tree that straddles chunk
//! boundaries at runtime. This module builds the tree in *world*
//! space instead: it returns the list of `(pos, block)` edits, and
//! the caller applies them through the shared edit funnel
//! (`world::edit::emit`) so each block is meshed + broadcast like any
//! other edit.
//!
//! W65.2 grows the small-oak shape (matching `worldgen::trees::oak`)
//! for every standard sapling species. Per-species canopies
//! (birch / spruce conifer / etc.) are a follow-up.

use infengine_voxel::{BlockId, World};

/// Oak log id (generic wood).
const WOOD: BlockId = BlockId(5);
/// Oak leaves id.
const LEAF: BlockId = BlockId(6);

/// Build the world-space `(pos, block)` edits to grow a small oak
/// with its trunk base at `base` (the sapling cell). Returns `None`
/// when the trunk lacks vertical headroom (the 3 cells above the
/// sapling aren't all air) so the caller can leave the sapling to
/// retry on a later random tick.
///
/// Shape mirrors `worldgen::trees::oak::place_at`: a 4-log trunk, two
/// 5x5 leaf discs (corners trimmed) at trunk-top -1 / 0 with the
/// lower disc's centre skipped, a 3x3 cap, and a single top leaf.
/// Leaves are written only into air so the canopy never clobbers
/// terrain, logs, or a neighbour's blocks.
pub(crate) fn grow_oak_edits(
    world: &World,
    base: [i32; 3],
) -> Option<Vec<([i32; 3], BlockId)>> {
    // Trunk headroom: the 3 cells above the sapling must be air.
    for dy in 1..=3 {
        if !world.get([base[0], base[1] + dy, base[2]]).is_air() {
            return None;
        }
    }
    let mut edits: Vec<([i32; 3], BlockId)> = Vec::with_capacity(56);
    // Trunk: 4 logs, base..=base+3 (base replaces the sapling). The
    // layer-1 disc below re-stamps a leaf over base+3, matching the
    // worldgen oak's "3 visible wood + leaf top".
    for dy in 0..=3 {
        edits.push(([base[0], base[1] + dy, base[2]], WOOD));
    }
    // Two 5x5 leaf discs (corners trimmed). Lower disc (layer 0)
    // skips the centre (trunk lives there); upper disc keeps it.
    for (layer, ly) in [base[1] + 2, base[1] + 3].into_iter().enumerate() {
        let skip_center = layer == 0;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                if dx.abs() == 2 && dz.abs() == 2 {
                    continue; // trim corners
                }
                if skip_center && dx == 0 && dz == 0 {
                    continue;
                }
                push_leaf(world, &mut edits, [base[0] + dx, ly, base[2] + dz]);
            }
        }
    }
    // 3x3 cap above the discs.
    for dx in -1i32..=1 {
        for dz in -1i32..=1 {
            push_leaf(world, &mut edits, [base[0] + dx, base[1] + 4, base[2] + dz]);
        }
    }
    // Single top leaf.
    push_leaf(world, &mut edits, [base[0], base[1] + 5, base[2]]);
    Some(edits)
}

/// Queue a leaf edit only when the target cell is currently air.
fn push_leaf(world: &World, edits: &mut Vec<([i32; 3], BlockId)>, pos: [i32; 3]) {
    if world.get(pos).is_air() {
        edits.push((pos, LEAF));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{Chunk, World};

    fn test_world() -> World {
        let mut w = World::new();
        for cx in 0..=1 {
            for cy in 0..=1 {
                for cz in 0..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        w
    }

    #[test]
    fn grows_trunk_and_canopy_when_clear() {
        let w = test_world();
        let edits = grow_oak_edits(&w, [8, 4, 8]).expect("clear headroom");
        let logs = edits.iter().filter(|(_, b)| *b == WOOD).count();
        let leaves = edits.iter().filter(|(_, b)| *b == LEAF).count();
        assert_eq!(logs, 4, "4-log trunk");
        // Same canopy count as the worldgen oak (51 leaves) since the
        // test world is otherwise empty (every disc cell is air).
        assert_eq!(leaves, 51, "full oak canopy");
    }

    #[test]
    fn trunk_base_replaces_sapling_cell() {
        let w = test_world();
        let edits = grow_oak_edits(&w, [8, 4, 8]).unwrap();
        assert!(edits.contains(&([8, 4, 8], WOOD)));
        assert!(edits.contains(&([8, 7, 8], WOOD))); // base+3
    }

    #[test]
    fn blocked_headroom_returns_none() {
        let mut w = test_world();
        w.set([8, 6, 8], BlockId(1)); // stone two above the sapling
        assert!(grow_oak_edits(&w, [8, 4, 8]).is_none());
    }

    #[test]
    fn leaves_never_overwrite_existing_blocks() {
        let mut w = test_world();
        // Pre-place a stone where a canopy leaf would land.
        w.set([7, 6, 8], BlockId(1)); // inside the lower 5x5 disc
        let edits = grow_oak_edits(&w, [8, 4, 8]).unwrap();
        assert!(
            !edits.iter().any(|(p, b)| *p == [7, 6, 8] && *b == LEAF),
            "leaf must not target an occupied cell"
        );
    }
}
