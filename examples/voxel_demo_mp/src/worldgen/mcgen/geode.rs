//! W225 - amethyst geodes, ported from the decompiled 26.1.2
//! `GeodeFeature` + the jar's `amethyst_geode` config.
//!
//! A geode is an inverse-square-distance shell around 3-4 random
//! points: the sum of `1/sqrt(distSq + offset)` per cell against the
//! 1/sqrt(layer) thresholds picks air filling, an amethyst inner layer
//! (8.3% budding, which hosts the clusters), a calcite crust and a
//! smooth-basalt hull; a 95%-chance vertical crack cuts one side open.
//! Rarity 1/24 chunks, centre y from `above_bottom 6` (MC -58) to 30 -
//! mostly below the engine floor, so like MC only a fraction of the
//! rolls materialise in the realized band.
//!
//! Cross-chunk determinism: the geode spans +-16, so the 3x3
//! source-chunk neighbourhood replays; the invalid-point abort reads
//! the PRE-DECORATION column bitsets (replay-invariant), writes stay
//! chunk-local with a bedrock guard. Clusters collapse the four bud
//! sizes onto the one engine `amethyst_cluster` block (cross render).

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

use super::features::{block_id_by_name, stream_seed};
use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::terrain::column_is_solid;
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

const SALT_GEODE: u64 = 0x6E0D_E000;

// amethyst_geode.json values (pinned by the parse test below).
const LAYER_FILLING: f64 = 1.7;
const LAYER_INNER: f64 = 2.2;
const LAYER_MIDDLE: f64 = 3.2;
const LAYER_OUTER: f64 = 4.2;
const CRACK_CHANCE: f64 = 0.95;
const BASE_CRACK_SIZE: f64 = 2.0;
const CRACK_POINT_OFFSET: i32 = 2;
const NOISE_MULTIPLIER: f64 = 0.05;
const BUDDING_CHANCE: f64 = 0.083;
const PLACEMENTS_CHANCE: f64 = 0.35;
const GEN_OFFSET: i32 = 16;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn uniform_i(r: &mut SplitMix64, min: i32, max: i32) -> i32 {
    min + next_int(r, max - min + 1)
}

struct GeodeBlocks {
    amethyst: u16,
    budding: u16,
    cluster: u16,
    calcite: u16,
    basalt: u16,
}

fn blocks() -> Option<GeodeBlocks> {
    Some(GeodeBlocks {
        amethyst: block_id_by_name("amethyst_block")?,
        budding: block_id_by_name("budding_amethyst")?,
        cluster: block_id_by_name("amethyst_cluster")?,
        calcite: block_id_by_name("calcite")?,
        basalt: block_id_by_name("smooth_basalt")?,
    })
}

/// Per-chunk geode pass (local_modifications step, all biomes).
pub(crate) fn place_geodes(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let Some(b) = blocks() else {
        return;
    };
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    // The geode noise: NormalNoise(worldSeed, -4, [1.0]).
    let noise = NormalNoise::new(
        seed as u64 ^ 0x6E0D,
        &NoiseParams { first_octave: -4, amplitudes: vec![1.0] },
    );
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_GEODE));
            // rarity_filter 24.
            if !rng.next_u64().is_multiple_of(24) {
                continue;
            }
            let ox = scx * cs + next_int(&mut rng, 16);
            let oz = scz * cs + next_int(&mut rng, 16);
            // height uniform [above_bottom 6 (MC -58) .. absolute 30];
            // W260: the deep band is realized, so sub-zero geodes
            // materialize in the deepslate strata like MC.
            let oy = uniform_i(&mut rng, -58, 30);
            geode(chunk, origin, ctx, &mut rng, &noise, [ox, oy, oz], &b);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn geode(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    noise: &NormalNoise,
    center: [i32; 3],
    b: &GeodeBlocks,
) {
    let cs = CHUNK_SIZE as i32;
    let num_points = uniform_i(rng, 3, 4);
    let crack_adjust = num_points as f64 / 6.0; // outerWallDistance.max
    let inner_air = 1.0 / LAYER_FILLING.sqrt();
    let innermost = 1.0 / (LAYER_INNER + crack_adjust).sqrt();
    let inner_crust = 1.0 / (LAYER_MIDDLE + crack_adjust).sqrt();
    let outer_crust = 1.0 / (LAYER_OUTER + crack_adjust).sqrt();
    let crack_size = 1.0
        / (BASE_CRACK_SIZE
            + rng.next_f64() / 2.0
            + if num_points > 3 { crack_adjust } else { 0.0 })
        .sqrt();
    let generate_crack = rng.next_f64() < CRACK_CHANCE;
    // Distribution points + the invalid-blocks abort (replay-invariant
    // solidity view; air/water points count as invalid).
    let mut points: Vec<([i32; 3], i32)> = Vec::new();
    let mut invalid = 0;
    for _ in 0..num_points {
        let dx = uniform_i(rng, 4, 6);
        let dy = uniform_i(rng, 4, 6);
        let dz = uniform_i(rng, 4, 6);
        let p = [center[0] + dx, center[1] + dy, center[2] + dz];
        if p[1] < 0 || !column_is_solid(ctx, p[0], p[1], p[2]) {
            invalid += 1;
            if invalid > 1 {
                // invalid_blocks_threshold 1: keep roll parity by
                // consuming the point offset before aborting.
                let _ = uniform_i(rng, 1, 2);
                return;
            }
        }
        points.push((p, uniform_i(rng, 1, 2)));
    }
    let mut cracks: Vec<[i32; 3]> = Vec::new();
    if generate_crack {
        let idx = next_int(rng, 4);
        let off = num_points * 2 + 1;
        let (cx, cz) = match idx {
            0 => (off, 0),
            1 => (0, off),
            2 => (off, off),
            _ => (0, 0),
        };
        for dy in [7, 5, 1] {
            cracks.push([center[0] + cx, center[1] + dy, center[2] + cz]);
        }
    }
    let inv_sqrt = |v: f64| 1.0 / v.sqrt();
    let mut crystal_spots: Vec<[i32; 3]> = Vec::new();
    for gx in -GEN_OFFSET..=GEN_OFFSET {
        for gy in -GEN_OFFSET..=GEN_OFFSET {
            for gz in -GEN_OFFSET..=GEN_OFFSET {
                let (wx, wy, wz) = (center[0] + gx, center[1] + gy, center[2] + gz);
                // Distances + noise are position-pure; per-cell random
                // rolls use a positional stream so out-of-chunk cells
                // stay roll-neutral.
                let n = noise.value(wx as f64, wy as f64, wz as f64) * NOISE_MULTIPLIER;
                let mut shell = 0.0;
                for (p, po) in &points {
                    let d = (wx - p[0]).pow(2) + (wy - p[1]).pow(2) + (wz - p[2]).pow(2);
                    shell += inv_sqrt((d + po) as f64) + n;
                }
                let mut crack = 0.0;
                for p in &cracks {
                    let d = (wx - p[0]).pow(2) + (wy - p[1]).pow(2) + (wz - p[2]).pow(2);
                    crack += inv_sqrt((d + CRACK_POINT_OFFSET) as f64) + n;
                }
                if shell < outer_crust {
                    continue;
                }
                let mut pos_rng = SplitMix64::new(
                    (ctx.effective_seed() as u64) ^ SALT_GEODE
                        ^ (wx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ (wy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                        ^ (wz as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
                );
                // The crack opening and the filling are both air in
                // the vanilla config; MC keeps them as separate rule
                // arms (the crack also schedules fluid ticks).
                let crack_open = generate_crack && crack >= crack_size && shell < inner_air;
                let state: Option<u16> = if crack_open || shell >= inner_air {
                    Some(0)
                } else if shell >= innermost {
                    let budding = pos_rng.next_f64() < BUDDING_CHANCE;
                    if budding && pos_rng.next_f64() < PLACEMENTS_CHANCE {
                        crystal_spots.push([wx, wy, wz]);
                    }
                    Some(if budding { b.budding } else { b.amethyst })
                } else if shell >= inner_crust {
                    Some(b.calcite)
                } else {
                    Some(b.basalt)
                };
                let Some(id) = state else { continue };
                let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
                if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
                    continue;
                }
                let here = chunk.get(lx as usize, ly as usize, lz as usize).0;
                // cannot_replace (bedrock stays).
                if crate::block::block_type(here).is_some_and(|bt| bt.name == "bedrock") {
                    continue;
                }
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
            }
        }
    }
    // Crystal clusters sprout off budding blocks into adjacent open
    // cells (buds collapse onto the one cluster block).
    for spot in crystal_spots {
        for (dx, dy, dz) in
            [(1i32, 0i32, 0i32), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
        {
            let (wx, wy, wz) = (spot[0] + dx, spot[1] + dy, spot[2] + dz);
            let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
            if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
                continue;
            }
            let here = chunk.get(lx as usize, ly as usize, lz as usize).0;
            // canClusterGrowAtState: air or water source.
            if here == 0 || (here == 36 && wy < SEA_LEVEL) {
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(b.cluster));
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code constants must match the embedded amethyst_geode JSON.
    #[test]
    fn constants_match_the_embedded_json() {
        let j: serde_json::Value =
            serde_json::from_str(include_str!("data/feature/configured/amethyst_geode.json"))
                .unwrap();
        let c = &j["config"];
        assert!((c["layers"]["filling"].as_f64().unwrap() - LAYER_FILLING).abs() < 1e-9);
        assert!((c["layers"]["inner_layer"].as_f64().unwrap() - LAYER_INNER).abs() < 1e-9);
        assert!((c["layers"]["middle_layer"].as_f64().unwrap() - LAYER_MIDDLE).abs() < 1e-9);
        assert!((c["layers"]["outer_layer"].as_f64().unwrap() - LAYER_OUTER).abs() < 1e-9);
        assert!(
            (c["crack"]["generate_crack_chance"].as_f64().unwrap() - CRACK_CHANCE).abs() < 1e-9
        );
        assert!((c["noise_multiplier"].as_f64().unwrap() - NOISE_MULTIPLIER).abs() < 1e-9);
        assert!(
            (c["use_alternate_layer0_chance"].as_f64().unwrap() - BUDDING_CHANCE).abs() < 1e-6
        );
        assert!(
            (c["use_potential_placements_chance"].as_f64().unwrap() - PLACEMENTS_CHANCE).abs()
                < 1e-6
        );
    }

    #[test]
    fn geode_blocks_resolve() {
        assert!(blocks().is_some(), "the amethyst block family must exist");
    }
}
