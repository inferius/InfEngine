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
const PUMPKIN: u16 = 448;
const SNOW_LAYER: u16 = 15;
const ICE: u16 = 400;

/// W146: rare pumpkin patches on grass, MC-style. A coarse 32x32
/// cell occasionally hosts a small clump (centre + density from the
/// chunk-independent cell hash, so clumps cross chunk borders
/// seamlessly).
fn pumpkin_here(wx: i32, wz: i32, seed: u32) -> bool {
    let cell_x = wx.div_euclid(32);
    let cell_z = wz.div_euclid(32);
    let cell = hash2_for(cell_x ^ 0x70AD, cell_z ^ 0x0BAD, seed);
    // 1 in 10 of the 32x32 cells hosts a clump.
    if !cell.is_multiple_of(10) {
        return false;
    }
    let cx = cell_x * 32 + ((cell >> 8) % 32) as i32;
    let cz = cell_z * 32 + ((cell >> 13) % 32) as i32;
    let (dx, dz) = (wx - cx, wz - cz);
    // Clump radius ~2.2 blocks, ~1/3 fill = 1-4 pumpkins.
    dx * dx + dz * dz <= 5
        && hash2_for(wx ^ 0x600D, wz ^ 0x0F0D, seed).is_multiple_of(3)
}

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
    let oy = origin.y as i32;
    let oz = origin.z as i32;
    let cs = CHUNK_SIZE as i32;
    for x in 0..cs {
        for z in 0..cs {
            let wx = ox + x;
            let wz = oz + z;
            let biome = biome_from_climate(climate_at(wx as f32 + 0.5, wz as f32 + 0.5, seed));
            let cold = matches!(biome, Biome::Tundra | Biome::SnowyMountain);
            // W149: MC freeze_top_layer - cold biomes turn exposed
            // surface WATER to ice. Only the cell AT sea level can be
            // an open-water surface (lower chunks of a sea column are
            // all water and must stay liquid below the ice sheet).
            if cold {
                let sea_ly = super::climate::SEA_LEVEL - oy;
                if (0..cs).contains(&sea_ly)
                    && chunk.get(x as usize, sea_ly as usize, z as usize).0 == WATER
                    && (sea_ly + 1 >= cs
                        || chunk.get(x as usize, (sea_ly + 1) as usize, z as usize).0 == 0)
                {
                    chunk.set(x as usize, sea_ly as usize, z as usize, BlockId(ICE));
                    continue;
                }
            }
            let Some(surf_y) = top_solid(chunk, x, z, cs) else {
                continue;
            };
            let surf = chunk.get(x as usize, surf_y as usize, z as usize).0;
            let air_y = surf_y + 1;
            if air_y >= cs || chunk.get(x as usize, air_y as usize, z as usize).0 != 0 {
                continue;
            }
            // W149: ...and every exposed cold surface gets a snow
            // layer on top (grass, sand, stone, even tree canopies),
            // exactly like the MC feature.
            if cold {
                chunk.set(x as usize, air_y as usize, z as usize, BlockId(SNOW_LAYER));
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
                continue;
            }
            // W146: rare pumpkin clumps on any grassy surface (MC
            // spawns them in every grass-floored biome).
            if surf == GRASS
                && biome != Biome::Desert
                && pumpkin_here(wx, wz, seed)
            {
                chunk.set(x as usize, air_y as usize, z as usize, BlockId(PUMPKIN));
            }
        }
    }
}
