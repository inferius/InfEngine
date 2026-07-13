//! W268 - nether biome decoration, ported from the 26.1.2 biome
//! feature lists (`biome/{nether_wastes, soul_sand_valley,
//! crimson_forest, warped_forest, basalt_deltas}.json`) and the
//! feature classes (`SpringFeature`, `GlowstoneFeature`,
//! `BasaltPillarFeature`, `DeltaFeature`, `BasaltColumnsFeature`,
//! `ReplaceBlobsFeature`, `ScatteredOreFeature`,
//! `CountOnEveryLayerPlacement`).
//!
//! Steps in biome-list order:
//! - 2 local_modifications: basalt_pillar (soul sand valley).
//! - 4 surface_structures: delta, small/large_basalt_columns
//!   (basalt deltas).
//! - 7 underground_decoration: springs, fire / soul-fire patches,
//!   glowstone blobs, nether mushrooms, the crimson-roots patch,
//!   the ore set (magma / soul sand / gravel / blackstone / gold /
//!   quartz / ancient debris) and the basalt+blackstone blobs.
//! - 9 vegetal_decoration lives in `nether_vegetation.rs`.
//!
//! Determinism follows the overworld feature doctrine: streams are
//! keyed by SOURCE chunk column + world seed + per-feature salt, the
//! 3x3 neighbourhood replays (every scatter here reaches < 16
//! blocks), and world reads go to the PRE-FEATURE terrain
//! (`nether_block_at`) plus the feature body's own scratch writes -
//! sibling features' output is invisible (the documented sculk-pass
//! divergence; MC decorates a fully materialised chunk instead).
//! `minecraft:spring_lava` (step 9) is a vanilla no-op in the
//! nether: its configured feature (`spring_lava_overworld`) only
//! fires inside overworld stone, so it is skipped outright.

use std::collections::HashMap;
use std::sync::OnceLock;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use crate::worldgen::WorldgenCtx;

use super::features::{
    self, CountProvider, HeightProvider, OreRule, OreTarget, block_id_by_name,
    ore_try, stream_seed,
};
use super::nether_mc::{self, LAVA_ID, SEA_LEVEL, TOP_Y};
use super::noise::SplitMix64;

pub(crate) const SALT_BASE: u64 = 0xD4E6_0200;

/// The per-feature-body scratch overlay: reads see the terrain plus
/// the body's OWN writes (blob growth, column stacking), and writes
/// clip into the chunk being generated. Terrain reads memoize - the
/// surface-band eval behind `nether_block_at` is the expensive part
/// and feature bodies re-read the same cells constantly.
pub(crate) struct Scratch<'a> {
    pub(crate) chunk: &'a mut Chunk,
    pub(crate) origin: [i32; 3],
    pub(crate) writes: HashMap<[i32; 3], u16>,
    terrain_cache: HashMap<[i32; 3], u16>,
}

impl<'a> Scratch<'a> {
    pub(crate) fn new(chunk: &'a mut Chunk, origin: [i32; 3]) -> Self {
        Scratch {
            chunk,
            origin,
            writes: HashMap::new(),
            terrain_cache: HashMap::new(),
        }
    }

    /// Pre-feature terrain + own writes.
    pub(crate) fn get(&mut self, ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> u16 {
        if let Some(id) = self.writes.get(&[wx, wy, wz]) {
            return *id;
        }
        *self
            .terrain_cache
            .entry([wx, wy, wz])
            .or_insert_with(|| nether_mc::nether_block_at(ctx, wx, wy, wz))
    }

    pub(crate) fn is_empty(&mut self, ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> bool {
        self.get(ctx, wx, wy, wz) == 0
    }

    pub(crate) fn set(&mut self, wx: i32, wy: i32, wz: i32, id: u16) {
        if !(0..=TOP_Y).contains(&wy) {
            return;
        }
        self.writes.insert([wx, wy, wz], id);
        let cs = CHUNK_SIZE as i32;
        let (lx, ly, lz) = (wx - self.origin[0], wy - self.origin[1], wz - self.origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            self.chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
        }
    }
}

/// Cached engine ids the pass compares against.
pub(crate) struct Ids {
    pub(crate) netherrack: u16,
    pub(crate) basalt: u16,
    pub(crate) blackstone: u16,
    pub(crate) glowstone: u16,
    pub(crate) magma: u16,
    pub(crate) soul_sand: u16,
    pub(crate) soul_soil: u16,
    pub(crate) gravel: u16,
    pub(crate) fire: u16,
    pub(crate) soul_fire: u16,
    pub(crate) bedrock: u16,
    pub(crate) brown_mushroom: u16,
    pub(crate) red_mushroom: u16,
    pub(crate) crimson_nylium: u16,
    pub(crate) warped_nylium: u16,
    pub(crate) crimson_roots: u16,
}

impl Ids {
    pub(crate) fn get() -> Option<&'static Ids> {
        static IDS: OnceLock<Option<Ids>> = OnceLock::new();
        IDS.get_or_init(|| {
            Some(Ids {
                netherrack: block_id_by_name("netherrack")?,
                basalt: block_id_by_name("basalt")?,
                blackstone: block_id_by_name("blackstone")?,
                glowstone: block_id_by_name("glowstone")?,
                magma: block_id_by_name("magma_block")?,
                soul_sand: block_id_by_name("soul_sand")?,
                soul_soil: block_id_by_name("soul_soil")?,
                gravel: block_id_by_name("gravel")?,
                fire: block_id_by_name("fire")?,
                soul_fire: block_id_by_name("soul_fire")?,
                bedrock: block_id_by_name("bedrock")?,
                brown_mushroom: block_id_by_name("brown_mushroom")?,
                red_mushroom: block_id_by_name("red_mushroom")?,
                crimson_nylium: block_id_by_name("crimson_nylium")?,
                warped_nylium: block_id_by_name("warped_nylium")?,
                crimson_roots: block_id_by_name("crimson_roots")?,
            })
        })
        .as_ref()
    }
}

/// `Mth.nextInt(rng, 0, n-1)`-style bounded roll.
pub(crate) fn next_int(rng: &mut SplitMix64, n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    (rng.next_u64() % n as u64) as i32
}

/// Inclusive `Mth.nextInt(rng, lo, hi)`.
pub(crate) fn next_between(rng: &mut SplitMix64, lo: i32, hi: i32) -> i32 {
    lo + next_int(rng, hi - lo + 1)
}

/// MC `TrapezoidInt` with plateau 0: `min + (a + b) / 2` over two
/// uniform rolls in [0, range].
pub(crate) fn trapezoid(rng: &mut SplitMix64, min: i32, max: i32) -> i32 {
    let range = max - min;
    min + (next_int(rng, range + 1) + next_int(rng, range + 1)) / 2
}

/// MC `BiasedToBottomInt`: `min + nextInt(nextInt(max - min + 1) + 1)`.
fn biased_to_bottom(rng: &mut SplitMix64, min: i32, max: i32) -> i32 {
    let inner = next_int(rng, max - min + 1);
    min + next_int(rng, inner + 1)
}

/// The nether biome at a source-column centre.
pub(crate) fn biome_of(ctx: &WorldgenCtx, scx: i32, scz: i32) -> &'static str {
    nether_mc::nether_col(ctx, scx, scz).biome[8][8]
}

/// `CountOnEveryLayerPlacement.findOnGroundYPosition`: the y of the
/// `layer`-th standable floor (empty cell above a non-bedrock solid)
/// scanning down from above the roof. `None` when the column has no
/// such layer. Lava counts as empty (the deltas generate at the lava
/// shore) and the bedrock roof/floor are excluded like MC.
fn layer_y(ctx: &WorldgenCtx, wx: i32, wz: i32, layer: u32) -> Option<i32> {
    let cs = CHUNK_SIZE as i32;
    let col = nether_mc::nether_col(ctx, wx.div_euclid(cs), wz.div_euclid(cs));
    let (lx, lz) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
    let mut current_empty = true; // y = TOP_Y + 1 is above the roof
    let mut current_layer = 0u32;
    for y in (1..=TOP_Y + 1).rev() {
        let below_solid = col.is_solid(lx, y - 1, lz);
        if below_solid && current_empty {
            let below_bedrock = super::surface_rule::is_bedrock_roof(wx, y - 1, wz, TOP_Y)
                || super::surface_rule::is_bedrock_floor(wx, y - 1, wz, 0);
            if !below_bedrock {
                if current_layer == layer {
                    return Some(y);
                }
                current_layer += 1;
            }
        }
        current_empty = !below_solid;
    }
    None
}

/// `count_on_every_layer`: collect the positions for a source chunk
/// exactly like the Java stream builder (layer-major; a layer with no
/// ground anywhere ends the walk).
pub(crate) fn every_layer_positions(
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    scx: i32,
    scz: i32,
    count: u32,
) -> Vec<[i32; 3]> {
    let cs = CHUNK_SIZE as i32;
    let mut out = Vec::new();
    let mut layer = 0u32;
    loop {
        let mut found = false;
        for _ in 0..count {
            let x = scx * cs + next_int(rng, 16);
            let z = scz * cs + next_int(rng, 16);
            if let Some(y) = layer_y(ctx, x, z, layer) {
                out.push([x, y, z]);
                found = true;
            }
        }
        layer += 1;
        if !found || layer > 32 {
            return out;
        }
    }
}

// ---------------------------------------------------------------------------
// Step 7: springs (SpringFeature, chunk-local single cells).
// ---------------------------------------------------------------------------

/// The nether spring variants:
/// (count, hole_count, rock_count, requires_below, wide_valid, y-band).
/// - spring_open: 8x, 1 hole / 4 rock, netherrack only, y 4..123.
/// - spring_closed(: 16x / _double 32x deltas), 0 hole / 5 rock,
///   y 10..117.
/// - spring_delta (`spring_lava_nether`): 16x, 1/4 + block below,
///   valid += soul_sand/gravel/magma/blackstone, y 4..123.
struct SpringRow {
    count: u32,
    holes: usize,
    rocks: usize,
    requires_below: bool,
    wide_valid: bool,
    y_lo: i32,
    y_hi: i32,
    salt: u64,
}

fn place_nether_springs(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
    ids: &Ids,
    biome: &str,
) {
    let cs = CHUNK_SIZE as i32;
    let deltas = biome == "basalt_deltas";
    let mut rows = vec![
        SpringRow {
            count: 8,
            holes: 1,
            rocks: 4,
            requires_below: false,
            wide_valid: false,
            y_lo: 4,
            y_hi: TOP_Y - 4,
            salt: SALT_BASE ^ 0x10,
        },
        SpringRow {
            count: if deltas { 32 } else { 16 },
            holes: 0,
            rocks: 5,
            requires_below: false,
            wide_valid: false,
            y_lo: 10,
            y_hi: TOP_Y - 10,
            salt: SALT_BASE ^ 0x11,
        },
    ];
    if deltas {
        // spring_delta replaces spring_open's slot in the deltas list.
        rows.push(SpringRow {
            count: 16,
            holes: 1,
            rocks: 4,
            requires_below: true,
            wide_valid: true,
            y_lo: 4,
            y_hi: TOP_Y - 4,
            salt: SALT_BASE ^ 0x12,
        });
    }
    let seed = ctx.effective_seed();
    for row in rows {
        let valid = |id: u16| -> bool {
            id == ids.netherrack
                || (row.wide_valid
                    && (id == ids.soul_sand
                        || id == ids.gravel
                        || id == ids.magma
                        || id == ids.blackstone))
        };
        // Chunk-local like the overworld springs pass (single cells;
        // border cells with off-chunk neighbour reads are skipped, and
        // each vertical chunk rolls its own stream).
        let mut rng = SplitMix64::new(stream_seed(
            seed,
            coord[0],
            coord[2],
            row.salt ^ ((coord[1] as u64) << 8),
        ));
        for _ in 0..row.count {
            let lx = next_int(&mut rng, 16);
            let lz = next_int(&mut rng, 16);
            let wy = next_between(&mut rng, row.y_lo, row.y_hi);
            let ly = wy - origin[1];
            if !(1..cs - 1).contains(&ly)
                || !(1..cs - 1).contains(&lx)
                || !(1..cs - 1).contains(&lz)
            {
                continue;
            }
            let (ux, uy, uz) = (lx as usize, ly as usize, lz as usize);
            let at = |x: usize, y: usize, z: usize| chunk.get(x, y, z).0;
            if !valid(at(ux, uy + 1, uz)) {
                continue;
            }
            if row.requires_below && !valid(at(ux, uy - 1, uz)) {
                continue;
            }
            let here = at(ux, uy, uz);
            if here != 0 && !valid(here) {
                continue;
            }
            let sides =
                [at(ux + 1, uy, uz), at(ux - 1, uy, uz), at(ux, uy, uz + 1), at(ux, uy, uz - 1)];
            let below = at(ux, uy - 1, uz);
            let rocks = sides.iter().filter(|id| valid(**id)).count()
                + usize::from(valid(below));
            let holes =
                sides.iter().filter(|id| **id == 0).count() + usize::from(below == 0);
            if rocks == row.rocks && holes == row.holes {
                chunk.set(ux, uy, uz, BlockId(LAVA_ID));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Step 7: the offset patches (fire, soul fire, mushrooms, crimson roots).
// ---------------------------------------------------------------------------

/// One `count 96 + random_offset(+-7 xz, +-3 y)` patch pass around a
/// sampled origin; `place` decides per cell.
#[allow(clippy::too_many_arguments)]
fn offset_patch(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    place: &mut dyn FnMut(&mut Scratch, &WorldgenCtx, i32, i32, i32),
) {
    for _ in 0..96 {
        let dx = trapezoid(rng, -7, 7);
        let dy = trapezoid(rng, -3, 3);
        let dz = trapezoid(rng, -7, 7);
        place(scratch, ctx, ox + dx, oy + dy, oz + dz);
    }
}

/// patch_fire / patch_soul_fire: count uniform 0..5 origins at
/// y 4..123, each expanding to 96 offset tries; a try places fire on
/// air whose floor is netherrack (fire) / soul soil (soul fire).
#[allow(clippy::too_many_arguments)]
fn place_fire_patches(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
    soul: bool,
) {
    let cs = CHUNK_SIZE as i32;
    let salt = SALT_BASE ^ if soul { 0x21 } else { 0x20 };
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, salt));
    let outer = next_between(&mut rng, 0, 5);
    let (floor, block) =
        if soul { (ids.soul_soil, ids.soul_fire) } else { (ids.netherrack, ids.fire) };
    for _ in 0..outer {
        let ox = scx * cs + next_int(&mut rng, 16);
        let oz = scz * cs + next_int(&mut rng, 16);
        let oy = next_between(&mut rng, 4, TOP_Y - 4);
        offset_patch(scratch, ctx, &mut rng, ox, oy, oz, &mut |s, c, x, y, z| {
            if s.is_empty(c, x, y, z) && s.get(c, x, y - 1, z) == floor {
                s.set(x, y, z, block);
            }
        });
    }
}

/// brown/red_mushroom_nether: rarity 1/2, one origin at y 0..127 + 96
/// offsets; `_normal` (vegetal step): rarity 1/256 / 1/512 anchored on
/// the motion-blocking heightmap. A try places on air over a solid,
/// non-bedrock floor (MushroomBlock.canSurvive's solid ground).
#[allow(clippy::too_many_arguments)]
fn place_nether_mushrooms(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
    red: bool,
    normal_variant: bool,
) {
    let cs = CHUNK_SIZE as i32;
    let salt = SALT_BASE
        ^ match (red, normal_variant) {
            (false, false) => 0x30,
            (true, false) => 0x31,
            (false, true) => 0x32,
            (true, true) => 0x33,
        };
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, salt));
    let rarity: u64 = match (red, normal_variant) {
        (false, false) | (true, false) => 2,
        (false, true) => 256,
        (true, true) => 512,
    };
    if !rng.next_u64().is_multiple_of(rarity) {
        return;
    }
    let ox = scx * cs + next_int(&mut rng, 16);
    let oz = scz * cs + next_int(&mut rng, 16);
    let oy = if normal_variant {
        // Heightmap MOTION_BLOCKING: one above the topmost solid or
        // lava cell of the column.
        let col = nether_mc::nether_col(ctx, ox.div_euclid(cs), oz.div_euclid(cs));
        let (lx, lz) = (ox.rem_euclid(cs) as usize, oz.rem_euclid(cs) as usize);
        let mut top = SEA_LEVEL - 1; // the lava sea blocks motion
        for y in (0..=TOP_Y).rev() {
            if col.is_solid(lx, y, lz) {
                top = top.max(y);
                break;
            }
        }
        top + 1
    } else {
        next_between(&mut rng, 0, TOP_Y)
    };
    let block = if red { ids.red_mushroom } else { ids.brown_mushroom };
    let bedrock = ids.bedrock;
    offset_patch(scratch, ctx, &mut rng, ox, oy, oz, &mut |s, c, x, y, z| {
        if !s.is_empty(c, x, y, z) {
            return;
        }
        let below = s.get(c, x, y - 1, z);
        if below != 0 && below != LAVA_ID && below != bedrock {
            s.set(x, y, z, block);
        }
    });
}

/// patch_crimson_roots (soul sand valley, step 7): one origin at the
/// CHUNK CORNER (the placed feature has no in_square) with y 0..127 +
/// 96 offsets; places roots on air over nylium / soul soil.
fn place_crimson_roots_patch(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
) {
    let cs = CHUNK_SIZE as i32;
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x34));
    let oy = next_between(&mut rng, 0, TOP_Y);
    let (ox, oz) = (scx * cs, scz * cs);
    let (nyl_c, nyl_w, soil, roots) =
        (ids.crimson_nylium, ids.warped_nylium, ids.soul_soil, ids.crimson_roots);
    offset_patch(scratch, ctx, &mut rng, ox, oy, oz, &mut |s, c, x, y, z| {
        if !s.is_empty(c, x, y, z) {
            return;
        }
        let below = s.get(c, x, y - 1, z);
        if below == nyl_c || below == nyl_w || below == soil {
            s.set(x, y, z, roots);
        }
    });
}

// ---------------------------------------------------------------------------
// Step 7: glowstone blobs (GlowstoneFeature).
// ---------------------------------------------------------------------------

/// One blob: seed cell must be air under a netherrack/basalt/
/// blackstone ceiling; 1500 growth tries each demand exactly one
/// glowstone neighbour (the blob's own cells - terrain never has
/// glowstone, so the neighbour check reads the scratch only).
fn glowstone_blob(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
) {
    if !scratch.is_empty(ctx, ox, oy, oz) {
        return;
    }
    let above = scratch.get(ctx, ox, oy + 1, oz);
    if above != ids.netherrack && above != ids.basalt && above != ids.blackstone {
        return;
    }
    scratch.set(ox, oy, oz, ids.glowstone);
    for _ in 0..1500 {
        let x = ox + next_int(rng, 8) - next_int(rng, 8);
        let y = oy - next_int(rng, 12);
        let z = oz + next_int(rng, 8) - next_int(rng, 8);
        if !scratch.is_empty(ctx, x, y, z) {
            continue;
        }
        let mut neighbours = 0;
        for (dx, dy, dz) in
            [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
        {
            if scratch.get(ctx, x + dx, y + dy, z + dz) == ids.glowstone {
                neighbours += 1;
                if neighbours > 1 {
                    break;
                }
            }
        }
        if neighbours == 1 {
            scratch.set(x, y, z, ids.glowstone);
        }
    }
}

/// glowstone_extra (count biased_to_bottom 0..9, y 4..123) +
/// glowstone (count 10, y 0..127) - every nether biome lists both.
fn place_glowstone(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
) {
    let cs = CHUNK_SIZE as i32;
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x40));
    let extra = biased_to_bottom(&mut rng, 0, 9);
    for _ in 0..extra {
        let ox = scx * cs + next_int(&mut rng, 16);
        let oz = scz * cs + next_int(&mut rng, 16);
        let oy = next_between(&mut rng, 4, TOP_Y - 4);
        glowstone_blob(scratch, ctx, &mut rng, ox, oy, oz, ids);
    }
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x41));
    for _ in 0..10 {
        let ox = scx * cs + next_int(&mut rng, 16);
        let oz = scz * cs + next_int(&mut rng, 16);
        let oy = next_between(&mut rng, 0, TOP_Y);
        glowstone_blob(scratch, ctx, &mut rng, ox, oy, oz, ids);
    }
}

// ---------------------------------------------------------------------------
// Step 7: ores (vein + scattered debris) and replace blobs.
// ---------------------------------------------------------------------------

/// The nether vein-ore placed features (biome-gated below).
const NETHER_ORE_PLACED: &[(&str, &str)] = &[
    ("ore_magma", include_str!("data/feature/placed/ore_magma.json")),
    ("ore_soul_sand", include_str!("data/feature/placed/ore_soul_sand.json")),
    ("ore_gravel_nether", include_str!("data/feature/placed/ore_gravel_nether.json")),
    ("ore_blackstone", include_str!("data/feature/placed/ore_blackstone.json")),
    ("ore_gold_nether", include_str!("data/feature/placed/ore_gold_nether.json")),
    ("ore_quartz_nether", include_str!("data/feature/placed/ore_quartz_nether.json")),
    ("ore_gold_deltas", include_str!("data/feature/placed/ore_gold_deltas.json")),
    ("ore_quartz_deltas", include_str!("data/feature/placed/ore_quartz_deltas.json")),
];

const NETHER_ORE_CONFIGURED: &[(&str, &str)] = &[
    ("ore_magma", include_str!("data/feature/configured/ore_magma.json")),
    ("ore_soul_sand", include_str!("data/feature/configured/ore_soul_sand.json")),
    ("ore_gravel_nether", include_str!("data/feature/configured/ore_gravel_nether.json")),
    ("ore_blackstone", include_str!("data/feature/configured/ore_blackstone.json")),
    ("ore_nether_gold", include_str!("data/feature/configured/ore_nether_gold.json")),
    ("ore_quartz", include_str!("data/feature/configured/ore_quartz.json")),
];

/// The scattered debris pair (ScatteredOreFeature, not the vein walk).
const DEBRIS_PLACED: &[(&str, &str)] = &[
    ("ore_ancient_debris_large", include_str!("data/feature/placed/ore_ancient_debris_large.json")),
    ("ore_debris_small", include_str!("data/feature/placed/ore_debris_small.json")),
];

const DEBRIS_CONFIGURED: &[(&str, &str)] = &[
    ("ore_ancient_debris_large", include_str!("data/feature/configured/ore_ancient_debris_large.json")),
    ("ore_ancient_debris_small", include_str!("data/feature/configured/ore_ancient_debris_small.json")),
];

/// `tags/block/base_stone_nether.json`.
fn base_stone_nether(ids: &Ids) -> [u16; 3] {
    [ids.netherrack, ids.basalt, ids.blackstone]
}

fn parse_nether_ore_rules() -> Vec<OreRule> {
    let ids = Ids::get().expect("nether ore ids");
    let configured: HashMap<&str, serde_json::Value> = NETHER_ORE_CONFIGURED
        .iter()
        .chain(DEBRIS_CONFIGURED)
        .map(|(n, j)| (*n, serde_json::from_str(j).expect("nether ore configured parses")))
        .collect();
    NETHER_ORE_PLACED
        .iter()
        .chain(DEBRIS_PLACED)
        .map(|(name, json)| {
            let placed: serde_json::Value =
                serde_json::from_str(json).expect("nether ore placed parses");
            let feature = placed["feature"].as_str().expect("feature ref");
            let feature = feature.strip_prefix("minecraft:").unwrap_or(feature);
            let conf = configured
                .get(feature)
                .unwrap_or_else(|| panic!("{name}: configured {feature} not embedded"));
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
                    let state = block_id_by_name(state_name)?;
                    // Nether targets: block_match (netherrack) or the
                    // base_stone_nether tag.
                    let replaceable: std::collections::HashSet<u16> =
                        if let Some(block) = t["target"]["block"].as_str() {
                            block_id_by_name(block).into_iter().collect()
                        } else {
                            base_stone_nether(ids).into_iter().collect()
                        };
                    Some(OreTarget { replaceable, state })
                })
                .collect();
            let mut count = CountProvider::Fixed(1);
            let mut height = None;
            for m in placed["placement"].as_array().expect("placement").iter() {
                match m["type"].as_str() {
                    Some("minecraft:count") => count = CountProvider::parse(&m["count"]),
                    Some("minecraft:height_range") => {
                        height = Some(HeightProvider::parse(&m["height"]))
                    }
                    _ => {}
                }
            }
            OreRule {
                name,
                count,
                rarity: None,
                height: height.unwrap_or_else(|| panic!("{name}: no height_range")),
                size,
                discard_on_air,
                targets,
            }
        })
        .collect()
}

fn nether_ore_rules() -> &'static [OreRule] {
    static RULES: OnceLock<Vec<OreRule>> = OnceLock::new();
    RULES.get_or_init(parse_nether_ore_rules)
}

/// ScatteredOreFeature: `nextInt(size + 1)` tries; try `i` offsets
/// each axis by `round((f - f) * min(i, 7))`; a cell in
/// base_stone_nether becomes debris unless air-exposed (the discard
/// chance is 1.0 for both variants = fully buried only).
#[allow(clippy::too_many_arguments)]
fn scattered_ore_try(
    chunk: &mut Chunk,
    origin: [i32; 3],
    ctx: &WorldgenCtx,
    rule: &OreRule,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
) {
    let cs = CHUNK_SIZE as i32;
    let tries = next_int(rng, rule.size as i32 + 1);
    for i in 0..tries {
        let reach = i.min(7);
        let axis = |rng: &mut SplitMix64| -> i32 {
            ((rng.next_f64() - rng.next_f64()) * reach as f64).round() as i32
        };
        let (dx, dy, dz) = (axis(rng), axis(rng), axis(rng));
        let (x, y, z) = (ox + dx, oy + dy, oz + dz);
        let here = nether_mc::nether_block_at(ctx, x, y, z);
        let Some(target) = rule.targets.iter().find(|t| t.replaceable.contains(&here))
        else {
            continue;
        };
        if rule.discard_on_air > 0.0 {
            let exposed = [
                (x + 1, y, z),
                (x - 1, y, z),
                (x, y + 1, z),
                (x, y - 1, z),
                (x, y, z + 1),
                (x, y, z - 1),
            ]
            .into_iter()
            .any(|(nx, ny, nz)| nether_mc::nether_block_at(ctx, nx, ny, nz) == 0);
            if exposed
                && features::pos_roll(x, y, z, ctx.effective_seed()) < rule.discard_on_air
            {
                continue;
            }
        }
        let (lx, ly, lz) = (x - origin[0], y - origin[1], z - origin[2]);
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(target.state));
        }
    }
}

fn place_nether_ores(
    chunk: &mut Chunk,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
    seed: u32,
    scx: i32,
    scz: i32,
    biome: &str,
) {
    let cs = CHUNK_SIZE as i32;
    let deltas = biome == "basalt_deltas";
    for (idx, rule) in nether_ore_rules().iter().enumerate() {
        // Biome gating per the feature lists: deltas swap the
        // gold/quartz rows for the double-count variants and skip
        // gravel; soul_sand is soul-sand-valley only; crimson forest
        // has no soul fire but shares every ore row.
        let allowed = match rule.name {
            "ore_soul_sand" => biome == "soul_sand_valley",
            "ore_gravel_nether" => !deltas,
            "ore_gold_nether" | "ore_quartz_nether" => !deltas,
            "ore_gold_deltas" | "ore_quartz_deltas" => deltas,
            _ => true,
        };
        if !allowed {
            continue;
        }
        let salt = SALT_BASE ^ 0x50 ^ ((idx as u64) << 32);
        let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, salt));
        let scattered = rule.name.contains("debris");
        let count = rule.count.sample(&mut rng);
        for _ in 0..count {
            if scattered {
                let ox = scx * cs + next_int(&mut rng, 16);
                let oz = scz * cs + next_int(&mut rng, 16);
                let Some(oy) = rule.height.sample_in(&mut rng, 0, TOP_Y) else {
                    continue;
                };
                scattered_ore_try(chunk, origin, ctx, rule, &mut rng, ox, oy, oz);
            } else {
                ore_try(chunk, origin, rule, &mut rng, scx, scz, seed, (0, TOP_Y));
            }
        }
    }
}

/// basalt_blobs (75x) + blackstone_blobs (25x), basalt deltas step 7:
/// walk down from a y 0..127 origin to the first netherrack, then
/// replace netherrack within a manhattan ball of per-axis radii
/// uniform 3..7.
fn place_replace_blobs(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
) {
    let cs = CHUNK_SIZE as i32;
    for (salt_off, count, state) in
        [(0x60u64, 75u32, ids.basalt), (0x61, 25, ids.blackstone)]
    {
        let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ salt_off));
        for _ in 0..count {
            let ox = scx * cs + next_int(&mut rng, 16);
            let oz = scz * cs + next_int(&mut rng, 16);
            let mut oy = next_between(&mut rng, 0, TOP_Y);
            // findTarget: down to the first netherrack.
            let mut found = false;
            while oy > 1 {
                if scratch.get(ctx, ox, oy, oz) == ids.netherrack {
                    found = true;
                    break;
                }
                oy -= 1;
            }
            let rx = next_between(&mut rng, 3, 7);
            let ry = next_between(&mut rng, 3, 7);
            let rz = next_between(&mut rng, 3, 7);
            if !found {
                continue;
            }
            let max_r = rx.max(ry).max(rz);
            for dx in -rx..=rx {
                for dy in -ry..=ry {
                    for dz in -rz..=rz {
                        if dx.abs() + dy.abs() + dz.abs() > max_r {
                            continue;
                        }
                        let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                        if scratch.get(ctx, x, y, z) == ids.netherrack {
                            scratch.set(x, y, z, state);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Step 2: basalt pillars (soul sand valley).
// ---------------------------------------------------------------------------

/// BasaltPillarFeature: from an air cell under a ceiling, drop a
/// basalt column to the ground with 9-in-10 side hang-offs that stop
/// for good on their first miss, then a rounded 7x7 base apron.
fn basalt_pillar(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    rng: &mut SplitMix64,
    ox: i32,
    oy: i32,
    oz: i32,
    ids: &Ids,
) {
    if !scratch.is_empty(ctx, ox, oy, oz) || scratch.is_empty(ctx, ox, oy + 1, oz) {
        return;
    }
    let mut y = oy;
    let mut hang = [true; 4];
    const SIDES: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    while scratch.is_empty(ctx, ox, y, oz) {
        if y < 0 {
            return;
        }
        scratch.set(ox, y, oz, ids.basalt);
        for (i, (dx, dz)) in SIDES.iter().enumerate() {
            if !hang[i] {
                continue;
            }
            if next_int(rng, 10) != 0 {
                scratch.set(ox + dx, y, oz + dz, ids.basalt);
            } else {
                hang[i] = false;
            }
        }
        y -= 1;
    }
    y += 1;
    for (dx, dz) in SIDES {
        if rng.next_u64() & 1 == 0 {
            scratch.set(ox + dx, y, oz + dz, ids.basalt);
        }
    }
    y -= 1;
    for dx in -3i32..4 {
        for dz in -3i32..4 {
            let p = dx * dz;
            if next_int(rng, 10) < 10 - p.abs() {
                let (bx, bz) = (ox + dx, oz + dz);
                let mut by = y;
                let mut drop = 3;
                while scratch.is_empty(ctx, bx, by - 1, bz) {
                    by -= 1;
                    drop -= 1;
                    if drop <= 0 {
                        break;
                    }
                }
                if !scratch.is_empty(ctx, bx, by - 1, bz) {
                    scratch.set(bx, by, bz, ids.basalt);
                }
            }
        }
    }
}

fn place_basalt_pillars(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
) {
    let cs = CHUNK_SIZE as i32;
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x70));
    for _ in 0..10 {
        let ox = scx * cs + next_int(&mut rng, 16);
        let oz = scz * cs + next_int(&mut rng, 16);
        let oy = next_between(&mut rng, 0, TOP_Y);
        basalt_pillar(scratch, ctx, &mut rng, ox, oy, oz, ids);
    }
}

// ---------------------------------------------------------------------------
// Step 4: deltas + basalt columns (basalt deltas).
// ---------------------------------------------------------------------------

fn delta_cannot_replace(id: u16, ids: &Ids) -> bool {
    id == ids.bedrock
}

/// DeltaFeature.isClear: not already contents, not protected, open to
/// the sky-above with a closed floor and walls.
fn delta_is_clear(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    x: i32,
    y: i32,
    z: i32,
    ids: &Ids,
) -> bool {
    let state = scratch.get(ctx, x, y, z);
    if state == LAVA_ID || delta_cannot_replace(state, ids) {
        return false;
    }
    if !scratch.is_empty(ctx, x, y + 1, z) {
        return false;
    }
    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
        if scratch.is_empty(ctx, x + dx, y + dy, z + dz) {
            return false;
        }
    }
    true
}

/// DeltaFeature: a manhattan-diamond lava pool with a 90%-chance
/// offset magma rim, on every layer x40.
fn place_deltas(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
) {
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, SALT_BASE ^ 0x80));
    let positions = every_layer_positions(ctx, &mut rng, scx, scz, 40);
    for [ox, oy, oz] in positions {
        let spawn_rim = rng.next_f64() < 0.9;
        let rim_x = if spawn_rim { next_between(&mut rng, 0, 2) } else { 0 };
        let rim_z = if spawn_rim { next_between(&mut rng, 0, 2) } else { 0 };
        let has_rim = spawn_rim && rim_x != 0 && rim_z != 0;
        let radius_x = next_between(&mut rng, 3, 7);
        let radius_z = next_between(&mut rng, 3, 7);
        let max_r = radius_x.max(radius_z);
        for dx in -radius_x..=radius_x {
            for dz in -radius_z..=radius_z {
                if dx.abs() + dz.abs() > max_r {
                    continue;
                }
                let (x, z) = (ox + dx, oz + dz);
                if delta_is_clear(scratch, ctx, x, oy, z, ids) {
                    if has_rim {
                        scratch.set(x, oy, z, ids.magma);
                    }
                    let (rx, rz) = (x + rim_x, z + rim_z);
                    if delta_is_clear(scratch, ctx, rx, oy, rz, ids) {
                        scratch.set(rx, oy, rz, LAVA_ID);
                    }
                }
            }
        }
    }
}

fn is_air_or_lava_ocean(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    let id = scratch.get(ctx, x, y, z);
    id == 0 || (id == LAVA_ID && y <= SEA_LEVEL)
}

fn column_cannot_place_on(id: u16, ids: &Ids) -> bool {
    id == LAVA_ID || id == ids.bedrock || id == ids.magma || id == ids.soul_sand
}

fn column_can_place_at(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    x: i32,
    y: i32,
    z: i32,
    ids: &Ids,
) -> bool {
    if !is_air_or_lava_ocean(scratch, ctx, x, y, z) {
        return false;
    }
    let below = scratch.get(ctx, x, y - 1, z);
    below != 0 && !column_cannot_place_on(below, ids)
}

/// BasaltColumnsFeature: a cluster of stubby columns rising from the
/// delta floor (small: height 1..4 reach 1 x4; large: height 5..10
/// reach 2..3 x2, on every layer).
#[allow(clippy::too_many_arguments)]
fn place_basalt_columns(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    seed: u32,
    scx: i32,
    scz: i32,
    ids: &Ids,
    large: bool,
) {
    let salt = SALT_BASE ^ if large { 0x82 } else { 0x81 };
    let mut rng = SplitMix64::new(stream_seed(seed, scx, scz, salt));
    let per_layer = if large { 2 } else { 4 };
    let positions = every_layer_positions(ctx, &mut rng, scx, scz, per_layer);
    for [ox, oy, oz] in positions {
        if !column_can_place_at(scratch, ctx, ox, oy, oz, ids) {
            continue;
        }
        let height = if large {
            next_between(&mut rng, 5, 10)
        } else {
            next_between(&mut rng, 1, 4)
        };
        let clustered = rng.next_f64() < 0.9;
        let reach = height.min(if clustered { 5 } else { 8 });
        let count = if clustered { 50 } else { 15 };
        for _ in 0..count {
            let px = ox + next_between(&mut rng, -reach, reach);
            let pz = oz + next_between(&mut rng, -reach, reach);
            let dist = (px - ox).abs() + (pz - oz).abs();
            let blocks_y = height - dist;
            if blocks_y < 0 {
                continue;
            }
            let col_reach = if large {
                next_between(&mut rng, 2, 3)
            } else {
                1
            };
            place_one_column(scratch, ctx, px, oy, pz, blocks_y, col_reach, ids);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn place_one_column(
    scratch: &mut Scratch,
    ctx: &WorldgenCtx,
    ox: i32,
    oy: i32,
    oz: i32,
    height: i32,
    reach: i32,
    ids: &Ids,
) {
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let (x, z) = (ox + dx, oz + dz);
            let step_limit = dx.abs() + dz.abs();
            // Find the column base: drop to a floor when starting in
            // open space, else climb to the first air.
            let base = if is_air_or_lava_ocean(scratch, ctx, x, oy, z) {
                let mut y = oy;
                let mut limit = step_limit;
                loop {
                    if y <= 1 || limit <= 0 {
                        break None;
                    }
                    limit -= 1;
                    if column_can_place_at(scratch, ctx, x, y, z, ids) {
                        break Some(y);
                    }
                    y -= 1;
                }
            } else {
                let mut y = oy;
                let mut limit = step_limit;
                loop {
                    if y > TOP_Y || limit <= 0 {
                        break None;
                    }
                    limit -= 1;
                    let id = scratch.get(ctx, x, y, z);
                    if column_cannot_place_on(id, ids) {
                        break None;
                    }
                    if id == 0 {
                        break Some(y);
                    }
                    y += 1;
                }
            };
            let Some(mut y) = base else {
                continue;
            };
            let mut blocks = height - step_limit / 2;
            while blocks >= 0 {
                if is_air_or_lava_ocean(scratch, ctx, x, y, z) {
                    scratch.set(x, y, z, ids.basalt);
                    y += 1;
                } else if scratch.get(ctx, x, y, z) == ids.basalt {
                    y += 1;
                } else {
                    break;
                }
                blocks -= 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The dispatcher.
// ---------------------------------------------------------------------------

/// Per-chunk nether decoration (Nether profile only; called from
/// `worldgen::decor` after the terrain fill). The 3x3 source-column
/// replay covers every scatter radius above.
pub(crate) fn place_from_coord_nether(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Nether) {
        return;
    }
    let Some(ids) = Ids::get() else {
        return;
    };
    let origin_v = World::chunk_world_origin(coord);
    let origin = [origin_v.x as i32, origin_v.y as i32, origin_v.z as i32];
    if origin[1] > TOP_Y {
        return;
    }
    let seed = ctx.effective_seed();

    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let biome = biome_of(ctx, scx, scz);
            let mut scratch = Scratch::new(chunk, origin);

            // Step 2 (local_modifications): soul-sand-valley pillars.
            if biome == "soul_sand_valley" {
                place_basalt_pillars(&mut scratch, ctx, seed, scx, scz, ids);
            }
            // Step 4 (surface_structures): the deltas' landscape.
            if biome == "basalt_deltas" {
                place_deltas(&mut scratch, ctx, seed, scx, scz, ids);
                place_basalt_columns(&mut scratch, ctx, seed, scx, scz, ids, false);
                place_basalt_columns(&mut scratch, ctx, seed, scx, scz, ids, true);
            }
            // Step 7 (underground_decoration), biome-list order.
            if biome == "basalt_deltas" {
                place_replace_blobs(&mut scratch, ctx, seed, scx, scz, ids);
            }
            place_fire_patches(&mut scratch, ctx, seed, scx, scz, ids, false);
            if biome != "crimson_forest" {
                place_fire_patches(&mut scratch, ctx, seed, scx, scz, ids, true);
            }
            place_glowstone(&mut scratch, ctx, seed, scx, scz, ids);
            if matches!(biome, "nether_wastes" | "basalt_deltas") {
                place_nether_mushrooms(&mut scratch, ctx, seed, scx, scz, ids, false, false);
                place_nether_mushrooms(&mut scratch, ctx, seed, scx, scz, ids, true, false);
            }
            if biome == "soul_sand_valley" {
                place_crimson_roots_patch(&mut scratch, ctx, seed, scx, scz, ids);
            }
            place_nether_ores(chunk, ctx, origin, seed, scx, scz, biome);

            // Step 9 (vegetal_decoration).
            let mut scratch = Scratch::new(chunk, origin);
            super::nether_vegetation::place_vegetal(
                &mut scratch,
                ctx,
                seed,
                scx,
                scz,
                ids,
                biome,
            );
        }
    }

    // Springs are chunk-local (single cells; every vertical chunk
    // rolls its own stream) - no replay loop.
    let biome = biome_of(ctx, coord[0], coord[2]);
    place_nether_springs(chunk, coord, ctx, origin, ids, biome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx { base_seed: 0, dim: "nether".into(), profile: WorldGen::Nether }
    }

    /// Find one chunk column per biome within a scan (all five exist
    /// - pinned by nether_mc's biome test).
    fn columns_per_biome(c: &WorldgenCtx) -> HashMap<&'static str, (i32, i32)> {
        let mut found: HashMap<&'static str, (i32, i32)> = HashMap::new();
        'scan: for r in 0..90 {
            for (cx, cz) in [(r, 0), (-r, 0), (0, r), (0, -r), (r, r), (-r, -r)] {
                let b = biome_of(c, cx * 3, cz * 3);
                found.entry(b).or_insert((cx * 3, cz * 3));
                if found.len() == 5 {
                    break 'scan;
                }
            }
        }
        found
    }

    fn count_column(c: &WorldgenCtx, cx: i32, cz: i32, tally: &mut HashMap<u16, u32>) {
        for cy in 0..8 {
            let ch = super::super::nether_mc::gen_nether_mc([cx, cy, cz], c);
            for x in 0..CHUNK_SIZE {
                for y in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
                        *tally.entry(ch.get(x, y, z).0).or_default() += 1;
                    }
                }
            }
        }
    }

    /// Per-biome feature-block census over a few generated columns.
    /// `--ignored --nocapture` to inspect the raw counts.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_nether_feature_census() {
        let c = ctx();
        for (biome, (cx, cz)) in columns_per_biome(&c) {
            let mut tally: HashMap<u16, u32> = HashMap::new();
            for dx in 0..2 {
                for dz in 0..2 {
                    count_column(&c, cx + dx, cz + dz, &mut tally);
                }
            }
            let mut rows: Vec<(u16, u32)> = tally.into_iter().collect();
            rows.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            println!("=== {biome} @ chunk ({cx},{cz}) ===");
            for (id, n) in rows.iter().take(24) {
                let name = crate::block::block_type(*id).map(|b| b.name).unwrap_or("?");
                println!("  {id:4} {name:24} {n}");
            }
        }
    }

    /// The nether decoration materializes: glowstone + quartz
    /// everywhere, biome-signature blocks in their homes (fungi
    /// stems + wart hats in the forests, magma delta rims + basalt
    /// pillars in the deltas / soul valley).
    #[test]
    fn nether_features_materialize_per_biome() {
        let c = ctx();
        let ids = Ids::get().unwrap();
        let found = columns_per_biome(&c);
        assert_eq!(found.len(), 5, "biomes found: {found:?}");
        let mut census: HashMap<&'static str, HashMap<u16, u32>> = HashMap::new();
        for (biome, (cx, cz)) in &found {
            let tally = census.entry(biome).or_default();
            for dx in 0..2 {
                for dz in 0..2 {
                    count_column(&c, cx + dx, cz + dz, tally);
                }
            }
        }
        let total = |id: u16| -> u32 { census.values().map(|t| t.get(&id).copied().unwrap_or(0)).sum() };
        let quartz = block_id_by_name("nether_quartz_ore").unwrap();
        let gold = block_id_by_name("nether_gold_ore").unwrap();
        assert!(total(ids.glowstone) > 50, "glowstone blobs: {}", total(ids.glowstone));
        assert!(total(quartz) > 100, "quartz veins: {}", total(quartz));
        assert!(total(gold) > 50, "nether gold veins: {}", total(gold));
        assert!(total(ids.fire) > 0, "fire patches: {}", total(ids.fire));

        let crimson = &census["crimson_forest"];
        let stem = block_id_by_name("crimson_stem").unwrap();
        let wart = block_id_by_name("nether_wart_block").unwrap();
        assert!(crimson.get(&stem).copied().unwrap_or(0) > 20, "crimson stems: {crimson:?}");
        assert!(crimson.get(&wart).copied().unwrap_or(0) > 20, "crimson hats");

        let warped = &census["warped_forest"];
        let wstem = block_id_by_name("warped_stem").unwrap();
        let wroots = block_id_by_name("warped_roots").unwrap();
        assert!(warped.get(&wstem).copied().unwrap_or(0) > 20, "warped stems");
        assert!(warped.get(&wroots).copied().unwrap_or(0) > 20, "warped roots cover");

        let deltas = &census["basalt_deltas"];
        assert!(deltas.get(&ids.magma).copied().unwrap_or(0) > 20, "delta magma rims");
        assert!(deltas.get(&ids.basalt).copied().unwrap_or(0) > 500, "delta basalt");

        let soul = &census["soul_sand_valley"];
        assert!(soul.get(&ids.basalt).copied().unwrap_or(0) > 20, "soul valley pillars");
    }
}
