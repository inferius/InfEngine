//! Flower decoration pass for the Classic generator. Sparse
//! 3-colour bloom on plains-biome grass; uses a different hash
//! salt than trees so the patterns don't overlap.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, World};

use super::WorldgenCtx;
use super::biome::{Biome, biome_at};
use super::hash2_for;
use super::height::height_at_classic;

/// Place flowers on the top voxel of any plains grass cell.
/// `amp_scale` tracks the height curve so Amplified still anchors
/// flowers to the right Y.
pub(crate) fn place_classic(
    chunk: &mut Chunk,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    amp_scale: f32,
    seed: u32,
) {
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5, seed);
            if biome != Biome::Plains {
                continue;
            }
            if hash2_for(wx ^ 0x9A75, wz ^ 0x5BA9, seed) % 18 != 3 {
                continue;
            }
            let ground = height_at_classic(wx as f32 + 0.5, wz as f32 + 0.5, biome, amp_scale);
            let local_y = (ground + 1) - origin_y;
            if !(0..cs).contains(&local_y) {
                continue;
            }
            let surface_y = local_y - 1;
            if !(0..cs).contains(&surface_y) {
                continue;
            }
            if chunk.get(x as usize, surface_y as usize, z as usize).0 != 3 {
                continue;
            }
            if chunk.get(x as usize, local_y as usize, z as usize).0 != 0 {
                continue;
            }
            let h = hash2_for(wx ^ 0x5FAD, wz ^ 0x9117, seed);
            let flower_id = match h % 3 {
                0 => 21u16, // yellow
                1 => 22,    // blue
                _ => 20,    // red
            };
            chunk.set(x as usize, local_y as usize, z as usize, BlockId(flower_id));
        }
    }
}

/// Helper used by both the chunk orchestrator + the test. The
/// orchestrator passes the chunk's world origin so this fn
/// doesn't need to redo the lookup.
pub(crate) fn place_classic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    amp_scale: f32,
    ctx: &WorldgenCtx,
) {
    let origin = World::chunk_world_origin(coord);
    place_classic(
        chunk,
        origin.x as i32,
        origin.y as i32,
        origin.z as i32,
        amp_scale,
        ctx.effective_seed(),
    );
}

// W146: the MC flower roster per biome (the named 26.x flower
// blocks, replacing the legacy 3-colour placeholders on the
// realistic path). Single-species CLUSTERED patches like MC's
// flower feature, not a uniform sprinkle.
const FLOWERS_PLAINS: &[u16] = &[480, 481, 482, 483, 478]; // dandelion/poppy/oxeye_daisy/cornflower/allium
const FLOWERS_FOREST: &[u16] = &[481, 480, 484]; // poppy/dandelion/lily_of_the_valley
const FLOWERS_SWAMP: &[u16] = &[479]; // blue_orchid

/// W146: MC-style clustered flower placement. A coarse 16x16 patch
/// cell rolls whether it hosts a patch, its centre and its single
/// species (the cell hash is chunk-independent so patches continue
/// seamlessly across chunk borders); columns within the patch
/// radius then roll densely. Returns the flower for this column.
pub(crate) fn flower_patch_pick(
    biome: Biome,
    wx: i32,
    wz: i32,
    seed: u32,
) -> Option<u16> {
    let set: &[u16] = match biome {
        Biome::Plains | Biome::Savanna => FLOWERS_PLAINS,
        Biome::Forest => FLOWERS_FOREST,
        Biome::Swamp => FLOWERS_SWAMP,
        _ => return None,
    };
    let cell_x = wx.div_euclid(16);
    let cell_z = wz.div_euclid(16);
    let cell = hash2_for(cell_x ^ 0x7F10, cell_z ^ 0x77E2, seed);
    // 1 in 3 cells hosts a patch.
    if !cell.is_multiple_of(3) {
        return None;
    }
    let cx = cell_x * 16 + ((cell >> 8) % 16) as i32;
    let cz = cell_z * 16 + ((cell >> 12) % 16) as i32;
    let (dx, dz) = (wx - cx, wz - cz);
    // Patch radius ~4.2 blocks.
    if dx * dx + dz * dz > 18 {
        return None;
    }
    // Per-column density inside the patch (~1/3 of cells bloom).
    if !hash2_for(wx ^ 0x5FAD, wz ^ 0x9117, seed).is_multiple_of(3) {
        return None;
    }
    Some(set[(cell >> 16) as usize % set.len()])
}

/// Realistic-profile flower pass: MC clustered single-species
/// patches on Plains / Savanna / Forest / Swamp grass (W146).
pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    ctx: &WorldgenCtx,
) {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let origin_x = origin.x as i32;
    let origin_z = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = super::realistic_biome_at(ctx, wx as f32 + 0.5, wz as f32 + 0.5);
            let Some(flower_id) = flower_patch_pick(biome, wx, wz, seed) else {
                continue;
            };
            // Walk down from the top to find the first grass
            // voxel; flowers sit on top of it. Cheaper than re-
            // running the height fn for every cell.
            let mut local_y = -1i32;
            for ly in (0..cs).rev() {
                if chunk.get(x as usize, ly as usize, z as usize).0 == 3 {
                    local_y = ly + 1;
                    break;
                }
            }
            if !(0..cs).contains(&local_y) {
                continue;
            }
            if chunk.get(x as usize, local_y as usize, z as usize).0 != 0 {
                continue;
            }
            chunk.set(x as usize, local_y as usize, z as usize, BlockId(flower_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W146: patches cluster - a hit column implies more hits in its
    /// 16x16 cell than a uniform 1/18 sprinkle would give, all of the
    /// SAME species; and the pick is chunk-independent (pure fn of
    /// world coords).
    #[test]
    fn flower_patches_cluster_single_species() {
        let seed = 1234u32;
        let mut total = 0usize;
        let mut best_cell = (0usize, 0u16);
        for cell_x in 0..40 {
            for cell_z in 0..40 {
                let mut here = 0usize;
                let mut species: Option<u16> = None;
                for lx in 0..16 {
                    for lz in 0..16 {
                        let wx = cell_x * 16 + lx;
                        let wz = cell_z * 16 + lz;
                        if let Some(f) =
                            flower_patch_pick(Biome::Plains, wx, wz, seed)
                        {
                            here += 1;
                            // Single species per patch cell.
                            assert!(
                                species.is_none() || species == Some(f),
                                "cell ({cell_x},{cell_z}) mixes species"
                            );
                            species = Some(f);
                            assert!(
                                FLOWERS_PLAINS.contains(&f),
                                "non-plains flower {f}"
                            );
                        }
                    }
                }
                total += here;
                if here > best_cell.0 {
                    best_cell = (here, species.unwrap_or(0));
                }
            }
        }
        assert!(total > 100, "plains should bloom, got {total}");
        assert!(
            best_cell.0 >= 5,
            "patches should cluster (best cell {best_cell:?})"
        );
        // Deserts stay bare.
        for wx in 0..640 {
            assert!(flower_patch_pick(Biome::Desert, wx, 7, seed).is_none());
        }
    }
}
