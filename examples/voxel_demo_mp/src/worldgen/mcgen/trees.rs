//! W208 - MC trees (Phase 6c of the worldgen port).
//!
//! Data-driven from the REAL MC worldgen JSON (the per-biome vegetal
//! tree entries, extracted from the 26.1.2 jar into `data/feature/`),
//! with the placement algorithms ported from the decompiled source:
//!
//! - `TreeFeature.doPlace` roll order: treeHeight = trunk.getTreeHeight
//!   (base + rand(a+1) + rand(b+1)), foliageHeight, trunkHeight = tree -
//!   foliage, leafRadius, then trunk -> foliage attachments -> leaves.
//! - `StraightTrunkPlacer` / `ForkingTrunkPlacer` (acacia's lean +
//!   branch, ported line-for-line).
//! - `BlobFoliagePlacer` / `SpruceFoliagePlacer` / `PineFoliagePlacer` /
//!   `AcaciaFoliagePlacer` / `BushFoliagePlacer` `createFoliage` +
//!   `shouldSkipLocation` shapes.
//! - `random_selector` tree pick per try + the `count` weighted_list
//!   placement modifier (e.g. plains: 0 trees w19 / 1 tree w1 per chunk).
//!
//! W209 adds the remaining shapes from source: `FancyTrunkPlacer` +
//! `FancyFoliagePlacer` (large oaks - sphere-profile branch clusters,
//! circular discs), `MegaJungleTrunkPlacer` (2x2 giant trunk + spiral
//! side branches) + `MegaJungleFoliagePlacer`, and the
//! `FallenTreeFeature` (stump + horizontal log run).
//!
//! Honest divergences, consistent with the whole mcgen port: SplitMix64
//! streams (no seed identity); decorators (bees, leaf litter, trunk
//! vines, attached mushrooms) are cosmetic and skipped; fancy's
//! free-space pre-checks are skipped (they consume no randomness, so
//! the stream stays aligned - fancy oaks just never abort); the
//! `would_survive` sapling filter reduces to "floor is a dirt-family
//! block". Still unsupported (falls back to the selector default):
//! dark_oak/bending placers (no engine biome lists them yet).

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};
use std::sync::OnceLock;

use crate::worldgen::WorldgenCtx;
use crate::worldgen::climate::SEA_LEVEL;

use super::features::{block_id_by_name, stream_seed};
use super::noise::SplitMix64;
use super::terrain::mc_floor_at;

/// Per-feature random-stream salt.
const SALT_TREES: u64 = 0x7EE5_0000;

// The vegetal tree roots per biome + the whole placed/configured
// closure they reference (extracted from the jar).
const TREE_PLACED: &[(&str, &str)] = &[
    ("acacia_checked", include_str!("data/feature/placed/acacia_checked.json")),
    ("birch_bees_0002", include_str!("data/feature/placed/birch_bees_0002.json")),
    ("birch_bees_0002_leaf_litter", include_str!("data/feature/placed/birch_bees_0002_leaf_litter.json")),
    ("fallen_birch_tree", include_str!("data/feature/placed/fallen_birch_tree.json")),
    ("fallen_jungle_tree", include_str!("data/feature/placed/fallen_jungle_tree.json")),
    ("fallen_oak_tree", include_str!("data/feature/placed/fallen_oak_tree.json")),
    ("fallen_spruce_tree", include_str!("data/feature/placed/fallen_spruce_tree.json")),
    ("fancy_oak_bees_0002_leaf_litter", include_str!("data/feature/placed/fancy_oak_bees_0002_leaf_litter.json")),
    ("fancy_oak_checked", include_str!("data/feature/placed/fancy_oak_checked.json")),
    ("jungle_bush", include_str!("data/feature/placed/jungle_bush.json")),
    ("jungle_tree", include_str!("data/feature/placed/jungle_tree.json")),
    ("mega_jungle_tree_checked", include_str!("data/feature/placed/mega_jungle_tree_checked.json")),
    ("mega_spruce_checked", include_str!("data/feature/placed/mega_spruce_checked.json")),
    ("mega_pine_checked", include_str!("data/feature/placed/mega_pine_checked.json")),
    ("super_birch_bees_0002", include_str!("data/feature/placed/super_birch_bees_0002.json")),
    ("fallen_super_birch_tree", include_str!("data/feature/placed/fallen_super_birch_tree.json")),
    ("oak_bees_0002_leaf_litter", include_str!("data/feature/placed/oak_bees_0002_leaf_litter.json")),
    ("oak_checked", include_str!("data/feature/placed/oak_checked.json")),
    ("pine_checked", include_str!("data/feature/placed/pine_checked.json")),
    ("spruce_checked", include_str!("data/feature/placed/spruce_checked.json")),
    ("birch_bees_002", include_str!("data/feature/placed/birch_bees_002.json")),
    ("fancy_oak_bees_002", include_str!("data/feature/placed/fancy_oak_bees_002.json")),
    ("oak_bees_002", include_str!("data/feature/placed/oak_bees_002.json")),
    ("trees_birch", include_str!("data/feature/placed/trees_birch.json")),
    ("trees_birch_and_oak_leaf_litter", include_str!("data/feature/placed/trees_birch_and_oak_leaf_litter.json")),
    ("trees_flower_forest", include_str!("data/feature/placed/trees_flower_forest.json")),
    // W222 audit batch: old-growth taigas, tall birch, sparse jungle,
    // ocean/river bank oaks.
    ("trees_old_growth_spruce_taiga", include_str!("data/feature/placed/trees_old_growth_spruce_taiga.json")),
    ("trees_old_growth_pine_taiga", include_str!("data/feature/placed/trees_old_growth_pine_taiga.json")),
    ("birch_tall", include_str!("data/feature/placed/birch_tall.json")),
    ("trees_sparse_jungle", include_str!("data/feature/placed/trees_sparse_jungle.json")),
    ("trees_water", include_str!("data/feature/placed/trees_water.json")),
    ("trees_jungle", include_str!("data/feature/placed/trees_jungle.json")),
    ("trees_plains", include_str!("data/feature/placed/trees_plains.json")),
    ("trees_savanna", include_str!("data/feature/placed/trees_savanna.json")),
    ("trees_snowy", include_str!("data/feature/placed/trees_snowy.json")),
    ("trees_swamp", include_str!("data/feature/placed/trees_swamp.json")),
    ("trees_taiga", include_str!("data/feature/placed/trees_taiga.json")),
    ("trees_windswept_hills", include_str!("data/feature/placed/trees_windswept_hills.json")),
];

const TREE_CONFIGURED: &[(&str, &str)] = &[
    ("acacia", include_str!("data/feature/configured/acacia.json")),
    ("birch_bees_0002", include_str!("data/feature/configured/birch_bees_0002.json")),
    ("birch_bees_0002_leaf_litter", include_str!("data/feature/configured/birch_bees_0002_leaf_litter.json")),
    ("fallen_birch_tree", include_str!("data/feature/configured/fallen_birch_tree.json")),
    ("fallen_jungle_tree", include_str!("data/feature/configured/fallen_jungle_tree.json")),
    ("fallen_oak_tree", include_str!("data/feature/configured/fallen_oak_tree.json")),
    ("fallen_spruce_tree", include_str!("data/feature/configured/fallen_spruce_tree.json")),
    ("fancy_oak", include_str!("data/feature/configured/fancy_oak.json")),
    ("fancy_oak_bees_0002_leaf_litter", include_str!("data/feature/configured/fancy_oak_bees_0002_leaf_litter.json")),
    ("fancy_oak_bees_005", include_str!("data/feature/configured/fancy_oak_bees_005.json")),
    ("jungle_bush", include_str!("data/feature/configured/jungle_bush.json")),
    ("jungle_tree", include_str!("data/feature/configured/jungle_tree.json")),
    ("mega_jungle_tree", include_str!("data/feature/configured/mega_jungle_tree.json")),
    ("oak", include_str!("data/feature/configured/oak.json")),
    ("oak_bees_0002_leaf_litter", include_str!("data/feature/configured/oak_bees_0002_leaf_litter.json")),
    ("oak_bees_005", include_str!("data/feature/configured/oak_bees_005.json")),
    ("pine", include_str!("data/feature/configured/pine.json")),
    ("spruce", include_str!("data/feature/configured/spruce.json")),
    ("swamp_oak", include_str!("data/feature/configured/swamp_oak.json")),
    ("birch_bees_002", include_str!("data/feature/configured/birch_bees_002.json")),
    ("fancy_oak_bees_002", include_str!("data/feature/configured/fancy_oak_bees_002.json")),
    ("oak_bees_002", include_str!("data/feature/configured/oak_bees_002.json")),
    ("trees_birch", include_str!("data/feature/configured/trees_birch.json")),
    ("trees_birch_and_oak_leaf_litter", include_str!("data/feature/configured/trees_birch_and_oak_leaf_litter.json")),
    ("trees_flower_forest", include_str!("data/feature/configured/trees_flower_forest.json")),
    // W222 audit batch.
    ("trees_old_growth_spruce_taiga", include_str!("data/feature/configured/trees_old_growth_spruce_taiga.json")),
    ("trees_old_growth_pine_taiga", include_str!("data/feature/configured/trees_old_growth_pine_taiga.json")),
    ("birch_tall", include_str!("data/feature/configured/birch_tall.json")),
    ("trees_sparse_jungle", include_str!("data/feature/configured/trees_sparse_jungle.json")),
    ("trees_water", include_str!("data/feature/configured/trees_water.json")),
    ("mega_spruce", include_str!("data/feature/configured/mega_spruce.json")),
    ("mega_pine", include_str!("data/feature/configured/mega_pine.json")),
    ("super_birch_bees_0002", include_str!("data/feature/configured/super_birch_bees_0002.json")),
    ("fallen_super_birch_tree", include_str!("data/feature/configured/fallen_super_birch_tree.json")),
    ("trees_jungle", include_str!("data/feature/configured/trees_jungle.json")),
    ("trees_plains", include_str!("data/feature/configured/trees_plains.json")),
    ("trees_savanna", include_str!("data/feature/configured/trees_savanna.json")),
    ("trees_snowy", include_str!("data/feature/configured/trees_snowy.json")),
    ("trees_taiga", include_str!("data/feature/configured/trees_taiga.json")),
    ("trees_windswept_hills", include_str!("data/feature/configured/trees_windswept_hills.json")),
];

/// MC IntProvider: constant or uniform.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntProv {
    min: i32,
    max: i32,
}

impl IntProv {
    fn parse(v: &serde_json::Value) -> IntProv {
        if let Some(n) = v.as_i64() {
            return IntProv { min: n as i32, max: n as i32 };
        }
        match v["type"].as_str() {
            Some("minecraft:uniform") => IntProv {
                min: v["min_inclusive"].as_i64().expect("min") as i32,
                max: v["max_inclusive"].as_i64().expect("max") as i32,
            },
            Some("minecraft:constant") => {
                let n = v["value"].as_i64().expect("value") as i32;
                IntProv { min: n, max: n }
            }
            other => panic!("unsupported IntProvider {other:?}"),
        }
    }
    fn sample(self, rng: &mut SplitMix64) -> i32 {
        if self.max <= self.min {
            return self.min;
        }
        self.min + (rng.next_u64() % (self.max - self.min + 1) as u64) as i32
    }
}

#[derive(Clone, Copy, Debug)]
enum Trunk {
    /// StraightTrunkPlacer.
    Straight,
    /// ForkingTrunkPlacer (acacia).
    Forking,
    /// FancyTrunkPlacer (large oaks): 0.618 trunk + sphere-shaped
    /// branch clusters, W209.
    Fancy,
    /// MegaJungleTrunkPlacer: 2x2 giant trunk + spiral side branches,
    /// W209.
    MegaJungle,
    /// W222: GiantTrunkPlacer - the plain 2x2 column (mega spruce /
    /// pine), no side branches.
    Giant,
}

#[derive(Clone, Debug)]
enum Foliage {
    Blob { height: i32 },
    Spruce { trunk_height: IntProv },
    Pine { height: IntProv },
    Acacia,
    Bush { height: i32 },
    /// FancyFoliagePlacer: circular discs, +1 radius on middle rows.
    Fancy { height: i32 },
    /// MegaJungleFoliagePlacer ("jungle_foliage_placer").
    MegaJungle { height: i32 },
    /// W222: MegaPineFoliagePlacer - the old-growth taiga crown
    /// (radius widens 3.5 blocks over the crown, +1 jag on even
    /// world-Y rows).
    MegaPine { crown: IntProv },
}

/// One fully-parsed MC tree configured_feature.
#[derive(Clone, Debug)]
pub(crate) struct TreeDef {
    pub(crate) name: &'static str,
    trunk: Trunk,
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
    foliage: Foliage,
    radius: IntProv,
    offset: IntProv,
    log: u16,
    leaves: u16,
}

/// W209: the fallen-tree feature (stump + a horizontal log run).
#[derive(Clone, Debug)]
pub(crate) struct FallenDef {
    log: u16,
    log_length: IntProv,
}

/// A selector entry / default resolves to a tree, a fallen trunk, or
/// "unsupported" (feature types not ported).
#[derive(Clone, Debug)]
enum TreeRef {
    Tree(TreeDef),
    Fallen(FallenDef),
    Unsupported(&'static str),
}

/// The per-biome tree placed_feature: the count distribution
/// (weighted_list of tries-per-chunk) + the tree selector.
pub(crate) struct BiomeTrees {
    pub(crate) name: &'static str,
    /// (weight, count) rows of the `count` weighted_list.
    count_dist: Vec<(u32, u32)>,
    entries: Vec<(f32, TreeRef)>,
    default: TreeRef,
}

fn lookup(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table.iter().find(|(n, _)| *n == name).map(|(_, j)| *j)
}

fn strip(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn parse_tree_conf(name: &'static str) -> TreeRef {
    let Some(json) = lookup(TREE_CONFIGURED, name) else {
        return TreeRef::Unsupported(name);
    };
    let cj: serde_json::Value = serde_json::from_str(json).expect("tree conf parses");
    // W209: fallen trees are their own feature type - a stump plus a
    // horizontal log run (decorators = cosmetics, skipped).
    if cj["type"].as_str() == Some("minecraft:fallen_tree") {
        let cfg = &cj["config"];
        let log_name = cfg["trunk_provider"]["state"]["Name"].as_str().expect("fallen trunk");
        let Some(log) = block_id_by_name(log_name) else {
            return TreeRef::Unsupported(name);
        };
        return TreeRef::Fallen(FallenDef {
            log,
            log_length: IntProv::parse(&cfg["log_length"]),
        });
    }
    if cj["type"].as_str() != Some("minecraft:tree") {
        return TreeRef::Unsupported(name);
    }
    let cfg = &cj["config"];
    let tp = &cfg["trunk_placer"];
    let trunk = match tp["type"].as_str() {
        Some("minecraft:straight_trunk_placer") => Trunk::Straight,
        Some("minecraft:forking_trunk_placer") => Trunk::Forking,
        Some("minecraft:fancy_trunk_placer") => Trunk::Fancy,
        Some("minecraft:mega_jungle_trunk_placer") => Trunk::MegaJungle,
        Some("minecraft:giant_trunk_placer") => Trunk::Giant,
        _ => return TreeRef::Unsupported(name), // dark_oak / bending / ...
    };
    let fp = &cfg["foliage_placer"];
    let foliage = match fp["type"].as_str() {
        Some("minecraft:blob_foliage_placer") => Foliage::Blob {
            height: fp["height"].as_i64().expect("blob height") as i32,
        },
        Some("minecraft:spruce_foliage_placer") => Foliage::Spruce {
            trunk_height: IntProv::parse(&fp["trunk_height"]),
        },
        Some("minecraft:pine_foliage_placer") => Foliage::Pine {
            height: IntProv::parse(&fp["height"]),
        },
        Some("minecraft:acacia_foliage_placer") => Foliage::Acacia,
        Some("minecraft:bush_foliage_placer") => Foliage::Bush {
            height: fp["height"].as_i64().unwrap_or(2) as i32,
        },
        Some("minecraft:fancy_foliage_placer") => Foliage::Fancy {
            height: fp["height"].as_i64().expect("fancy height") as i32,
        },
        Some("minecraft:jungle_foliage_placer") => Foliage::MegaJungle {
            height: fp["height"].as_i64().expect("jungle height") as i32,
        },
        Some("minecraft:mega_pine_foliage_placer") => Foliage::MegaPine {
            crown: IntProv::parse(&fp["crown_height"]),
        },
        _ => return TreeRef::Unsupported(name),
    };
    let log_name = cfg["trunk_provider"]["state"]["Name"].as_str().expect("trunk block");
    let leaves_name = cfg["foliage_provider"]["state"]["Name"].as_str().expect("foliage block");
    let (Some(log), Some(leaves)) =
        (block_id_by_name(log_name), block_id_by_name(leaves_name))
    else {
        tracing::warn!(name, log_name, leaves_name, "W208: tree blocks missing in registry");
        return TreeRef::Unsupported(name);
    };
    TreeRef::Tree(TreeDef {
        name,
        trunk,
        base_height: tp["base_height"].as_i64().expect("base_height") as i32,
        height_rand_a: tp["height_rand_a"].as_i64().unwrap_or(0) as i32,
        height_rand_b: tp["height_rand_b"].as_i64().unwrap_or(0) as i32,
        foliage,
        radius: IntProv::parse(&fp["radius"]),
        offset: IntProv::parse(&fp["offset"]),
        log,
        leaves,
    })
}

/// Resolve a selector entry: a string is a PLACED feature reference
/// (follow it to its configured feature); an object is an inline
/// placed wrapper around a CONFIGURED reference.
fn parse_entry(v: &serde_json::Value) -> TreeRef {
    let conf_name: &'static str = if let Some(s) = v.as_str() {
        // Placed reference -> read its `feature` field.
        let placed_name = strip(s);
        let Some(pjson) = lookup(TREE_PLACED, placed_name) else {
            return TreeRef::Unsupported("missing-placed");
        };
        let pj: serde_json::Value = serde_json::from_str(pjson).expect("placed parses");
        let f = pj["feature"].as_str().expect("feature ref").to_owned();
        // Leak-free static lookup: find the table key matching the name.
        match TREE_CONFIGURED.iter().find(|(n, _)| *n == strip(&f)) {
            Some((n, _)) => n,
            None => return TreeRef::Unsupported("missing-configured"),
        }
    } else {
        let f = v["feature"].as_str().expect("inline feature ref");
        match TREE_CONFIGURED.iter().find(|(n, _)| *n == strip(f)) {
            Some((n, _)) => n,
            None => return TreeRef::Unsupported("missing-configured"),
        }
    };
    parse_tree_conf(conf_name)
}

fn parse_biome_trees(root: &'static str) -> BiomeTrees {
    let pjson = lookup(TREE_PLACED, root).expect("root placed embedded");
    let pj: serde_json::Value = serde_json::from_str(pjson).expect("root parses");
    // `count`: literal or weighted_list of counts.
    let mut count_dist: Vec<(u32, u32)> = vec![(1, 1)];
    for m in pj["placement"].as_array().expect("placement") {
        if m["type"].as_str() == Some("minecraft:count") {
            let c = &m["count"];
            if let Some(n) = c.as_u64() {
                count_dist = vec![(1, n as u32)];
            } else if c["type"].as_str() == Some("minecraft:weighted_list") {
                count_dist = c["distribution"]
                    .as_array()
                    .expect("distribution")
                    .iter()
                    .map(|e| {
                        (
                            e["weight"].as_u64().expect("weight") as u32,
                            e["data"].as_u64().expect("data") as u32,
                        )
                    })
                    .collect();
            }
        }
    }
    // The configured root: a random_selector, or directly a tree
    // (trees_swamp).
    let conf_name = strip(pj["feature"].as_str().expect("feature"));
    let conf_key = TREE_CONFIGURED
        .iter()
        .find(|(n, _)| *n == conf_name)
        .map(|(n, _)| *n)
        .expect("root configured embedded");
    let cjson = lookup(TREE_CONFIGURED, conf_key).unwrap();
    let cj: serde_json::Value = serde_json::from_str(cjson).expect("conf parses");
    let (entries, default) = if cj["type"].as_str() == Some("minecraft:random_selector") {
        let cfg = &cj["config"];
        let entries = cfg["features"]
            .as_array()
            .expect("selector features")
            .iter()
            .map(|e| {
                (
                    e["chance"].as_f64().expect("chance") as f32,
                    parse_entry(&e["feature"]),
                )
            })
            .collect();
        (entries, parse_entry(&cfg["default"]))
    } else {
        (Vec::new(), parse_tree_conf(conf_key))
    };
    BiomeTrees { name: root, count_dist, entries, default }
}

pub(crate) fn biome_trees(root: &'static str) -> &'static BiomeTrees {
    static CACHE: OnceLock<Vec<BiomeTrees>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        [
            "trees_plains",
            "trees_birch_and_oak_leaf_litter",
            "trees_taiga",
            "trees_snowy",
            "trees_windswept_hills",
            "trees_jungle",
            "trees_savanna",
            "trees_swamp",
            "trees_birch",
            "trees_flower_forest",
            "trees_old_growth_spruce_taiga",
            "trees_old_growth_pine_taiga",
            "birch_tall",
            "trees_sparse_jungle",
            "trees_water",
        ]
        .into_iter()
        .map(parse_biome_trees)
        .collect()
    });
    all.iter().find(|b| b.name == root).expect("known tree root")
}

/// W211/W222: the vegetal tree root per REAL MC biome. Remaining
/// honest fallbacks (unported blocks): dark forest (dark_oak placer)
/// uses the plain forest list, bamboo jungle (bamboo) the jungle list,
/// mangrove swamp the swamp oaks.
pub(crate) fn tree_root_for(mc: crate::worldgen::mcgen::biome::McBiome) -> Option<&'static str> {
    use crate::worldgen::mcgen::biome::McBiome as B;
    match mc {
        B::Plains | B::SunflowerPlains => Some("trees_plains"),
        B::Forest | B::DarkForest => Some("trees_birch_and_oak_leaf_litter"),
        B::FlowerForest => Some("trees_flower_forest"),
        B::BirchForest => Some("trees_birch"),
        // W222: tall super birches (with the fallen variants).
        B::OldGrowthBirchForest => Some("birch_tall"),
        B::Taiga | B::SnowyTaiga => Some("trees_taiga"),
        // W222: the real old-growth lists (mega spruce / mega pine).
        B::OldGrowthSpruceTaiga => Some("trees_old_growth_spruce_taiga"),
        B::OldGrowthPineTaiga => Some("trees_old_growth_pine_taiga"),
        B::SnowyPlains | B::IceSpikes | B::SnowySlopes => Some("trees_snowy"),
        B::WindsweptHills => Some("trees_windswept_hills"),
        B::Jungle | B::BambooJungle => Some("trees_jungle"),
        // W222: the real sparse-jungle list (bushes + fancy oaks).
        B::SparseJungle => Some("trees_sparse_jungle"),
        B::Savanna => Some("trees_savanna"),
        // Mangrove swamp: swamp oaks stand in until the mangrove placer
        // + roots/propagule blocks exist.
        B::Swamp | B::MangroveSwamp => Some("trees_swamp"),
        // W222: trees_water - the rare bank/island oaks the ocean +
        // river biomes list (surface_water_depth 0 keeps them on land).
        B::River
        | B::FrozenRiver
        | B::Ocean
        | B::DeepOcean
        | B::ColdOcean
        | B::DeepColdOcean
        | B::FrozenOcean
        | B::DeepFrozenOcean
        | B::LukewarmOcean
        | B::DeepLukewarmOcean
        | B::WarmOcean => Some("trees_water"),
        B::Desert | B::Beach | B::SnowyBeach => None,
    }
}

/// Sample the weighted-list count (tries per chunk).
fn sample_count(dist: &[(u32, u32)], rng: &mut SplitMix64) -> u32 {
    let total: u32 = dist.iter().map(|(w, _)| *w).sum();
    if total == 0 {
        return 0;
    }
    let mut roll = (rng.next_u64() % total as u64) as u32;
    for (w, data) in dist {
        if roll < *w {
            return *data;
        }
        roll -= w;
    }
    0
}

/// What one selector try resolved to.
enum Pick<'a> {
    Tree(&'a TreeDef),
    Fallen(&'a FallenDef),
}

fn as_pick(tref: &TreeRef) -> Option<Pick<'_>> {
    match tref {
        TreeRef::Tree(def) => Some(Pick::Tree(def)),
        TreeRef::Fallen(def) => Some(Pick::Fallen(def)),
        TreeRef::Unsupported(_) => None,
    }
}

/// `random_selector`: first entry whose chance roll hits wins, else the
/// default. An unsupported pick falls through to the default so the
/// tree density stays right even where a shape isn't ported yet.
fn select_tree<'a>(bt: &'a BiomeTrees, rng: &mut SplitMix64) -> Option<Pick<'a>> {
    for (chance, tref) in &bt.entries {
        if (rng.next_f64() as f32) < *chance {
            return as_pick(tref).or_else(|| as_pick(&bt.default));
        }
    }
    as_pick(&bt.default)
}

/// Dirt-family floor check (the `would_survive` oak-sapling predicate
/// reduced to `mayPlaceOn`: dirt-ish blocks).
fn floor_supports_tree(id: u16) -> bool {
    // grass_block(3) + dirt(2) are the legacy rows; the rest by name.
    if id == 2 || id == 3 {
        return true;
    }
    crate::block::block_type(id).is_some_and(|bt| {
        matches!(
            bt.name,
            "podzol" | "coarse_dirt" | "mycelium" | "rooted_dirt" | "mud" | "moss_block"
        )
    })
}

/// Leaf write: only into air / replaceable cells (never through logs
/// or terrain) - MC's TreeFeature.validTreePos.
fn set_leaf(chunk: &mut Chunk, origin: [i32; 3], wx: i32, wy: i32, wz: i32, id: u16) {
    let cs = CHUNK_SIZE as i32;
    let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
    if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
        return;
    }
    let here = chunk.get(lx as usize, ly as usize, lz as usize);
    if here.is_air() || crate::block::block_has_flag(here, crate::block::F_REPLACEABLE) {
        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
    }
}

/// Log write: overwrites air, replaceables AND leaves (the trunk goes
/// through the canopy, exactly MC).
fn set_log(chunk: &mut Chunk, origin: [i32; 3], wx: i32, wy: i32, wz: i32, id: u16) {
    let cs = CHUNK_SIZE as i32;
    let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
    if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
        return;
    }
    let here = chunk.get(lx as usize, ly as usize, lz as usize);
    let is_leaves = crate::block::block_type(here.0)
        .is_some_and(|bt| bt.name == "leaves" || bt.name.ends_with("_leaves"));
    if here.is_air()
        || is_leaves
        || crate::block::block_has_flag(here, crate::block::F_REPLACEABLE)
    {
        chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
    }
}

/// FoliagePlacer.placeLeavesRow: a square ring/disk of leaves at
/// `att + (dx, y, dz)`, with the per-placer corner-skip rule. A
/// double trunk (mega jungle 2x2) widens the scan by +1 and folds the
/// coordinate via min(|d|, |d-1|) - the shouldSkipLocationSigned rule.
#[allow(clippy::too_many_arguments)]
fn place_leaves_row(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    def: &TreeDef,
    att: [i32; 3],
    radius: i32,
    y: i32,
    double_trunk: bool,
) {
    let ext = if double_trunk { 1 } else { 0 };
    for dx in -radius..=radius + ext {
        for dz in -radius..=radius + ext {
            let (adx, adz) = if double_trunk {
                (dx.abs().min((dx - 1).abs()), dz.abs().min((dz - 1).abs()))
            } else {
                (dx.abs(), dz.abs())
            };
            // shouldSkipLocation per placer (signed wrapper passes the
            // folded values).
            let skip = match &def.foliage {
                Foliage::Blob { .. } => {
                    adx == radius
                        && adz == radius
                        && (rng.next_u64().is_multiple_of(2) || y == 0)
                }
                Foliage::Spruce { .. } | Foliage::Pine { .. } => {
                    adx == radius && adz == radius && radius > 0
                }
                Foliage::Acacia => {
                    if y == 0 {
                        (adx > 1 || adz > 1) && adx != 0 && adz != 0
                    } else {
                        adx == radius && adz == radius && radius > 0
                    }
                }
                Foliage::Bush { .. } => {
                    adx == radius && adz == radius && rng.next_u64().is_multiple_of(2)
                }
                // FancyFoliagePlacer: circular disc test on cell centres.
                Foliage::Fancy { .. } => {
                    let fx = adx as f32 + 0.5;
                    let fz = adz as f32 + 0.5;
                    fx * fx + fz * fz > (radius * radius) as f32
                }
                // MegaJungle + MegaPine share the circular + dx+dz >= 7
                // cut (both placers' shouldSkipLocation).
                Foliage::MegaJungle { .. } | Foliage::MegaPine { .. } => {
                    adx + adz >= 7 || adx * adx + adz * adz > radius * radius
                }
            };
            if !skip {
                set_leaf(chunk, origin, att[0] + dx, att[1] + y, att[2] + dz, def.leaves);
            }
        }
    }
}

/// Place one tree with base (trunk bottom) at `base`. Ported from
/// TreeFeature.doPlace + the trunk/foliage placers named in the module
/// docs. Writes only cells inside `chunk` (the 3x3 source replay covers
/// the spill into neighbours).
fn place_tree(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    def: &TreeDef,
    base: [i32; 3],
) {
    let tree_height = def.base_height
        + (rng.next_u64() % (def.height_rand_a + 1) as u64) as i32
        + (rng.next_u64() % (def.height_rand_b + 1) as u64) as i32;
    let foliage_height = match &def.foliage {
        Foliage::Blob { height }
        | Foliage::Bush { height }
        | Foliage::Fancy { height }
        | Foliage::MegaJungle { height } => *height,
        Foliage::Spruce { trunk_height } => {
            (tree_height - trunk_height.sample(rng)).max(4)
        }
        Foliage::Pine { height } => height.sample(rng),
        Foliage::MegaPine { crown } => crown.sample(rng),
        Foliage::Acacia => 0,
    };
    let trunk_height = tree_height - foliage_height;
    let mut leaf_radius = def.radius.sample(rng);
    if matches!(def.foliage, Foliage::Pine { .. }) {
        // PineFoliagePlacer.foliageRadius: + nextInt(max(trunkHeight+1, 1)).
        leaf_radius += (rng.next_u64() % trunk_height.max(0).max(0).wrapping_add(1).max(1) as u64) as i32;
    }
    let offset = def.offset.sample(rng);

    // Trunk -> foliage attachments (pos, radius_offset, double_trunk).
    let mut attachments: Vec<([i32; 3], i32, bool)> = Vec::new();
    match def.trunk {
        Trunk::Straight => {
            for y in 0..tree_height {
                set_log(chunk, origin, base[0], base[1] + y, base[2], def.log);
            }
            attachments.push(([base[0], base[1] + tree_height, base[2]], 0, false));
        }
        Trunk::Fancy => {
            // FancyTrunkPlacer (large oak). height = treeHeight + 2;
            // trunk = 0.618 of it; branch clusters follow the sphere
            // profile treeShape(). The free-space pre-checks consume no
            // randomness in MC, so skipping them (chunk-local world)
            // keeps the stream aligned - fancy oaks just never abort.
            let height = tree_height + 2;
            let trunk_height = (height as f64 * 0.618) as i32;
            let trunk_top = base[1] + trunk_height;
            // (attachment, branch_base_y)
            let mut coords: Vec<([i32; 3], i32)> = Vec::new();
            coords.push(([base[0], base[1] + (height - 5), base[2]], trunk_top));
            let tree_shape = |y: i32| -> f32 {
                if (y as f32) < height as f32 * 0.3 {
                    return -1.0;
                }
                let radius = height as f32 / 2.0;
                let adjacent = radius - y as f32;
                if adjacent == 0.0 {
                    return radius * 0.5;
                }
                if adjacent.abs() >= radius {
                    return 0.0;
                }
                (radius * radius - adjacent * adjacent).sqrt() * 0.5
            };
            let mut rel_y = height - 5;
            while rel_y >= 0 {
                let shape = tree_shape(rel_y);
                if shape >= 0.0 {
                    // clustersPerY = min(1, floor(1.382 + (h/13)^2)) - the
                    // vanilla quirk makes this always exactly 1.
                    let radius = (shape as f64) * (rng.next_f64() + 0.328);
                    let angle = rng.next_f64() * 2.0 * std::f64::consts::PI;
                    let cx = (radius * angle.sin() + 0.5).floor() as i32;
                    let cz = (radius * angle.cos() + 0.5).floor() as i32;
                    let limb_start = [base[0] + cx, base[1] + rel_y - 1, base[2] + cz];
                    // branchHeight = start.y - horizontalDist * 0.381,
                    // clamped to the trunk top.
                    let dist = ((cx * cx + cz * cz) as f64).sqrt() * 0.381;
                    let branch_height = limb_start[1] as f64 - dist;
                    let branch_top = if branch_height > trunk_top as f64 {
                        trunk_top
                    } else {
                        branch_height as i32
                    };
                    coords.push(([limb_start[0], limb_start[1], limb_start[2]], branch_top));
                }
                rel_y -= 1;
            }
            // The vertical trunk.
            make_limb(chunk, origin, def.log, base, [base[0], base[1] + trunk_height, base[2]]);
            // Branches from the trunk out to each kept cluster.
            let trim = |branch_base: i32| branch_base - base[1] >= (height as f32 * 0.2) as i32;
            for (pos, branch_base) in &coords {
                let from = [base[0], *branch_base, base[2]];
                if from != *pos && trim(*branch_base) {
                    make_limb(chunk, origin, def.log, from, *pos);
                }
            }
            for (pos, branch_base) in coords {
                if trim(branch_base) {
                    attachments.push((pos, 0, false));
                }
            }
        }
        Trunk::MegaJungle => {
            // GiantTrunkPlacer: a 2x2 column (the +x/+z edge stops one
            // short of the top)...
            for y in 0..tree_height {
                set_log(chunk, origin, base[0], base[1] + y, base[2], def.log);
                if y < tree_height - 1 {
                    set_log(chunk, origin, base[0] + 1, base[1] + y, base[2], def.log);
                    set_log(chunk, origin, base[0] + 1, base[1] + y, base[2] + 1, def.log);
                    set_log(chunk, origin, base[0], base[1] + y, base[2] + 1, def.log);
                }
            }
            attachments.push(([base[0], base[1] + tree_height, base[2]], 0, true));
            // ...plus MegaJungleTrunkPlacer's spiralling side branches
            // down the top half, each with a radiusOffset of -2.
            let mut branch_h = tree_height - 2 - (rng.next_u64() % 4) as i32;
            while branch_h > tree_height / 2 {
                let angle = rng.next_f64() * 2.0 * std::f64::consts::PI;
                let mut bx = 0;
                let mut bz = 0;
                for b in 0..5 {
                    bx = (1.5 + angle.cos() * b as f64) as i32;
                    bz = (1.5 + angle.sin() * b as f64) as i32;
                    set_log(
                        chunk,
                        origin,
                        base[0] + bx,
                        base[1] + branch_h - 3 + b / 2,
                        base[2] + bz,
                        def.log,
                    );
                }
                attachments.push(([base[0] + bx, base[1] + branch_h, base[2] + bz], -2, false));
                branch_h -= 2 + (rng.next_u64() % 4) as i32;
            }
        }
        Trunk::Giant => {
            // W222: GiantTrunkPlacer - a plain 2x2 column, the three
            // extra corners stopping one short of the top; one crown
            // attachment (double trunk).
            for y in 0..tree_height {
                set_log(chunk, origin, base[0], base[1] + y, base[2], def.log);
                if y < tree_height - 1 {
                    set_log(chunk, origin, base[0] + 1, base[1] + y, base[2], def.log);
                    set_log(chunk, origin, base[0] + 1, base[1] + y, base[2] + 1, def.log);
                    set_log(chunk, origin, base[0], base[1] + y, base[2] + 1, def.log);
                }
            }
            attachments.push(([base[0], base[1] + tree_height, base[2]], 0, true));
        }
        Trunk::Forking => {
            // ForkingTrunkPlacer, line-for-line: a lean near the top +
            // an optional second branch.
            const DIRS: [[i32; 2]; 4] = [[0, -1], [0, 1], [-1, 0], [1, 0]];
            let lean_dir = DIRS[(rng.next_u64() % 4) as usize];
            let lean_height = tree_height - (rng.next_u64() % 4) as i32 - 1;
            let mut lean_steps = 3 - (rng.next_u64() % 3) as i32;
            let (mut tx, mut tz) = (base[0], base[2]);
            let mut end_y = None;
            for yo in 0..tree_height {
                let yy = base[1] + yo;
                if yo >= lean_height && lean_steps > 0 {
                    tx += lean_dir[0];
                    tz += lean_dir[1];
                    lean_steps -= 1;
                }
                set_log(chunk, origin, tx, yy, tz, def.log);
                end_y = Some(yy + 1);
            }
            if let Some(ey) = end_y {
                attachments.push(([tx, ey, tz], 1, false));
            }
            let (mut tx, mut tz) = (base[0], base[2]);
            let branch_dir = DIRS[(rng.next_u64() % 4) as usize];
            if branch_dir != lean_dir {
                let branch_start = lean_height - (rng.next_u64() % 2) as i32 - 1;
                let mut branch_steps = 1 + (rng.next_u64() % 3) as i32;
                let mut end_y = None;
                let mut yo = branch_start;
                while yo < tree_height && branch_steps > 0 {
                    if yo >= 1 {
                        let yy = base[1] + yo;
                        tx += branch_dir[0];
                        tz += branch_dir[1];
                        set_log(chunk, origin, tx, yy, tz, def.log);
                        end_y = Some(yy + 1);
                    }
                    yo += 1;
                    branch_steps -= 1;
                }
                if let Some(ey) = end_y {
                    attachments.push(([tx, ey, tz], 0, false));
                }
            }
        }
    }

    // Foliage per attachment (createFoliage per placer).
    for (att, rad_off, double_trunk) in attachments {
        match &def.foliage {
            Foliage::Blob { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = (leaf_radius + rad_off - 1 - yo / 2).max(0);
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    yo -= 1;
                }
            }
            Foliage::Spruce { .. } => {
                let mut r = (rng.next_u64() % 2) as i32;
                let mut max_r = 1;
                let mut min_r = 0;
                let mut yo = offset;
                while yo >= -foliage_height {
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    if r >= max_r {
                        r = min_r;
                        min_r = 1;
                        max_r = (max_r + 1).min(leaf_radius + rad_off);
                    } else {
                        r += 1;
                    }
                    yo -= 1;
                }
            }
            Foliage::Pine { .. } => {
                let mut r = 0;
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    if r >= 1 && yo == offset - foliage_height + 1 {
                        r -= 1;
                    } else if r < leaf_radius + rad_off {
                        r += 1;
                    }
                    yo -= 1;
                }
            }
            Foliage::Acacia => {
                let ap = [att[0], att[1] + offset, att[2]];
                place_leaves_row(chunk, origin, rng, def, ap, leaf_radius + rad_off, -1 - foliage_height, double_trunk);
                place_leaves_row(chunk, origin, rng, def, ap, leaf_radius - 1, -foliage_height, double_trunk);
                place_leaves_row(chunk, origin, rng, def, ap, leaf_radius + rad_off - 1, 0, double_trunk);
            }
            Foliage::Bush { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = leaf_radius + rad_off - 1 - yo;
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    yo -= 1;
                }
            }
            Foliage::Fancy { .. } => {
                // FancyFoliagePlacer: +1 radius on the middle rows.
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = leaf_radius
                        + if yo != offset && yo != offset - foliage_height { 1 } else { 0 };
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    yo -= 1;
                }
            }
            Foliage::MegaJungle { .. } => {
                // MegaJungleFoliagePlacer: the double-trunk crown uses
                // the full foliage height; side branches 1 + rand(2).
                let leaf_height = if double_trunk {
                    foliage_height
                } else {
                    1 + (rng.next_u64() % 2) as i32
                };
                let mut yo = offset;
                while yo >= offset - leaf_height {
                    let r = leaf_radius + rad_off + 1 - yo;
                    place_leaves_row(chunk, origin, rng, def, att, r, yo, double_trunk);
                    yo -= 1;
                }
            }
            Foliage::MegaPine { .. } => {
                // W222: MegaPineFoliagePlacer, ascending rows: radius
                // widens by floor(yo/height * 3.5) toward the bottom,
                // with a +1 jag when the radius repeats on an even
                // world-Y row.
                let mut prev = i32::MIN;
                let mut row_y = att[1] - foliage_height + offset;
                while row_y <= att[1] + offset {
                    let yo = att[1] - row_y;
                    let smooth = leaf_radius
                        + rad_off
                        + ((yo as f32 / foliage_height.max(1) as f32) * 3.5).floor() as i32;
                    let jagged = if yo > 0 && smooth == prev && row_y & 1 == 0 {
                        smooth + 1
                    } else {
                        smooth
                    };
                    place_leaves_row(
                        chunk,
                        origin,
                        rng,
                        def,
                        att,
                        jagged,
                        row_y - att[1],
                        double_trunk,
                    );
                    prev = smooth;
                    row_y += 1;
                }
            }
        }
    }
}

/// FancyTrunkPlacer.makeLimb (doPlace variant): logs along the max-axis
/// stepped line from `from` to `to` inclusive.
fn make_limb(
    chunk: &mut Chunk,
    origin: [i32; 3],
    log: u16,
    from: [i32; 3],
    to: [i32; 3],
) {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let steps = d[0].abs().max(d[1].abs()).max(d[2].abs());
    if steps == 0 {
        set_log(chunk, origin, from[0], from[1], from[2], log);
        return;
    }
    let (fx, fy, fz) = (
        d[0] as f32 / steps as f32,
        d[1] as f32 / steps as f32,
        d[2] as f32 / steps as f32,
    );
    for i in 0..=steps {
        set_log(
            chunk,
            origin,
            from[0] + (0.5 + i as f32 * fx).floor() as i32,
            from[1] + (0.5 + i as f32 * fy).floor() as i32,
            from[2] + (0.5 + i as f32 * fz).floor() as i32,
            log,
        );
    }
}

/// FallenTreeFeature: a stump at the origin plus a horizontal log run
/// a couple of blocks away (decorators skipped). The ground-follow and
/// full-run validity checks reduce to "each segment sits on/near the
/// floor at the START height" - MC aborts on uneven ground; we do too
/// (past a 1-block tolerance).
fn place_fallen(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    def: &FallenDef,
    base: [i32; 3],
) {
    // Stump.
    set_log(chunk, origin, base[0], base[1], base[2], def.log);
    const DIRS: [[i32; 2]; 4] = [[0, -1], [0, 1], [-1, 0], [1, 0]];
    let dir = DIRS[(rng.next_u64() % 4) as usize];
    let log_length = def.log_length.sample(rng) - 2;
    let gap = 2 + (rng.next_u64() % 2) as i32;
    let sx = base[0] + dir[0] * gap;
    let sz = base[2] + dir[1] * gap;
    let sy = mc_floor_at(ctx, sx, sz) + 1;
    // Reject a run over uneven ground (MC's gapInGround > 2 abort,
    // tightened to a straight-line fit).
    for i in 0..log_length {
        let (px, pz) = (sx + dir[0] * i, sz + dir[1] * i);
        if (mc_floor_at(ctx, px, pz) + 1 - sy).abs() > 1 {
            return;
        }
    }
    for i in 0..log_length {
        set_log(chunk, origin, sx + dir[0] * i, sy, sz + dir[1] * i, def.log);
    }
}

/// Per-chunk MC tree pass (Mc profile; called from
/// `features::place_from_coord`). Column-keyed streams + 3x3 source
/// replay, the same determinism scheme as the flora/ore passes -
/// canopies and leaning acacia trunks cross chunk borders seamlessly.
pub(crate) fn place_trees(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_TREES));
            // Biome sampled at the source-chunk centre picks the
            // vegetal list (MC picks per placed pos; centre keeps the
            // stream fixed-shape per column).
            let bx = scx * cs + 8;
            let bz = scz * cs + 8;
            let mc = super::terrain::mc_full_biome_at(ctx, bx, bz);
            let Some(root) = tree_root_for(mc) else {
                continue;
            };
            let bt = biome_trees(root);
            let tries = sample_count(&bt.count_dist, &mut rng);
            for _ in 0..tries {
                // in_square.
                let rx = (rng.next_u64() % 16) as i32;
                let rz = (rng.next_u64() % 16) as i32;
                let wx = scx * cs + rx;
                let wz = scz * cs + rz;
                let Some(pick) = select_tree(bt, &mut rng) else {
                    continue;
                };
                let floor = mc_floor_at(ctx, wx, wz);
                // surface_water_depth_filter(max 0): dry land only.
                if floor < SEA_LEVEL {
                    continue;
                }
                // would_survive(oak_sapling): dirt-family floor. The
                // floor block isn't in OUR chunk necessarily; derive it
                // from the surface rule the fill used - cheap proxy:
                // check the chunk cell when local, else accept (the
                // biome + height already filtered deserts/beaches).
                let (flx, fly, flz) = (wx - origin[0], floor - origin[1], wz - origin[2]);
                if (0..cs).contains(&flx)
                    && (0..cs).contains(&fly)
                    && (0..cs).contains(&flz)
                    && !floor_supports_tree(chunk.get(flx as usize, fly as usize, flz as usize).0)
                {
                    continue;
                }
                match pick {
                    Pick::Tree(def) => {
                        place_tree(chunk, origin, &mut rng, def, [wx, floor + 1, wz]);
                    }
                    Pick::Fallen(def) => {
                        place_fallen(chunk, origin, ctx, &mut rng, def, [wx, floor + 1, wz]);
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
    fn tree_tables_parse_the_embedded_mc_json() {
        let plains = biome_trees("trees_plains");
        // Pins from the 26.1.2 data: plains count weighted_list
        // {0: w19, 1: w1}; selector fancy chance 1/3 -> default oak_bees_005.
        assert_eq!(plains.count_dist, vec![(19, 0), (1, 1)]);
        assert!(matches!(plains.default, TreeRef::Tree(ref d) if d.name == "oak_bees_005"));
        let taiga = biome_trees("trees_taiga");
        // taiga: pine 1/3 else spruce; both must parse as real trees.
        assert!(taiga.entries.iter().any(|(c, t)| (*c - 0.33333334).abs() < 1e-6
            && matches!(t, TreeRef::Tree(d) if d.name == "pine")));
        assert!(matches!(taiga.default, TreeRef::Tree(ref d) if d.name == "spruce"));
        // savanna: acacia (forking trunk) parses.
        let sav = biome_trees("trees_savanna");
        assert!(sav.entries.iter().any(|(_, t)| matches!(t, TreeRef::Tree(d) if d.name == "acacia")));
    }

    #[test]
    fn tree_block_ids_resolve() {
        for (name, want_log) in [
            ("oak", "oak_log"),
            ("birch_bees_0002", "birch_log"),
            ("spruce", "spruce_log"),
            ("acacia", "acacia_log"),
            ("jungle_tree", "jungle_log"),
            // W209: the previously-unsupported shapes must parse now.
            ("fancy_oak", "oak_log"),
            ("fancy_oak_bees_005", "oak_log"),
            ("mega_jungle_tree", "jungle_log"),
        ] {
            match parse_tree_conf(TREE_CONFIGURED.iter().find(|(n, _)| *n == name).unwrap().0) {
                TreeRef::Tree(d) => {
                    assert_eq!(Some(d.log), block_id_by_name(want_log), "{name} log id");
                    assert!(d.leaves != 0, "{name} leaves id");
                }
                _ => panic!("{name} should parse as a tree"),
            }
        }
        // W209: fallen trees parse as their own feature kind.
        match parse_tree_conf("fallen_oak_tree") {
            TreeRef::Fallen(f) => {
                assert_eq!(Some(f.log), block_id_by_name("oak_log"));
                assert_eq!((f.log_length.min, f.log_length.max), (4, 7));
            }
            _ => panic!("fallen_oak_tree should parse as Fallen"),
        }
    }

    /// Evidence: land chunks in tree biomes actually grow logs + leaves.
    #[test]
    fn land_chunks_grow_trees() {
        let c = ctx();
        let mut logs = 0u32;
        let mut leaves = 0u32;
        for cx in -16..16 {
            for cz in -16..16 {
                let wx = cx * 16 + 8;
                let wz = cz * 16 + 8;
                let floor = mc_floor_at(&c, wx, wz);
                if floor < SEA_LEVEL {
                    continue;
                }
                let mc = crate::worldgen::mcgen::terrain::mc_full_biome_at(&c, wx, wz);
                if tree_root_for(mc).is_none() {
                    continue;
                }
                // The vertical chunk holding the surface + canopy.
                let cy = (floor + 4).div_euclid(16);
                let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                for x in 0..16 {
                    for y in 0..16 {
                        for z in 0..16 {
                            let id = chunk.get(x, y, z).0;
                            let Some(bt) = crate::block::block_type(id) else { continue };
                            if bt.name == "wood" || bt.name.ends_with("_log") {
                                logs += 1;
                            } else if bt.name == "leaves" || bt.name.ends_with("_leaves") {
                                leaves += 1;
                            }
                        }
                    }
                }
                if logs > 0 && leaves > 0 {
                    return;
                }
            }
        }
        panic!("no trees found: logs={logs} leaves={leaves}");
    }
}
