//! W224 - `VinesFeature` (jungle vines), from the decompiled 26.1.2
//! source: 127 tries per chunk at y 64..100; an air cell with a solid
//! neighbour grows a vine. The engine vine block (775) renders as a
//! cross instead of MC's face-hugging quads - documented divergence;
//! the jungle silhouette (green strands under canopies and along
//! trunks) is the point.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::mc_full_biome_at;
use crate::worldgen::WorldgenCtx;

const SALT_VINES: u64 = 0x71E5_0000;

/// Per-chunk vine pass (the three jungle biomes list it).
pub(crate) fn place_vines(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let Some(vine) = block_id_by_name("vine") else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let mc = mc_full_biome_at(ctx, coord[0] * cs + 8, coord[2] * cs + 8);
    if !matches!(mc, McBiome::Jungle | McBiome::SparseJungle | McBiome::BambooJungle) {
        return;
    }
    let seed = ctx.effective_seed();
    let mut rng = SplitMix64::new(stream_seed(
        seed,
        coord[0],
        coord[2],
        SALT_VINES ^ (coord[1] as u64) << 8,
    ));
    for _ in 0..127 {
        let lx = (rng.next_u64() % 16) as i32;
        let lz = (rng.next_u64() % 16) as i32;
        let wy = 64 + (rng.next_u64() % 37) as i32; // uniform 64..=100
        let ly = wy - origin[1];
        if !(0..cs).contains(&ly) || !(1..cs - 1).contains(&lx) || !(1..cs - 1).contains(&lz) {
            continue;
        }
        let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
        if chunk.get(ux, uy, uz).0 != 0 {
            continue;
        }
        // A solid (non-passable) horizontal neighbour hosts the vine
        // (VinesBlock.canSurvive against any side face).
        let solid = |id: u16| {
            id != 0
                && !crate::block::block_is_aabb_passable(BlockId(id))
                && !crate::block::block_is_watery(id)
        };
        let hosted = solid(chunk.get(ux + 1, uy, uz).0)
            || solid(chunk.get(ux - 1, uy, uz).0)
            || solid(chunk.get(ux, uy, uz + 1).0)
            || solid(chunk.get(ux, uy, uz - 1).0)
            || (ly + 1 < cs && solid(chunk.get(ux, uy + 1, uz).0));
        if hosted {
            chunk.set(ux, uy, uz, BlockId(vine));
        }
    }
}
