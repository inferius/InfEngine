//! W201/W202 - MC placed_features: ocean flora (Phase 6a) + ore veins
//! (Phase 6b).
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

/// W211: every aquatic-flora placed_feature the ocean-variant /
/// river biomes list (frozen oceans list none).
const FLORA_PLACED: &[(&str, &str)] = &[
    ("seagrass_normal", include_str!("data/feature/placed/seagrass_normal.json")),
    ("seagrass_cold", include_str!("data/feature/placed/seagrass_cold.json")),
    ("seagrass_deep", include_str!("data/feature/placed/seagrass_deep.json")),
    ("seagrass_deep_cold", include_str!("data/feature/placed/seagrass_deep_cold.json")),
    ("seagrass_warm", include_str!("data/feature/placed/seagrass_warm.json")),
    ("seagrass_deep_warm", include_str!("data/feature/placed/seagrass_deep_warm.json")),
    ("seagrass_river", include_str!("data/feature/placed/seagrass_river.json")),
    ("kelp_cold", include_str!("data/feature/placed/kelp_cold.json")),
    ("kelp_warm", include_str!("data/feature/placed/kelp_warm.json")),
    ("seagrass_swamp", include_str!("data/feature/placed/seagrass_swamp.json")),
];

const FLORA_CONFIGURED: &[(&str, &str)] = &[
    ("seagrass_short", include_str!("data/feature/configured/seagrass_short.json")),
    ("seagrass_slightly_less_short", include_str!("data/feature/configured/seagrass_slightly_less_short.json")),
    ("seagrass_mid", include_str!("data/feature/configured/seagrass_mid.json")),
    ("seagrass_tall", include_str!("data/feature/configured/seagrass_tall.json")),
    ("kelp", include_str!("data/feature/configured/kelp.json")),
];

/// W202: every `ore_*` placed_feature the overworld biomes list in their
/// underground_ores step (the set is identical across biomes; extracted
/// from `biome/ocean.json`).
const ORE_PLACED: &[(&str, &str)] = &[
    ("ore_dirt", include_str!("data/feature/placed/ore_dirt.json")),
    ("ore_gravel", include_str!("data/feature/placed/ore_gravel.json")),
    ("ore_granite_upper", include_str!("data/feature/placed/ore_granite_upper.json")),
    ("ore_granite_lower", include_str!("data/feature/placed/ore_granite_lower.json")),
    ("ore_diorite_upper", include_str!("data/feature/placed/ore_diorite_upper.json")),
    ("ore_diorite_lower", include_str!("data/feature/placed/ore_diorite_lower.json")),
    ("ore_andesite_upper", include_str!("data/feature/placed/ore_andesite_upper.json")),
    ("ore_andesite_lower", include_str!("data/feature/placed/ore_andesite_lower.json")),
    ("ore_tuff", include_str!("data/feature/placed/ore_tuff.json")),
    ("ore_coal_upper", include_str!("data/feature/placed/ore_coal_upper.json")),
    ("ore_coal_lower", include_str!("data/feature/placed/ore_coal_lower.json")),
    ("ore_iron_upper", include_str!("data/feature/placed/ore_iron_upper.json")),
    ("ore_iron_middle", include_str!("data/feature/placed/ore_iron_middle.json")),
    ("ore_iron_small", include_str!("data/feature/placed/ore_iron_small.json")),
    ("ore_gold", include_str!("data/feature/placed/ore_gold.json")),
    ("ore_gold_lower", include_str!("data/feature/placed/ore_gold_lower.json")),
    ("ore_redstone", include_str!("data/feature/placed/ore_redstone.json")),
    ("ore_redstone_lower", include_str!("data/feature/placed/ore_redstone_lower.json")),
    ("ore_diamond", include_str!("data/feature/placed/ore_diamond.json")),
    ("ore_diamond_medium", include_str!("data/feature/placed/ore_diamond_medium.json")),
    ("ore_diamond_large", include_str!("data/feature/placed/ore_diamond_large.json")),
    ("ore_diamond_buried", include_str!("data/feature/placed/ore_diamond_buried.json")),
    ("ore_lapis", include_str!("data/feature/placed/ore_lapis.json")),
    ("ore_lapis_buried", include_str!("data/feature/placed/ore_lapis_buried.json")),
    ("ore_copper", include_str!("data/feature/placed/ore_copper.json")),
    // W219/W223: windswept-hills-only (the biome step gate below).
    ("ore_emerald", include_str!("data/feature/placed/ore_emerald.json")),
    ("ore_infested", include_str!("data/feature/placed/ore_infested.json")),
];

/// The configured ore features the placed set references.
const ORE_CONFIGURED: &[(&str, &str)] = &[
    ("ore_andesite", include_str!("data/feature/configured/ore_andesite.json")),
    ("ore_coal", include_str!("data/feature/configured/ore_coal.json")),
    ("ore_coal_buried", include_str!("data/feature/configured/ore_coal_buried.json")),
    ("ore_copper_small", include_str!("data/feature/configured/ore_copper_small.json")),
    ("ore_diamond_buried", include_str!("data/feature/configured/ore_diamond_buried.json")),
    ("ore_diamond_large", include_str!("data/feature/configured/ore_diamond_large.json")),
    ("ore_diamond_medium", include_str!("data/feature/configured/ore_diamond_medium.json")),
    ("ore_diamond_small", include_str!("data/feature/configured/ore_diamond_small.json")),
    ("ore_diorite", include_str!("data/feature/configured/ore_diorite.json")),
    ("ore_dirt", include_str!("data/feature/configured/ore_dirt.json")),
    ("ore_gold_buried", include_str!("data/feature/configured/ore_gold_buried.json")),
    ("ore_granite", include_str!("data/feature/configured/ore_granite.json")),
    ("ore_gravel", include_str!("data/feature/configured/ore_gravel.json")),
    ("ore_iron", include_str!("data/feature/configured/ore_iron.json")),
    ("ore_iron_small", include_str!("data/feature/configured/ore_iron_small.json")),
    ("ore_lapis", include_str!("data/feature/configured/ore_lapis.json")),
    ("ore_lapis_buried", include_str!("data/feature/configured/ore_lapis_buried.json")),
    ("ore_redstone", include_str!("data/feature/configured/ore_redstone.json")),
    ("ore_tuff", include_str!("data/feature/configured/ore_tuff.json")),
    ("ore_emerald", include_str!("data/feature/configured/ore_emerald.json")),
    ("ore_infested", include_str!("data/feature/configured/ore_infested.json")),
];

/// The MC block tags the ore targets replace (extracted from
/// `data/minecraft/tags/block/`).
const TAG_STONE_ORE_REPLACEABLES: &str =
    include_str!("data/feature/tags/stone_ore_replaceables.json");
const TAG_DEEPSLATE_ORE_REPLACEABLES: &str =
    include_str!("data/feature/tags/deepslate_ore_replaceables.json");
const TAG_BASE_STONE_OVERWORLD: &str =
    include_str!("data/feature/tags/base_stone_overworld.json");

/// Per-feature random-stream salt (arbitrary, fixed); each flora rule
/// folds its list index in so the streams stay independent.
const SALT_FLORA: u64 = 0x5EA6_7A55;

/// How many tries a flora placed_feature runs per chunk.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FloraCount {
    Fixed(u32),
    /// noise_based_count (the patchy kelp forests).
    NoiseBased { factor: f64, offset: f64, ratio: f64 },
}

/// What one try places.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FloraKind {
    /// SeagrassFeature with the tall-variant probability.
    Seagrass { tall_probability: f64 },
    /// KelpFeature (a kelp_plant column topped by kelp).
    Kelp,
}

/// One parsed aquatic-flora placed_feature.
pub(crate) struct FloraRule {
    pub(crate) name: &'static str,
    pub(crate) count: FloraCount,
    pub(crate) kind: FloraKind,
}

fn parse_flora(name: &'static str, json: &str) -> FloraRule {
    let placed: serde_json::Value = serde_json::from_str(json).expect("flora placed parses");
    let conf_name = placed["feature"].as_str().expect("feature ref");
    let conf_name = conf_name.strip_prefix("minecraft:").unwrap_or(conf_name);
    let conf_json = FLORA_CONFIGURED
        .iter()
        .find(|(n, _)| *n == conf_name)
        .unwrap_or_else(|| panic!("{name}: configured {conf_name} not embedded"))
        .1;
    let conf: serde_json::Value = serde_json::from_str(conf_json).expect("flora conf parses");
    let kind = match conf["type"].as_str() {
        Some("minecraft:seagrass") => FloraKind::Seagrass {
            tall_probability: conf["config"]["probability"].as_f64().expect("probability"),
        },
        Some("minecraft:kelp") => FloraKind::Kelp,
        other => panic!("{name}: unsupported flora feature {other:?}"),
    };
    let mut count = FloraCount::Fixed(1);
    for m in placed["placement"].as_array().expect("placement") {
        match m["type"].as_str() {
            Some("minecraft:count") => {
                count = FloraCount::Fixed(m["count"].as_u64().expect("count") as u32);
            }
            Some("minecraft:noise_based_count") => {
                count = FloraCount::NoiseBased {
                    factor: m["noise_factor"].as_f64().expect("noise_factor"),
                    offset: m["noise_offset"].as_f64().unwrap_or(0.0),
                    ratio: m["noise_to_count_ratio"].as_f64().expect("ratio"),
                };
            }
            _ => {}
        }
    }
    FloraRule { name, count, kind }
}

pub(crate) fn flora_rule(name: &'static str) -> &'static FloraRule {
    static RULES: OnceLock<Vec<FloraRule>> = OnceLock::new();
    RULES
        .get_or_init(|| FLORA_PLACED.iter().map(|(n, j)| parse_flora(n, j)).collect())
        .iter()
        .find(|r| r.name == name)
        .expect("known flora rule")
}

/// The aquatic-flora list per MC biome (from the biome JSONs' vegetal
/// step; frozen oceans list none, warm ocean's sea_pickle waits for its
/// block).
pub(crate) fn flora_for(mc: super::biome::McBiome) -> &'static [&'static str] {
    use super::biome::McBiome as B;
    match mc {
        B::Ocean => &["seagrass_normal", "kelp_cold"],
        B::DeepOcean => &["seagrass_deep", "kelp_cold"],
        B::ColdOcean => &["seagrass_cold", "kelp_cold"],
        B::DeepColdOcean => &["seagrass_deep_cold", "kelp_cold"],
        B::LukewarmOcean => &["seagrass_warm", "kelp_warm"],
        B::DeepLukewarmOcean => &["seagrass_deep_warm", "kelp_warm"],
        B::WarmOcean => &["seagrass_warm"],
        B::River => &["seagrass_river"],
        B::FrozenOcean | B::DeepFrozenOcean | B::FrozenRiver => &[],
        // The swamps list their own seagrass (count 64 of the 0.6-tall
        // mid variant) for their pools.
        B::Swamp | B::MangroveSwamp => &["seagrass_swamp"],
        _ => &[],
    }
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

/// Tries per chunk for a flora rule. Fixed count, or
/// `NoiseBasedCountPlacement.getCount`: ceil((noise(x/f, z/f) + offset)
/// * ratio), clamped at 0. `(wx, wz)` is the chunk origin.
fn flora_count(rule: &FloraRule, wx: i32, wz: i32) -> u32 {
    match rule.count {
        FloraCount::Fixed(n) => n,
        FloraCount::NoiseBased { factor, offset, ratio } => {
            let n = biome_info_noise().value(wx as f64 / factor, 0.0, wz as f64 / factor);
            ((n + offset) * ratio).ceil().max(0.0) as u32
        }
    }
}

/// Mixed per-(source chunk column, feature) stream seed.
pub(crate) fn stream_seed(seed: u32, cx: i32, cz: i32, salt: u64) -> u64 {
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

/// Is the chunk cell for world `(wx, wy, wz)` water? Cells outside
/// this chunk (or vertical slice) count as water - sub-sea fill IS
/// water, and every slice replays the same walk, so the owning slice
/// applies the real check to its own cells. (W226.1 honest edge: a
/// feature-placed obstacle in a NEIGHBOURING slice is invisible here,
/// so a pair/stalk crossing a slice border can straddle it - cosmetic,
/// since every write still goes through `set_if_water`.)
fn water_or_off_slice(chunk: &Chunk, origin: [i32; 3], wx: i32, wy: i32, wz: i32) -> bool {
    let cs = CHUNK_SIZE as i32;
    let lx = wx - origin[0];
    let ly = wy - origin[1];
    let lz = wz - origin[2];
    if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
        return true;
    }
    chunk.get(lx as usize, ly as usize, lz as usize).0 == WATER_ID
}

/// One seagrass try from source chunk `(scx, scz)` - MC's
/// SeagrassFeature: +/-8 scatter, OCEAN_FLOOR heightmap, tall roll.
/// W226.1: the tall pair places atomically - MC writes both halves
/// only when the base cell is water (a blocked base places NOTHING),
/// emulated slice-independently via the `occupied` column tracker.
#[allow(clippy::too_many_arguments)]
fn seagrass_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    tall_probability: f64,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
    occupied: &mut std::collections::HashSet<(i32, i32)>,
) {
    let cs = CHUNK_SIZE as i32;
    // in_square: uniform 0..16 inside the source chunk.
    let rx = (rng.next_u64() % 16) as i32;
    let rz = (rng.next_u64() % 16) as i32;
    // SeagrassFeature's own +/-8 scatter (two nextInt(8) rolls per axis).
    let ox = (rng.next_u64() % 8) as i32 - (rng.next_u64() % 8) as i32;
    let oz = (rng.next_u64() % 8) as i32 - (rng.next_u64() % 8) as i32;
    let tall = rng.next_f64() < tall_probability;
    let wx = scx * cs + rx + ox;
    let wz = scz * cs + rz + oz;
    let floor = mc_floor_at(ctx, wx, wz);
    if floor >= SEA_LEVEL {
        return; // not ocean floor (the `biome` modifier at our resolution)
    }
    // MC: an occupied grassPos skips the whole try. The tracker stands
    // in for the cell read so every vertical slice decides identically
    // (a pair's base cell is invisible across a slice border).
    if !occupied.insert((wx, wz)) {
        return;
    }
    let base = floor + 1;
    if tall {
        // The upper half must land in water too - at the surface
        // (base + 1 == SEA_LEVEL is air) MC places nothing.
        if base + 1 >= SEA_LEVEL {
            return;
        }
        set_if_water(chunk, origin, wx, base, wz, TALL_SEAGRASS_BOTTOM_ID);
        set_if_water(chunk, origin, wx, base + 1, wz, TALL_SEAGRASS_TOP_ID);
    } else {
        set_if_water(chunk, origin, wx, base, wz, SEAGRASS_ID);
    }
}

/// One kelp try - MC's KelpFeature: walk up from the OCEAN_FLOOR
/// heightmap cell writing kelp_plant while both the cell and the cell
/// above are water; the first obstacle ends the stalk by replacing its
/// last kelp_plant with the kelp tip (W226.1 - the old port skipped
/// obstacles and kept writing above them, leaving floating segments).
#[allow(clippy::too_many_arguments)]
fn kelp_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
    occupied: &mut std::collections::HashSet<(i32, i32)>,
) {
    let cs = CHUNK_SIZE as i32;
    let rx = (rng.next_u64() % 16) as i32;
    let rz = (rng.next_u64() % 16) as i32;
    let height = 1 + (rng.next_u64() % 10) as i32;
    let wx = scx * cs + rx;
    let wz = scz * cs + rz;
    let floor = mc_floor_at(ctx, wx, wz);
    if floor >= SEA_LEVEL {
        return;
    }
    // Column already claimed by an earlier aquatic plant (MC: the base
    // cell wouldn't be water) - slice-independent, see seagrass_try.
    if !occupied.insert((wx, wz)) {
        return;
    }
    for (wy, h) in (floor + 1..).zip(0..=height) {
        // MC: current AND above must be water (above sea level is air).
        let cur_water = wy < SEA_LEVEL && water_or_off_slice(chunk, origin, wx, wy, wz);
        let above_water =
            wy + 1 < SEA_LEVEL && water_or_off_slice(chunk, origin, wx, wy + 1, wz);
        if cur_water && above_water {
            let id = if h == height { KELP_ID } else { KELP_PLANT_ID };
            set_if_water(chunk, origin, wx, wy, wz, id);
        } else {
            if h > 0 {
                // End the stalk: its last kelp_plant becomes the tip.
                let (lx, ly, lz) = (wx - origin[0], wy - 1 - origin[1], wz - origin[2]);
                if (0..cs).contains(&lx)
                    && (0..cs).contains(&ly)
                    && (0..cs).contains(&lz)
                    && chunk.get(lx as usize, ly as usize, lz as usize).0 == KELP_PLANT_ID
                {
                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(KELP_ID));
                }
            }
            return;
        }
    }
}

// ===================================================================
// W202 - MC ore veins (Phase 6b).
// ===================================================================

/// A parsed MC block-tag: the set of engine block ids it contains.
/// Names the engine has no block for are simply absent from the set.
fn parse_tag(json: &str) -> std::collections::HashSet<u16> {
    let v: serde_json::Value = serde_json::from_str(json).expect("tag json parses");
    v["values"]
        .as_array()
        .expect("tag values")
        .iter()
        .filter_map(|n| block_id_by_name(n.as_str()?))
        .collect()
}

/// Engine block id for an MC block name ("minecraft:coal_ore" or bare
/// "coal_ore") via the registry name column. The legacy rows keep
/// their original short names ("coal" 13, "iron" 14, "wood" 5,
/// "leaves" 6 - renaming them would ripple through every name-driven
/// system), so the MC names alias onto them explicitly.
pub(crate) fn block_id_by_name(name: &str) -> Option<u16> {
    use std::collections::HashMap;
    static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m: HashMap<&'static str, u16> =
            crate::block::BLOCK_TYPES.iter().map(|bt| (bt.name, bt.id)).collect();
        m.insert("coal_ore", 13);
        m.insert("iron_ore", 14);
        // W208: the oak set are the legacy wood/leaves rows.
        m.insert("oak_log", 5);
        m.insert("oak_leaves", 6);
        m
    });
    map.get(name.strip_prefix("minecraft:").unwrap_or(name)).copied()
}

/// MC vertical anchors, adapted to the engine's world band: the engine
/// has no negative depth (MC -64..320 vs engine 0..255), so
/// `above_bottom`/`below_top` anchor to the ENGINE bottom/top. That
/// keeps each distribution's SHAPE (e.g. diamond's trapezoid peaking at
/// the world floor) inside the depth that exists; sampled ys outside
/// the band are discarded like MC discards out-of-dimension positions.
#[derive(Clone, Copy, Debug)]
enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

const ENGINE_BOTTOM: i32 = 1;
const ENGINE_TOP: i32 = 255;

impl Anchor {
    fn parse(v: &serde_json::Value) -> Anchor {
        if let Some(a) = v.get("absolute").and_then(|x| x.as_i64()) {
            Anchor::Absolute(a as i32)
        } else if let Some(a) = v.get("above_bottom").and_then(|x| x.as_i64()) {
            Anchor::AboveBottom(a as i32)
        } else if let Some(a) = v.get("below_top").and_then(|x| x.as_i64()) {
            Anchor::BelowTop(a as i32)
        } else {
            panic!("unknown height anchor: {v}")
        }
    }
    fn resolve(self) -> i32 {
        match self {
            Anchor::Absolute(v) => v,
            Anchor::AboveBottom(v) => ENGINE_BOTTOM + v,
            Anchor::BelowTop(v) => ENGINE_TOP - v,
        }
    }
}

/// MC height providers used by the ore set.
#[derive(Clone, Copy, Debug)]
enum HeightProvider {
    Uniform { min: Anchor, max: Anchor },
    /// MC trapezoid (plateau 0): the mean of two uniform rolls.
    Trapezoid { min: Anchor, max: Anchor },
}

impl HeightProvider {
    fn parse(v: &serde_json::Value) -> HeightProvider {
        let min = Anchor::parse(&v["min_inclusive"]);
        let max = Anchor::parse(&v["max_inclusive"]);
        match v["type"].as_str() {
            Some("minecraft:uniform") => HeightProvider::Uniform { min, max },
            Some("minecraft:trapezoid") => HeightProvider::Trapezoid { min, max },
            other => panic!("unsupported height provider {other:?}"),
        }
    }
    /// Sample a y. Consumes a FIXED number of rolls per variant so the
    /// replayed streams stay aligned across chunks.
    fn sample(self, rng: &mut SplitMix64) -> Option<i32> {
        let (lo, hi, y) = match self {
            HeightProvider::Uniform { min, max } => {
                let lo = min.resolve();
                let hi = max.resolve();
                if hi < lo {
                    let _ = rng.next_u64();
                    return None;
                }
                let span = (hi - lo + 1) as u64;
                (lo, hi, lo + (rng.next_u64() % span) as i32)
            }
            HeightProvider::Trapezoid { min, max } => {
                let lo = min.resolve();
                let hi = max.resolve();
                if hi < lo {
                    let _ = rng.next_u64();
                    let _ = rng.next_u64();
                    return None;
                }
                let span = (hi - lo + 1) as u64;
                let a = (rng.next_u64() % span) as i32;
                let b = (rng.next_u64() % span) as i32;
                (lo, hi, lo + (a + b) / 2)
            }
        };
        let _ = (lo, hi);
        (ENGINE_BOTTOM..=ENGINE_TOP).contains(&y).then_some(y)
    }
}

/// `count` is either a literal or a uniform int provider (ore_gold_lower).
#[derive(Clone, Copy, Debug)]
enum CountProvider {
    Fixed(u32),
    Uniform { min: u32, max: u32 },
}

impl CountProvider {
    fn parse(v: &serde_json::Value) -> CountProvider {
        if let Some(n) = v.as_u64() {
            return CountProvider::Fixed(n as u32);
        }
        assert_eq!(v["type"].as_str(), Some("minecraft:uniform"), "count provider {v}");
        CountProvider::Uniform {
            min: v["min_inclusive"].as_u64().expect("count min") as u32,
            max: v["max_inclusive"].as_u64().expect("count max") as u32,
        }
    }
    fn sample(self, rng: &mut SplitMix64) -> u32 {
        match self {
            CountProvider::Fixed(n) => n,
            CountProvider::Uniform { min, max } => {
                min + (rng.next_u64() % (max - min + 1) as u64) as u32
            }
        }
    }
}

/// One replaceable->ore mapping of an ore configured_feature.
struct OreTarget {
    replaceable: std::collections::HashSet<u16>,
    state: u16,
}

/// A fully-resolved ore placed_feature.
pub(crate) struct OreRule {
    pub(crate) name: &'static str,
    count: CountProvider,
    /// rarity_filter: Some(n) = 1-in-n chunks run this feature.
    rarity: Option<u32>,
    height: HeightProvider,
    size: u32,
    discard_on_air: f64,
    targets: Vec<OreTarget>,
}

fn parse_ore_rules() -> Vec<OreRule> {
    let configured: std::collections::HashMap<&str, serde_json::Value> = ORE_CONFIGURED
        .iter()
        .map(|(n, j)| (*n, serde_json::from_str(j).expect("ore configured parses")))
        .collect();
    let tag_of = |name: &str| -> std::collections::HashSet<u16> {
        match name {
            "minecraft:stone_ore_replaceables" => parse_tag(TAG_STONE_ORE_REPLACEABLES),
            "minecraft:deepslate_ore_replaceables" => {
                parse_tag(TAG_DEEPSLATE_ORE_REPLACEABLES)
            }
            "minecraft:base_stone_overworld" => parse_tag(TAG_BASE_STONE_OVERWORLD),
            other => panic!("unembedded ore target tag {other}"),
        }
    };
    ORE_PLACED
        .iter()
        .map(|(name, json)| {
            let placed: serde_json::Value =
                serde_json::from_str(json).expect("ore placed parses");
            let feature = placed["feature"].as_str().expect("feature ref");
            let feature = feature.strip_prefix("minecraft:").unwrap_or(feature);
            let conf = configured
                .get(feature)
                .unwrap_or_else(|| panic!("{name}: configured {feature} not embedded"));
            assert_eq!(conf["type"].as_str(), Some("minecraft:ore"), "{name}");
            let cfg = &conf["config"];
            let size = cfg["size"].as_u64().expect("ore size") as u32;
            let discard_on_air =
                cfg["discard_chance_on_air_exposure"].as_f64().unwrap_or(0.0);
            let targets = cfg["targets"]
                .as_array()
                .expect("ore targets")
                .iter()
                .filter_map(|t| {
                    let state_name = t["state"]["Name"].as_str()?;
                    let Some(state) = block_id_by_name(state_name) else {
                        // The engine has no block for this state (e.g. a
                        // deepslate variant not yet in the registry) -
                        // skip the target; the vein just won't paint
                        // into that stratum.
                        tracing::warn!(state_name, rule = name, "W202: ore state has no engine block");
                        return None;
                    };
                    let tag = t["target"]["tag"].as_str()?;
                    Some(OreTarget { replaceable: tag_of(tag), state })
                })
                .collect();

            let mut count = CountProvider::Fixed(1);
            let mut rarity = None;
            let mut height = None;
            for m in placed["placement"].as_array().expect("placement").iter() {
                match m["type"].as_str() {
                    Some("minecraft:count") => count = CountProvider::parse(&m["count"]),
                    Some("minecraft:rarity_filter") => {
                        rarity = Some(m["chance"].as_u64().expect("rarity chance") as u32)
                    }
                    Some("minecraft:height_range") => {
                        height = Some(HeightProvider::parse(&m["height"]))
                    }
                    _ => {} // in_square + biome are implicit in the pass
                }
            }
            OreRule {
                name,
                count,
                rarity,
                height: height.unwrap_or_else(|| panic!("{name}: no height_range")),
                size,
                discard_on_air,
                targets,
            }
        })
        .collect()
}

pub(crate) fn ore_rules() -> &'static [OreRule] {
    static RULES: OnceLock<Vec<OreRule>> = OnceLock::new();
    RULES.get_or_init(parse_ore_rules)
}

/// Deterministic [0,1) roll from a world position (for
/// discard_chance_on_air_exposure - keyed by position, not stream, so
/// vertical chunk replays agree without tracking stream consumption).
fn pos_roll(x: i32, y: i32, z: i32, seed: u32) -> f64 {
    let mut h = SplitMix64::new(
        (seed as u64)
            ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
    );
    h.next_f64()
}

/// One ore vein, the MC `OreFeature.place` shape: a short random
/// diagonal, walked in `size` steps; each step carries a sphere whose
/// radius follows a half-sine bump along the walk. Cells inside a
/// sphere that match a target's replaceable tag become that target's
/// ore state. RNG consumption is fixed per call (3 + size rolls after
/// the y sample) so replayed streams stay aligned; the air-exposure
/// discard rolls come from `pos_roll`, not the stream.
#[allow(clippy::too_many_arguments)]
fn ore_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rule: &OreRule,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
    seed: u32,
) {
    let cs = CHUNK_SIZE as i32;
    let rx = (rng.next_u64() % 16) as i32;
    let rz = (rng.next_u64() % 16) as i32;
    let Some(cy) = rule.height.sample(rng) else {
        // Keep consumption fixed: the angle roll below still burns even
        // for a discarded y (mirrors the "always roll" discipline).
        let _ = rng.next_f64();
        return;
    };
    let cx = scx * cs + rx;
    let cz = scz * cs + rz;

    let angle = rng.next_f64() * std::f64::consts::PI;
    let g = rule.size as f64 / 8.0;
    // Line endpoints (MC: +/- sin/cos * size/8 around the center, with
    // each end's y jittered by nextInt(3) - 2).
    let x1 = cx as f64 + angle.sin() * g;
    let x2 = cx as f64 - angle.sin() * g;
    let z1 = cz as f64 + angle.cos() * g;
    let z2 = cz as f64 - angle.cos() * g;
    let y1 = (cy as i64 + (rng.next_u64() % 3) as i64 - 2) as f64;
    let y2 = (cy as i64 + (rng.next_u64() % 3) as i64 - 2) as f64;

    // Quick reject once the fixed rolls are burnt: can this vein touch
    // this chunk at all? Max reach = line half-length + max sphere
    // radius (~size/16 + 1).
    let reach = g + rule.size as f64 / 16.0 + 2.0;
    let (ox, oy, oz) = (origin[0], origin[1], origin[2]);
    let hit = (cx as f64 + reach) >= ox as f64
        && (cx as f64 - reach) < (ox + cs) as f64
        && (cz as f64 + reach) >= oz as f64
        && (cz as f64 - reach) < (oz + cs) as f64
        && (cy as f64 + reach) >= oy as f64
        && (cy as f64 - reach) < (oy + cs) as f64;

    let size = rule.size.max(1);
    for s in 0..size {
        let t = s as f64 / size as f64;
        // The radius roll burns whether or not we paint (alignment).
        let rad_roll = rng.next_f64() * rule.size as f64 / 16.0;
        if !hit {
            continue;
        }
        let px = x1 + (x2 - x1) * t;
        let py = y1 + (y2 - y1) * t;
        let pz = z1 + (z2 - z1) * t;
        let r = (((std::f64::consts::PI * t).sin() + 1.0) * rad_roll + 1.0) / 2.0;
        let r2 = r * r;
        let (bx0, bx1) = ((px - r).floor() as i32, (px + r).ceil() as i32);
        let (by0, by1) = ((py - r).floor() as i32, (py + r).ceil() as i32);
        let (bz0, bz1) = ((pz - r).floor() as i32, (pz + r).ceil() as i32);
        for bx in bx0..=bx1 {
            for by in by0..=by1 {
                for bz in bz0..=bz1 {
                    let dx = bx as f64 + 0.5 - px;
                    let dy = by as f64 + 0.5 - py;
                    let dz = bz as f64 + 0.5 - pz;
                    if dx * dx + dy * dy + dz * dz >= r2 {
                        continue;
                    }
                    let (lx, ly, lz) = (bx - ox, by - oy, bz - oz);
                    if !(0..cs).contains(&lx)
                        || !(0..cs).contains(&ly)
                        || !(0..cs).contains(&lz)
                    {
                        continue;
                    }
                    let here = chunk.get(lx as usize, ly as usize, lz as usize).0;
                    let Some(target) =
                        rule.targets.iter().find(|t| t.replaceable.contains(&here))
                    else {
                        continue;
                    };
                    // discard_chance_on_air_exposure: skip if a face
                    // neighbour (within this chunk) is air and the roll
                    // fails. Buried variants use 1.0.
                    if rule.discard_on_air > 0.0 {
                        let exposed = [
                            (lx + 1, ly, lz),
                            (lx - 1, ly, lz),
                            (lx, ly + 1, lz),
                            (lx, ly - 1, lz),
                            (lx, ly, lz + 1),
                            (lx, ly, lz - 1),
                        ]
                        .into_iter()
                        .any(|(nx, ny, nz)| {
                            (0..cs).contains(&nx)
                                && (0..cs).contains(&ny)
                                && (0..cs).contains(&nz)
                                && chunk.get(nx as usize, ny as usize, nz as usize).is_air()
                        });
                        if exposed && pos_roll(bx, by, bz, seed) < rule.discard_on_air {
                            continue;
                        }
                    }
                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(target.state));
                }
            }
        }
    }
}

/// Per-feature salt so every ore rule gets an independent stream.
fn ore_salt(index: usize) -> u64 {
    0x04E5_0000u64 ^ ((index as u64) << 32)
}

/// The per-chunk ore pass: replay the 3x3 source-column neighbourhood
/// of every ore rule (veins reach a few blocks past their source
/// chunk) and paint the cells that land here.
fn place_ores(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let seed = ctx.effective_seed();
    for (idx, rule) in ore_rules().iter().enumerate() {
        for scx in (coord[0] - 1)..=(coord[0] + 1) {
            for scz in (coord[2] - 1)..=(coord[2] + 1) {
                // W219/W223: the windswept-hills-gated ores (emerald in
                // step 6, infested stone in step 7).
                if matches!(rule.name, "ore_emerald" | "ore_infested")
                    && !matches!(
                        super::terrain::mc_full_biome_at(
                            ctx,
                            scx * CHUNK_SIZE as i32 + 8,
                            scz * CHUNK_SIZE as i32 + 8,
                        ),
                        super::biome::McBiome::WindsweptHills
                    )
                {
                    continue;
                }
                let mut rng =
                    SplitMix64::new(stream_seed(seed, scx, scz, ore_salt(idx)));
                if let Some(r) = rule.rarity {
                    // rarity_filter: 1-in-r chunks run the feature.
                    if !rng.next_u64().is_multiple_of(r as u64) {
                        continue;
                    }
                }
                let count = rule.count.sample(&mut rng);
                for _ in 0..count {
                    ore_try(chunk, origin, rule, &mut rng, scx, scz, seed);
                }
            }
        }
    }
}

/// Per-chunk MC placed-features pass (ores + ocean flora). Mc profile
/// only (the rules are MC's).
pub(crate) fn place_from_coord(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return;
    }
    let origin_v = World::chunk_world_origin(coord);
    let origin = [origin_v.x as i32, origin_v.y as i32, origin_v.z as i32];
    // W220: lakes (step 1) - lava pools, surface + underground.
    super::misc_features::place_lava_lakes(chunk, coord, ctx, origin);
    // W225/W226: local_modifications (step 2) - amethyst geodes lead
    // the step, then icebergs, then the old-growth taiga boulders.
    super::geode::place_geodes(chunk, coord, ctx, origin);
    super::iceberg::place_icebergs(chunk, coord, ctx, origin);
    super::misc_features::place_forest_rocks(chunk, coord, ctx, origin);
    // W202: ore veins (underground_ores, step 6) before vegetal
    // decoration. Ores run at EVERY height (coal/iron reach the
    // mountain tops); flora is gated below sea level.
    place_ores(chunk, coord, ctx, origin);
    // W219: the floor disks share step 6 (they run after the ore
    // entries in every biome list).
    super::misc_features::place_disks(chunk, coord, ctx, origin);
    // W223: underwater magma patches close step 6.
    super::aquatic_features::place_underwater_magma(chunk, coord, ctx, origin);
    // W223: glow lichen (underground_decoration, step 7).
    super::cave_features::place_glow_lichen(chunk, coord, ctx, origin);
    // W219: fluid springs (step 8) before the vegetal step.
    super::misc_features::place_springs(chunk, coord, ctx, origin);

    // W223: blue-ice sprouts (frozen oceans, surface_structures step).
    super::aquatic_features::place_blue_ice(chunk, coord, ctx, origin);
    // W208: MC trees (vegetal step) - before the underwater plants,
    // matching the biome feature-list order.
    super::trees::place_trees(chunk, coord, ctx, origin);
    // W223: bamboo stands + sea pickles share the vegetal step.
    super::bamboo::place_bamboo(chunk, coord, ctx, origin);
    super::aquatic_features::place_sea_pickles(chunk, coord, ctx, origin);
    // W224: jungle vines (after the trees so canopies host them).
    super::vines::place_vines(chunk, coord, ctx, origin);
    // W210: grass/flower/mushroom patches, then the freeze_top_layer
    // (ice + snow blanket) as the LAST step, exactly MC's step order -
    // snow must land on the finished canopy.
    super::vegetation::place_vegetation(chunk, coord, ctx, origin);
    super::vegetation::place_freeze_top_layer(chunk, coord, ctx, origin);

    // Whole chunk above sea level -> no aquatic flora can place.
    if origin[1] > SEA_LEVEL {
        return;
    }
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;

    // W211: the flora set is the REAL MC biome's list (ocean variants,
    // rivers). Seagrass scatters +/-8 out of its source chunk, so the
    // 3x3 source-chunk neighbourhood replays; kelp stays in_square.
    //
    // W226.1: `occupied` = one aquatic plant per column, decided purely
    // stream-side. Every vertical slice replays the same streams in the
    // same order, so the tracker reaches identical decisions everywhere
    // - unlike a chunk-cell read, which can't see a pair's base cell
    // across a slice border (the source of orphaned tall-seagrass tops).
    let mut occupied: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = super::terrain::mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            for (idx, name) in flora_for(mc).iter().enumerate() {
                let rule = flora_rule(name);
                let salt = SALT_FLORA ^ ((idx as u64) << 40);
                let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, salt));
                let count = flora_count(rule, scx * cs, scz * cs);
                match rule.kind {
                    FloraKind::Seagrass { tall_probability } => {
                        for _ in 0..count {
                            seagrass_try(
                                chunk,
                                origin,
                                ctx,
                                tall_probability,
                                &mut rng,
                                scx,
                                scz,
                                &mut occupied,
                            );
                        }
                    }
                    FloraKind::Kelp => {
                        // Kelp writes only inside its source chunk; skip
                        // the neighbour replays entirely (their streams
                        // are independent, so skipping burns no rolls).
                        if scx == coord[0] && scz == coord[2] {
                            for _ in 0..count {
                                kelp_try(chunk, origin, ctx, &mut rng, scx, scz, &mut occupied);
                            }
                        }
                    }
                }
            }
        }
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
        // The MC data values (26.1.2 jar): count 48, probability 0.3,
        // noise_based_count(80, 0, 120). If a future data refresh
        // changes them, the parser must track the JSON - these pins
        // prove we read the file, not a hardcode.
        let normal = flora_rule("seagrass_normal");
        assert!(matches!(normal.count, FloraCount::Fixed(48)));
        assert!(
            matches!(normal.kind, FloraKind::Seagrass { tall_probability } if (tall_probability - 0.3).abs() < 1e-9)
        );
        let kelp = flora_rule("kelp_cold");
        assert!(matches!(
            kelp.count,
            FloraCount::NoiseBased { factor, ratio, .. }
                if (factor - 80.0).abs() < 1e-9 && (ratio - 120.0).abs() < 1e-9
        ));
        // W211: the ocean variants parse too (deep = tall seagrass 0.8).
        let deep = flora_rule("seagrass_deep");
        assert!(
            matches!(deep.kind, FloraKind::Seagrass { tall_probability } if tall_probability > 0.5)
        );
        assert!(matches!(flora_rule("kelp_warm").kind, FloraKind::Kelp));
        assert!(matches!(flora_rule("seagrass_river").kind, FloraKind::Seagrass { .. }));
    }

    #[test]
    fn kelp_count_is_bounded_and_varies() {
        let r = flora_rule("kelp_cold");
        let mut seen = std::collections::HashSet::new();
        for cx in 0..64 {
            let c = flora_count(r, cx * 16, -cx * 32);
            assert!(c <= 180);
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

    /// W226.1 regression (user report, repro seed 3592732596): aquatic
    /// plants must not float in the water column - seagrass/pickles
    /// need a solid floor cell below, kelp columns must be anchored
    /// (solid or continuing kelp_plant below).
    #[test]
    fn no_floating_ocean_plants() {
        let pickle = block_id_by_name("sea_pickle").unwrap();
        let plant = |id: u16| {
            id == SEAGRASS_ID
                || id == TALL_SEAGRASS_BOTTOM_ID
                || id == TALL_SEAGRASS_TOP_ID
                || id == KELP_ID
                || id == KELP_PLANT_ID
                || id == pickle
        };
        let c = WorldgenCtx {
            base_seed: 3592732596,
            dim: String::new(),
            profile: WorldGen::Mc,
        };
        let mut floating: Vec<String> = Vec::new();
        for cx in -8..8 {
            for cz in -8..8 {
                let centre_floor = mc_floor_at(&c, cx * 16 + 8, cz * 16 + 8);
                if centre_floor >= SEA_LEVEL - 2 {
                    continue;
                }
                // Column stack spanning seabed-2 .. sea surface.
                let cy_lo = (centre_floor - 2).div_euclid(16).max(0);
                let cy_hi = SEA_LEVEL.div_euclid(16);
                let slices: Vec<Chunk> = (cy_lo..=cy_hi)
                    .map(|cy| generate_chunk_at_ctx([cx, cy, cz], &c))
                    .collect();
                let get = |x: usize, wy: i32, z: usize| -> u16 {
                    let idx = wy.div_euclid(16) - cy_lo;
                    if idx < 0 || idx as usize >= slices.len() {
                        return u16::MAX; // outside the stack: don't judge
                    }
                    slices[idx as usize].get(x, wy.rem_euclid(16) as usize, z).0
                };
                for x in 0..16usize {
                    for z in 0..16usize {
                        for wy in (cy_lo * 16 + 1)..((cy_hi + 1) * 16) {
                            let id = get(x, wy, z);
                            if !plant(id) {
                                continue;
                            }
                            let below = get(x, wy - 1, z);
                            if below == u16::MAX {
                                continue;
                            }
                            let ok = match id {
                                i if i == TALL_SEAGRASS_TOP_ID => below == TALL_SEAGRASS_BOTTOM_ID,
                                i if i == KELP_ID || i == KELP_PLANT_ID => {
                                    below == KELP_PLANT_ID
                                        || (below != 0 && below != WATER_ID && !plant(below))
                                }
                                // seagrass / tall bottom / pickle: solid floor.
                                _ => below != 0 && below != WATER_ID && !plant(below),
                            };
                            if !ok && floating.len() < 40 {
                                floating.push(format!(
                                    "id {id} at ({},{wy},{}) over below-id {below}",
                                    cx * 16 + x as i32,
                                    cz * 16 + z as i32,
                                ));
                            }
                            // Pair integrity: a bottom half must carry
                            // its top half above it.
                            if id == TALL_SEAGRASS_BOTTOM_ID {
                                let above = get(x, wy + 1, z);
                                if above != u16::MAX
                                    && above != TALL_SEAGRASS_TOP_ID
                                    && floating.len() < 40
                                {
                                    floating.push(format!(
                                        "orphan tall-bottom at ({},{wy},{}) under above-id {above}",
                                        cx * 16 + x as i32,
                                        cz * 16 + z as i32,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            floating.is_empty(),
            "floating aquatic plants found:\n{}",
            floating.join("\n")
        );
    }

    // ---------------- W202: ores ----------------

    #[test]
    fn ore_rules_parse_the_embedded_mc_json() {
        let rules = ore_rules();
        assert_eq!(rules.len(), 27, "one rule per embedded ore placed_feature");
        let by_name = |n: &str| rules.iter().find(|r| r.name == n).unwrap();
        // W219: emerald parses (100 tries, trapezoid) - the biome gate
        // lives in place_ores, not the rule.
        let emerald = by_name("ore_emerald");
        assert!(matches!(emerald.count, CountProvider::Fixed(100)));
        assert_eq!(emerald.size, 3);
        // Pins from the 26.1.2 data: prove the values come from the
        // JSON, not a hardcode.
        let coal = by_name("ore_coal_upper");
        assert!(matches!(coal.count, CountProvider::Fixed(30)));
        assert_eq!(coal.size, 17);
        let dia = by_name("ore_diamond_large");
        assert_eq!(dia.rarity, Some(9));
        assert!((dia.discard_on_air - 0.7).abs() < 1e-9 || dia.discard_on_air >= 0.0);
        let gold_lower = by_name("ore_gold_lower");
        assert!(matches!(gold_lower.count, CountProvider::Uniform { min: 0, max: 1 }));
        // Every rule must have resolved at least one paintable target
        // (a rule with zero targets would silently no-op).
        for r in rules {
            assert!(!r.targets.is_empty(), "{} has no engine targets", r.name);
        }
    }

    #[test]
    fn key_ore_states_resolve_to_engine_blocks() {
        // The core ore roster must exist in the registry, in both stone
        // and deepslate forms - otherwise veins skip that stratum.
        for name in [
            "coal_ore", "iron_ore", "copper_ore", "gold_ore", "redstone_ore",
            "diamond_ore", "lapis_ore", "deepslate_coal_ore", "deepslate_iron_ore",
            "deepslate_gold_ore", "deepslate_diamond_ore", "stone", "granite",
            "diorite", "andesite", "tuff", "deepslate", "dirt", "gravel",
        ] {
            assert!(
                block_id_by_name(name).is_some(),
                "registry has no block named {name}"
            );
        }
    }

    /// Evidence: veins actually paint ore into underground chunks.
    #[test]
    fn underground_chunks_grow_ores() {
        let c = ctx();
        let coal = block_id_by_name("coal_ore").unwrap();
        let deep_coal = block_id_by_name("deepslate_coal_ore").unwrap();
        let iron = block_id_by_name("iron_ore").unwrap();
        let deep_iron = block_id_by_name("deepslate_iron_ore").unwrap();
        let mut found_coal = 0u32;
        let mut found_iron = 0u32;
        for cx in -6..6 {
            for cz in -6..6 {
                // A solidly-underground band (y 16..48).
                for cy in 1..3 {
                    let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                let id = chunk.get(x, y, z).0;
                                if id == coal || id == deep_coal {
                                    found_coal += 1;
                                } else if id == iron || id == deep_iron {
                                    found_iron += 1;
                                }
                            }
                        }
                    }
                }
                if found_coal > 0 && found_iron > 0 {
                    return;
                }
            }
        }
        panic!("no ores underground: coal={found_coal} iron={found_iron}");
    }
}
