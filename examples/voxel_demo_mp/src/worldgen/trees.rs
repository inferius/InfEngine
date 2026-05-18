//! Tree decoration pass for the Classic generator. Drops small
//! oak trees on plains tiles - a wood pillar + leafy canopy that
//! overhangs the chunk edge via a -2 .. CHUNK_SIZE+2 sweep.
//!
//! Phase E.4 expands this into a `trees/` folder with multiple
//! species (pine / birch / sequoia / jungle bigleaf / acacia);
//! for E.1 we just lift the existing oak code as-is so the split
//! commit stays byte-identical at chunk level.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, World};

use super::biome::{Biome, biome_at};
use super::hash2;
use super::height::height_at_classic;

/// Drop a single oak tree at the given (x, z) in chunk-local
/// coords. The trunk anchors on top of the ground voxel + the
/// canopy is two 5x5 layers (corners trimmed), a 3x3 cap, and a
/// single top block.
fn place_oak(
    chunk: &mut Chunk,
    local_x: i32,
    local_z: i32,
    ground: i32,
    origin_y: i32,
) {
    let cs = CHUNK_SIZE as i32;
    let set_in_chunk = |chunk: &mut Chunk, lx: i32, ly: i32, lz: i32, block: BlockId| {
        if lx < 0 || lx >= cs || lz < 0 || lz >= cs || ly < 0 || ly >= cs {
            return;
        }
        chunk.set(lx as usize, ly as usize, lz as usize, block);
    };
    // Trunk: 4 wood blocks above the surface voxel.
    for ty in 1..=4 {
        let wy = ground + ty;
        let local_y = wy - origin_y;
        set_in_chunk(chunk, local_x, local_y, local_z, BlockId(5));
    }
    // Canopy: two 5x5 layers (corners trimmed) at trunk top -1 / 0,
    // a 3x3 cap above, and a single top block.
    for layer in 0i32..2 {
        let wy = ground + 3 + layer;
        let local_y = wy - origin_y;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                if dx.abs() == 2 && dz.abs() == 2 {
                    continue;
                }
                if dx == 0 && dz == 0 && layer == 0 {
                    continue;
                }
                set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, BlockId(6));
            }
        }
    }
    {
        let wy = ground + 5;
        let local_y = wy - origin_y;
        for dx in -1i32..=1 {
            for dz in -1i32..=1 {
                set_in_chunk(chunk, local_x + dx, local_y, local_z + dz, BlockId(6));
            }
        }
    }
    // Single block top.
    {
        let wy = ground + 6;
        let local_y = wy - origin_y;
        set_in_chunk(chunk, local_x, local_y, local_z, BlockId(6));
    }
}

/// Walk a `(-2 .. CHUNK_SIZE+2)` box so trees anchored just
/// outside the chunk still drop their leaves into this chunk.
/// Density: roughly 1 tree per 32 cells in plains.
pub(crate) fn place_classic(chunk: &mut Chunk, origin_x: i32, origin_y: i32, origin_z: i32, amp_scale: f32) {
    let cs = CHUNK_SIZE as i32;
    for x in -2..(cs + 2) {
        for z in -2..(cs + 2) {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Plains {
                continue;
            }
            if hash2(wx, wz) % 32 != 7 {
                continue;
            }
            let ground = height_at_classic(wx as f32 + 0.5, wz as f32 + 0.5, biome, amp_scale);
            place_oak(chunk, x, z, ground, origin_y);
        }
    }
}

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

/// Realistic-profile tree pass. Stub for E.2: drops the same
/// oak template the Classic pass uses, anywhere the climate map
/// resolves to a forest-like biome. Species dispatch lands in
/// Phase E.4b.
pub(crate) fn place_realistic_from_coord(chunk: &mut Chunk, coord: infengine_voxel::ChunkCoord) {
    use super::biome_table::biome_from_climate;
    use super::climate::{climate_at, realistic_height};
    let origin = World::chunk_world_origin(coord);
    let origin_x = origin.x as i32;
    let origin_y = origin.y as i32;
    let origin_z = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in -2..(cs + 2) {
        for z in -2..(cs + 2) {
            let wx = origin_x + x;
            let wz = origin_z + z;
            let climate = climate_at(wx as f32 + 0.5, wz as f32 + 0.5);
            let biome = biome_from_climate(climate);
            // For E.2 only Forest + Plains qualify; other biomes
            // get tree species in E.4b.
            let qualifies = matches!(biome, Biome::Plains | Biome::Forest);
            if !qualifies {
                continue;
            }
            // Forest = ~2.5x more trees than Classic plains;
            // Plains keeps the legacy density.
            let modulus = if biome == Biome::Forest { 12 } else { 32 };
            if hash2(wx, wz) % modulus != 7 {
                continue;
            }
            let ground = realistic_height(climate, biome);
            place_oak(chunk, x, z, ground, origin_y);
        }
    }
}
