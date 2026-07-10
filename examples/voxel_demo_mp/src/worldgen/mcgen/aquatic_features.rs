//! W223 - the small aquatic features, from the decompiled 26.1.2
//! sources: sea pickles (warm oceans), underwater magma patches, and
//! blue-ice sprouts (frozen oceans; they grow off packed ice, so they
//! stay dormant until the iceberg wave lands packed-ice bodies).
//!
//! Determinism: column-keyed SplitMix64 streams; sea pickles scatter
//! +-7 so the 3x3 source-chunk neighbourhood replays; the other two
//! stay chunk-local.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

const SALT_PICKLE: u64 = 0x5EA9_1C1E;
const SALT_MAGMA: u64 = 0x9A69_A000;
const SALT_BLUE_ICE: u64 = 0xB10E_1CE0;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `SeaPickleFeature` (warm ocean, rarity 16, count 20): pickles on the
/// ocean floor where the cell is water.
pub(crate) fn place_sea_pickles(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(pickle) = block_id_by_name("sea_pickle") else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            if mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8) != McBiome::WarmOcean {
                continue;
            }
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_PICKLE));
            // rarity_filter 16.
            if !rng.next_u64().is_multiple_of(16) {
                continue;
            }
            let ox = scx * cs + next_int(&mut rng, 16);
            let oz = scz * cs + next_int(&mut rng, 16);
            for _ in 0..20 {
                let x = ox + next_int(&mut rng, 8) - next_int(&mut rng, 8);
                let z = oz + next_int(&mut rng, 8) - next_int(&mut rng, 8);
                let y = mc_floor_at(ctx, x, z) + 1;
                // (The pickles-per-cluster state roll burns for stream
                // alignment; the engine block renders one cluster.)
                let _ = next_int(&mut rng, 4);
                if y >= SEA_LEVEL {
                    continue; // must sit under water
                }
                let (lx, ly, lz) = (x - origin[0], y - origin[1], z - origin[2]);
                if (0..cs).contains(&lx)
                    && (0..cs).contains(&ly)
                    && (0..cs).contains(&lz)
                    && chunk.get(lx as usize, ly as usize, lz as usize).0 == 36
                {
                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(pickle));
                }
            }
        }
    }
}

/// `UnderwaterMagmaFeature` (all overworld biomes). W229.1 corrected
/// to the real placement pipeline: count uniform 44..52 per chunk
/// COLUMN (the old port rolled it per 16^3 slice), height_range over
/// the full world, and the `surface_relative_threshold_filter`
/// (OCEAN_FLOOR_WG, max -2) that keeps only tries >= 2 blocks BELOW
/// the floor heightmap - so magma grows in underwater cavities, not
/// across the open seabed (the user-visible "way too much magma").
/// Then MC's feature body: the water-column floor within
/// `floor_search_range`(5) is found, and solid cells in the +-1 box
/// around it whose UP face touches water convert with probability 0.5.
pub(crate) fn place_underwater_magma(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(magma) = block_id_by_name("magma_block") else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    // Column-keyed stream: every vertical slice replays the same rolls
    // and applies only its own cells.
    let mut rng = SplitMix64::new(stream_seed(seed, coord[0], coord[2], SALT_MAGMA));
    let count = 44 + next_int(&mut rng, 9); // uniform 44..=52
    for _ in 0..count {
        let lx = next_int(&mut rng, 16);
        let lz = next_int(&mut rng, 16);
        // height_range uniform [above_bottom 0 .. absolute 256] in MC
        // coords (-64..256); tries below the engine band discard like
        // the W202 ore anchors (shape preserved, totals lower).
        let wy = -64 + next_int(&mut rng, 321);
        let (wx, wz) = (origin[0] + lx, origin[2] + lz);
        if wy > mc_floor_at(ctx, wx, wz) - 2 {
            continue; // surface_relative_threshold_filter max -2
        }
        let ly = wy - origin[1];
        if !(1..cs - 1).contains(&ly) || !(1..cs - 1).contains(&lx) || !(1..cs - 1).contains(&lz) {
            continue;
        }
        // Scan down (max 5) for the first solid under water.
        let mut fy = ly;
        let mut found = false;
        for _ in 0..5 {
            let here = chunk.get(lx as usize, fy as usize, lz as usize).0;
            if here != 36 {
                found = here != 0;
                break;
            }
            if fy == 1 {
                break;
            }
            fy -= 1;
        }
        if !found {
            continue;
        }
        // +-1 box around the floor cell: solid cells with water above
        // convert at 0.5.
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let (bx, by, bz) = (lx + dx, fy + dy, lz + dz);
                    if !(0..cs).contains(&bx)
                        || !(1..cs - 1).contains(&by)
                        || !(0..cs).contains(&bz)
                    {
                        continue;
                    }
                    let roll = rng.next_f64() < 0.5;
                    if !roll {
                        continue;
                    }
                    let here = chunk.get(bx as usize, by as usize, bz as usize).0;
                    let above = chunk.get(bx as usize, (by + 1) as usize, bz as usize).0;
                    if here != 0 && here != 36 && !crate::block::block_is_watery(here) && above == 36
                    {
                        chunk.set(bx as usize, by as usize, bz as usize, BlockId(magma));
                    }
                }
            }
        }
    }
}

/// `BlueIceFeature` (frozen oceans, count 0..19, y 30..61): blue-ice
/// sprouts growing off packed ice under the sea surface, exactly the
/// source's 200-iteration accretion.
pub(crate) fn place_blue_ice(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let Some(blue) = block_id_by_name("blue_ice") else {
        return;
    };
    let packed = block_id_by_name("packed_ice");
    let ice = block_id_by_name("ice");
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let mc = mc_full_biome_at(ctx, coord[0] * cs + 8, coord[2] * cs + 8);
    if !matches!(mc, McBiome::FrozenOcean | McBiome::DeepFrozenOcean) {
        return;
    }
    let mut rng = SplitMix64::new(stream_seed(
        seed,
        coord[0],
        coord[2],
        SALT_BLUE_ICE ^ (coord[1] as u64) << 8,
    ));
    let count = next_int(&mut rng, 20); // uniform 0..=19
    fn cell(chunk: &Chunk, cs: i32, x: i32, y: i32, z: i32) -> Option<u16> {
        if (0..cs).contains(&x) && (0..cs).contains(&y) && (0..cs).contains(&z) {
            Some(chunk.get(x as usize, y as usize, z as usize).0)
        } else {
            None
        }
    }
    let is_growable =
        |id: u16| id == 0 || id == 36 || Some(id) == packed || Some(id) == ice;
    for _ in 0..count {
        let lx = next_int(&mut rng, 16);
        let lz = next_int(&mut rng, 16);
        let wy = 30 + next_int(&mut rng, 32); // uniform 30..=61
        let ly = wy - origin[1];
        if !(0..cs).contains(&ly) || wy > SEA_LEVEL - 1 {
            continue;
        }
        // Origin (or below) must be water, with packed ice adjacent
        // (not below).
        let here = cell(chunk, cs, lx, ly, lz);
        let below = cell(chunk, cs, lx, ly - 1, lz);
        if here != Some(36) && below != Some(36) {
            continue;
        }
        let adj = [
            cell(chunk, cs, lx + 1, ly, lz),
            cell(chunk, cs, lx - 1, ly, lz),
            cell(chunk, cs, lx, ly, lz + 1),
            cell(chunk, cs, lx, ly, lz - 1),
            cell(chunk, cs, lx, ly + 1, lz),
        ];
        if !adj.iter().any(|n| n.is_some() && *n == packed) {
            continue;
        }
        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(blue));
        for _ in 0..200 {
            let yo = next_int(&mut rng, 5) - next_int(&mut rng, 6);
            let mut xz = 3;
            if yo < 2 {
                xz += yo / 2;
            }
            if xz < 1 {
                continue;
            }
            let bx = lx + next_int(&mut rng, xz) - next_int(&mut rng, xz);
            let by = ly + yo;
            let bz = lz + next_int(&mut rng, xz) - next_int(&mut rng, xz);
            let Some(state) = cell(chunk, cs, bx, by, bz) else { continue };
            if is_growable(state) {
                let neighbours = [
                    cell(chunk, cs, bx + 1, by, bz),
                    cell(chunk, cs, bx - 1, by, bz),
                    cell(chunk, cs, bx, by + 1, bz),
                    cell(chunk, cs, bx, by - 1, bz),
                    cell(chunk, cs, bx, by, bz + 1),
                    cell(chunk, cs, bx, by, bz - 1),
                ];
                if neighbours.contains(&Some(blue)) {
                    chunk.set(bx as usize, by as usize, bz as usize, BlockId(blue));
                }
            }
        }
    }
}
