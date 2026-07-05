//! W223 - `BambooFeature`, from the decompiled 26.1.2 source.
//!
//! Bamboo columns 5..16 tall on the jungle floor; the podzol variant
//! (`bamboo_some_podzol`, probability 0.2) also paints a podzol disc
//! radius 1..4 under the stand. The engine bamboo block (500) is one
//! stalk id, so the LARGE/SMALL leaf states collapse onto it (honest
//! note; the silhouette is the stalk column either way).
//!
//! Placements from the jar: `bamboo` = noise_based_count(80, 0.3, 160)
//! in bamboo_jungle; `bamboo_light` = rarity 4 in jungle. 3x3 source
//! replay covers the podzol disc spill.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::terrain::{mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

const SALT_BAMBOO: u64 = 0xBA6B_0000;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

/// The shared BIOME_INFO_NOISE spectrum for noise_based_count (the
/// same constant-2345 class the kelp counts use).
fn info_noise() -> &'static NormalNoise {
    use std::sync::OnceLock;
    static NOISE: OnceLock<NormalNoise> = OnceLock::new();
    NOISE.get_or_init(|| {
        NormalNoise::new(2345, &NoiseParams { first_octave: 0, amplitudes: vec![1.0] })
    })
}

/// One bamboo stand at `(wx, wz)`; chunk-local writes.
#[allow(clippy::too_many_arguments)]
fn bamboo_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    wx: i32,
    wz: i32,
    podzol_probability: f64,
    bamboo: u16,
) {
    let cs = CHUNK_SIZE as i32;
    let floor = mc_floor_at(ctx, wx, wz);
    let base = floor + 1;
    let height = next_int(rng, 12) + 5;
    let podzol = rng.next_f64() < podzol_probability;
    let radius = if podzol { next_int(rng, 4) + 1 } else { 0 };
    if base <= SEA_LEVEL && floor < SEA_LEVEL {
        return; // underwater floor - bamboo wants dry jungle ground
    }
    if podzol {
        let Some(podzol_id) = block_id_by_name("podzol") else {
            return;
        };
        for bx in (wx - radius)..=(wx + radius) {
            for bz in (wz - radius)..=(wz + radius) {
                let (dx, dz) = (bx - wx, bz - wz);
                if dx * dx + dz * dz > radius * radius {
                    continue;
                }
                let fy = mc_floor_at(ctx, bx, bz);
                let (lx, ly, lz) = (bx - origin[0], fy - origin[1], bz - origin[2]);
                if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
                    let here = chunk.get(lx as usize, ly as usize, lz as usize).0;
                    // BENEATH_BAMBOO_PODZOL_REPLACEABLE ~ the dirt family.
                    if matches!(here, 2 | 3)
                        || crate::block::block_type(here)
                            .is_some_and(|bt| matches!(bt.name, "coarse_dirt" | "mycelium"))
                    {
                        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(podzol_id));
                    }
                }
            }
        }
    }
    // The stalk column (stops at the first non-air cell).
    let (lx, lz) = (wx - origin[0], wz - origin[2]);
    if !(0..cs).contains(&lx) || !(0..cs).contains(&lz) {
        return;
    }
    // Floor must be dirt/grass/sand-ish (BambooStalkBlock.canSurvive).
    let fly = floor - origin[1];
    if (0..cs).contains(&fly) {
        let f = chunk.get(lx as usize, fly as usize, lz as usize).0;
        let ok = matches!(f, 2..=4)
            || crate::block::block_type(f)
                .is_some_and(|bt| matches!(bt.name, "podzol" | "coarse_dirt" | "mud"));
        if !ok {
            return;
        }
    }
    for i in 0..height {
        let ly = base + i - origin[1];
        if !(0..cs).contains(&ly) {
            continue;
        }
        if chunk.get(lx as usize, ly as usize, lz as usize).0 != 0 {
            break;
        }
        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(bamboo));
    }
}

/// Per-chunk bamboo pass (vegetal step; bamboo_jungle dense stands +
/// jungle's light scatter).
pub(crate) fn place_bamboo(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let Some(bamboo) = block_id_by_name("bamboo") else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BAMBOO));
            match mc {
                McBiome::BambooJungle => {
                    // bamboo (some podzol): noise_based_count(80, 0.3, 160).
                    let n = info_noise().value(
                        (scx * cs) as f64 / 80.0,
                        0.0,
                        (scz * cs) as f64 / 80.0,
                    );
                    let count = (((n + 0.3) * 160.0).ceil().max(0.0)) as i32;
                    for _ in 0..count {
                        let wx = scx * cs + next_int(&mut rng, 16);
                        let wz = scz * cs + next_int(&mut rng, 16);
                        bamboo_try(chunk, origin, ctx, &mut rng, wx, wz, 0.2, bamboo);
                    }
                }
                // bamboo_light: rarity 4, no podzol.
                McBiome::Jungle if rng.next_u64().is_multiple_of(4) => {
                    let wx = scx * cs + next_int(&mut rng, 16);
                    let wz = scz * cs + next_int(&mut rng, 16);
                    bamboo_try(chunk, origin, ctx, &mut rng, wx, wz, 0.0, bamboo);
                }
                _ => {}
            }
        }
    }
}
