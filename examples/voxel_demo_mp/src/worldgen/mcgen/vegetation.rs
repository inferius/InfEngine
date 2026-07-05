//! W210 - MC vegetation patches + freeze_top_layer (Phase 6d).
//!
//! 26.x rebuilt the grass/flower patches as `simple_block` configured
//! features driven entirely by PLACEMENT MODIFIER pipelines (count /
//! noise_threshold_count / rarity_filter / in_square / heightmap /
//! random_offset / block_predicate_filter). This module interprets the
//! embedded placed_feature JSONs as that pipeline, generically: a
//! position list is folded through the modifiers in FILE ORDER, exactly
//! like MC's PlacementContext stream - so plains grass really is
//! "5-or-10 noise anchors, each in_square'd, each duplicated 32x, each
//! trapezoid-scattered +-7/+-3" and grows in natural-looking tufts.
//!
//! State providers ported from source: simple_state_provider,
//! weighted_state_provider, noise_threshold_provider
//! (NoiseThresholdProvider.getState: below threshold -> random low
//! state; else high_chance -> random high state, else default) and
//! noise_provider (NoiseProvider.getRandomState: clamp((1+noise)/2)
//! indexes the states list - the flower-forest flower bands). Both
//! noise providers sample their OWN NormalNoise built from the JSON's
//! seed + scale at the full 3D position, exactly
//! NoiseBasedStateProvider.getNoiseValue.
//!
//! freeze_top_layer is SnowAndFreezeFeature: per column, ice on
//! exposed water + a snow layer on the exposed top when
//! `Biome.coldEnoughToSnow` - the real height-adjusted temperature
//! model (base biome temperature; above seaLevel+17 it drops by
//! (TEMPERATURE_NOISE(x/8, z/8) * 8 + y - snowLevel) * 0.05 / 40,
//! snow below 0.15). Biome base temperatures come from the extracted
//! `biome_temperatures.json`, not hand-picked numbers.
//!
//! Honest divergences: SplitMix64 streams (no seed identity); the two
//! heightmap flavours (WORLD_SURFACE_WG / MOTION_BLOCKING) both
//! resolve to the terrain heightmap (`mc_floor_at`) - plants place
//! under canopies, which the air + dirt-floor predicates then filter
//! exactly like MC's canSurvive; forest_flowers (lilac/peony/rose
//! bush) and waterlily wait for their blocks; snow on a canopy that
//! ends exactly at a chunk-Y seam can miss (chunk-local scan).

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};
use std::sync::{Arc, OnceLock};

use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

use super::biome::McBiome;
use super::features::{block_id_by_name, stream_seed};
use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::terrain::{mc_floor_at, mc_full_biome_at};

const SALT_VEGETATION: u64 = 0x0F10_7A00;
const SALT_FREEZE: u64 = 0x1CE5_0000;

const VEG_PLACED: &[(&str, &str)] = &[
    ("patch_grass_plain", include_str!("data/feature/placed/patch_grass_plain.json")),
    ("flower_plains", include_str!("data/feature/placed/flower_plains.json")),
    ("patch_tall_grass_2", include_str!("data/feature/placed/patch_tall_grass_2.json")),
    ("patch_grass_forest", include_str!("data/feature/placed/patch_grass_forest.json")),
    ("flower_default", include_str!("data/feature/placed/flower_default.json")),
    ("patch_grass_taiga_2", include_str!("data/feature/placed/patch_grass_taiga_2.json")),
    ("patch_large_fern", include_str!("data/feature/placed/patch_large_fern.json")),
    ("patch_berry_common", include_str!("data/feature/placed/patch_berry_common.json")),
    ("patch_grass_jungle", include_str!("data/feature/placed/patch_grass_jungle.json")),
    ("flower_warm", include_str!("data/feature/placed/flower_warm.json")),
    ("patch_grass_savanna", include_str!("data/feature/placed/patch_grass_savanna.json")),
    ("patch_tall_grass", include_str!("data/feature/placed/patch_tall_grass.json")),
    ("patch_grass_normal", include_str!("data/feature/placed/patch_grass_normal.json")),
    ("flower_swamp", include_str!("data/feature/placed/flower_swamp.json")),
    ("patch_grass_badlands", include_str!("data/feature/placed/patch_grass_badlands.json")),
    ("brown_mushroom_normal", include_str!("data/feature/placed/brown_mushroom_normal.json")),
    ("red_mushroom_normal", include_str!("data/feature/placed/red_mushroom_normal.json")),
    // W211: the real MC biomes' variant patches.
    ("flower_flower_forest", include_str!("data/feature/placed/flower_flower_forest.json")),
    ("patch_grass_taiga", include_str!("data/feature/placed/patch_grass_taiga.json")),
    ("patch_berry_rare", include_str!("data/feature/placed/patch_berry_rare.json")),
    ("brown_mushroom_taiga", include_str!("data/feature/placed/brown_mushroom_taiga.json")),
    ("red_mushroom_taiga", include_str!("data/feature/placed/red_mushroom_taiga.json")),
    // W218: sugar cane / cactus / dead bush / pumpkin / melon retire
    // the legacy biome_features + cactus passes for the Mc profile.
    ("patch_sugar_cane", include_str!("data/feature/placed/patch_sugar_cane.json")),
    ("patch_sugar_cane_desert", include_str!("data/feature/placed/patch_sugar_cane_desert.json")),
    ("patch_sugar_cane_swamp", include_str!("data/feature/placed/patch_sugar_cane_swamp.json")),
    ("patch_cactus_desert", include_str!("data/feature/placed/patch_cactus_desert.json")),
    ("patch_dead_bush", include_str!("data/feature/placed/patch_dead_bush.json")),
    ("patch_dead_bush_2", include_str!("data/feature/placed/patch_dead_bush_2.json")),
    ("patch_pumpkin", include_str!("data/feature/placed/patch_pumpkin.json")),
    ("patch_melon", include_str!("data/feature/placed/patch_melon.json")),
    ("patch_melon_sparse", include_str!("data/feature/placed/patch_melon_sparse.json")),
    // W222 audit batch: the old-growth + swamp mushroom variants.
    ("brown_mushroom_old_growth", include_str!("data/feature/placed/brown_mushroom_old_growth.json")),
    ("red_mushroom_old_growth", include_str!("data/feature/placed/red_mushroom_old_growth.json")),
    ("brown_mushroom_swamp", include_str!("data/feature/placed/brown_mushroom_swamp.json")),
    ("red_mushroom_swamp", include_str!("data/feature/placed/red_mushroom_swamp.json")),
];

const VEG_CONFIGURED: &[(&str, &str)] = &[
    ("grass", include_str!("data/feature/configured/grass.json")),
    ("flower_plain", include_str!("data/feature/configured/flower_plain.json")),
    ("tall_grass", include_str!("data/feature/configured/tall_grass.json")),
    ("flower_default", include_str!("data/feature/configured/flower_default.json")),
    ("taiga_grass", include_str!("data/feature/configured/taiga_grass.json")),
    ("large_fern", include_str!("data/feature/configured/large_fern.json")),
    ("berry_bush", include_str!("data/feature/configured/berry_bush.json")),
    ("grass_jungle", include_str!("data/feature/configured/grass_jungle.json")),
    ("flower_swamp", include_str!("data/feature/configured/flower_swamp.json")),
    ("brown_mushroom", include_str!("data/feature/configured/brown_mushroom.json")),
    ("red_mushroom", include_str!("data/feature/configured/red_mushroom.json")),
    ("flower_flower_forest", include_str!("data/feature/configured/flower_flower_forest.json")),
    ("sugar_cane", include_str!("data/feature/configured/sugar_cane.json")),
    ("cactus", include_str!("data/feature/configured/cactus.json")),
    ("dead_bush", include_str!("data/feature/configured/dead_bush.json")),
    ("pumpkin", include_str!("data/feature/configured/pumpkin.json")),
    ("melon", include_str!("data/feature/configured/melon.json")),
];

const BIOME_TEMPERATURES: &str = include_str!("data/feature/biome_temperatures.json");

/// A resolved plant: single block, an MC half=lower/upper pair, or a
/// W218 `block_column` (sugar cane / cactus - biased-to-bottom height).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Plant {
    Single(u16),
    Double(u16, u16),
    Column { id: u16, min_h: i32, max_h: i32 },
}

/// W218: what the base cell's floor must be - the engine stand-in for
/// each block's `canSurvive` (MC's simple_block / block_column features
/// survival-check every placement).
#[derive(Clone, Copy, Debug, PartialEq)]
enum FloorRule {
    /// The W210 default: any dirt-family floor (grass / flowers /
    /// mushrooms).
    DirtFamily,
    /// `matching_blocks grass_block (0,-1,0)` predicate (berries,
    /// pumpkin, melon).
    GrassBlock,
    /// SugarCaneBlock.canSurvive: dirt/sand-family floor with water
    /// horizontally adjacent to the FLOOR cell.
    SugarCane,
    /// CactusBlock.canSurvive: sand-family floor + no solid horizontal
    /// neighbour at the base cell.
    Cactus,
    /// DeadBushBlock.mayPlaceOn: sand / red sand / dirt family.
    DeadBush,
}

/// MC block-state name -> engine plant, folding the two-block plants
/// onto their bottom/top id pairs.
fn plant_for(name: &str) -> Option<Plant> {
    let stem = name.strip_prefix("minecraft:").unwrap_or(name);
    match stem {
        "tall_grass" => Some(Plant::Double(
            block_id_by_name("tall_grass_bottom")?,
            block_id_by_name("tall_grass_top")?,
        )),
        "large_fern" => Some(Plant::Double(
            block_id_by_name("large_fern_bottom")?,
            block_id_by_name("large_fern_top")?,
        )),
        _ => block_id_by_name(stem).map(Plant::Single),
    }
}

/// The provider's own noise field: NoiseBasedStateProvider builds a
/// NormalNoise from the JSON's `seed` (world-independent) + `noise`
/// params and samples it at `pos * scale` in full 3D.
#[derive(Clone)]
struct ProviderNoise {
    noise: Arc<NormalNoise>,
    scale: f64,
}

impl ProviderNoise {
    fn parse(v: &serde_json::Value) -> Option<ProviderNoise> {
        let params: NoiseParams = serde_json::from_value(v["noise"].clone()).ok()?;
        Some(ProviderNoise {
            noise: Arc::new(NormalNoise::new(v["seed"].as_i64()? as u64, &params)),
            scale: v["scale"].as_f64()?,
        })
    }

    fn value(&self, wx: i32, wy: i32, wz: i32) -> f64 {
        self.noise.value(
            wx as f64 * self.scale,
            wy as f64 * self.scale,
            wz as f64 * self.scale,
        )
    }
}

/// The block-state provider of a `simple_block` feature.
#[derive(Clone)]
enum Provider {
    Simple(Plant),
    Weighted(Vec<(u32, Plant)>),
    /// NoiseThresholdProvider (flower_plain): below threshold -> random
    /// low state; else high_chance -> random high state, else default.
    NoiseThreshold {
        threshold: f64,
        high_chance: f64,
        default: Plant,
        low: Vec<Plant>,
        high: Vec<Plant>,
        noise: ProviderNoise,
    },
    /// NoiseProvider (flower_flower_forest): clamp((1+noise)/2, 0,
    /// 0.9999) * len indexes the states list - contiguous flower bands.
    Noise { states: Vec<Plant>, noise: ProviderNoise },
}

impl Provider {
    fn parse(v: &serde_json::Value) -> Option<Provider> {
        match v["type"].as_str() {
            Some("minecraft:simple_state_provider") => {
                Some(Provider::Simple(plant_for(v["state"]["Name"].as_str()?)?))
            }
            Some("minecraft:weighted_state_provider") => {
                let mut entries = Vec::new();
                for e in v["entries"].as_array()? {
                    // Skip states the engine has no block for; the rest
                    // renormalise naturally through the weight sum.
                    if let Some(p) = plant_for(e["data"]["Name"].as_str()?) {
                        entries.push((e["weight"].as_u64().unwrap_or(1) as u32, p));
                    }
                }
                (!entries.is_empty()).then_some(Provider::Weighted(entries))
            }
            Some("minecraft:noise_threshold_provider") => {
                let collect = |key: &str| -> Vec<Plant> {
                    v[key].as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| plant_for(s["Name"].as_str()?))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                Some(Provider::NoiseThreshold {
                    threshold: v["threshold"].as_f64()?,
                    high_chance: v["high_chance"].as_f64()?,
                    default: plant_for(v["default_state"]["Name"].as_str()?)?,
                    low: collect("low_states"),
                    high: collect("high_states"),
                    noise: ProviderNoise::parse(v)?,
                })
            }
            Some("minecraft:noise_provider") => {
                // Strict: every state must resolve, or the noise->index
                // mapping would shift against MC's.
                let mut states = Vec::new();
                for s in v["states"].as_array()? {
                    states.push(plant_for(s["Name"].as_str()?)?);
                }
                (!states.is_empty())
                    .then(|| ProviderNoise::parse(v).map(|noise| Provider::Noise { states, noise }))
                    .flatten()
            }
            _ => None,
        }
    }

    fn pick(&self, rng: &mut SplitMix64, wx: i32, wy: i32, wz: i32) -> Plant {
        match self {
            Provider::Simple(p) => *p,
            Provider::Weighted(entries) => {
                let total: u32 = entries.iter().map(|(w, _)| *w).sum();
                let mut roll = (rng.next_u64() % total.max(1) as u64) as u32;
                for (w, p) in entries {
                    if roll < *w {
                        return *p;
                    }
                    roll -= w;
                }
                entries[0].1
            }
            Provider::NoiseThreshold { threshold, high_chance, default, low, high, noise } => {
                let n = noise.value(wx, wy, wz);
                if n < *threshold && !low.is_empty() {
                    low[(rng.next_u64() % low.len() as u64) as usize]
                } else if rng.next_f64() < *high_chance && !high.is_empty() {
                    high[(rng.next_u64() % high.len() as u64) as usize]
                } else {
                    *default
                }
            }
            Provider::Noise { states, noise } => {
                let n = noise.value(wx, wy, wz);
                let t = ((1.0 + n) / 2.0).clamp(0.0, 0.9999);
                states[(t * states.len() as f64) as usize]
            }
        }
    }
}

/// One placement-pipeline step, in file order.
#[derive(Clone, Debug)]
enum Step {
    Count(u32),
    /// noise_threshold_count: anchors = noise(origin/200) < level ? below : above.
    NoiseCount { level: f64, below: u32, above: u32 },
    Rarity(u32),
    InSquare,
    Heightmap,
    /// random_offset with trapezoid spreads.
    RandomOffset { xz: (i32, i32), y: (i32, i32) },
}

/// A parsed vegetation placed_feature.
pub(crate) struct PatchDef {
    pub(crate) name: &'static str,
    steps: Vec<Step>,
    provider: Provider,
    /// W218: the base cell's floor requirement (per-block canSurvive).
    floor: FloorRule,
}

fn lookup(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table.iter().find(|(n, _)| *n == name).map(|(_, j)| *j)
}

/// W218: parse a `block_column` configured feature (sugar cane /
/// cactus): the FIRST layer's simple state + biased_to_bottom height.
/// Extra layers whose block the engine lacks (cactus_flower) drop out.
fn parse_block_column(cj: &serde_json::Value) -> Option<(Provider, &'static str)> {
    let layer = cj["config"]["layers"].as_array()?.first()?;
    let state = layer["provider"]["state"]["Name"].as_str()?;
    let stem = state.strip_prefix("minecraft:").unwrap_or(state);
    let id = block_id_by_name(stem)?;
    let h = &layer["height"];
    let (min_h, max_h) = if h["type"].as_str() == Some("minecraft:biased_to_bottom") {
        (
            h["min_inclusive"].as_i64().unwrap_or(1) as i32,
            h["max_inclusive"].as_i64().unwrap_or(1) as i32,
        )
    } else {
        (1, 1)
    };
    let survive = match stem {
        "sugar_cane" => "sugar_cane",
        "cactus" => "cactus",
        _ => "",
    };
    Some((Provider::Simple(Plant::Column { id, min_h, max_h }), survive))
}

fn parse_patch(name: &'static str) -> Option<PatchDef> {
    let pj: serde_json::Value =
        serde_json::from_str(lookup(VEG_PLACED, name)?).expect("veg placed parses");
    let conf_name = pj["feature"].as_str()?.trim_start_matches("minecraft:");
    let cj: serde_json::Value =
        serde_json::from_str(lookup(VEG_CONFIGURED, conf_name)?).expect("veg conf parses");
    let (provider, mut survive) = match cj["type"].as_str() {
        Some("minecraft:simple_block") => (Provider::parse(&cj["config"]["to_place"])?, ""),
        Some("minecraft:block_column") => parse_block_column(&cj)?,
        _ => return None,
    };
    // simple_block canSurvive: dead_bush has sand-family placement
    // rules of its own (no would_survive in the placed pipeline).
    if conf_name == "dead_bush" {
        survive = "dead_bush";
    }
    let mut steps = Vec::new();
    let mut needs_grass_floor = false;
    let trap = |v: &serde_json::Value| -> (i32, i32) {
        (
            v["min"].as_i64().unwrap_or(0) as i32,
            v["max"].as_i64().unwrap_or(0) as i32,
        )
    };
    for m in pj["placement"].as_array()? {
        match m["type"].as_str() {
            Some("minecraft:count") => {
                steps.push(Step::Count(m["count"].as_u64().unwrap_or(1) as u32));
            }
            Some("minecraft:noise_threshold_count") => steps.push(Step::NoiseCount {
                level: m["noise_level"].as_f64().unwrap_or(0.0),
                below: m["below_noise"].as_u64().unwrap_or(0) as u32,
                above: m["above_noise"].as_u64().unwrap_or(0) as u32,
            }),
            Some("minecraft:rarity_filter") => {
                steps.push(Step::Rarity(m["chance"].as_u64().unwrap_or(1) as u32));
            }
            Some("minecraft:in_square") => steps.push(Step::InSquare),
            Some("minecraft:heightmap") => steps.push(Step::Heightmap),
            Some("minecraft:random_offset") => steps.push(Step::RandomOffset {
                xz: trap(&m["xz_spread"]),
                y: trap(&m["y_spread"]),
            }),
            Some("minecraft:block_predicate_filter") => {
                // air is implicit in the placer; grass_block floors
                // (berries/pumpkin/melon) + would_survive states
                // (sugar cane / cactus) map onto FloorRule.
                let s = m["predicate"].to_string();
                if s.contains("grass_block") {
                    needs_grass_floor = true;
                }
                if s.contains("would_survive") {
                    if s.contains("sugar_cane") {
                        survive = "sugar_cane";
                    } else if s.contains("cactus") {
                        survive = "cactus";
                    }
                }
            }
            _ => {} // biome: implicit (we drive from the biome's list)
        }
    }
    let floor = match survive {
        "sugar_cane" => FloorRule::SugarCane,
        "cactus" => FloorRule::Cactus,
        "dead_bush" => FloorRule::DeadBush,
        _ if needs_grass_floor => FloorRule::GrassBlock,
        _ => FloorRule::DirtFamily,
    };
    Some(PatchDef { name, steps, provider, floor })
}

pub(crate) fn patch(name: &'static str) -> Option<&'static PatchDef> {
    static CACHE: OnceLock<Vec<PatchDef>> = OnceLock::new();
    CACHE
        .get_or_init(|| VEG_PLACED.iter().filter_map(|(n, _)| parse_patch(n)).collect())
        .iter()
        .find(|p| p.name == name)
}

/// W211: the vegetal patch list per REAL MC biome, straight from the
/// biome JSONs' vegetal_decoration step (restricted to the patches the
/// engine has blocks for; unported entries - sunflowers, forest_flowers
/// doubles, dead bushes, waterlily, bamboo, old-growth mushrooms - are
/// honest gaps, not substitutions).
pub(crate) fn patches_for(mc: McBiome) -> &'static [&'static str] {
    use McBiome as B;
    match mc {
        B::Plains | B::SunflowerPlains => &[
            "patch_tall_grass_2", "flower_plains", "patch_grass_plain",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::Forest | B::BirchForest | B::OldGrowthBirchForest | B::DarkForest => &[
            "flower_default", "patch_grass_forest",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::FlowerForest => &[
            "flower_flower_forest", "patch_grass_badlands",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::Taiga => &[
            "patch_large_fern", "flower_default", "patch_grass_taiga_2",
            "brown_mushroom_taiga", "red_mushroom_taiga", "patch_berry_common",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::SnowyTaiga => &[
            "patch_large_fern", "flower_default", "patch_grass_taiga_2",
            "brown_mushroom_taiga", "red_mushroom_taiga", "patch_berry_rare",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::OldGrowthSpruceTaiga | B::OldGrowthPineTaiga => &[
            "patch_large_fern", "flower_default", "patch_grass_taiga",
            "patch_dead_bush", "brown_mushroom_normal", "red_mushroom_normal",
            "brown_mushroom_old_growth", "red_mushroom_old_growth",
            "patch_berry_common", "patch_pumpkin", "patch_sugar_cane",
        ],
        B::Jungle | B::BambooJungle => &[
            "flower_warm", "patch_grass_jungle",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane", "patch_melon",
        ],
        B::SparseJungle => &[
            "flower_warm", "patch_grass_jungle",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane", "patch_melon_sparse",
        ],
        B::Savanna => &[
            "patch_tall_grass", "flower_warm", "patch_grass_savanna",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        B::Swamp => &[
            "flower_swamp", "patch_grass_normal", "patch_dead_bush",
            "brown_mushroom_swamp", "red_mushroom_swamp",
            "patch_sugar_cane_swamp", "patch_pumpkin",
        ],
        // mangrove_swamp lists no flowers or mushrooms; its grass is the
        // plain patch (trees_mangrove / waterlily unhosted).
        B::MangroveSwamp => &["patch_grass_normal", "patch_dead_bush"],
        B::Desert => &[
            "flower_default", "patch_grass_badlands", "patch_dead_bush_2",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_sugar_cane_desert", "patch_pumpkin", "patch_cactus_desert",
        ],
        // The addDefaultFlowers + badlands-grass set shared by the
        // plain cold / shore biomes.
        B::SnowyPlains | B::IceSpikes | B::WindsweptHills
        | B::Beach | B::SnowyBeach | B::River | B::FrozenRiver => &[
            "flower_default", "patch_grass_badlands",
            "brown_mushroom_normal", "red_mushroom_normal",
            "patch_pumpkin", "patch_sugar_cane",
        ],
        // snowy_slopes: pumpkins are its only hosted vegetal entry.
        B::SnowySlopes => &["patch_pumpkin"],
        B::Ocean | B::DeepOcean | B::ColdOcean | B::DeepColdOcean
        | B::FrozenOcean | B::DeepFrozenOcean | B::LukewarmOcean
        | B::DeepLukewarmOcean | B::WarmOcean => &[],
    }
}

/// MC `Biome.BIOME_INFO_NOISE` stand-in (shared with features.rs's
/// kelp count - same constant 2345 seed class).
fn biome_info_noise() -> &'static NormalNoise {
    static NOISE: OnceLock<NormalNoise> = OnceLock::new();
    NOISE.get_or_init(|| {
        let params = NoiseParams { first_octave: 0, amplitudes: vec![1.0] };
        NormalNoise::new(2345, &params)
    })
}

/// MC `Biome.TEMPERATURE_NOISE` stand-in (constant seed 1234).
fn temperature_noise() -> &'static NormalNoise {
    static NOISE: OnceLock<NormalNoise> = OnceLock::new();
    NOISE.get_or_init(|| {
        let params = NoiseParams { first_octave: 0, amplitudes: vec![1.0] };
        NormalNoise::new(1234, &params)
    })
}

/// Base biome temperature from the extracted biome JSONs, keyed by the
/// MC registry name (W211: every [`McBiome`] has its own row).
pub(crate) fn base_temperature(mc: McBiome) -> f32 {
    static TEMPS: OnceLock<std::collections::HashMap<String, f32>> = OnceLock::new();
    let temps = TEMPS.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(BIOME_TEMPERATURES).expect("biome temps parse");
        v.as_object()
            .expect("temps object")
            .iter()
            .map(|(k, t)| (k.clone(), t.as_f64().expect("temp") as f32))
            .collect()
    });
    temps[mc.name()]
}

/// `Biome.getHeightAdjustedTemperature`, ported exactly: above
/// seaLevel+17 the temperature falls with altitude plus a small noise
/// wobble; snow forms below 0.15.
pub(crate) fn height_adjusted_temperature(mc: McBiome, wx: i32, wy: i32, wz: i32) -> f32 {
    let base = base_temperature(mc);
    let snow_level = SEA_LEVEL + 17;
    if wy > snow_level {
        let wobble =
            (temperature_noise().value(wx as f64 / 8.0, 0.0, wz as f64 / 8.0) * 8.0) as f32;
        base - (wobble + wy as f32 - snow_level as f32) * 0.05 / 40.0
    } else {
        base
    }
}

fn cold_enough_to_snow(mc: McBiome, wx: i32, wy: i32, wz: i32) -> bool {
    height_adjusted_temperature(mc, wx, wy, wz) < 0.15
}

/// W221: the surface rule's `temperature` condition - the same real
/// coldEnoughToSnow model, exported for the terrain fill.
pub(crate) fn cold_enough_to_snow_at(mc: McBiome, wx: i32, wy: i32, wz: i32) -> bool {
    cold_enough_to_snow(mc, wx, wy, wz)
}

/// Is `id` a solid-ish floor a snow layer may rest on (MC: sturdy top
/// face - excludes air, liquids and the passable plants).
fn snow_supporting(id: u16) -> bool {
    if id == 0 || crate::block::block_is_watery(id) {
        return false;
    }
    let b = BlockId(id);
    !crate::block::block_is_aabb_passable(b)
        && !crate::block::block_skips_chunk_mesh(b)
}

fn dirt_floor(id: u16) -> bool {
    if id == 2 || id == 3 {
        return true;
    }
    crate::block::block_type(id).is_some_and(|bt| {
        matches!(
            bt.name,
            "podzol" | "coarse_dirt" | "mycelium" | "rooted_dirt" | "mud" | "moss_block"
                | "snow_block"
        )
    })
}

/// W218: sand-family floors (cactus / dead bush / sugar cane accept).
fn sand_floor(id: u16) -> bool {
    crate::block::block_type(id).is_some_and(|bt| matches!(bt.name, "sand" | "red_sand"))
}

/// Write one plant at `pos` (base cell) if the target(s) are air and
/// the floor supports it per `floor`. Chunk-local writes only.
#[allow(clippy::too_many_arguments)]
fn place_plant(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    plant: Plant,
    wx: i32,
    wy: i32,
    wz: i32,
    floor: FloorRule,
    rng: &mut SplitMix64,
) {
    let cs = CHUNK_SIZE as i32;
    let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
    if !(0..cs).contains(&lx) || !(0..cs).contains(&lz) {
        return;
    }
    // Floor check: prefer the in-chunk cell; at the vertical seam fall
    // back to "the terrain floor is directly below" via the heightmap
    // (dirt-family rule only - the specialised rules need real cells).
    let floor_ok = if (1..cs).contains(&ly) {
        let below = chunk.get(lx as usize, (ly - 1) as usize, lz as usize).0;
        match floor {
            FloorRule::DirtFamily => dirt_floor(below),
            FloorRule::GrassBlock => below == 3,
            FloorRule::DeadBush => sand_floor(below) || dirt_floor(below),
            FloorRule::SugarCane => {
                // Dirt/sand-family floor + water horizontally adjacent
                // to the FLOOR cell (SugarCaneBlock.canSurvive).
                // Chunk-local neighbour reads - an off-chunk water
                // neighbour is missed (border canes thin out; honest).
                (dirt_floor(below) || sand_floor(below)) && {
                    let fy = (ly - 1) as usize;
                    let mut wet = false;
                    for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (lx + dx, lz + dz);
                        if (0..cs).contains(&nx)
                            && (0..cs).contains(&nz)
                            && crate::block::block_is_watery(
                                chunk.get(nx as usize, fy, nz as usize).0,
                            )
                        {
                            wet = true;
                            break;
                        }
                    }
                    wet
                }
            }
            FloorRule::Cactus => {
                // Sand floor + no solid horizontal neighbour at the
                // base cell (CactusBlock.canSurvive).
                sand_floor(below) && {
                    let mut clear = true;
                    for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (lx + dx, lz + dz);
                        if (0..cs).contains(&nx) && (0..cs).contains(&nz) {
                            let n = chunk.get(nx as usize, ly as usize, nz as usize).0;
                            if n != 0
                                && !crate::block::block_is_aabb_passable(BlockId(n))
                                && !crate::block::block_is_watery(n)
                            {
                                clear = false;
                                break;
                            }
                        }
                    }
                    clear
                }
            }
        }
    } else {
        mc_floor_at(ctx, wx, wz) == wy - 1 && floor == FloorRule::DirtFamily
    };
    if !floor_ok {
        return;
    }
    let mut set_if_air = |y: i32, id: u16| -> bool {
        let ly = y - origin[1];
        if !(0..cs).contains(&ly) {
            // Out of this vertical slice: report success so the pair
            // logic still writes the half that IS local.
            return true;
        }
        if chunk.get(lx as usize, ly as usize, lz as usize).is_air() {
            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
            true
        } else {
            false
        }
    };
    match plant {
        Plant::Single(id) => {
            set_if_air(wy, id);
        }
        Plant::Double(bottom, top) => {
            if set_if_air(wy, bottom) {
                set_if_air(wy + 1, top);
            }
        }
        Plant::Column { id, min_h, max_h } => {
            // BiasedToBottomInt.sample: min + rnd(rnd(max - min + 1) + 1).
            let span = (max_h - min_h + 1).max(1) as u64;
            let outer = (rng.next_u64() % span) as i32 + 1;
            let h = min_h + (rng.next_u64() % outer as u64) as i32;
            for i in 0..h {
                if !set_if_air(wy + i, id) {
                    break;
                }
            }
        }
    }
}

/// Run one patch's placement pipeline for source chunk `(scx, scz)` and
/// write the cells that land in `chunk`.
#[allow(clippy::too_many_arguments)]
fn run_patch(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    def: &PatchDef,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
) {
    let cs = CHUNK_SIZE as i32;
    let base = [scx * cs, 0, scz * cs];
    let mut positions: Vec<[i32; 3]> = vec![base];
    let trap_sample = |rng: &mut SplitMix64, (min, max): (i32, i32)| -> i32 {
        if max <= min {
            return min;
        }
        let span = (max - min + 1) as u64;
        let a = (rng.next_u64() % span) as i32;
        let b = (rng.next_u64() % span) as i32;
        min + (a + b) / 2
    };
    for step in &def.steps {
        match step {
            Step::Count(n) => {
                let mut next = Vec::with_capacity(positions.len() * *n as usize);
                for p in positions {
                    for _ in 0..*n {
                        next.push(p);
                    }
                }
                positions = next;
            }
            Step::NoiseCount { level, below, above } => {
                let mut next = Vec::new();
                for p in positions {
                    let n = biome_info_noise().value(p[0] as f64 / 200.0, 0.0, p[2] as f64 / 200.0);
                    let count = if n < *level { *below } else { *above };
                    for _ in 0..count {
                        next.push(p);
                    }
                }
                positions = next;
            }
            Step::Rarity(chance) => {
                positions.retain(|_| rng.next_u64().is_multiple_of((*chance).max(1) as u64));
            }
            Step::InSquare => {
                for p in &mut positions {
                    p[0] += (rng.next_u64() % 16) as i32;
                    p[2] += (rng.next_u64() % 16) as i32;
                }
            }
            Step::Heightmap => {
                for p in &mut positions {
                    p[1] = mc_floor_at(ctx, p[0], p[2]) + 1;
                }
            }
            Step::RandomOffset { xz, y } => {
                for p in &mut positions {
                    p[0] += trap_sample(rng, *xz);
                    p[2] += trap_sample(rng, *xz);
                    p[1] += trap_sample(rng, *y);
                }
            }
        }
    }
    for p in positions {
        let plant = def.provider.pick(rng, p[0], p[1], p[2]);
        place_plant(chunk, origin, ctx, plant, p[0], p[1], p[2], def.floor, rng);
    }
}

/// Per-chunk vegetation pass (Mc profile; called from
/// `features::place_from_coord`). 3x3 source replay covers the +-7
/// trapezoid scatter across chunk borders.
pub(crate) fn place_vegetation(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            let mut rng = SplitMix64::new(stream_seed(ctx.effective_seed(), scx, scz, SALT_VEGETATION));
            for name in patches_for(mc) {
                let Some(def) = patch(name) else { continue };
                run_patch(chunk, origin, ctx, def, &mut rng, scx, scz);
            }
        }
    }
}

/// freeze_top_layer (SnowAndFreezeFeature): per own-chunk column, ice
/// on exposed sea-level water + a snow layer on the exposed top when
/// cold enough. Chunk-local; runs last so canopies collect snow too.
pub(crate) fn place_freeze_top_layer(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    const SNOW_LAYER: u16 = 15;
    const ICE: u16 = crate::block::ICE_ID;
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        coord[0],
        coord[2],
        SALT_FREEZE,
    ));
    let _ = &mut rng; // the feature is deterministic; kept for parity hooks
    for lx in 0..cs {
        for lz in 0..cs {
            let wx = origin[0] + lx;
            let wz = origin[2] + lz;
            let mc = mc_full_biome_at(ctx, wx, wz);
            // Ice: the sea-surface cell freezes when cold (MC
            // shouldFreeze on the water below the motion-blocking top).
            let sea_ly = SEA_LEVEL - origin[1];
            if (0..cs).contains(&sea_ly)
                && chunk.get(lx as usize, sea_ly as usize, lz as usize).0 == crate::block::WATER_ID
                && (sea_ly + 1 >= cs
                    || chunk.get(lx as usize, (sea_ly + 1) as usize, lz as usize).is_air())
                && cold_enough_to_snow(mc, wx, SEA_LEVEL, wz)
            {
                chunk.set(lx as usize, sea_ly as usize, lz as usize, BlockId(ICE));
                continue;
            }
            // Snow layer on the exposed top (terrain or canopy).
            let mut top: Option<i32> = None;
            for ly in (0..cs).rev() {
                if !chunk.get(lx as usize, ly as usize, lz as usize).is_air() {
                    top = Some(ly);
                    break;
                }
            }
            let Some(top_ly) = top else { continue };
            let snow_ly = top_ly + 1;
            if snow_ly >= cs {
                continue; // the seam case - the chunk above owns it
            }
            let below = chunk.get(lx as usize, top_ly as usize, lz as usize).0;
            let wy = origin[1] + snow_ly;
            if snow_supporting(below)
                && cold_enough_to_snow(mc, wx, wy, wz)
                && chunk.get(lx as usize, snow_ly as usize, lz as usize).is_air()
            {
                chunk.set(lx as usize, snow_ly as usize, lz as usize, BlockId(SNOW_LAYER));
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
    fn patches_parse_the_embedded_mc_json() {
        // Pins from the 26.1.2 data.
        let plains = patch("patch_grass_plain").expect("plains grass parses");
        assert!(plains.steps.iter().any(|s| matches!(
            s,
            Step::NoiseCount { below: 5, above: 10, .. }
        )));
        assert!(plains.steps.iter().any(|s| matches!(s, Step::Count(32))));
        assert!(matches!(plains.provider, Provider::Simple(Plant::Single(id)) if id == 651));
        let flowers = patch("flower_plains").expect("plains flowers parse");
        assert!(flowers.steps.iter().any(|s| matches!(s, Step::Rarity(32))));
        assert!(matches!(flowers.provider, Provider::NoiseThreshold { .. }));
        let tall = patch("patch_tall_grass_2").expect("tall grass parses");
        assert!(matches!(tall.provider, Provider::Simple(Plant::Double(758, 759))));
        let berry = patch("patch_berry_common").expect("berry parses");
        assert_eq!(berry.floor, FloorRule::GrassBlock);
        // W218: block_column features + the survival floor rules.
        let cane = patch("patch_sugar_cane").expect("sugar cane parses");
        assert_eq!(cane.floor, FloorRule::SugarCane);
        assert!(matches!(
            cane.provider,
            Provider::Simple(Plant::Column { min_h: 2, max_h: 4, .. })
        ));
        let cactus = patch("patch_cactus_desert").expect("cactus parses");
        assert_eq!(cactus.floor, FloorRule::Cactus);
        assert!(matches!(
            cactus.provider,
            Provider::Simple(Plant::Column { min_h: 1, max_h: 3, .. })
        ));
        let bush = patch("patch_dead_bush_2").expect("dead bush parses");
        assert_eq!(bush.floor, FloorRule::DeadBush);
        let pumpkin = patch("patch_pumpkin").expect("pumpkin parses");
        assert_eq!(pumpkin.floor, FloorRule::GrassBlock);
        assert!(pumpkin.steps.iter().any(|s| matches!(s, Step::Rarity(300))));
        // W211: flower_forest's noise_provider - all 11 flowers resolve
        // and the noise scale is the JSON's 1/48, not a global constant.
        let ff = patch("flower_flower_forest").expect("flower forest parses");
        match &ff.provider {
            Provider::Noise { states, noise } => {
                assert_eq!(states.len(), 11);
                assert!((noise.scale - 0.020833334).abs() < 1e-9);
            }
            _ => panic!("flower_flower_forest should parse as noise_provider"),
        }
        // flower_plain's threshold provider carries its own 0.005 scale.
        match &flowers.provider {
            Provider::NoiseThreshold { noise, .. } => {
                assert!((noise.scale - 0.005).abs() < 1e-9);
            }
            _ => panic!("flower_plains should parse as noise_threshold_provider"),
        }
    }

    #[test]
    fn temperature_model_matches_mc_shape() {
        // Base temps come straight from the biome JSONs.
        assert!((base_temperature(McBiome::Plains) - 0.8).abs() < 1e-6);
        assert!((base_temperature(McBiome::SnowyPlains) - 0.0).abs() < 1e-6);
        // At sea level: plains never snow, snowy plains always do.
        assert!(!cold_enough_to_snow(McBiome::Plains, 0, SEA_LEVEL, 0));
        assert!(cold_enough_to_snow(McBiome::SnowyPlains, 0, SEA_LEVEL, 0));
        // Windswept hills (0.2): warm at sea level, snowy high up.
        assert!(!cold_enough_to_snow(McBiome::WindsweptHills, 0, SEA_LEVEL, 0));
        assert!(cold_enough_to_snow(McBiome::WindsweptHills, 0, SEA_LEVEL + 130, 0));
    }

    /// W211: every McBiome must have a temperature row + a patch list
    /// whose entries all parse (typo in either table = panic here, not
    /// silence in a world).
    #[test]
    fn all_mc_biomes_have_temps_and_valid_patch_lists() {
        for mc in McBiome::ALL {
            let _ = base_temperature(mc);
            for name in patches_for(mc) {
                assert!(patch(name).is_some(), "{}: patch {name} must parse", mc.name());
            }
        }
    }

    /// Evidence: grass tufts actually appear on plains-ish land chunks.
    #[test]
    fn land_chunks_grow_grass() {
        let c = ctx();
        let mut grass = 0u32;
        for cx in -16..16 {
            for cz in -16..16 {
                let wx = cx * 16 + 8;
                let wz = cz * 16 + 8;
                let floor = mc_floor_at(&c, wx, wz);
                if floor < SEA_LEVEL {
                    continue;
                }
                let cy = (floor + 1).div_euclid(16);
                let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                for x in 0..16 {
                    for y in 0..16 {
                        for z in 0..16 {
                            let id = chunk.get(x, y, z).0;
                            if id == 651 || id == 652 || id == 758 || id == 760 {
                                grass += 1;
                            }
                        }
                    }
                }
                if grass > 0 {
                    return;
                }
            }
        }
        panic!("no grass found on land");
    }
}
