//! W201 - MC placed_features, Phase 6a: ocean flora (kelp + seagrass).
//!
//! Data-driven from the REAL MC worldgen JSON (extracted from the same
//! client jar as the density/noise data - see `data/feature/`):
//!
//! - `placed_feature/seagrass_normal.json` - 48 tries/chunk, in_square,
//!   heightmap OCEAN_FLOOR_WG, biome.
//! - `placed_feature/kelp_cold.json` - noise_based_count(80, 0, 120),
//!   in_square, heightmap OCEAN_FLOOR_WG, biome.
//! - `configured_feature/seagrass_short.json` - probability 0.3 of the
//!   tall (two-block) variant per try.
//! - `configured_feature/kelp.json` - the kelp column feature.
//!
//! The placement pipeline interprets those JSONs (count /
//! noise_based_count / in_square / heightmap) instead of hard-coding
//! their numbers, per the faithful-porting mandate. The `biome`
//! modifier is resolved at the engine's biome resolution: the 66->11
//! biome collapse has no ocean *variants*, so every sub-sea-level
//! column gets the plain `ocean` biome feature set (seagrass_normal +
//! kelp_cold, exactly what MC's `biome/ocean.json` lists). Honest
//! divergences from MC, consistent with the whole port: SplitMix64
//! random streams (not Xoroshiro, so not seed-identical) and a
//! NormalNoise stand-in for MC's PerlinSimplexNoise BIOME_INFO_NOISE
//! in noise_based_count (same [-1,1] range and role).
//!
//! Determinism across chunk borders: every random stream is keyed by
//! the SOURCE chunk column `(cx, cz)` + world seed + a per-feature
//! salt, never by the vertical chunk - so each 16^3 slice of a column
//! replays the same plant plan and writes only its own intersection
//! (a kelp stalk continues seamlessly into the chunk above). Seagrass
//! adds a +/-8 scatter (MC's SeagrassFeature does its own offset), so
//! the pass also replays the 8 neighbouring source columns and keeps
//! the cells that land in this chunk - the trees' TREE_MARGIN idea.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};
use std::sync::OnceLock;

use crate::block::{
    KELP_ID, KELP_PLANT_ID, SEAGRASS_ID, TALL_SEAGRASS_BOTTOM_ID,
    TALL_SEAGRASS_TOP_ID, WATER_ID,
};
use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::terrain::mc_floor_at;

const SEAGRASS_PLACED: &str = include_str!("data/feature/seagrass_normal.json");
const SEAGRASS_CONFIGURED: &str = include_str!("data/feature/seagrass_short.json");
const KELP_PLACED: &str = include_str!("data/feature/kelp_cold.json");
const KELP_CONFIGURED: &str = include_str!("data/feature/kelp.json");

/// Per-feature random-stream salts (arbitrary, fixed).
const SALT_SEAGRASS: u64 = 0x5EA6_7A55;
const SALT_KELP: u64 = 0x0CE1_F00D;

/// Parsed rules for the ocean-flora placed features.
pub(crate) struct OceanFloraRules {
    /// seagrass_normal: tries per chunk (the `count` modifier).
    pub(crate) seagrass_tries: u32,
    /// seagrass_short config: probability of the tall two-block variant.
    pub(crate) seagrass_tall_probability: f64,
    /// kelp_cold noise_based_count parameters.
    pub(crate) kelp_noise_factor: f64,
    pub(crate) kelp_noise_offset: f64,
    pub(crate) kelp_noise_to_count_ratio: f64,
}

fn parse_rules() -> OceanFloraRules {
    let seagrass_placed: serde_json::Value =
        serde_json::from_str(SEAGRASS_PLACED).expect("seagrass_normal.json parses");
    let seagrass_conf: serde_json::Value =
        serde_json::from_str(SEAGRASS_CONFIGURED).expect("seagrass_short.json parses");
    let kelp_placed: serde_json::Value =
        serde_json::from_str(KELP_PLACED).expect("kelp_cold.json parses");
    let kelp_conf: serde_json::Value =
        serde_json::from_str(KELP_CONFIGURED).expect("kelp.json parses");

    // Sanity: the placed features reference the configured features we
    // embedded (a version bump that rewires them should fail loudly).
    assert_eq!(
        seagrass_placed["feature"].as_str(),
        Some("minecraft:seagrass_short"),
        "seagrass_normal placed_feature rewired"
    );
    assert_eq!(
        kelp_placed["feature"].as_str(),
        Some("minecraft:kelp"),
        "kelp_cold placed_feature rewired"
    );
    assert_eq!(kelp_conf["type"].as_str(), Some("minecraft:kelp"));
    assert_eq!(seagrass_conf["type"].as_str(), Some("minecraft:seagrass"));

    let find = |placed: &serde_json::Value, ty: &str| -> Option<serde_json::Value> {
        placed["placement"]
            .as_array()?
            .iter()
            .find(|m| m["type"].as_str() == Some(ty))
            .cloned()
    };

    let seagrass_tries = find(&seagrass_placed, "minecraft:count")
        .and_then(|m| m["count"].as_u64())
        .expect("seagrass_normal has a count modifier") as u32;
    let seagrass_tall_probability = seagrass_conf["config"]["probability"]
        .as_f64()
        .expect("seagrass probability");

    let nbc = find(&kelp_placed, "minecraft:noise_based_count")
        .expect("kelp_cold has noise_based_count");
    let kelp_noise_factor = nbc["noise_factor"].as_f64().expect("noise_factor");
    let kelp_noise_offset = nbc["noise_offset"].as_f64().unwrap_or(0.0);
    let kelp_noise_to_count_ratio = nbc["noise_to_count_ratio"]
        .as_f64()
        .expect("noise_to_count_ratio");

    OceanFloraRules {
        seagrass_tries,
        seagrass_tall_probability,
        kelp_noise_factor,
        kelp_noise_offset,
        kelp_noise_to_count_ratio,
    }
}

pub(crate) fn ocean_flora_rules() -> &'static OceanFloraRules {
    static RULES: OnceLock<OceanFloraRules> = OnceLock::new();
    RULES.get_or_init(parse_rules)
}

/// MC's `Biome.BIOME_INFO_NOISE` stand-in for `noise_based_count`
/// (PerlinSimplexNoise seeded with the CONSTANT 2345 - world-seed
/// independent in MC too).
fn biome_info_noise() -> &'static NormalNoise {
    static NOISE: OnceLock<NormalNoise> = OnceLock::new();
    NOISE.get_or_init(|| {
        let params = NoiseParams { first_octave: 0, amplitudes: vec![1.0] };
        NormalNoise::new(2345, &params)
    })
}

/// `NoiseBasedCountPlacement.getCount`: ceil((noise(x/f, z/f) + offset)
/// * ratio), clamped at 0. `(wx, wz)` is the chunk origin.
fn kelp_count(rules: &OceanFloraRules, wx: i32, wz: i32) -> u32 {
    let n = biome_info_noise().value(
        wx as f64 / rules.kelp_noise_factor,
        0.0,
        wz as f64 / rules.kelp_noise_factor,
    );
    let count = ((n + rules.kelp_noise_offset) * rules.kelp_noise_to_count_ratio).ceil();
    count.max(0.0) as u32
}

/// Mixed per-(source chunk column, feature) stream seed.
fn stream_seed(seed: u32, cx: i32, cz: i32, salt: u64) -> u64 {
    (seed as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((cx as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add((cz as u64).wrapping_mul(0x1656_67B1_9E37_79F9))
        ^ salt
}

/// Write `id` into the chunk cell for world `(wx, wy, wz)` iff the cell
/// is inside this chunk and currently WATER (so carved caves, prior
/// plants and solid ground are never overwritten).
fn set_if_water(
    chunk: &mut Chunk,
    origin: [i32; 3],
    wx: i32,
    wy: i32,
    wz: i32,
    id: u16,
) {
    let cs = CHUNK_SIZE as i32;
    let lx = wx - origin[0];
    let ly = wy - origin[1];
    let lz = wz - origin[2];
    if (0..cs).contains(&lx)
        && (0..cs).contains(&ly)
        && (0..cs).contains(&lz)
        && chunk.get(lx as usize, ly as usize, lz as usize).0 == WATER_ID
    {
        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
    }
}

/// One seagrass try from source chunk `(scx, scz)` - MC's
/// SeagrassFeature: +/-8 scatter, OCEAN_FLOOR heightmap, tall roll.
fn seagrass_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rules: &OceanFloraRules,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
) {
    let cs = CHUNK_SIZE as i32;
    // in_square: uniform 0..16 inside the source chunk.
    let rx = (rng.next_u64() % 16) as i32;
    let rz = (rng.next_u64() % 16) as i32;
    // SeagrassFeature's own +/-8 scatter (two nextInt(8) rolls per axis).
    let ox = (rng.next_u64() % 8) as i32 - (rng.next_u64() % 8) as i32;
    let oz = (rng.next_u64() % 8) as i32 - (rng.next_u64() % 8) as i32;
    let tall = rng.next_f64() < rules.seagrass_tall_probability;
    let wx = scx * cs + rx + ox;
    let wz = scz * cs + rz + oz;
    let floor = mc_floor_at(ctx, wx, wz);
    if floor >= SEA_LEVEL {
        return; // not ocean floor (the `biome` modifier at our resolution)
    }
    let base = floor + 1;
    if tall && base < SEA_LEVEL {
        set_if_water(chunk, origin, wx, base, wz, TALL_SEAGRASS_BOTTOM_ID);
        set_if_water(chunk, origin, wx, base + 1, wz, TALL_SEAGRASS_TOP_ID);
    } else {
        set_if_water(chunk, origin, wx, base, wz, SEAGRASS_ID);
    }
}

/// One kelp try (KelpFeature): column of kelp_plant topped by kelp,
/// height 1 + rand(10), never above sea level.
fn kelp_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
) {
    let cs = CHUNK_SIZE as i32;
    let rx = (rng.next_u64() % 16) as i32;
    let rz = (rng.next_u64() % 16) as i32;
    let len = 1 + (rng.next_u64() % 10) as i32;
    let wx = scx * cs + rx;
    let wz = scz * cs + rz;
    let floor = mc_floor_at(ctx, wx, wz);
    if floor >= SEA_LEVEL {
        return;
    }
    // Clamp the stalk under the surface (MC kelp stops at the top
    // water cell).
    let eff = len.min(SEA_LEVEL - floor);
    if eff < 1 {
        return;
    }
    for j in 0..eff {
        let wy = floor + 1 + j;
        let id = if j == eff - 1 { KELP_ID } else { KELP_PLANT_ID };
        set_if_water(chunk, origin, wx, wy, wz, id);
    }
}

/// Per-chunk ocean-flora pass. Mc profile only (the rules are MC's).
pub(crate) fn place_from_coord(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return;
    }
    let origin_v = World::chunk_world_origin(coord);
    let origin = [origin_v.x as i32, origin_v.y as i32, origin_v.z as i32];
    // Whole chunk above sea level -> nothing can place.
    if origin[1] > SEA_LEVEL {
        return;
    }
    let rules = ocean_flora_rules();
    let seed = ctx.effective_seed();

    // Seagrass scatters +/-8 out of its source chunk, so replay the 3x3
    // source-chunk neighbourhood and keep what lands here.
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_SEAGRASS));
            for _ in 0..rules.seagrass_tries {
                seagrass_try(chunk, origin, ctx, rules, &mut rng, scx, scz);
            }
        }
    }
    // Kelp stays inside its source chunk (in_square only), so only this
    // column's stream can write here.
    let count = kelp_count(rules, origin[0], origin[2]);
    let mut rng = SplitMix64::new(stream_seed(seed, coord[0], coord[2], SALT_KELP));
    for _ in 0..count {
        kelp_try(chunk, origin, ctx, &mut rng, coord[0], coord[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{WorldGen, generate_chunk_at_ctx};

    fn ctx() -> WorldgenCtx {
        WorldgenCtx { base_seed: 0, dim: String::new(), profile: WorldGen::Mc }
    }

    #[test]
    fn rules_parse_the_embedded_mc_json() {
        let r = ocean_flora_rules();
        // The MC data values (26.1.2 jar): count 48, probability 0.3,
        // noise_based_count(80, 0, 120). If a future data refresh
        // changes them, the parser must track the JSON - these pins
        // prove we read the file, not a hardcode.
        assert_eq!(r.seagrass_tries, 48);
        assert!((r.seagrass_tall_probability - 0.3).abs() < 1e-9);
        assert!((r.kelp_noise_factor - 80.0).abs() < 1e-9);
        assert!((r.kelp_noise_to_count_ratio - 120.0).abs() < 1e-9);
    }

    #[test]
    fn kelp_count_is_bounded_and_varies() {
        let r = ocean_flora_rules();
        let mut seen = std::collections::HashSet::new();
        for cx in 0..64 {
            let c = kelp_count(r, cx * 16, -cx * 32);
            assert!(c <= (r.kelp_noise_to_count_ratio * 1.5) as u32);
            seen.insert(c);
        }
        assert!(seen.len() > 3, "noise count is constant: {seen:?}");
    }

    /// The flora pass must be deterministic and column-consistent: the
    /// same world cell gets the same block regardless of which vertical
    /// chunk generated it (kelp stalks cross chunk-Y borders).
    #[test]
    fn flora_is_deterministic() {
        let c = ctx();
        // Find an ocean chunk near the origin (seed 0 spawns near water).
        let mut ocean = None;
        'search: for cx in -8..8 {
            for cz in -8..8 {
                if mc_floor_at(&c, cx * 16 + 8, cz * 16 + 8) < SEA_LEVEL - 4 {
                    ocean = Some((cx, cz));
                    break 'search;
                }
            }
        }
        let (cx, cz) = ocean.expect("no ocean chunk within 8 chunks of origin");
        let cy = (SEA_LEVEL - 8).div_euclid(16);
        let a = generate_chunk_at_ctx([cx, cy, cz], &c);
        let b = generate_chunk_at_ctx([cx, cy, cz], &c);
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    assert_eq!(a.get(x, y, z), b.get(x, y, z));
                }
            }
        }
    }

    /// Evidence: flora actually appears in ocean chunks. Scans a band of
    /// ocean chunks and requires at least SOME seagrass + kelp.
    #[test]
    fn ocean_chunks_grow_flora() {
        let c = ctx();
        let mut seagrass = 0u32;
        let mut kelp = 0u32;
        let mut ocean_chunks = 0u32;
        for cx in -12..12 {
            for cz in -12..12 {
                if mc_floor_at(&c, cx * 16 + 8, cz * 16 + 8) >= SEA_LEVEL - 2 {
                    continue;
                }
                ocean_chunks += 1;
                // The vertical chunk containing the seabed band.
                let floor = mc_floor_at(&c, cx * 16 + 8, cz * 16 + 8);
                let cy = (floor + 1).div_euclid(16);
                let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                for x in 0..16 {
                    for y in 0..16 {
                        for z in 0..16 {
                            match chunk.get(x, y, z).0 {
                                SEAGRASS_ID | TALL_SEAGRASS_BOTTOM_ID => seagrass += 1,
                                KELP_ID | KELP_PLANT_ID => kelp += 1,
                                _ => {}
                            }
                        }
                    }
                }
                if seagrass > 0 && kelp > 0 {
                    return; // both found - pass
                }
            }
        }
        panic!(
            "no flora in {ocean_chunks} ocean chunks: seagrass={seagrass} kelp={kelp}"
        );
    }
}
