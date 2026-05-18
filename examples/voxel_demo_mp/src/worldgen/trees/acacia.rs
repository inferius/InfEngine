//! Acacia: savanna tree. Short trunk + two angled branches
//! ending in flat-disc canopies. Distinctive flat-top
//! silhouette.

use infengine_voxel::{BlockId, Chunk};

use super::shapes::{set_in_chunk, stamp_leaf_disc_3, stamp_leaf_disc_5, stamp_trunk_column};

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
    // Trunk: 5 blocks high; the branches split off at the top.
    stamp_trunk_column(
        chunk,
        local_x,
        local_z,
        ground + 1 - origin_y,
        ground + 5 - origin_y,
        WOOD,
    );
    // Pick two angled directions from the rng for the branches.
    // 4 cardinal options; second branch picks the next-in-rotation
    // direction so they never overlap.
    let dir1 = (rng % 4) as i32;
    let (dx1, dz1) = match dir1 {
        0 => (1, 0),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (0, -1),
    };
    let dir2 = (dir1 + 1) % 4;
    let (dx2, dz2) = match dir2 {
        0 => (1, 0),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (0, -1),
    };
    // Branches: 2 wood blocks angling up + outward, ending in a
    // flat 5x5 leaf disc.
    let stamp_branch = |chunk: &mut Chunk, dx: i32, dz: i32| {
        set_in_chunk(chunk, local_x + dx, ground + 6 - origin_y, local_z + dz, WOOD);
        set_in_chunk(
            chunk,
            local_x + 2 * dx,
            ground + 7 - origin_y,
            local_z + 2 * dz,
            WOOD,
        );
        // Flat canopy: 5x5 layer + small 3x3 cap above.
        stamp_leaf_disc_5(
            chunk,
            local_x + 2 * dx,
            ground + 7 - origin_y,
            local_z + 2 * dz,
            LEAF,
            true,
        );
        stamp_leaf_disc_3(
            chunk,
            local_x + 2 * dx,
            ground + 8 - origin_y,
            local_z + 2 * dz,
            LEAF,
        );
    };
    stamp_branch(chunk, dx1, dz1);
    stamp_branch(chunk, dx2, dz2);
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
    fn acacia_trunk_is_5_blocks() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 1, 0, 0);
        let wood = count_id(&c, 5);
        // Main trunk (5) + 2 branches x 2 wood blocks = 9.
        assert_eq!(wood, 9);
    }

    #[test]
    fn acacia_is_deterministic_per_rng() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        place_at(&mut a, 8, 8, 1, 0, 0xACAC1A);
        place_at(&mut b, 8, 8, 1, 0, 0xACAC1A);
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }

    #[test]
    fn acacia_has_two_canopies() {
        let mut c = Chunk::empty();
        place_at(&mut c, 8, 8, 1, 0, 0);
        // 2 canopies x (20 + 9) = 58 leaves.
        assert!(count_id(&c, 6) > 30);
    }
}
