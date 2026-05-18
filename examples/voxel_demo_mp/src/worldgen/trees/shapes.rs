//! Tree-template primitives shared by every species. Each fn is
//! deterministic + bounded; species `place_at` implementations
//! compose these to build their canopy.
//!
//! All `stamp_*` fns clip writes to the chunk bounds via
//! `set_in_chunk` so a tree anchored just outside the chunk only
//! drops the part of its silhouette that overlaps. The `margin`
//! sweep in `place_realistic_from_coord` is what makes that work
//! across chunk boundaries.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk};

/// Write `block` into `(lx, ly, lz)` only when the coord sits
/// inside the chunk. Shared by every species. Inlined so the
/// per-stamp loops stay tight.
#[inline]
pub(crate) fn set_in_chunk(chunk: &mut Chunk, lx: i32, ly: i32, lz: i32, block: BlockId) {
    let cs = CHUNK_SIZE as i32;
    if lx < 0 || lx >= cs || lz < 0 || lz >= cs || ly < 0 || ly >= cs {
        return;
    }
    chunk.set(lx as usize, ly as usize, lz as usize, block);
}

/// Vertical wood column from `local_y_start ..= local_y_end`
/// (inclusive). Used by every tree species for the trunk; pine +
/// sequoia raise tall columns, oak + acacia drop short ones.
pub(crate) fn stamp_trunk_column(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    local_y_start: i32,
    local_y_end: i32,
    wood: BlockId,
) {
    for y in local_y_start..=local_y_end {
        set_in_chunk(chunk, local_x, y, local_z, wood);
    }
}

/// 5x5 leaf layer with corners trimmed. The oak canopy's bread
/// + butter shape; used by birch + jungle layers too.
pub(crate) fn stamp_leaf_disc_5(
    chunk: &mut Chunk,
    local_x: i32,
    local_y: i32,
    local_z: i32,
    leaf: BlockId,
    skip_center: bool,
) {
    for dx in -2i32..=2 {
        for dz in -2i32..=2 {
            if dx.abs() == 2 && dz.abs() == 2 {
                continue;
            }
            if skip_center && dx == 0 && dz == 0 {
                continue;
            }
            set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, leaf);
        }
    }
}

/// 3x3 leaf cap, full square. The "shrinking pyramid" top of an
/// oak canopy + small layers for birch + savanna trees.
pub(crate) fn stamp_leaf_disc_3(
    chunk: &mut Chunk,
    local_x: i32,
    local_y: i32,
    local_z: i32,
    leaf: BlockId,
) {
    for dx in -1i32..=1 {
        for dz in -1i32..=1 {
            set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, leaf);
        }
    }
}

/// 7x7 leaf layer with corners trimmed. Used by sequoia + jungle
/// bigleaf for broad top canopies.
pub(crate) fn stamp_leaf_disc_7(
    chunk: &mut Chunk,
    local_x: i32,
    local_y: i32,
    local_z: i32,
    leaf: BlockId,
) {
    for dx in -3i32..=3 {
        for dz in -3i32..=3 {
            let r2 = dx * dx + dz * dz;
            if r2 > 9 {
                continue; // clip the 4-corner cells of the 7x7
            }
            set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, leaf);
        }
    }
}

/// Cone-shaped pine layer: thicker at the bottom, narrows toward
/// the top. `radius` controls the half-width; the function clips
/// against a Chebyshev distance so the layer reads as a square
/// silhouette (matches typical voxel pine art).
pub(crate) fn stamp_leaf_square(
    chunk: &mut Chunk,
    local_x: i32,
    local_y: i32,
    local_z: i32,
    radius: i32,
    leaf: BlockId,
    skip_center: bool,
) {
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if skip_center && dx == 0 && dz == 0 {
                continue;
            }
            set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, leaf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::Chunk;

    fn count_blocks_with_id(chunk: &Chunk, id: u16) -> usize {
        let mut n = 0;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if chunk.get(x, y, z).0 == id {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    #[test]
    fn set_in_chunk_clips_negative() {
        let mut c = Chunk::empty();
        set_in_chunk(&mut c, -1, 0, 0, BlockId(6));
        set_in_chunk(&mut c, 0, -1, 0, BlockId(6));
        set_in_chunk(&mut c, 0, 0, -1, BlockId(6));
        assert_eq!(count_blocks_with_id(&c, 6), 0);
    }

    #[test]
    fn set_in_chunk_clips_overflow() {
        let mut c = Chunk::empty();
        let cs = CHUNK_SIZE as i32;
        set_in_chunk(&mut c, cs, 0, 0, BlockId(6));
        set_in_chunk(&mut c, 0, cs, 0, BlockId(6));
        set_in_chunk(&mut c, 0, 0, cs, BlockId(6));
        assert_eq!(count_blocks_with_id(&c, 6), 0);
    }

    #[test]
    fn trunk_column_writes_exactly_n_blocks() {
        let mut c = Chunk::empty();
        stamp_trunk_column(&mut c, 5, 5, 1, 6, BlockId(5));
        assert_eq!(count_blocks_with_id(&c, 5), 6);
    }

    #[test]
    fn leaf_disc_5_writes_21_blocks_no_skip() {
        let mut c = Chunk::empty();
        // 5x5 = 25, minus 4 corners = 21.
        stamp_leaf_disc_5(&mut c, 5, 5, 5, BlockId(6), false);
        assert_eq!(count_blocks_with_id(&c, 6), 21);
    }

    #[test]
    fn leaf_disc_5_skip_center_writes_20() {
        let mut c = Chunk::empty();
        stamp_leaf_disc_5(&mut c, 5, 5, 5, BlockId(6), true);
        assert_eq!(count_blocks_with_id(&c, 6), 20);
    }

    #[test]
    fn leaf_disc_3_writes_9_blocks() {
        let mut c = Chunk::empty();
        stamp_leaf_disc_3(&mut c, 5, 5, 5, BlockId(6));
        assert_eq!(count_blocks_with_id(&c, 6), 9);
    }

    #[test]
    fn leaf_disc_7_writes_29_blocks() {
        // 7x7 = 49; clip cells with r^2 > 9. Cells with r^2 <= 9
        // count to 29 (rings of r2 = 0,1,2,4,5,8,9 -> sizes
        // 1+4+4+4+8+4+4 = 29). The "7-wide disc" name is loose:
        // the discriminant trims it into a near-circle.
        let mut c = Chunk::empty();
        stamp_leaf_disc_7(&mut c, 8, 8, 8, BlockId(6));
        assert_eq!(count_blocks_with_id(&c, 6), 29);
    }

    #[test]
    fn leaf_square_radius_2_writes_25_blocks() {
        let mut c = Chunk::empty();
        stamp_leaf_square(&mut c, 5, 5, 5, 2, BlockId(6), false);
        assert_eq!(count_blocks_with_id(&c, 6), 25);
    }

    #[test]
    fn leaf_square_skip_center_writes_one_less() {
        let mut c = Chunk::empty();
        stamp_leaf_square(&mut c, 5, 5, 5, 2, BlockId(6), true);
        assert_eq!(count_blocks_with_id(&c, 6), 24);
    }
}
