//! W80 - MC-style ground cover: short grass + ferns scattered densely
//! on the grass surface. This is the biggest visual gap vs MC - bare
//! grass reads as flat and empty; a lush carpet of tufts + ferns reads
//! as a real overworld.
//!
//! Like `flowers`, the pass scans each column for the top grass voxel
//! and drops a cross-pattern plant in the air cell above it (no height
//! recompute needed - the chunk is already filled + carved). Bare
//! biomes (desert / beach / tundra / mountains) get nothing.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::biome::Biome;
use super::biome_table::biome_from_climate;
use super::climate::climate_at;
use super::{WorldgenCtx, hash2_for};
use crate::block::{FERN_ID, SHORT_GRASS_ID};

/// Grass-block id the cover sits on, and the air id checked above it.
const GRASS_BLOCK: u16 = 3;

/// Pick a ground-cover plant for a biome given a per-column roll, or
/// `None` for bare biomes. Density is baked into the roll: grassy biomes
/// read as lush, taiga / jungle mix in ferns, deserts stay empty.
fn pick_cover(biome: Biome, rng: u32) -> Option<u16> {
    let r = rng % 100;
    match biome {
        Biome::Plains | Biome::Savanna => (r < 50).then_some(SHORT_GRASS_ID),
        Biome::Forest => (r < 42).then_some(SHORT_GRASS_ID),
        Biome::Swamp => (r < 32).then_some(SHORT_GRASS_ID),
        // Taiga: ferns + a little grass.
        Biome::Taiga => (r < 40).then_some(if r.is_multiple_of(2) { FERN_ID } else { SHORT_GRASS_ID }),
        // Jungle floor is the lushest - mostly ferns.
        Biome::Jungle => {
            (r < 60).then_some(if r.is_multiple_of(3) { SHORT_GRASS_ID } else { FERN_ID })
        }
        _ => None,
    }
}

pub(crate) fn place_realistic_from_coord(
    chunk: &mut Chunk,
    coord: ChunkCoord,
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
            let biome = biome_from_climate(climate_at(wx as f32 + 0.5, wz as f32 + 0.5, seed));
            // Distinct salt from the flower pass so cover + flowers don't
            // land on a correlated subset of columns.
            let rng = hash2_for(wx ^ 0x6A3D, wz ^ 0x2C17, seed);
            let Some(plant) = pick_cover(biome, rng) else {
                continue;
            };
            // Air cell directly above the top grass voxel.
            let mut local_y = -1i32;
            for ly in (0..cs).rev() {
                if chunk.get(x as usize, ly as usize, z as usize).0 == GRASS_BLOCK {
                    local_y = ly + 1;
                    break;
                }
            }
            if !(0..cs).contains(&local_y) {
                continue;
            }
            // Skip if something (a flower, a trunk) already occupies it.
            if chunk.get(x as usize, local_y as usize, z as usize).0 != 0 {
                continue;
            }
            chunk.set(x as usize, local_y as usize, z as usize, BlockId(plant));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plains_is_grassy_deserts_bare() {
        // Over many rolls, plains yields short grass on roughly half the
        // columns; desert yields nothing.
        let mut grassy = 0;
        for r in 0..1000u32 {
            if pick_cover(Biome::Plains, r.wrapping_mul(2_654_435_761)).is_some() {
                grassy += 1;
            }
            assert!(pick_cover(Biome::Desert, r).is_none(), "desert must stay bare");
        }
        assert!((300..700).contains(&grassy), "plains cover density off: {grassy}/1000");
    }
}
