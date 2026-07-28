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

/// W222: where a disk try anchors + what gates it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DiskGate {
    /// Origin cell (floor + 1) must be water (sand/clay/gravel disks).
    Underwater,
    /// Origin is the FLOOR cell itself (random_offset y -1) and must be
    /// this block (ice_patch on snow_block, disk_grass on mud).
    FloorIs(&'static str),
}

/// One disk rule (from the disk_* / ice_patch configured JSONs).
struct DiskDef {
    /// Fallback state + the below-is-air override (sand -> sandstone).
    state: u16,
    below_air_state: Option<u16>,
    /// W222 disk_grass rule: grass_block where nothing solid/watery
    /// sits above, dirt otherwise.
    grass_when_open_above: bool,
    /// Blocks the disk replaces.
    targets: &'static [&'static str],
    half_height: i32,
    r_min: i32,
    r_max: i32,
    /// count placement (disk_sand 3; the rest 1-2).
    count: u32,
    gate: DiskGate,
    /// Biome restriction: None = the 33-biome default set (everything
    /// except the swamps); Some = exactly these.
    biomes: Option<&'static [McBiome]>,
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
                grass_when_open_above: false,
                targets: &["dirt", "grass_block"],
                half_height: 2,
                r_min: 2,
                r_max: 6,
                count: 3,
                gate: DiskGate::Underwater,
                biomes: None,
            },
            // disk_clay: r 2..3, half 1, targets dirt+clay, count 1
            // (every biome incl. the swamps).
            DiskDef {
                state: id("clay"),
                below_air_state: None,
                grass_when_open_above: false,
                targets: &["dirt", "clay"],
                half_height: 1,
                r_min: 2,
                r_max: 3,
                count: 1,
                gate: DiskGate::Underwater,
                biomes: Some(McBiome::ALL.as_slice()),
            },
            // disk_gravel: r 2..5, half 2, targets dirt+grass, count 1.
            DiskDef {
                state: id("gravel"),
                below_air_state: None,
                grass_when_open_above: false,
                targets: &["dirt", "grass_block"],
                half_height: 2,
                r_min: 2,
                r_max: 5,
                count: 1,
                gate: DiskGate::Underwater,
                biomes: None,
            },
            // W222 ice_patch (snowy_slopes): packed ice poking through
            // the snow, r 2..3, half 1, count 2, anchored on snow_block.
            DiskDef {
                state: id("packed_ice"),
                below_air_state: None,
                grass_when_open_above: false,
                targets: &[
                    "dirt", "grass_block", "podzol", "coarse_dirt", "mycelium",
                    "snow_block", "ice",
                ],
                half_height: 1,
                r_min: 2,
                r_max: 3,
                count: 2,
                gate: DiskGate::FloorIs("snow_block"),
                biomes: Some(&[McBiome::SnowySlopes]),
            },
            // W222 disk_grass (mangrove swamp): grass/dirt patches in
            // the mud, r 2..6, half 2, count 1, anchored on mud.
            DiskDef {
                state: id("dirt"),
                below_air_state: None,
                grass_when_open_above: true,
                targets: &["dirt", "mud"],
                half_height: 2,
                r_min: 2,
                r_max: 6,
                count: 1,
                gate: DiskGate::FloorIs("mud"),
                biomes: Some(&[McBiome::MangroveSwamp]),
            },
        ]
    })
}

fn target_ids(names: &[&str]) -> HashSet<u16> {
    names.iter().filter_map(|n| block_id_by_name(n)).collect()
}

/// W373: `IcebergFeature` - packed/blue-ice bergs in the frozen
/// oceans (`iceberg_packed` at 1-in-16 chunks, `iceberg_blue` at
/// 1-in-200). Vanilla builds a noise-perturbed double ellipsoid;
/// this keeps the two lobes (a cap above sea level, a wider keel
/// below) without the jitter. 3x3 source replay so a berg crosses
/// chunk borders seamlessly; writes replace only water/air, never
/// terrain.
pub(crate) fn place_icebergs(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    const SALT_ICEBERG: u64 = 0x1CE0_0373;
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let sea = crate::worldgen::climate::SEA_LEVEL;
    let Some(packed) = block_id_by_name("packed_ice") else {
        return;
    };
    let blue = block_id_by_name("blue_ice").unwrap_or(packed);
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            if !matches!(mc, McBiome::FrozenOcean | McBiome::DeepFrozenOcean) {
                continue;
            }
            let mut rng =
                SplitMix64::new(stream_seed(seed, scx, scz, SALT_ICEBERG));
            let roll = next_int(&mut rng, 200);
            let ice = if roll == 0 {
                blue
            } else if roll < 12 {
                packed
            } else {
                continue;
            };
            let wx = scx * cs + next_int(&mut rng, 16);
            let wz = scz * cs + next_int(&mut rng, 16);
            let h_above = 2 + next_int(&mut rng, 7);
            let r_cap = 3 + next_int(&mut rng, 4);
            let depth = 3 + next_int(&mut rng, 5);
            let r_keel = r_cap + 2;
            for dy in -depth..=h_above {
                let (r, hh) = if dy >= 0 {
                    (r_cap as f32, h_above as f32)
                } else {
                    (r_keel as f32, depth as f32)
                };
                let t = 1.0 - (dy as f32 / hh.max(1.0)).powi(2);
                if t <= 0.0 {
                    continue;
                }
                let rr = (r * t.sqrt()).floor() as i32;
                for dx in -rr..=rr {
                    for dz in -rr..=rr {
                        if dx * dx + dz * dz > rr * rr {
                            continue;
                        }
                        let (x, y, z) = (wx + dx, sea + dy, wz + dz);
                        let (lx, ly, lz) =
                            (x - origin[0], y - origin[1], z - origin[2]);
                        if !(0..cs).contains(&lx)
                            || !(0..cs).contains(&ly)
                            || !(0..cs).contains(&lz)
                        {
                            continue;
                        }
                        let here =
                            chunk.get(lx as usize, ly as usize, lz as usize);
                        if here.is_air()
                            || crate::block::block_is_watery(here.0)
                        {
                            chunk.set(
                                lx as usize,
                                ly as usize,
                                lz as usize,
                                BlockId(ice),
                            );
                        }
                    }
                }
            }
        }
    }
}

/// W219: the underwater floor disks (step 6, after ores). 3x3 source
/// replay; a try only runs where the origin cell is water (the placed
/// predicate).
/// W427: one attempted-disk placement from a source chunk's
/// stream. Everything COLUMN-derived (biome, floor, radius rolls)
/// is resolved once per source and memoised; the chunk-dependent
/// half (the FloorIs anchor gate + the target-block writes) stays
/// in `place_disks`. Before this every Y-chunk of the 24-tall
/// column re-paid 9 sources of cold `mc_floor_at` column solves -
/// the W310/W404 bench still showed disks at ~80 ms/chunk cold.
struct DiskPlacement {
    di: u8,
    wx: i32,
    wy: i32,
    wz: i32,
    r: i32,
}

type DisksMemoMap =
    std::collections::HashMap<(i32, i32, u32), std::rc::Rc<Vec<DiskPlacement>>>;

thread_local! {
    static DISKS_MEMO: std::cell::RefCell<DisksMemoMap> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn disk_placements_for_source(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> std::rc::Rc<Vec<DiskPlacement>> {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    if let Some(hit) =
        DISKS_MEMO.with(|m| m.borrow().get(&(scx, scz, seed)).cloned())
    {
        return hit;
    }
    let mut out: Vec<DiskPlacement> = Vec::new();
    for (di, def) in disk_defs().iter().enumerate() {
        if let Some(list) = def.biomes {
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            if !list.contains(&mc) {
                continue;
            }
        } else {
            // The 33-biome default set: everything but the swamps.
            let mc = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
            if matches!(mc, McBiome::Swamp | McBiome::MangroveSwamp) {
                continue;
            }
        }
        let mut rng = SplitMix64::new(stream_seed(
            seed,
            scx,
            scz,
            SALT_DISK ^ ((di as u64) << 40),
        ));
        for _ in 0..def.count {
            let wx = scx * cs + next_int(&mut rng, 16);
            let wz = scz * cs + next_int(&mut rng, 16);
            let floor = mc_floor_at(ctx, wx, wz);
            let wy = match def.gate {
                DiskGate::Underwater => floor + 1,
                DiskGate::FloorIs(_) => floor,
            };
            // The radius roll consumes REGARDLESS of the gate (the
            // pre-W427 code burned one equal-width roll on a gate
            // fail), so rolling here keeps the stream aligned while
            // the chunk-dependent FloorIs gate moves to the apply.
            let r = next_between(&mut rng, def.r_min, def.r_max);
            if matches!(def.gate, DiskGate::Underwater)
                && wy >= crate::worldgen::climate::SEA_LEVEL
            {
                continue;
            }
            out.push(DiskPlacement { di: di as u8, wx, wy, wz, r });
        }
    }
    let rc = std::rc::Rc::new(out);
    DISKS_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 4096 {
            m.clear();
        }
        m.insert((scx, scz, seed), rc.clone());
    });
    rc
}

pub(crate) fn place_disks(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    // W404: disks are a SURFACE feature (a few blocks around the
    // floor line) - chunks whose Y range cannot touch the disk band
    // skip everything.
    {
        let ow = super::terrain::overworld_for(seed);
        let prelim =
            ow.preliminary_surface(coord[0] * cs + 8, coord[2] * cs + 8);
        if origin[1] > prelim + 24 || origin[1] + cs < prelim - 24 {
            return;
        }
    }
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            let placements = disk_placements_for_source(ctx, scx, scz);
            for p in placements.iter() {
                let def = &disk_defs()[p.di as usize];
                let targets = target_ids(def.targets);
                let (wx, wy, wz, r) = (p.wx, p.wy, p.wz, p.r);
                if let DiskGate::FloorIs(name) = def.gate {
                    // Chunk-local anchor read; off-chunk anchors
                    // skip (their own column's replay covers them).
                    let anchor_block = block_id_by_name(name);
                    let (lx, ly, lz) =
                        (wx - origin[0], wy - origin[1], wz - origin[2]);
                    let ok = (0..cs).contains(&lx)
                        && (0..cs).contains(&ly)
                        && (0..cs).contains(&lz)
                        && anchor_block
                            == Some(
                                chunk
                                    .get(lx as usize, ly as usize, lz as usize)
                                    .0,
                            );
                    if !ok {
                        continue;
                    }
                }
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
                            // rule_based providers: sandstone under
                            // air (disk_sand); grass where nothing
                            // sits above (disk_grass).
                            let below_air = ly > 0
                                && chunk.get(lx as usize, (ly - 1) as usize, lz as usize).is_air();
                            let open_above = ly + 1 < cs
                                && chunk.get(lx as usize, (ly + 1) as usize, lz as usize).is_air();
                            let state = if def.grass_when_open_above && open_above {
                                3 // grass_block
                            } else {
                                match (below_air, def.below_air_state) {
                                    (true, Some(s)) => s,
                                    _ => def.state,
                                }
                            };
                            chunk.set(lx as usize, ly as usize, lz as usize, BlockId(state));
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
/// W222: spring_lava_frozen's valid set (snow / ice rock).
fn spring_valid_frozen() -> &'static HashSet<u16> {
    static SET: OnceLock<HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        ["snow_block", "powder_snow", "packed_ice"]
            .iter()
            .filter_map(|n| block_id_by_name(n))
            .collect()
    })
}

pub(crate) fn place_springs(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx, origin: [i32; 3]) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    // W222: snowy_slopes additionally runs the frozen lava variant
    // (springs through snow/ice walls).
    let frozen_biome = matches!(
        mc_full_biome_at(ctx, coord[0] * cs + 8, coord[2] * cs + 8),
        McBiome::SnowySlopes
    );
    // (fluid id, count, salt, lava?, frozen valid set?).
    let mut rows: Vec<(u16, u32, u64, bool, bool)> = vec![
        (36u16, 25, SALT_SPRING, false, false),
        (37, 20, SALT_SPRING ^ (1 << 40), true, false),
    ];
    if frozen_biome {
        rows.push((37, 20, SALT_SPRING ^ (2 << 40), true, true));
    }
    for (fluid, count, salt, lava, frozen) in rows {
        let valid = if frozen { spring_valid_frozen() } else { spring_valid() };
        let mut rng = SplitMix64::new(stream_seed(seed, coord[0], coord[2], salt ^ (coord[1] as u64) << 8));
        for _ in 0..count {
            let lx = next_int(&mut rng, 16);
            let lz = next_int(&mut rng, 16);
            let wy = if lava {
                // very_biased_to_bottom(above_bottom 0 = -64,
                // below_top 8 = 311, inner 8) - W260: MC anchors.
                let upper = next_between(&mut rng, -64 + 8, 311);
                let biased = next_between(&mut rng, -64, upper - 1);
                next_between(&mut rng, -64, biased - 1 + 8)
            } else {
                // uniform(above_bottom 0 = -64, absolute 192).
                next_between(&mut rng, -64, 192)
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

const SALT_LAKE: u64 = 0x1A7E_0000;

/// W220: `LakeFeature` (lava lakes). The lake occupies a 16x8x16 box;
/// 4-7 random ellipsoid "spots" union into the body; the box border is
/// validated (no liquids beside the upper half, solid ground beside
/// the lower half), then the body fills (lava below the y+4 waterline,
/// air above) and a stone barrier wraps adjacent solid cells (always
/// below the waterline, 1-in-2 above).
///
/// Cross-chunk determinism: every abort / barrier decision reads the
/// PRE-DECORATION solidity from the cached column bitsets (
/// `terrain::column_is_solid`) - replay-invariant, so all chunks the
/// box touches agree; the writes themselves stay chunk-local and
/// respect the live chunk (bedrock guard). Honest divergences: the
/// border "liquid" test is open-below-sea (aquifer cave lakes are not
/// seen), and carver tunnels are invisible to the abort (a lake next
/// to a fresh tunnel may spill - the liquid sim handles it, as MC's
/// scheduled ticks do).
pub(crate) fn place_lava_lakes(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let seed = ctx.effective_seed();
    let solid = |x: i32, y: i32, z: i32| super::terrain::column_is_solid(ctx, x, y, z);
    let liquidish = |x: i32, y: i32, z: i32| {
        !solid(x, y, z) && y < crate::worldgen::climate::SEA_LEVEL
    };
    for scx in (coord[0] - 1)..=(coord[0] + 1) {
        for scz in (coord[2] - 1)..=(coord[2] + 1) {
            // (salt, rarity, underground?)
            for (k, chance, underground) in [(0u64, 200u64, false), (1, 9, true)] {
                let mut rng =
                    SplitMix64::new(stream_seed(seed, scx, scz, SALT_LAKE ^ (k << 40)));
                // rarity_filter.
                if !rng.next_u64().is_multiple_of(chance) {
                    continue;
                }
                let px = scx * cs + next_int(&mut rng, 16);
                let pz = scz * cs + next_int(&mut rng, 16);
                let floor = mc_floor_at(ctx, px, pz);
                let py = if underground {
                    // uniform [absolute 0 .. below_top 0] = 0..=319,
                    // environment_scan down (max 32) to solid with
                    // inside_world_bounds offset -5, then the
                    // OCEAN_FLOOR-relative threshold <= -5.
                    let start = next_between(&mut rng, 0, 319);
                    let mut y = start;
                    let mut found = false;
                    for _ in 0..=32 {
                        if solid(px, y, pz) {
                            found = true;
                            break;
                        }
                        y -= 1;
                        if y - 5 < -64 {
                            break;
                        }
                    }
                    if !found || y > floor - 5 {
                        continue;
                    }
                    y
                } else {
                    floor + 1
                };
                if py <= 4 {
                    continue; // level.getMinY() + 4 guard
                }
                lava_lake(chunk, origin, &mut rng, [px - 8, py - 4, pz - 8], &solid, &liquidish);
            }
        }
    }
}

/// One lake body at box origin `base` (16x8x16). Rolls, checks and
/// barrier decisions run in MC's exact scan order so every chunk's
/// replay agrees.
fn lava_lake(
    chunk: &mut Chunk,
    origin: [i32; 3],
    rng: &mut SplitMix64,
    base: [i32; 3],
    solid: &dyn Fn(i32, i32, i32) -> bool,
    liquidish: &dyn Fn(i32, i32, i32) -> bool,
) {
    let cs = CHUNK_SIZE as i32;
    let mut grid = [false; 2048];
    let idx = |x: i32, z: i32, y: i32| ((x * 16 + z) * 8 + y) as usize;
    let spots = next_int(rng, 4) + 4;
    for _ in 0..spots {
        let xr = rng.next_f64() * 6.0 + 3.0;
        let yr = rng.next_f64() * 4.0 + 2.0;
        let zr = rng.next_f64() * 6.0 + 3.0;
        let xp = rng.next_f64() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
        let yp = rng.next_f64() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
        let zp = rng.next_f64() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
        for xx in 1..15 {
            for zz in 1..15 {
                for yy in 1..7 {
                    let xd = (xx as f64 - xp) / (xr / 2.0);
                    let yd = (yy as f64 - yp) / (yr / 2.0);
                    let zd = (zz as f64 - zp) / (zr / 2.0);
                    if xd * xd + yd * yd + zd * zd < 1.0 {
                        grid[idx(xx, zz, yy)] = true;
                    }
                }
            }
        }
    }
    let borders = |grid: &[bool; 2048], xx: i32, zz: i32, yy: i32| -> bool {
        !grid[idx(xx, zz, yy)]
            && ((xx < 15 && grid[idx(xx + 1, zz, yy)])
                || (xx > 0 && grid[idx(xx - 1, zz, yy)])
                || (zz < 15 && grid[idx(xx, zz + 1, yy)])
                || (zz > 0 && grid[idx(xx, zz - 1, yy)])
                || (yy < 7 && grid[idx(xx, zz, yy + 1)])
                || (yy > 0 && grid[idx(xx, zz, yy - 1)]))
    };
    // Border validation (replay-invariant world view).
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                if !borders(&grid, xx, zz, yy) {
                    continue;
                }
                let (wx, wy, wz) = (base[0] + xx, base[1] + yy, base[2] + zz);
                if yy >= 4 && liquidish(wx, wy, wz) {
                    return;
                }
                if yy < 4 && !solid(wx, wy, wz) {
                    return;
                }
            }
        }
    }
    // Fill: lava below the waterline (yy < 4), air above. Writes are
    // chunk-local; bedrock stays.
    let bedrock_guard = |chunk: &Chunk, lx: i32, ly: i32, lz: i32| {
        let id = chunk.get(lx as usize, ly as usize, lz as usize).0;
        crate::block::block_type(id).is_some_and(|bt| bt.name == "bedrock")
    };
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                if !grid[idx(xx, zz, yy)] {
                    continue;
                }
                let (wx, wy, wz) = (base[0] + xx, base[1] + yy, base[2] + zz);
                let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
                if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
                    continue;
                }
                if bedrock_guard(chunk, lx, ly, lz) {
                    continue;
                }
                let state = if yy >= 4 { BlockId::AIR } else { BlockId(37) };
                chunk.set(lx as usize, ly as usize, lz as usize, state);
            }
        }
    }
    // Stone barrier around the body: solid neighbours become stone
    // (always below the waterline, 1-in-2 above - the roll burns in
    // scan order whether or not the cell lands in this chunk).
    for xx in 0..16 {
        for zz in 0..16 {
            for yy in 0..8 {
                if !borders(&grid, xx, zz, yy) {
                    continue;
                }
                let keep_roll = yy >= 4 && next_int(rng, 2) == 0;
                if keep_roll {
                    continue;
                }
                let (wx, wy, wz) = (base[0] + xx, base[1] + yy, base[2] + zz);
                if !solid(wx, wy, wz) {
                    continue;
                }
                let (lx, ly, lz) = (wx - origin[0], wy - origin[1], wz - origin[2]);
                if !(0..cs).contains(&lx) || !(0..cs).contains(&ly) || !(0..cs).contains(&lz) {
                    continue;
                }
                if bedrock_guard(chunk, lx, ly, lz) {
                    continue;
                }
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(1));
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
        assert_eq!(defs.len(), 5);
        // sand disk paints sandstone under air; clay resolves the W219
        // block; the W222 rows anchor on their gate blocks.
        assert!(defs[0].below_air_state.is_some());
        assert_eq!(block_id_by_name("clay"), Some(defs[1].state));
        assert_eq!(defs[3].gate, DiskGate::FloorIs("snow_block"));
        assert!(defs[4].grass_when_open_above);
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
            let upper = next_between(&mut r, -64 + 8, 311);
            let biased = next_between(&mut r, -64, upper - 1);
            let y = next_between(&mut r, -64, biased - 1 + 8);
            assert!((-64..=311).contains(&y));
        }
    }
}
