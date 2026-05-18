//! Jungle bigleaf: 2x2 trunk + 10-15 high + broad flat canopy.
//! The 2x2 trunk is what distinguishes it from oak/birch at a
//! glance; the canopy reads as a wide leafy hat.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{set_in_chunk, stamp_leaf_disc_5, stamp_leaf_disc_7, stamp_trunk_column};

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
    // Trunk height jitters 10-13.
    let trunk_h = 10 + ((rng >> 4) % 4) as i32;
    // 2x2 trunk anchored at (local_x..=local_x+1, local_z..=local_z+1).
    for ox in 0i32..2 {
        for oz in 0i32..2 {
            stamp_trunk_column(
                chunk,
                local_x + ox,
                local_z + oz,
                ground + 1 - origin_y,
                ground + trunk_h - origin_y,
                WOOD,
            );
        }
    }
    // Canopy: broad 7-disc + a 5-disc one above, both centred
    // between the four trunks (local_x .. local_x+1).
    let cx = local_x; // bias one side; close enough on a 2-block trunk
    let cz = local_z;
    let top = ground + trunk_h;
    stamp_leaf_disc_7(chunk, cx, top - origin_y, cz, LEAF);
    stamp_leaf_disc_7(chunk, cx + 1, top - origin_y, cz + 1, LEAF);
    stamp_leaf_disc_5(chunk, cx, top + 1 - origin_y, cz, LEAF, false);
    set_in_chunk(chunk, cx, top + 2 - origin_y, cz, LEAF);
    set_in_chunk(chunk, cx + 1, top + 2 - origin_y, cz + 1, LEAF);
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
    fn jungle_has_2x2_trunk() {
        let mut c = Chunk::empty();
        // Anchor low so the canopy fits inside the chunk.
        place_at(&mut c, 8, 8, 1, 0, 0);
        let trunk_h = count_id(&c, 5);
        // Trunk: 4 columns x (10..=13) rows = 40..=52. But the
        // 7-disc canopy stamps leaves over the top voxel of each
        // trunk column (4 cells), leaving 36..=48 wood blocks.
        assert!(
            (36..=48).contains(&trunk_h),
            "trunk_h={trunk_h}"
        );
    }

    #[test]
    fn jungle_is_deterministic_per_rng() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 8, 8, 1, 0, 0xC0DE_BABE);
        place_at(&mut b, 8, 8, 1, 0, 0xC0DE_BABE);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }

    #[test]
    fn jungle_has_broad_canopy() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 1, 0, 0);
        // Two 7-discs (37 each) + one 5-disc full (21) + 2 caps
        // = 97. Some leaf overlap between the two 7-discs reduces
        // the count; assert a generous lower bound.
        assert!(count_id(&c, 6) > 60);
    }
}
