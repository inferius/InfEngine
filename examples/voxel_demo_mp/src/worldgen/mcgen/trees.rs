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
    // W224: the dark forest canopy + the leaf-litter tree wrappers.
    ("dark_forest_vegetation", include_str!("data/feature/placed/dark_forest_vegetation.json")),
    ("dark_oak_leaf_litter", include_str!("data/feature/placed/dark_oak_leaf_litter.json")),
    ("oak_leaf_litter", include_str!("data/feature/placed/oak_leaf_litter.json")),
    ("birch_leaf_litter", include_str!("data/feature/placed/birch_leaf_litter.json")),
    ("fancy_oak_leaf_litter", include_str!("data/feature/placed/fancy_oak_leaf_litter.json")),
    ("trees_jungle", include_str!("data/feature/placed/trees_jungle.json")),
    ("trees_plains", include_str!("data/feature/placed/trees_plains.json")),
    ("trees_savanna", include_str!("data/feature/placed/trees_savanna.json")),
    ("trees_snowy", include_str!("data/feature/placed/trees_snowy.json")),
    ("trees_swamp", include_str!("data/feature/placed/trees_swamp.json")),
    ("trees_taiga", include_str!("data/feature/placed/trees_taiga.json")),
    ("trees_windswept_hills", include_str!("data/feature/placed/trees_windswept_hills.json")),
    // W227: the high/peaks slice biomes' tree roots.
    ("trees_meadow", include_str!("data/feature/placed/trees_meadow.json")),
    ("trees_badlands", include_str!("data/feature/placed/trees_badlands.json")),
    ("trees_grove", include_str!("data/feature/placed/trees_grove.json")),
    ("trees_windswept_forest", include_str!("data/feature/placed/trees_windswept_forest.json")),
    ("trees_windswept_savanna", include_str!("data/feature/placed/trees_windswept_savanna.json")),
    ("spruce_on_snow", include_str!("data/feature/placed/spruce_on_snow.json")),
    ("pine_on_snow", include_str!("data/feature/placed/pine_on_snow.json")),
    ("super_birch_bees", include_str!("data/feature/placed/super_birch_bees.json")),
    ("fancy_oak_bees", include_str!("data/feature/placed/fancy_oak_bees.json")),
    // W227.2.
    ("trees_cherry", include_str!("data/feature/placed/trees_cherry.json")),
    // W229.
    ("trees_mangrove", include_str!("data/feature/placed/trees_mangrove.json")),
    ("mangrove_checked", include_str!("data/feature/placed/mangrove_checked.json")),
    ("tall_mangrove_checked", include_str!("data/feature/placed/tall_mangrove_checked.json")),
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
    // W224.
    ("dark_forest_vegetation", include_str!("data/feature/configured/dark_forest_vegetation.json")),
    ("dark_oak", include_str!("data/feature/configured/dark_oak.json")),
    ("dark_oak_leaf_litter", include_str!("data/feature/configured/dark_oak_leaf_litter.json")),
    ("oak_leaf_litter", include_str!("data/feature/configured/oak_leaf_litter.json")),
    ("birch_leaf_litter", include_str!("data/feature/configured/birch_leaf_litter.json")),
    ("fancy_oak_leaf_litter", include_str!("data/feature/configured/fancy_oak_leaf_litter.json")),
    ("trees_jungle", include_str!("data/feature/configured/trees_jungle.json")),
    ("trees_plains", include_str!("data/feature/configured/trees_plains.json")),
    ("trees_savanna", include_str!("data/feature/configured/trees_savanna.json")),
    ("trees_snowy", include_str!("data/feature/configured/trees_snowy.json")),
    ("trees_taiga", include_str!("data/feature/configured/trees_taiga.json")),
    ("trees_windswept_hills", include_str!("data/feature/configured/trees_windswept_hills.json")),
    // W227.
    ("meadow_trees", include_str!("data/feature/configured/meadow_trees.json")),
    ("trees_badlands", include_str!("data/feature/configured/trees_badlands.json")),
    ("trees_grove", include_str!("data/feature/configured/trees_grove.json")),
    ("super_birch_bees", include_str!("data/feature/configured/super_birch_bees.json")),
    ("fancy_oak_bees", include_str!("data/feature/configured/fancy_oak_bees.json")),
    // W227.2.
    ("cherry_bees_005", include_str!("data/feature/configured/cherry_bees_005.json")),
    // W229.
    ("mangrove_vegetation", include_str!("data/feature/configured/mangrove_vegetation.json")),
    ("mangrove", include_str!("data/feature/configured/mangrove.json")),
    ("tall_mangrove", include_str!("data/feature/configured/tall_mangrove.json")),
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
            // W227.2: UniformInt.MAP_CODEC inlines without a "type"
            // (cherry branch_start_offset_from_top).
            None if v["min_inclusive"].is_i64() && v["max_inclusive"].is_i64() => IntProv {
                min: v["min_inclusive"].as_i64().unwrap() as i32,
                max: v["max_inclusive"].as_i64().unwrap() as i32,
            },
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
    /// W224: DarkOakTrunkPlacer - a leaning 2x2 column with short
    /// corner branches under the crown.
    DarkOak,
    /// W227.2: CherryTrunkPlacer - a straight trunk that ends in 1-3
    /// branches (two opposite side branches + an optional middle top),
    /// each walking to its end point by manhattan-distance chance.
    Cherry {
        branch_count: IntProv,
        branch_horizontal: IntProv,
        branch_start: IntProv,
        branch_end: IntProv,
    },
    /// W229: UpwardsBranchingTrunkPlacer (mangrove) - a straight trunk
    /// where each log may sprout an upward-stepping side branch, every
    /// branch log carrying its own foliage attachment.
    UpwardsBranching {
        extra_steps: IntProv,
        branch_prob: f32,
        extra_len: IntProv,
    },
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
    /// W224: DarkOakFoliagePlacer - the wide flat canopy (rows -1..=2
    /// with the corner trims from source).
    DarkOak,
    /// W227.2: CherryFoliagePlacer - the wide pink crown with corner
    /// holes and hanging leaves under the two bottom rows.
    Cherry {
        height: IntProv,
        wide_bottom_hole: f32,
        corner_hole: f32,
        hanging: f32,
        hanging_ext: f32,
    },
    /// W229: RandomSpreadFoliagePlacer (mangrove) - N random offsets
    /// in a radius x height box around each attachment.
    RandomSpread { foliage_height: IntProv, attempts: i32 },
}

/// W229: MangroveRootPlacer's config - the trunk stilt offset + the
/// root walk limits and blocks.
#[derive(Clone, Debug)]
pub(crate) struct RootDef {
    trunk_offset_y: IntProv,
    root: u16,
    muddy: u16,
    moss: u16,
    moss_chance: f32,
    max_length: i32,
    max_width: i32,
    skew: f32,
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
    /// W229: mangrove stilt roots (None for every other tree).
    root: Option<RootDef>,
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
        Some("minecraft:dark_oak_trunk_placer") => Trunk::DarkOak,
        Some("minecraft:cherry_trunk_placer") => Trunk::Cherry {
            // branch_count is a weighted_list of {1,2,3} with EQUAL
            // weights in the jar data - identical to a uniform pick,
            // which IntProv models; unequal weights would need a
            // weighted provider (fail loudly via the range assert).
            branch_count: {
                let c = &tp["branch_count"];
                if c["type"].as_str() == Some("minecraft:weighted_list") {
                    let rows: Vec<(u64, i64)> = c["distribution"]
                        .as_array()
                        .expect("branch_count distribution")
                        .iter()
                        .map(|e| {
                            (e["weight"].as_u64().expect("weight"), e["data"].as_i64().expect("data"))
                        })
                        .collect();
                    assert!(
                        rows.windows(2).all(|w| w[0].0 == w[1].0),
                        "cherry branch_count weights must be uniform"
                    );
                    IntProv {
                        min: rows.iter().map(|(_, d)| *d).min().unwrap() as i32,
                        max: rows.iter().map(|(_, d)| *d).max().unwrap() as i32,
                    }
                } else {
                    IntProv::parse(c)
                }
            },
            branch_horizontal: IntProv::parse(&tp["branch_horizontal_length"]),
            branch_start: IntProv::parse(&tp["branch_start_offset_from_top"]),
            branch_end: IntProv::parse(&tp["branch_end_offset_from_top"]),
        },
        Some("minecraft:upwards_branching_trunk_placer") => Trunk::UpwardsBranching {
            extra_steps: IntProv::parse(&tp["extra_branch_steps"]),
            branch_prob: tp["place_branch_per_log_probability"].as_f64().expect("prob") as f32,
            extra_len: IntProv::parse(&tp["extra_branch_length"]),
        },
        _ => return TreeRef::Unsupported(name), // bending / ...
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
        Some("minecraft:dark_oak_foliage_placer") => Foliage::DarkOak,
        Some("minecraft:cherry_foliage_placer") => Foliage::Cherry {
            height: IntProv::parse(&fp["height"]),
            wide_bottom_hole: fp["wide_bottom_layer_hole_chance"].as_f64().expect("hole") as f32,
            corner_hole: fp["corner_hole_chance"].as_f64().expect("corner") as f32,
            hanging: fp["hanging_leaves_chance"].as_f64().expect("hanging") as f32,
            hanging_ext: fp["hanging_leaves_extension_chance"].as_f64().expect("ext") as f32,
        },
        Some("minecraft:random_spread_foliage_placer") => Foliage::RandomSpread {
            foliage_height: IntProv::parse(&fp["foliage_height"]),
            attempts: fp["leaf_placement_attempts"].as_i64().expect("attempts") as i32,
        },
        _ => return TreeRef::Unsupported(name),
    };
    // W229: the mangrove root placer (stilt roots).
    let root = cfg.get("root_placer").and_then(|rp| {
        if rp["type"].as_str() != Some("minecraft:mangrove_root_placer") {
            return None;
        }
        let mrp = &rp["mangrove_root_placement"];
        Some(RootDef {
            trunk_offset_y: IntProv::parse(&rp["trunk_offset_y"]),
            root: block_id_by_name("mangrove_roots")?,
            muddy: block_id_by_name("muddy_mangrove_roots")?,
            moss: block_id_by_name("moss_carpet")?,
            moss_chance: rp["above_root_placement"]["above_root_placement_chance"]
                .as_f64()
                .unwrap_or(0.5) as f32,
            max_length: mrp["max_root_length"].as_i64().unwrap_or(15) as i32,
            max_width: mrp["max_root_width"].as_i64().unwrap_or(8) as i32,
            skew: mrp["random_skew_chance"].as_f64().unwrap_or(0.2) as f32,
        })
    });
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
        root,
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
            "dark_forest_vegetation",
            // W227.
            "trees_meadow",
            "trees_badlands",
            "trees_grove",
            "trees_windswept_forest",
            "trees_windswept_savanna",
            "trees_cherry",
            // W229.
            "trees_mangrove",
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
        B::Forest => Some("trees_birch_and_oak_leaf_litter"),
        // W224: the real dark-forest canopy (dark oaks, leaf-litter
        // oaks/birches; the huge mushrooms wait for their blocks).
        B::DarkForest => Some("dark_forest_vegetation"),
        B::FlowerForest => Some("trees_flower_forest"),
        B::BirchForest => Some("trees_birch"),
        // W222: tall super birches (with the fallen variants).
        B::OldGrowthBirchForest => Some("birch_tall"),
        B::Taiga | B::SnowyTaiga => Some("trees_taiga"),
        // W222: the real old-growth lists (mega spruce / mega pine).
        B::OldGrowthSpruceTaiga => Some("trees_old_growth_spruce_taiga"),
        B::OldGrowthPineTaiga => Some("trees_old_growth_pine_taiga"),
        B::SnowyPlains | B::IceSpikes | B::SnowySlopes => Some("trees_snowy"),
        B::WindsweptHills | B::WindsweptGravellyHills => Some("trees_windswept_hills"),
        B::Jungle | B::BambooJungle => Some("trees_jungle"),
        // W222: the real sparse-jungle list (bushes + fancy oaks).
        B::SparseJungle => Some("trees_sparse_jungle"),
        B::Savanna | B::SavannaPlateau => Some("trees_savanna"),
        B::Swamp => Some("trees_swamp"),
        // W229: the real mangroves (UpwardsBranchingTrunkPlacer +
        // MangroveRootPlacer + RandomSpread foliage).
        B::MangroveSwamp => Some("trees_mangrove"),
        // W227: the high/peaks slice biomes. Pale garden hosts the
        // dark-forest canopy (pale oaks share the dark-oak placer
        // shape; only the blocks are missing).
        B::Meadow => Some("trees_meadow"),
        B::WoodedBadlands => Some("trees_badlands"),
        B::Grove => Some("trees_grove"),
        B::WindsweptForest => Some("trees_windswept_forest"),
        B::WindsweptSavanna => Some("trees_windswept_savanna"),
        B::PaleGarden => Some("dark_forest_vegetation"),
        // W227.2: the real cherry placers.
        B::CherryGrove => Some("trees_cherry"),
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
        B::Desert
        | B::Beach
        | B::SnowyBeach
        | B::StonyShore
        | B::Badlands
        | B::ErodedBadlands
        | B::JaggedPeaks
        | B::FrozenPeaks
        | B::StonyPeaks
        | B::MushroomFields => None,
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
                // DarkOak / Cherry / RandomSpread place inline and
                // never route through this helper.
                Foliage::DarkOak | Foliage::Cherry { .. } | Foliage::RandomSpread { .. } => false,
            };
            if !skip {
                set_leaf(chunk, origin, att[0] + dx, att[1] + y, att[2] + dz, def.leaves);
            }
        }
    }
}

/// W227.2 `CherryFoliagePlacer.shouldSkipLocation` (abs-folded coords):
/// bottom-row edge holes, then corner holes (widened past 2r-2 on wide
/// layers). Short-circuit order preserved - rolls fire exactly when
/// MC's do (geometry-gated, chunk-independent).
fn cherry_skip(
    rng: &mut SplitMix64,
    adx: i32,
    y: i32,
    adz: i32,
    r: i32,
    wide_bottom_hole: f32,
    corner_hole: f32,
) -> bool {
    if y == -1 && (adx == r || adz == r) && rng.next_f64() < wide_bottom_hole as f64 {
        return true;
    }
    let corner = adx == r && adz == r;
    if r > 2 {
        corner || (adx + adz > 2 * r - 2 && rng.next_f64() < corner_hole as f64)
    } else {
        corner && rng.next_f64() < corner_hole as f64
    }
}

/// One cherry leaves row, tracking written cells - the stand-in for
/// MC's `FoliageSetter.isSet` (feature-local, chunk-independent, so
/// every replay decides the hanging pass identically).
#[allow(clippy::too_many_arguments)]
fn cherry_row(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    def: &TreeDef,
    att: [i32; 3],
    r: i32,
    y: i32,
    wide_bottom_hole: f32,
    corner_hole: f32,
    written: &mut std::collections::HashSet<(i32, i32, i32)>,
) {
    if r < 0 {
        return;
    }
    for dx in -r..=r {
        for dz in -r..=r {
            if !cherry_skip(rng, dx.abs(), y, dz.abs(), r, wide_bottom_hole, corner_hole) {
                let (x, yy, z) = (att[0] + dx, att[1] + y, att[2] + dz);
                written.insert((x, yy, z));
                set_leaf(chunk, origin, x, yy, z, def.leaves);
            }
        }
    }
}

/// `FoliagePlacer.placeLeavesRowWithHangingLeavesBelow` for the cherry
/// crown: the row itself, then hanging leaves under the row's edge
/// cells (chance-gated, manhattan-limited from the attachment log),
/// with a second chance-gated extension one deeper.
#[allow(clippy::too_many_arguments)]
fn cherry_hanging_row(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    def: &TreeDef,
    att: [i32; 3],
    r: i32,
    y: i32,
    wide_bottom_hole: f32,
    corner_hole: f32,
    hanging: f32,
    hanging_ext: f32,
    written: &mut std::collections::HashSet<(i32, i32, i32)>,
) {
    cherry_row(chunk, origin, rng, def, att, r, y, wide_bottom_hole, corner_hole, written);
    if r < 0 {
        return;
    }
    let log_pos = [att[0], att[1] - 1, att[2]];
    let manhattan = |p: [i32; 3]| {
        (p[0] - log_pos[0]).abs() + (p[1] - log_pos[1]).abs() + (p[2] - log_pos[2]).abs()
    };
    // Plane.HORIZONTAL order (N, E, S, W); the edge normal is the
    // clockwise direction of the walk direction.
    const ALONG: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    const TO_EDGE: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];
    for e in 0..4 {
        let along = ALONG[e];
        let to_edge = TO_EDGE[e];
        let mut pos = [
            att[0] + to_edge[0] * r - along[0] * r,
            att[1] + y - 1,
            att[2] + to_edge[1] * r - along[1] * r,
        ];
        let mut o = -r;
        while o < r {
            let above_set = written.contains(&(pos[0], pos[1] + 1, pos[2]));
            if above_set && manhattan(pos) < 7 && rng.next_f64() < hanging as f64 {
                written.insert((pos[0], pos[1], pos[2]));
                set_leaf(chunk, origin, pos[0], pos[1], pos[2], def.leaves);
                let below = [pos[0], pos[1] - 1, pos[2]];
                if manhattan(below) < 7 && rng.next_f64() < hanging_ext as f64 {
                    written.insert((below[0], below[1], below[2]));
                    set_leaf(chunk, origin, below[0], below[1], below[2], def.leaves);
                }
            }
            o += 1;
            pos[0] += along[0];
            pos[2] += along[1];
        }
    }
}

/// W227.2 `CherryTrunkPlacer.generateBranch`: two horizontal steps out
/// of the trunk (one when the branch ends the tree), then a walk to the
/// end point choosing vertical-vs-horizontal by manhattan-distance
/// ratio. Returns the branch-end foliage attachment.
#[allow(clippy::too_many_arguments)]
fn cherry_branch(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    def: &TreeDef,
    base: [i32; 3],
    tree_height: i32,
    dir: [i32; 2],
    offset_from_origin: i32,
    middle_continues_up: bool,
    branch_horizontal: IntProv,
    branch_end: IntProv,
) -> ([i32; 3], i32, bool) {
    let mut log = [base[0], base[1] + offset_from_origin, base[2]];
    let end_off = tree_height - 1 + branch_end.sample(rng);
    let extend = middle_continues_up || end_off < offset_from_origin;
    let dist = branch_horizontal.sample(rng) + if extend { 1 } else { 0 };
    let end = [base[0] + dir[0] * dist, base[1] + end_off, base[2] + dir[1] * dist];
    let steps = if extend { 2 } else { 1 };
    for _ in 0..steps {
        log[0] += dir[0];
        log[2] += dir[1];
        set_log(chunk, origin, log[0], log[1], log[2], def.log);
    }
    let vdir = if end[1] > log[1] { 1 } else { -1 };
    loop {
        let d = (end[0] - log[0]).abs() + (end[1] - log[1]).abs() + (end[2] - log[2]).abs();
        if d == 0 {
            return ([end[0], end[1] + 1, end[2]], 0, false);
        }
        let chance = (end[1] - log[1]).abs() as f64 / d as f64;
        if rng.next_f64() < chance {
            log[1] += vdir;
        } else {
            log[0] += dir[0];
            log[2] += dir[1];
        }
        set_log(chunk, origin, log[0], log[1], log[2], def.log);
    }
}

/// W229 `MangroveRootPlacer.placeRoots`: raise the trunk onto stilts
/// (trunk_offset_y), walk the four horizontal root arms down/outward
/// (`simulateRoots` / `potentialRootPositions` exactly), then write
/// roots (muddy inside the ground band) + the 50% moss carpets. All
/// DECISIONS use replay-invariant data (the shared post-carve floor +
/// this feature's own root set standing in for MC's world reads);
/// writes guard on the actual cell. Returns the trunk origin.
fn place_mangrove_roots(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    root: &RootDef,
    base: [i32; 3],
) -> [i32; 3] {
    use std::collections::HashSet;
    const DIRS: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    let trunk_origin = [base[0], base[1] + root.trunk_offset_y.sample(rng), base[2]];
    // canPlaceRoot, replay-invariant: at most one block into the ground
    // (the mangrove mud band; MC grows through mud/roots/moss only) and
    // never above the world.
    let can_place = |p: [i32; 3]| -> bool {
        (1..255).contains(&p[1]) && p[1] >= mc_floor_at(ctx, p[0], p[2]) - 1
    };
    #[allow(clippy::too_many_arguments)]
    fn simulate(
        rng: &mut SplitMix64,
        can_place: &dyn Fn([i32; 3]) -> bool,
        pos: [i32; 3],
        dir: [i32; 2],
        root_origin: [i32; 3],
        out: &mut Vec<[i32; 3]>,
        layer: i32,
        cfg: &RootDef,
    ) -> bool {
        if layer == cfg.max_length || out.len() > cfg.max_length as usize {
            return false;
        }
        // potentialRootPositions.
        let below = [pos[0], pos[1] - 1, pos[2]];
        let next_to = [pos[0] + dir[0], pos[1], pos[2] + dir[1]];
        let width = (pos[0] - root_origin[0]).abs()
            + (pos[1] - root_origin[1]).abs()
            + (pos[2] - root_origin[2]).abs();
        let candidates: Vec<[i32; 3]> = if width > cfg.max_width - 3 && width <= cfg.max_width {
            if rng.next_f64() < cfg.skew as f64 {
                vec![below, [next_to[0], next_to[1] - 1, next_to[2]]]
            } else {
                vec![below]
            }
        } else if width > cfg.max_width {
            vec![below]
        } else {
            // Inside the width band: the skew roll fires first, then
            // the 50/50 sideways pick only when it misses (the
            // source's else-chain).
            let skewed = rng.next_f64() < cfg.skew as f64;
            let sideways = !skewed && rng.next_u64().is_multiple_of(2);
            if sideways { vec![next_to] } else { vec![below] }
        };
        for p in candidates {
            if can_place(p) {
                out.push(p);
                if !simulate(rng, can_place, p, dir, root_origin, out, layer + 1, cfg) {
                    return false;
                }
            }
        }
        true
    }
    let mut roots: Vec<[i32; 3]> = vec![[trunk_origin[0], trunk_origin[1] - 1, trunk_origin[2]]];
    for dir in DIRS {
        let start = [trunk_origin[0] + dir[0], trunk_origin[1], trunk_origin[2] + dir[1]];
        let mut in_dir = Vec::new();
        simulate(rng, &can_place, start, dir, trunk_origin, &mut in_dir, 0, root);
        roots.extend(in_dir);
        roots.push(start);
    }
    // placeRoot per position: muddy roots inside the ground band (no
    // above roll, like the source's muddy branch); stilt roots roll the
    // 50% moss carpet gated on a virtual-air cell above.
    let root_set: HashSet<[i32; 3]> = roots.iter().copied().collect();
    let mud_id = block_id_by_name("mud");
    let cs = CHUNK_SIZE as i32;
    let mut write = |p: [i32; 3], id: u16, replace_ground: bool| {
        let (lx, ly, lz) = (p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]);
        if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
            return;
        }
        let cur = chunk.get(lx as usize, ly as usize, lz as usize).0;
        let writable = cur == 0
            || cur == 36
            || (replace_ground && Some(cur) == mud_id)
            || crate::block::block_type(cur)
                .is_some_and(|bt| bt.flags & crate::block::F_REPLACEABLE != 0);
        if writable {
            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    };
    for p in roots.iter() {
        let in_ground = p[1] <= mc_floor_at(ctx, p[0], p[2]);
        if in_ground {
            write(*p, root.muddy, true);
        } else {
            write(*p, root.root, false);
            // Above-root placement: the roll fires like the source
            // (before the emptiness test), the write needs virtual air
            // (above sea level, open, not another root).
            let above = [p[0], p[1] + 1, p[2]];
            if rng.next_f64() < root.moss_chance as f64
                && above[1] >= SEA_LEVEL
                && !super::terrain::column_is_solid(ctx, above[0], above[1], above[2])
                && !root_set.contains(&above)
            {
                write(above, root.moss, false);
            }
        }
    }
    trunk_origin
}

/// Place one tree with base (trunk bottom) at `base`. Ported from
/// TreeFeature.doPlace + the trunk/foliage placers named in the module
/// docs. Writes only cells inside `chunk` (the 3x3 source replay covers
/// the spill into neighbours).
fn place_tree(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    def: &TreeDef,
    base: [i32; 3],
) {
    // W229: mangrove stilt roots first - the trunk grows from their top.
    let base = match &def.root {
        Some(root) => place_mangrove_roots(chunk, origin, ctx, rng, root, base),
        None => base,
    };
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
        Foliage::DarkOak => 4,
        Foliage::Acacia => 0,
        Foliage::Cherry { height, .. } => height.sample(rng),
        Foliage::RandomSpread { foliage_height, .. } => foliage_height.sample(rng),
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
        Trunk::DarkOak => {
            // W224: DarkOakTrunkPlacer - the 2x2 column leans 0..2
            // steps sideways near the top; short corner branches hang
            // under the crown, each with its own small canopy.
            const DIRS: [[i32; 2]; 4] = [[0, -1], [0, 1], [-1, 0], [1, 0]];
            let lean_dir = DIRS[(rng.next_u64() % 4) as usize];
            let lean_height = tree_height - (rng.next_u64() % 4) as i32;
            let mut lean_steps = 2 - (rng.next_u64() % 3) as i32;
            let (mut tx, mut tz) = (base[0], base[2]);
            let ey = base[1] + tree_height - 1;
            for dy in 0..tree_height {
                if dy >= lean_height && lean_steps > 0 {
                    tx += lean_dir[0];
                    tz += lean_dir[1];
                    lean_steps -= 1;
                }
                let yy = base[1] + dy;
                set_log(chunk, origin, tx, yy, tz, def.log);
                set_log(chunk, origin, tx + 1, yy, tz, def.log);
                set_log(chunk, origin, tx, yy, tz + 1, def.log);
                set_log(chunk, origin, tx + 1, yy, tz + 1, def.log);
            }
            attachments.push(([tx, ey, tz], 0, true));
            for ox in -1..=2 {
                for oz in -1..=2 {
                    if (0..=1).contains(&ox) && (0..=1).contains(&oz) {
                        continue;
                    }
                    if !rng.next_u64().is_multiple_of(3) {
                        continue;
                    }
                    let length = (rng.next_u64() % 3) as i32 + 2;
                    for by in 0..length {
                        set_log(chunk, origin, base[0] + ox, ey - by - 1, base[2] + oz, def.log);
                    }
                    attachments.push(([base[0] + ox, ey, base[2] + oz], 0, false));
                }
            }
        }
        Trunk::Cherry { branch_count, branch_horizontal, branch_start, branch_end } => {
            // CherryTrunkPlacer.placeTrunk, line-for-line (the
            // below-trunk dirt provider is skipped like every other
            // placer's). Branch starts hang off the top; the second
            // start samples one shorter and shifts up past the first.
            let first = (tree_height - 1 + branch_start.sample(rng)).max(0);
            let second_prov = IntProv { min: branch_start.min, max: branch_start.max - 1 };
            let mut second = (tree_height - 1 + second_prov.sample(rng)).max(0);
            if second >= first {
                second += 1;
            }
            let count = branch_count.sample(rng);
            let middle = count == 3;
            let both = count >= 2;
            let trunk_h = if middle {
                tree_height
            } else if both {
                first.max(second) + 1
            } else {
                first + 1
            };
            for y in 0..trunk_h {
                set_log(chunk, origin, base[0], base[1] + y, base[2], def.log);
            }
            if middle {
                attachments.push(([base[0], base[1] + trunk_h, base[2]], 0, false));
            }
            const DIRS: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
            let dir = DIRS[(rng.next_u64() % 4) as usize];
            attachments.push(cherry_branch(
                chunk,
                origin,
                rng,
                def,
                base,
                tree_height,
                dir,
                first,
                first < trunk_h - 1,
                branch_horizontal,
                branch_end,
            ));
            if both {
                attachments.push(cherry_branch(
                    chunk,
                    origin,
                    rng,
                    def,
                    base,
                    tree_height,
                    [-dir[0], -dir[1]],
                    second,
                    second < trunk_h - 1,
                    branch_horizontal,
                    branch_end,
                ));
            }
        }
        Trunk::UpwardsBranching { extra_steps, branch_prob, extra_len } => {
            // W229 UpwardsBranchingTrunkPlacer.placeTrunk: every trunk
            // log below the top may sprout an upward-stepping branch;
            // each branch log carries a foliage attachment, plus the
            // two extra crown attachments when the branch ran. placeLog
            // is treated as always-true (the port's convention - the
            // free-space checks consume no randomness).
            const DIRS: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
            for h in 0..tree_height {
                let y = base[1] + h;
                set_log(chunk, origin, base[0], y, base[2], def.log);
                if h < tree_height - 1 && rng.next_f64() < branch_prob as f64 {
                    let dir = DIRS[(rng.next_u64() % 4) as usize];
                    let len = extra_len.sample(rng);
                    let start = (len - extra_len.sample(rng) - 1).max(0);
                    let steps = extra_steps.sample(rng);
                    let (mut lx, mut lz) = (base[0], base[2]);
                    let mut along = y + start;
                    let mut idx = start;
                    let mut s = steps;
                    while idx < tree_height && s > 0 {
                        if idx >= 1 {
                            let ph = y + idx;
                            lx += dir[0];
                            lz += dir[1];
                            set_log(chunk, origin, lx, ph, lz, def.log);
                            along = ph + 1;
                            attachments.push(([lx, ph, lz], 0, false));
                        }
                        idx += 1;
                        s -= 1;
                    }
                    if along - y > 1 {
                        attachments.push(([lx, along, lz], 0, false));
                        attachments.push(([lx, along - 2, lz], 0, false));
                    }
                }
                if h == tree_height - 1 {
                    attachments.push(([base[0], y + 1, base[2]], 0, false));
                }
            }
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
            Foliage::DarkOak => {
                // W224: fixed rows -1..=2 with the source's trims. The
                // corner rules need SIGNED offsets, so the rows place
                // inline instead of through place_leaves_row.
                let r = leaf_radius + rad_off;
                let rows: &[(i32, i32, bool)] = if double_trunk {
                    // (y offset, radius, fifty-fifty gate)
                    &[(-1, 2, false), (0, 3, false), (1, 2, false), (2, 0, true)]
                } else {
                    &[(-1, 2, false), (0, 1, false)]
                };
                for &(yo, extra, gated) in rows {
                    if gated && !rng.next_u64().is_multiple_of(2) {
                        continue;
                    }
                    let radius = r + extra;
                    let ext = if double_trunk { 1 } else { 0 };
                    for dx in -radius..=radius + ext {
                        for dz in -radius..=radius + ext {
                            // Signed corner trim (shouldSkipLocationSigned):
                            // row 0 of a double trunk drops the square's
                            // corners.
                            if yo == 0
                                && double_trunk
                                && (dx == -radius || dx >= radius)
                                && (dz == -radius || dz >= radius)
                            {
                                continue;
                            }
                            let (adx, adz) = if double_trunk {
                                (dx.abs().min((dx - 1).abs()), dz.abs().min((dz - 1).abs()))
                            } else {
                                (dx.abs(), dz.abs())
                            };
                            // Unsigned rules: bottom row of the single
                            // canopies drops its far corner; row 1 trims
                            // to the diamond.
                            let skip = if yo == -1 && !double_trunk {
                                adx == radius && adz == radius
                            } else if yo == 1 {
                                adx + adz > radius * 2 - 2
                            } else {
                                false
                            };
                            if !skip {
                                set_leaf(chunk, origin, att[0] + dx, att[1] + yo, att[2] + dz, def.leaves);
                            }
                        }
                    }
                }
            }
            Foliage::RandomSpread { attempts, .. } => {
                // W229 RandomSpreadFoliagePlacer.createFoliage: N
                // random offsets in the radius/height box per
                // attachment (offsets sampled even when they land
                // outside this chunk - stream stays aligned).
                let r = (leaf_radius + rad_off).max(1);
                let fh = foliage_height.max(1);
                let ni = |rng: &mut SplitMix64, n: i32| (rng.next_u64() % n.max(1) as u64) as i32;
                for _ in 0..*attempts {
                    let dx = ni(rng, r) - ni(rng, r);
                    let dy = ni(rng, fh) - ni(rng, fh);
                    let dz = ni(rng, r) - ni(rng, r);
                    set_leaf(chunk, origin, att[0] + dx, att[1] + offset + dy, att[2] + dz, def.leaves);
                }
            }
            Foliage::Cherry { wide_bottom_hole, corner_hole, hanging, hanging_ext, .. } => {
                // W227.2 CherryFoliagePlacer.createFoliage: two
                // shrinking top rows, the full-radius body, then the
                // two bottom rows growing hanging leaves below their
                // edges. `written` stands in for MC's FoliageSetter
                // set-tracking (feature-local, replay-consistent).
                let (wb, ch, hg, he) = (*wide_bottom_hole, *corner_hole, *hanging, *hanging_ext);
                let ap = [att[0], att[1] + offset, att[2]];
                let cr = leaf_radius + rad_off - 1;
                let mut written: std::collections::HashSet<(i32, i32, i32)> =
                    std::collections::HashSet::new();
                cherry_row(chunk, origin, rng, def, ap, cr - 2, foliage_height - 3, wb, ch, &mut written);
                cherry_row(chunk, origin, rng, def, ap, cr - 1, foliage_height - 4, wb, ch, &mut written);
                let mut y = foliage_height - 5;
                while y >= 0 {
                    cherry_row(chunk, origin, rng, def, ap, cr, y, wb, ch, &mut written);
                    y -= 1;
                }
                cherry_hanging_row(chunk, origin, rng, def, ap, cr, -1, wb, ch, hg, he, &mut written);
                cherry_hanging_row(
                    chunk, origin, rng, def, ap, cr - 1, -2, wb, ch, hg, he, &mut written,
                );
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
                // surface_water_depth_filter: max 0 (dry land) for
                // every root except mangrove's max 5 (W229 -
                // trees_mangrove wades into the swamp shallows).
                let max_water_depth = if bt.name == "trees_mangrove" { 5 } else { 0 };
                if floor < SEA_LEVEL - max_water_depth {
                    continue;
                }
                // would_survive(oak_sapling): dirt-family floor. The
                // floor block isn't in OUR chunk necessarily; derive it
                // from the surface rule the fill used - cheap proxy:
                // check the chunk cell when local, else accept (the
                // biome + height already filtered deserts/beaches).
                // W227 grove: spruce/pine_on_snow carry a
                // block_predicate_filter for a SNOW floor instead
                // (grove surfaces are snow_block).
                // W317: the floor gate must be CHUNK-INDEPENDENT.
                // The old check only ran when the floor cell sat in
                // the CURRENT chunk - a rejected try then skipped the
                // shape rng that accepting slices consumed, so every
                // vertical slice of the source chunk drifted onto
                // DIFFERENT trees (the sliced/floating spruce
                // screenshot). The surface-rule block is
                // deterministic per column: every replay agrees and
                // the rng stream stays in lockstep.
                let surf = super::terrain::mc_surface_block_at(ctx, wx, wz);
                let floor_ok = if bt.name == "trees_grove" {
                    crate::block::block_type(surf)
                        .is_some_and(|b| b.name == "snow_block")
                } else {
                    floor_supports_tree(surf)
                };
                if !floor_ok {
                    continue;
                }
                // W285: never root a tree inside a village/outpost
                // piece (docx img30 trees-through-houses). The
                // in-chunk floor check above rejects LOCAL bases
                // standing on structure blocks, but a base in a
                // neighbour source chunk skips it (those cells are
                // not available here) - the canopy then wrote
                // straight through whatever piece stands there while
                // the base's own chunk rejected the same try (the
                // orphaned floating canopies, img27). The box probe
                // is deterministic per seed, so every chunk slice of
                // the try agrees.
                if super::village::pos_in_village(ctx, [wx, floor, wz])
                    || super::outpost::pos_in_outpost(ctx, [wx, floor, wz])
                {
                    continue;
                }
                match pick {
                    Pick::Tree(def) => {
                        place_tree(chunk, origin, ctx, &mut rng, def, [wx, floor + 1, wz]);
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

    /// W229: the mangrove placers parse (upwards-branching trunk, the
    /// root placer, random-spread foliage) and mangrove swamps grow
    /// real mangroves with stilt roots.
    #[test]
    fn mangrove_parses_and_swamps_grow_mangroves() {
        for name in ["mangrove", "tall_mangrove"] {
            let TreeRef::Tree(d) =
                parse_tree_conf(TREE_CONFIGURED.iter().find(|(n, _)| *n == name).unwrap().0)
            else {
                panic!("{name} should parse as a tree");
            };
            assert_eq!(Some(d.log), block_id_by_name("mangrove_log"), "{name} log");
            assert!(matches!(d.trunk, Trunk::UpwardsBranching { .. }), "{name} trunk");
            assert!(
                matches!(d.foliage, Foliage::RandomSpread { attempts: 70, .. }),
                "{name} foliage"
            );
            let root = d.root.as_ref().expect("mangrove root placer parses");
            assert_eq!((root.max_length, root.max_width), (15, 8));
            assert_eq!(Some(root.muddy), block_id_by_name("muddy_mangrove_roots"));
        }
        // Evidence: a mangrove-swamp chunk grows logs + roots.
        let c = ctx();
        let (log, roots, muddy) = (
            block_id_by_name("mangrove_log").unwrap(),
            block_id_by_name("mangrove_roots").unwrap(),
            block_id_by_name("muddy_mangrove_roots").unwrap(),
        );
        let mut seen_logs = 0u32;
        let mut seen_roots = 0u32;
        let mut swamp_chunks = 0u32;
        // Mangrove swamps are hot erosion-6 valleys - sparse near the
        // origin on this seed, so sweep wide (biome sampling is cheap;
        // chunks generate only on a hit, and the test exits early).
        for cx in -150..150 {
            for cz in -150..150 {
                let (wx, wz) = (cx * 16 + 8, cz * 16 + 8);
                if crate::worldgen::mcgen::terrain::mc_full_biome_at(&c, wx, wz)
                    != crate::worldgen::mcgen::biome::McBiome::MangroveSwamp
                {
                    continue;
                }
                swamp_chunks += 1;
                let floor = mc_floor_at(&c, wx, wz);
                let cy = (floor + 4).div_euclid(16);
                let chunk = generate_chunk_at_ctx([cx, cy, cz], &c);
                for x in 0..16 {
                    for y in 0..16 {
                        for z in 0..16 {
                            let id = chunk.get(x, y, z).0;
                            if id == log {
                                seen_logs += 1;
                            } else if id == roots || id == muddy {
                                seen_roots += 1;
                            }
                        }
                    }
                }
                if seen_logs > 0 && seen_roots > 0 {
                    return;
                }
            }
        }
        panic!(
            "no mangroves found: {swamp_chunks} swamp chunks, logs={seen_logs} roots={seen_roots}"
        );
    }

    /// W227.2: the cherry placers parse from the jar data and place a
    /// branching trunk with a leafy crown.
    #[test]
    fn cherry_tree_parses_and_places() {
        let TreeRef::Tree(d) = parse_tree_conf("cherry_bees_005") else {
            panic!("cherry_bees_005 should parse as a tree");
        };
        assert_eq!(Some(d.log), block_id_by_name("cherry_log"));
        assert_eq!(Some(d.leaves), block_id_by_name("cherry_leaves"));
        assert!(matches!(d.trunk, Trunk::Cherry { .. }));
        assert!(
            matches!(d.foliage, Foliage::Cherry { hanging, .. } if (hanging - 1.0 / 6.0).abs() < 1e-6)
        );
        let mut chunk = Chunk::empty();
        let mut rng = SplitMix64::new(7);
        place_tree(&mut chunk, [0, 0, 0], &ctx(), &mut rng, &d, [8, 1, 8]);
        let mut logs = 0u32;
        let mut off_axis_logs = 0u32;
        let mut leaves = 0u32;
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    let id = chunk.get(x, y, z).0;
                    if Some(id) == block_id_by_name("cherry_log") {
                        logs += 1;
                        if (x, z) != (8, 8) {
                            off_axis_logs += 1;
                        }
                    } else if Some(id) == block_id_by_name("cherry_leaves") {
                        leaves += 1;
                    }
                }
            }
        }
        assert!(logs >= 8, "cherry trunk + branches expected, got {logs} logs");
        assert!(off_axis_logs > 0, "cherry must grow side branches");
        assert!(leaves > 30, "cherry crown expected, got {leaves} leaves");
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

/// W329: one far-LOD tree impostor site.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LodTreeSite {
    pub(crate) wx: i32,
    pub(crate) wz: i32,
    /// Log block id (trunk texture source).
    pub(crate) log: u16,
    /// Leaves block id (canopy texture source).
    pub(crate) leaves: u16,
    /// Trunk height roll (base + rand parts, MC's height formula).
    pub(crate) height: i32,
}

/// W329: deterministic tree-impostor sites for one chunk column.
/// Rolls the REAL biome tree tables (count distribution + species
/// weights + height providers) on a dedicated seed stream, so the
/// far LOD shows the right forest density and species mix. The
/// positions are NOT the exact full-res trees - matching them 1:1
/// would require replaying every shape roll the growers consume -
/// but at LOD distances the composition is what reads, and today's
/// alternative is a completely bare shell.
pub(crate) fn lod_tree_sites(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Vec<LodTreeSite> {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let mut out = Vec::new();
    let mut rng =
        SplitMix64::new(stream_seed(seed, scx, scz, SALT_TREES ^ 0x4C4F_4400));
    let mc = super::terrain::mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
    let Some(root) = tree_root_for(mc) else {
        return out;
    };
    let bt = biome_trees(root);
    let tries = sample_count(&bt.count_dist, &mut rng);
    for _ in 0..tries {
        let rx = (rng.next_u64() % 16) as i32;
        let rz = (rng.next_u64() % 16) as i32;
        let Some(pick) = select_tree(bt, &mut rng) else {
            continue;
        };
        let Pick::Tree(def) = pick else {
            continue; // fallen logs are ground clutter - skip at LOD
        };
        let ra = def.height_rand_a.max(0) as u64 + 1;
        let rb = def.height_rand_b.max(0) as u64 + 1;
        let height = def.base_height
            + (rng.next_u64() % ra) as i32
            + (rng.next_u64() % rb) as i32;
        out.push(LodTreeSite {
            wx: scx * cs + rx,
            wz: scz * cs + rz,
            log: def.log,
            leaves: def.leaves,
            height,
        });
    }
    out
}
