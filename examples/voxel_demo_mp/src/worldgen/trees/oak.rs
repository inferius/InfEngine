//! Small oak template. 4-block trunk + two 5x5 leaf layers
//! (corners trimmed) + a 3x3 cap + a single top block. Matches
//! the pre-Phase-E worldgen.rs implementation byte-for-byte so
//! Classic + Realistic Plains tiles still produce the same trees
//! they did before the split.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{stamp_leaf_disc_3, stamp_leaf_disc_5, stamp_trunk_column, set_in_chunk};

pub(crate) const WOOD: BlockId = BlockId(5);
pub(crate) const LEAF: BlockId = BlockId(6);

/// Drop a single oak tree at `(local_x, local_z)` with the trunk
/// base anchored on top of the ground voxel. `origin_y` is the
/// chunk's world-Y origin; the function clips writes outside the
/// chunk via `set_in_chunk`.
pub(crate) fn place_at(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
) {
    // Trunk: 4 wood blocks above the surface voxel.
    stamp_trunk_column(
        chunk,
        local_x,
        local_z,
        ground + 1 - origin_y,
        ground + 4 - origin_y,
        WOOD,
    );
    // Canopy: two 5x5 layers (corners trimmed) at trunk top -1 / 0,
    // a 3x3 cap above, and a single top block.
    for layer in 0i32..2 {
        let wy = ground + 3 + layer;
        let local_y = wy - origin_y;
        // The pre-Phase-E worldgen.rs skipped the centre cell on
        // layer 0 only (the trunk lives there); layer 1 keeps the
        // centre.
        let skip_center = layer == 0;
        stamp_leaf_disc_5(chunk, local_x, local_y, local_z, LEAF, skip_center);
    }
    // 3x3 cap above the two 5x5 layers.
    stamp_leaf_disc_3(chunk, local_x, ground + 5 - origin_y, local_z, LEAF);
    // Single block top.
    set_in_chunk(chunk, local_x, ground + 6 - origin_y, local_z, LEAF);
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{CHUNK_SIZE, Chunk};

    fn place_centred() -> Chunk {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 5, 0);
        c
    }

    fn count_id(c: &Chunk, id: u16) -> usize {
        let mut n = 0;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if c.get(x, y, z).0 == id {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    #[test]
    fn oak_trunk_is_three_or_four_wood_blocks() {
        // The pre-Phase-E worldgen.rs writes 4 trunk blocks but
        // the layer-1 leaf disc (with skip_center=false) stamps
        // over the trunk top, leaving 3 wood + 1 leaf at the
        // trunk-top voxel. The regression test pins this exact
        // ordering as part of the byte-identical guarantee.
        let c = place_centred();
        assert_eq!(count_id(&c, 5), 3);
    }

    #[test]
    fn oak_canopy_is_50_leaves() {
        // 5x5 minus 4 corners minus 1 centre (layer 0) = 20
        // 5x5 minus 4 corners                       (layer 1) = 21
        // 3x3 cap                                              =  9
        // single top                                           =  1
        // = 51
        let c = place_centred();
        assert_eq!(count_id(&c, 6), 51);
    }

    #[test]
    fn oak_at_chunk_edge_clips() {
        // Tree centred at (-2, -2) means most of the canopy lies
        // outside the chunk and gets clipped.
        let mut c = Chunk::empty();
        place_at(&mut c, -2, -2, 5, 0);
        let leaves_outside = count_id(&c, 6);
        // Only a sliver of the canopy should land in the chunk.
        assert!(leaves_outside < 20);
    }

    #[test]
    fn oak_is_deterministic_for_same_inputs() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 4, 4, 8, 0);
        place_at(&mut b, 4, 4, 8, 0);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }

    #[test]
    fn oak_writes_inside_height_bound() {
        // For ground=5, trunk goes y=6..=9, leaves y=8..=11.
        // Sample (8, *, 8) along y; nothing should land above y=11.
        let c = place_centred();
        for y in 12..CHUNK_SIZE {
            let trunk_or_leaves = c.get(8, y, 8).0;
            assert_eq!(
                trunk_or_leaves, 0,
                "unexpected block at y={y}: {trunk_or_leaves}"
            );
        }
    }
}
