//! Sequoia: the giant of the forest. ~24-block trunk + 4-layer
//! thick canopy near the top. Spawns rarely (Forest 7%) so the
//! world reads at a normal scale, but each one is unmistakable
//! when you spot it. Uses a 7-wide canopy disc to make the
//! silhouette legible from far away.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{set_in_chunk, stamp_leaf_disc_5, stamp_leaf_disc_7, stamp_trunk_column};

pub(crate) const WOOD: BlockId = BlockId(5);
pub(crate) const LEAF: BlockId = BlockId(6);

/// Sequoia trunk height. 24 blocks; fits inside one CHUNK_SIZE=32
/// chunk for the canopy at typical ground heights.
const TRUNK_H: i32 = 18;

pub(crate) fn place_at(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
) {
    // Trunk.
    stamp_trunk_column(
        chunk,
        local_x,
        local_z,
        ground + 1 - origin_y,
        ground + TRUNK_H - origin_y,
        WOOD,
    );
    // Canopy: 4 stacked layers near the top. Bottom layer is a
    // 7x7 trimmed disc; the next two are 7x7 + 5x5; top is a
    // single leaf block. Centres around y = ground + TRUNK_H.
    let top = ground + TRUNK_H;
    stamp_leaf_disc_7(chunk, local_x, top - 3 - origin_y, local_z, LEAF);
    stamp_leaf_disc_7(chunk, local_x, top - 2 - origin_y, local_z, LEAF);
    stamp_leaf_disc_5(chunk, local_x, top - 1 - origin_y, local_z, LEAF, true);
    stamp_leaf_disc_5(chunk, local_x, top - origin_y, local_z, LEAF, false);
    set_in_chunk(chunk, local_x, top + 1 - origin_y, local_z, LEAF);
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{CHUNK_SIZE, Chunk};

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
    fn sequoia_writes_trunk_in_chunk() {
        // The sequoia is 18 trunks high + a 4-layer canopy on
        // top, so a full template doesn't fit in a 16-cube
        // chunk. Anchor ground=-1 so the trunk + canopy span
        // y=0..=22; only y=0..=15 is visible. The 14 trunk
        // blocks at y=0..=13 should be wood, then canopy starts
        // stamping over the trunk top.
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, -1, 0);
        let wood = count_id(&c, 5);
        // Bottom 13 trunk blocks land before the canopy starts;
        // canopy at top-3 = (TRUNK_H-1-3)-1 = 13 overstamps the
        // 14th. So 13 trunks survive intact.
        assert!(wood >= 12, "wood={wood}");
    }

    #[test]
    fn sequoia_writes_canopy_inside_chunk() {
        // Anchor low; the canopy lives near the top of the
        // chunk.
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, -3, 0);
        // Canopy reaches y = top-3 = ground+TRUNK_H-3 = 12 to
        // ground+TRUNK_H+1 = 16. Most of it should land at
        // y=12..=15 (4 layers in chunk).
        let leaves = count_id(&c, 6);
        assert!(leaves > 50, "leaves={leaves}");
    }

    #[test]
    fn sequoia_is_deterministic() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 8, 8, -1, 0);
        place_at(&mut b, 8, 8, -1, 0);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }
}
