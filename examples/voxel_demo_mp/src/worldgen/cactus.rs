//! Cactus pass: sparse 2-3 block columns on desert sand. Same
//! surface + air checks as flowers; height varies with the cell
//! hash so dunes don't look uniform.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, World};

use super::WorldgenCtx;
use super::biome::{Biome, biome_at};
use super::hash2_for;
use super::height::height_at_classic;

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
            if biome != Biome::Desert {
                continue;
            }
            let h = hash2_for(wx ^ 0xCAC7, wz ^ 0x1A55, seed);
            if h % 24 != 5 {
                continue;
            }
            let ground = height_at_classic(wx as f32 + 0.5, wz as f32 + 0.5, biome, amp_scale);
            let height_voxels = 2 + ((h >> 4) % 2) as i32; // 2 or 3
            for step in 1..=height_voxels {
                let wy = ground + step;
                let local_y = wy - origin_y;
                if !(0..cs).contains(&local_y) {
                    continue;
                }
                chunk.set(x as usize, local_y as usize, z as usize, BlockId(27));
            }
        }
    }
}

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

/// Realistic-profile cactus pass. Cactus columns sit on Desert
/// sand (climate-driven). Skips Savanna for now - acacia trees
/// in E.4b will handle that biome's vertical decoration.
pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: infengine_voxel::ChunkCoord,
    ctx: &WorldgenCtx,
) {
    use super::chunk::column_height;
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
            let biome = super::realistic_biome_at(ctx, wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Desert {
                continue;
            }
            let h = hash2_for(wx ^ 0xCAC7, wz ^ 0x1A55, seed);
            if h % 24 != 5 {
                continue;
            }
            // W78: profile-correct column height so cacti sit on the
            // real terrain (Natural fills with natural_height, not
            // realistic_height).
            let ground = column_height(ctx, wx, wz);
            let height_voxels = 2 + ((h >> 4) % 2) as i32;
            for step in 1..=height_voxels {
                let wy = ground + step;
                let local_y = wy - origin_y;
                if !(0..cs).contains(&local_y) {
                    continue;
                }
                chunk.set(x as usize, local_y as usize, z as usize, BlockId(27));
            }
        }
    }
}
