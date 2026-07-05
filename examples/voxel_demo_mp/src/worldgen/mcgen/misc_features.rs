//! W219 - the small terrain-dressing features, ported from the
//! decompiled 26.1.2 sources + the jar's feature JSONs:
//!
//! - **disks** (`DiskFeature`): sand / clay / gravel patches replacing
//!   the floor under water (rivers, lake shores, shallow seas). The
//!   sand disk's rule-based provider turns cells with air below into
//!   SANDSTONE, exactly the JSON's rule_based_state_provider.
//! - **springs** (`SpringFeature`): single falling water / lava source
//!   cells on cave walls and cliff faces - the block must have exactly
//!   `rock_count`(4) valid-rock neighbours + `hole_count`(1) air
//!   neighbour among (E,W,N,S,below), with valid rock above.
//! - **forest rock** (`BlockBlobFeature`): the mossy cobblestone
//!   boulders of the old-growth taigas (3 fused blobs walking down to
//!   solid ground).
//!
//! Pipelines come from the placed JSONs (count / rarity / in_square /
//! heightmap / height providers incl. very_biased_to_bottom, ported
//! exactly). Determinism: column-keyed SplitMix64 streams; disks +
//! rocks replay the 3x3 source-chunk neighbourhood (radius <= 6);
//! springs are chunk-local single cells (border cells with off-chunk
//! neighbours are skipped - honest thinning at seams).

use std::collections::HashSet;
use std::sync::OnceLock;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

const SALT_DISK: u64 = 0xD15C_0000;
const SALT_SPRING: u64 = 0x5991_0000;
const SALT_ROCK: u64 = 0x60CE_0000;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// `Mth.nextInt` (inclusive bounds).
fn next_between(r: &mut SplitMix64, min: i32, max: i32) -> i32 {
    min + next_int(r, max - min + 1)
}

/// One disk rule (from the disk_* configured JSONs).
struct DiskDef {
    /// Fallback state + the below-is-air override (sand -> sandstone).
    state: u16,
    below_air_state: Option<u16>,
    /// Blocks the disk replaces.
    targets: &'static [&'static str],
    half_height: i32,
    r_min: i32,
    r_max: i32,
    /// count placement (disk_sand 3; clay/gravel 1).
    count: u32,
}

fn disk_defs() -> &'static Vec<DiskDef> {
    static DEFS: OnceLock<Vec<DiskDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let id = |n: &str| block_id_by_name(n).unwrap_or(1);
        vec![
            // disk_sand: r 2..6, half 2, targets dirt+grass, sandstone
            // under air, count 3.
            DiskDef {
                state: id("sand"),
                below_air_state: Some(id("sandstone")),
                targets: &["dirt", "grass_block"],
                half_height: 2,
                r_min: 2,
                r_max: 6,
                count: 3,
            },
            // disk_clay: r 2..3, half 1, targets dirt+clay, count 1.
            DiskDef {
                state: id("clay"),
                below_air_state: None,
                targets: &["dirt", "clay"],
                half_height: 1,
                r_min: 2,
                r_max: 3,
                count: 1,
            },
            // disk_gravel: r 2..5, half 2, targets dirt+grass, count 1.
            DiskDef {
                state: id("gravel"),
                below_air_state: None,
                targets: &["dirt", "grass_block"],
                half_height: 2,
                r_min: 2,
                r_max: 5,
                count: 1,
            },
        ]
    })
}

fn target_ids(names: &[&str]) -> HashSet<u16> {
    names.iter().filter_map(|n| block_id_by_name(n)).collect()
}

/// W219: the underwater floor disks (step 6, after ores). 3x3 source
/// replay; a try only runs where the origin cell is water (the placed
/// predicate).
pub(crate) fn place_disks(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    for (di, def) in disk_defs().iter().enumerate() {
        let targets = target_ids(def.targets);
        for scx in (coord[0] - 1)..=(coord[0] + 1) {
            for scz in (coord[2] - 1)..=(coord[2] + 1) {
                let mut rng =
                    SplitMix64::new(stream_seed(seed, scx, scz, SALT_DISK ^ ((di as u64) << 40)));
                for _ in 0..def.count {
                    let wx = scx * cs + next_int(&mut rng, 16);
                    let wz = scz * cs + next_int(&mut rng, 16);
                    let floor = mc_floor_at(ctx, wx, wz);
                    let wy = floor + 1;
                    // matching_fluids water: only under water.
                    if wy >= crate::worldgen::climate::SEA_LEVEL {
                        // Roll consumption stays fixed for alignment.
                        let _ = next_int(&mut rng, def.r_max - def.r_min + 1);
                        continue;
                    }
                    let r = next_between(&mut rng, def.r_min, def.r_max);
                    let top = wy + def.half_height;
                    let bottom = wy - def.half_height - 1;
                    for bx in (wx - r)..=(wx + r) {
                        for bz in (wz - r)..=(wz + r) {
                            let (xd, zd) = (bx - wx, bz - wz);
                            if xd * xd + zd * zd > r * r {
                                continue;
                            }
                            let (lx, lz) = (bx - origin[0], bz - origin[2]);
                            if !(0..cs).contains(&lx) || !(0..cs).contains(&lz) {
                                continue;
                            }
                            for by in (bottom + 1..=top).rev() {
                                let ly = by - origin[1];
                                if !(0..cs).contains(&ly) {
                                    continue;
                                }
                                let here = chunk.get(lx as usize, ly as usize, lz as usize).0;
                                if !targets.contains(&here) {
                                    continue;
                                }
                                // rule_based provider: below air -> the
                                // override (sandstone under sand).
                                let below_air = ly > 0
                                    && chunk.get(lx as usize, (ly - 1) as usize, lz as usize).is_air();
                                let state = match (below_air, def.below_air_state) {
                                    (true, Some(s)) => s,
                                    _ => def.state,
                                };
                                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(state));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The spring valid-rock set (spring_water's list; spring_lava's is the
/// same minus snow/ice - close enough that one engine set serves both,
/// resolved from the names present).
fn spring_valid() -> &'static HashSet<u16> {
    static SET: OnceLock<HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "stone", "granite", "diorite", "andesite", "deepslate", "tuff",
            "calcite", "dirt", "snow_block", "packed_ice",
        ]
        .iter()
        .filter_map(|n| block_id_by_name(n))
        .collect()
    })
}

/// W219: SpringFeature (step 8). Chunk-local: water springs count 25
/// (uniform y 1..192), lava springs count 20 (very_biased_to_bottom
/// 1..247 inner 8).
pub(crate) fn place_springs(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let valid = spring_valid();
    // (fluid id, count, salt, lava?).
    for (fluid, count, salt, lava) in [(36u16, 25, SALT_SPRING, false), (37, 20, SALT_SPRING ^ (1 << 40), true)] {
        let mut rng = SplitMix64::new(stream_seed(seed, coord[0], coord[2], salt ^ (coord[1] as u64) << 8));
        for _ in 0..count {
            let lx = next_int(&mut rng, 16);
            let lz = next_int(&mut rng, 16);
            let wy = if lava {
                // very_biased_to_bottom(min 1, max 247, inner 8).
                let upper = next_between(&mut rng, 1 + 8, 247);
                let biased = next_between(&mut rng, 1, upper - 1);
                next_between(&mut rng, 1, biased - 1 + 8)
            } else {
                // uniform(1, 192).
                next_between(&mut rng, 1, 192)
            };
            let ly = wy - origin[1];
            // Springs are single cells: only in-chunk tries with all six
            // neighbour reads in-chunk qualify.
            if !(1..cs - 1).contains(&ly) || !(1..cs - 1).contains(&lx) || !(1..cs - 1).contains(&lz)
            {
                continue;
            }
            let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
            let at = |x: usize, y: usize, z: usize| chunk.get(x, y, z).0;
            if !valid.contains(&at(ux, uy + 1, uz)) {
                continue; // block above must be valid rock
            }
            if !valid.contains(&at(ux, uy - 1, uz)) {
                continue; // requires_block_below (both configs set it)
            }
            let here = at(ux, uy, uz);
            if here != 0 && !valid.contains(&here) {
                continue;
            }
            let sides = [at(ux + 1, uy, uz), at(ux - 1, uy, uz), at(ux, uy, uz + 1), at(ux, uy, uz - 1)];
            let below = at(ux, uy - 1, uz);
            let rock = sides.iter().filter(|id| valid.contains(id)).count()
                + usize::from(valid.contains(&below));
            let holes = sides.iter().filter(|id| **id == 0).count()
                + usize::from(below == 0);
            if rock == 4 && holes == 1 {
                chunk.set(ux, uy, uz, BlockId(fluid));
            }
        }
    }
}

/// W219: BlockBlobFeature - the old-growth taiga mossy boulders
/// (count 2 per chunk, MOTION_BLOCKING heightmap, 3 fused blobs).
pub(crate) fn place_forest_rocks(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let Some(mossy) = block_id_by_name("mossy_cobblestone") else {
        return;
    };
    // forest_rock_can_place_on: dirt family + stone-ish ground.
    let floor_ok = |id: u16| {
        matches!(id, 1..=4)
            || crate::block::block_type(id).is_some_and(|bt| {
                matches!(bt.name, "podzol" | "coarse_dirt" | "gravel" | "mossy_cobblestone")
            })
    };
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            if !matches!(mc, McBiome::OldGrowthPineTaiga | McBiome::OldGrowthSpruceTaiga) {
                continue;
            }
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_ROCK));
            for _ in 0..2 {
                let wx = scx * cs + next_int(&mut rng, 16);
                let wz = scz * cs + next_int(&mut rng, 16);
                let mut y = mc_floor_at(ctx, wx, wz) + 1;
                // Walk down to can_place_on ground (the heightmap floor
                // qualifies by construction; the loop mirrors source).
                // Only cells fully inside THIS chunk can be inspected;
                // otherwise trust the column floor.
                let (lx0, lz0) = (wx - origin[0], wz - origin[2]);
                let mut guard = 0;
                while y > 4 && guard < 8 && (0..cs).contains(&lx0) && (0..cs).contains(&lz0) {
                    let ly = y - 1 - origin[1];
                    if !(0..cs).contains(&ly) {
                        break;
                    }
                    let below = chunk.get(lx0 as usize, ly as usize, lz0 as usize).0;
                    if floor_ok(below) {
                        break;
                    }
                    y -= 1;
                    guard += 1;
                }
                let (mut cx, mut cy, mut cz) = (wx, y, wz);
                for _ in 0..3 {
                    let xr = next_int(&mut rng, 2);
                    let yr = next_int(&mut rng, 2);
                    let zr = next_int(&mut rng, 2);
                    let tr = (xr + yr + zr) as f64 * 0.333 + 0.5;
                    for bx in (cx - xr)..=(cx + xr) {
                        for by in (cy - yr)..=(cy + yr) {
                            for bz in (cz - zr)..=(cz + zr) {
                                let d = ((bx - cx).pow(2) + (by - cy).pow(2) + (bz - cz).pow(2)) as f64;
                                if d > tr * tr {
                                    continue;
                                }
                                let (lx, ly, lz) =
                                    (bx - origin[0], by - origin[1], bz - origin[2]);
                                if (0..cs).contains(&lx)
                                    && (0..cs).contains(&ly)
                                    && (0..cs).contains(&lz)
                                {
                                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(mossy));
                                }
                            }
                        }
                    }
                    cx += next_int(&mut rng, 2) - 1;
                    cy -= next_int(&mut rng, 2);
                    cz += next_int(&mut rng, 2) - 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_defs_resolve_engine_blocks() {
        let defs = disk_defs();
        assert_eq!(defs.len(), 3);
        // sand disk paints sandstone under air; clay resolves the W219
        // block.
        assert!(defs[0].below_air_state.is_some());
        assert_eq!(block_id_by_name("clay"), Some(defs[1].state));
    }

    #[test]
    fn spring_valid_rocks_resolve() {
        // The core stone set must be present or springs never fire.
        let v = spring_valid();
        assert!(v.contains(&1), "stone missing from spring valid set");
        assert!(v.len() >= 6, "too few valid spring rocks: {}", v.len());
    }

    #[test]
    fn biased_bottom_sampler_in_range() {
        let mut r = SplitMix64::new(9);
        for _ in 0..2000 {
            let upper = next_between(&mut r, 9, 247);
            let biased = next_between(&mut r, 1, upper - 1);
            let y = next_between(&mut r, 1, biased - 1 + 8);
            assert!((1..=247).contains(&y));
        }
    }
}
