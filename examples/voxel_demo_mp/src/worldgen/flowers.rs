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

/// Realistic-profile flower pass. Drops the same 3-colour bloom
/// on Plains / Forest grass. Other biomes stay flowerless until
/// E.4 adds biome-specific decorations.
pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    ctx: &WorldgenCtx,
) {
    use super::biome_table::biome_from_climate;
    use super::climate::climate_at;
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let origin_x = origin.x as i32;
    let origin_y = origin.y as i32;
    let origin_z = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let climate = climate_at(wx as f32 + 0.5, wz as f32 + 0.5, seed);
            let biome = biome_from_climate(climate);
            if !matches!(biome, Biome::Plains | Biome::Forest) {
                continue;
            }
            if hash2_for(wx ^ 0x9A75, wz ^ 0x5BA9, seed) % 18 != 3 {
                continue;
            }
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
            let h = hash2_for(wx ^ 0x5FAD, wz ^ 0x9117, seed);
            let flower_id = match h % 3 {
                0 => 21u16,
                1 => 22,
                _ => 20,
            };
            chunk.set(x as usize, local_y as usize, z as usize, BlockId(flower_id));
            let _ = origin_y; // origin_y intentionally unused in this path
        }
    }
}
