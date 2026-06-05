//! W81 - Mega spruce: the 2x2-trunk giant conifer of old-growth taiga
//! (MC's giant_spruce_taiga tree). A tall trunk (16-22) wears a narrow,
//! ragged cone of square leaf skirts. Reuses pine's wood/leaf blocks so
//! it reads as a much larger sibling of the regular taiga pine.

use infengine_voxel::Chunk;

use super::pine::{LEAF, WOOD};
use super::shapes::{set_in_chunk, stamp_leaf_square, stamp_trunk_column};

pub(crate) fn place_at(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
    rng: u32,
) {
    let trunk_h = 16 + ((rng >> 4) % 7) as i32; // 16..=22
    // Canopy first: a ragged cone over the top ~55% of the trunk, square
    // leaf skirts of radius 2-3 pinching to a 1-wide tip. Two skirts per
    // layer (offset by the 2x2) fill the footprint. The trunk is stamped
    // AFTER so its wood pokes through the cone instead of being eaten by
    // the (overwriting) leaf stamps.
    let canopy_h = (trunk_h * 55 / 100).max(7);
    let base = ground + trunk_h - canopy_h;
    for i in 0..canopy_h {
        let y = base + i - origin_y;
        let near_top = i * 100 / canopy_h > 80;
        let r = if near_top {
            1
        } else if i % 3 == 0 {
            3
        } else {
            2
        };
        stamp_leaf_square(chunk, local_x, y, local_z, r, LEAF, false);
        stamp_leaf_square(chunk, local_x + 1, y, local_z + 1, r, LEAF, false);
    }
    set_in_chunk(chunk, local_x, ground + trunk_h + 1 - origin_y, local_z, LEAF);
    // 2x2 trunk - the giant's signature - stamped last so it reads
    // through the foliage. Stops one below the cap so the tip is leaves.
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        stamp_trunk_column(
            chunk,
            local_x + dx,
            local_z + dz,
            ground + 1 - origin_y,
            ground + trunk_h - 1 - origin_y,
            WOOD,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::CHUNK_SIZE;

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
    fn mega_spruce_is_a_tall_2x2_giant() {
        let mut c = Chunk::empty();
        // Place low so the whole trunk fits in one chunk.
        place_at(&mut c, 6, 6, 1, 0, 0xABCD_1234);
        // A 2x2 trunk that's 16+ tall is far more wood than a normal
        // pine (a single ~9-block column).
        assert!(count_id(&c, WOOD.0) > 30, "trunk too small for a giant");
        assert!(count_id(&c, LEAF.0) > 40, "canopy too sparse");
    }

    #[test]
    fn mega_spruce_is_deterministic_per_rng() {
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
