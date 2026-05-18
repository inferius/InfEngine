//! Pine: tall conifer. Trunk ~8-10 high with progressively
//! narrower square leaf layers from radius 3 down to radius 1,
//! capped with a single leaf block. Reads as a classic christmas
//! tree silhouette.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{set_in_chunk, stamp_leaf_square, stamp_trunk_column};

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
    // Trunk height jitters 8-10. Pines stand taller than oaks /
    // birches so the canopy can stack three+ layers.
    let trunk_h = 8 + ((rng >> 4) % 3) as i32;
    stamp_trunk_column(
        chunk,
        local_x,
        local_z,
        ground + 1 - origin_y,
        ground + trunk_h - origin_y,
        WOOD,
    );
    // Canopy: 4 stacked layers, radius 3 -> 2 -> 1 -> single
    // block on top.
    let canopy_base = ground + trunk_h - 3;
    stamp_leaf_square(chunk, local_x, canopy_base - origin_y, local_z, 3, LEAF, true);
    stamp_leaf_square(chunk, local_x, canopy_base + 1 - origin_y, local_z, 2, LEAF, true);
    stamp_leaf_square(chunk, local_x, canopy_base + 2 - origin_y, local_z, 1, LEAF, true);
    set_in_chunk(chunk, local_x, canopy_base + 3 - origin_y, local_z, LEAF);
    set_in_chunk(chunk, local_x, canopy_base + 4 - origin_y, local_z, LEAF);
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
    fn pine_trunk_is_tall() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 1, 0, 0);
        let trunk_h = count_id(&c, 5);
        // Trunk is 8..=10 wood blocks, but the canopy's single
        // leaf cap stamps over the trunk top so the surviving
        // wood count is one less.
        assert!((7..=9).contains(&trunk_h), "trunk_h={trunk_h}");
    }

    #[test]
    fn pine_canopy_is_pyramidal() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 1, 0, 0);
        // Pyramidal leaf stack; assert a generous lower bound
        // (parts of the top discs may overflow chunk Y bounds
        // at higher anchor positions, but at ground=1 most of
        // the canopy fits).
        assert!(count_id(&c, 6) > 30);
    }

    #[test]
    fn pine_is_deterministic_per_rng() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 8, 8, 1, 0, 0xDEAD_BEEF);
        place_at(&mut b, 8, 8, 1, 0, 0xDEAD_BEEF);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }
}
