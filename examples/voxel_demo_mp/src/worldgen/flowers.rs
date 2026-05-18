//! Flower decoration pass for the Classic generator. Sparse
//! 3-colour bloom on plains-biome grass; uses a different hash
//! salt than trees so the patterns don't overlap.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, World};

use super::biome::{Biome, biome_at};
use super::hash2;
use super::height::height_at_classic;

/// Place flowers on the top voxel of any plains grass cell.
/// `amp_scale` tracks the height curve so Amplified still anchors
/// flowers to the right Y.
pub(crate) fn place_classic(chunk: &mut Chunk, origin_x: i32, origin_y: i32, origin_z: i32, amp_scale: f32) {
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Plains {
                continue;
            }
            if hash2(wx ^ 0x9A75, wz ^ 0x5BA9) % 18 != 3 {
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
            let h = hash2(wx ^ 0x5FAD, wz ^ 0x9117);
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
pub(crate) fn place_classic_from_coord(chunk: &mut Chunk, coord: infengine_voxel::ChunkCoord, amp_scale: f32) {
    let origin = World::chunk_world_origin(coord);
    place_classic(
        chunk,
        origin.x as i32,
        origin.y as i32,
        origin.z as i32,
        amp_scale,
    );
}
