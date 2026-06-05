//! W82 - miscellaneous biome ground features built from existing plant
//! blocks: dead bush (desert sand), bamboo thickets (jungle), and sugar
//! cane along fresh-water edges. Each is a sparse per-column roll on the
//! already-filled + carved chunk, so no height recompute is needed.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::biome::Biome;
use super::biome_table::biome_from_climate;
use super::climate::climate_at;
use super::{WorldgenCtx, hash2_for};

const DEAD_BUSH: u16 = 492;
const SUGAR_CANE: u16 = 499;
const BAMBOO: u16 = 500;
const WATER: u16 = 36;
const GRASS: u16 = 3;
const SAND: u16 = 4;

/// Topmost non-air, non-water voxel y in a column, or `None`.
fn top_solid(chunk: &Chunk, x: i32, z: i32, cs: i32) -> Option<i32> {
    (0..cs).rev().find(|&ly| {
        let b = chunk.get(x as usize, ly as usize, z as usize).0;
        b != 0 && b != WATER
    })
}

/// True if any of the 4 horizontal neighbours is water at `surf_y` -
/// i.e. this surface cell sits on a water shoreline.
fn touches_water(chunk: &Chunk, x: i32, surf_y: i32, z: i32, cs: i32) -> bool {
    [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dz)| {
        let nx = x + dx;
        let nz = z + dz;
        (0..cs).contains(&nx)
            && (0..cs).contains(&nz)
            && chunk.get(nx as usize, surf_y as usize, nz as usize).0 == WATER
    })
}

/// Stack `count` of `block` upward from `air_y`, clipped to the chunk.
fn stack(chunk: &mut Chunk, x: i32, air_y: i32, z: i32, cs: i32, count: i32, block: u16) {
    for i in 0..count {
        let y = air_y + i;
        if y >= cs {
            break;
        }
        chunk.set(x as usize, y as usize, z as usize, BlockId(block));
    }
}

pub(crate) fn place_realistic_from_coord(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let ox = origin.x as i32;
    let oz = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = ox + x;
            let wz = oz + z;
            let biome = biome_from_climate(climate_at(wx as f32 + 0.5, wz as f32 + 0.5, seed));
            let Some(surf_y) = top_solid(chunk, x, z, cs) else {
                continue;
            };
            let surf = chunk.get(x as usize, surf_y as usize, z as usize).0;
            let air_y = surf_y + 1;
            if air_y >= cs || chunk.get(x as usize, air_y as usize, z as usize).0 != 0 {
                continue;
            }
            let rng = hash2_for(wx ^ 0x5EED, wz ^ 0x1B7A, seed);

            // Dead bush dots the desert sand.
            if biome == Biome::Desert && surf == SAND && rng % 14 == 3 {
                chunk.set(x as usize, air_y as usize, z as usize, BlockId(DEAD_BUSH));
                continue;
            }
            // Bamboo thickets rise from the jungle floor.
            if biome == Biome::Jungle && surf == GRASS && rng % 22 == 5 {
                stack(chunk, x, air_y, z, cs, 3 + ((rng >> 5) % 4) as i32, BAMBOO);
                continue;
            }
            // Sugar cane on grass / sand right at a water edge.
            if (surf == GRASS || surf == SAND)
                && rng % 24 == 7
                && touches_water(chunk, x, surf_y, z, cs)
            {
                stack(chunk, x, air_y, z, cs, 1 + ((rng >> 5) % 3) as i32, SUGAR_CANE);
            }
        }
    }
}
