//! Climate-driven biome map for the `Realistic` generator.
//! Phase E.2 lands the noise functions + Climate struct; E.3
//! lands the `gen_realistic` chunk path. Until then this is a
//! pass-through stub that delegates to the Classic generator so
//! the dispatch table in `chunk::generate_chunk_at` compiles.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

/// Stub: y=8 dirt + grass cake, matching the Flat profile output
/// shape. Replaced in E.3 with the climate-driven implementation.
pub(crate) fn gen_realistic(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|_x, y, _z| {
        let wy = origin.y as i32 + y as i32;
        match wy {
            n if n > 8 => BlockId::AIR,
            8 => BlockId(3),
            5..=7 => BlockId(2),
            _ => BlockId(1),
        }
    });
    chunk
}
