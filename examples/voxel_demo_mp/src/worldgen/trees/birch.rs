//! Birch tree: a taller, thinner oak with light-bark wood. We
//! share the wood/leaf block ids with oak (no birch-specific
//! textures in the atlas yet); the silhouette is a 6-block trunk
//! plus a single 5x5 canopy plus a 3x3 cap, much more vertical
//! than the chunky oak.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{
    set_in_chunk, stamp_leaf_disc_3, stamp_leaf_disc_5, stamp_trunk_column,
};

pub(crate) const WOOD: BlockId = BlockId(5);
pub(crate) const LEAF: BlockId = BlockId(6);

pub(crate) fn place_at(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
    rng: u32,
) {
    // Trunk height varies 6-7 from the rng. Two-block jitter so
    // a stand of birches doesn't read as a hedge.
    let trunk_h = 6 + ((rng >> 4) % 2) as i32;
    stamp_trunk_column(
        chunk,
        local_x,
        local_z,
        ground + 1 - origin_y,
        ground + trunk_h - origin_y,
        WOOD,
    );
    // Single 5x5 canopy at trunk top, then a 3x3 cap, then a
    // single leaf block on top.
    let top = ground + trunk_h;
    stamp_leaf_disc_5(chunk, local_x, top - 1 - origin_y, local_z, LEAF, true);
    stamp_leaf_disc_3(chunk, local_x, top + 1 - origin_y, local_z, LEAF);
    set_in_chunk(chunk, local_x, top + 2 - origin_y, local_z, LEAF);
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
    fn birch_trunk_is_6_or_7_blocks() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 5, 0, 0);
        let trunk_h = count_id(&c, 5);
        assert!((6..=7).contains(&trunk_h), "trunk_h={trunk_h}");
    }

    #[test]
    fn birch_is_deterministic_per_rng() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 8, 8, 5, 0, 0x1234_5678);
        place_at(&mut b, 8, 8, 5, 0, 0x1234_5678);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }

    #[test]
    fn birch_has_leaves() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 5, 0, 0);
        assert!(count_id(&c, 6) > 20);
    }
}
