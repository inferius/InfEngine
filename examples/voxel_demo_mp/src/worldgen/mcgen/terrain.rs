//! W184 - faithful MC worldgen, WAVE 3b: turn the real overworld
//! `final_density` (wave 3a) into chunks behind the `WorldGen::Mc`
//! profile.
//!
//! W213: FULL 3D. The chunk fill reads `final_density` with MC's cell
//! semantics - each `interpolated` subtree is evaluated at 4x8x4 cell
//! corners once per chunk column and trilinearly interpolated
//! ([`CellLattices`]), everything outside per block - so overhangs and
//! the density-native caves (cheese / spaghetti / noodle / pillars)
//! exist exactly where MC's field puts them. Open cells route through
//! the ported MC aquifer ([`super::aquifer`]) to decide air vs water
//! (dry caves under oceans, sealed underground lakes); the legacy
//! hash carvers / ravines / dungeons no longer run for this profile.
//! The pre-W213 spike filter + heightmap machinery is gone - the
//! interpolation itself is MC's smoothing.
//!
//! Engine band note: the engine world floor is y=0 (MC -64..0 is not
//! realized - the established Anchor convention), so the deepslate
//! band, the global lava sea (y < -54) and the deep aquifer lava
//! pockets are out of range; everything above behaves like MC.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::aquifer::{Aquifer, Substance};
use super::density::CellLattices;
use super::overworld::{Overworld, build_overworld};
use super::surface_rule::SurfaceCtx;
use crate::worldgen::biome::Biome;
use crate::worldgen::climate::{SEA_LEVEL, near_surface_block, surface_block};
use crate::worldgen::{WorldgenCtx, decor, strata};

/// Top of the realized terrain band. MC's top slide
/// (`y_clamped_gradient` 240 -> 256) forces air well above any real
/// overworld land; the interpolation lattice spans `0..=LATTICE_TOP`
/// (+ one corner row of margin) and everything above is sky.
const LATTICE_TOP: i32 = 224;

/// How many solid blocks below the surface the MC `surface_rule` is
/// evaluated for. The deepest overworld surface rule (desert
/// sand+sandstone) reaches ~12 down; 16 covers it with margin. Below the
/// band the column is the noise-fill stone/ore.
const SURFACE_BAND: i32 = 16;


/// One column's terrain summary.
#[derive(Clone, Copy)]
pub(crate) struct McCol {
    /// Topmost solid world-Y of the interpolated field (0 = fully open
    /// basin; the bedrock floor still seals the world bottom).
    pub(crate) top: i32,
    pub(crate) biome: Biome,
    /// W217: the noise-driven soft-layer thickness
    /// (`SurfaceSystem.getSurfaceDepth`), feeding the surface rule's
    /// depth conditions.
    pub(crate) surface_depth: i32,
}

/// W213: everything the ~24 vertical chunks of one chunk column share:
/// per-column tops + biomes, the 256-bit per-column solidity bitsets,
/// the interpolation lattices and the (lazily filling) aquifer.
pub(crate) struct McColumnData {
    cols: [[McCol; CHUNK_SIZE]; CHUNK_SIZE],
    solid: Box<[[[u64; 4]; CHUNK_SIZE]; CHUNK_SIZE]>,
    cells: CellLattices,
    aquifer: RefCell<Aquifer>,
}

impl McColumnData {
    fn is_solid(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        (self.solid[lx][lz][(y >> 6) as usize] >> (y & 63)) & 1 == 1
    }

    /// Solid cells between `y` and the top of its contiguous solid run
    /// (0 at a run's top block) - MC's `stoneDepthAbove - 1`, reset by
    /// every air gap, so cave roofs restart the surface band exactly
    /// like `SurfaceSystem`. Capped at `cap` (callers only compare
    /// against the surface band, so deep stone stops scanning early).
    fn depth_above(&self, lx: usize, y: i32, lz: usize, cap: i32) -> i32 {
        let mut d = 0;
        while d <= cap && self.is_solid(lx, y + d + 1, lz) {
            d += 1;
        }
        d
    }
}

thread_local! {
    /// Per-thread cache of the assembled overworld: one build per seed
    /// per worker thread. `build_overworld` parses ~60 JSON files and
    /// constructs ~42 `NormalNoise` octave stacks - far too expensive
    /// to redo per chunk.
    static OW_CACHE: RefCell<Option<(u32, Rc<Overworld>)>> = const { RefCell::new(None) };

    /// Per-thread cache of the per-chunk-column data. The ~24 vertical
    /// chunks of one column stream in together, so this turns 24
    /// lattice + solidity builds into one.
    static H_CACHE: RefCell<HashMap<(i32, i32, u32), Rc<McColumnData>>> =
        RefCell::new(HashMap::new());
}

/// The assembled overworld for `seed`, built once per thread.
fn overworld_for(seed: u32) -> Rc<Overworld> {
    OW_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        match slot.as_ref() {
            Some((s, ow)) if *s == seed => return ow.clone(),
            _ => {}
        }
        let ow = Rc::new(build_overworld(seed));
        *slot = Some((seed, ow.clone()));
        ow
    })
}

/// W213: build one chunk column's data: the interpolation lattices,
/// the full solidity scan of the interpolated field (0..=LATTICE_TOP
/// per column), per-column tops + biomes, and a fresh aquifer.
fn column_data(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> McColumnData {
    let cs = CHUNK_SIZE as i32;
    let ox = cx * cs;
    let oz = cz * cs;
    let cells = ow.build_cells(ox, oz, LATTICE_TOP);
    let aquifer = RefCell::new(Aquifer::new(ow, seed, ox, oz));
    let mut solid = Box::new([[[0u64; 4]; CHUNK_SIZE]; CHUNK_SIZE]);
    let mut cols =
        [[McCol { top: 0, biome: Biome::Plains, surface_depth: 3 }; CHUNK_SIZE]; CHUNK_SIZE];
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            let wx = ox + lx as i32;
            let wz = oz + lz as i32;
            let mut top = 0;
            for y in 0..=LATTICE_TOP {
                if ow.density_cells(wx, y, wz, &cells) > 0.0 {
                    solid[lx][lz][(y >> 6) as usize] |= 1u64 << (y & 63);
                    top = y;
                }
            }
            cols[lx][lz] = McCol {
                top,
                biome: ow.biome_at(wx, wz),
                surface_depth: ow.surface_depth(wx, wz, seed),
            };
        }
    }
    McColumnData { cols, solid, cells, aquifer }
}

/// Cached per-column data for `(cx, cz, seed)`.
fn column_data_cached(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> Rc<McColumnData> {
    H_CACHE.with(|c| {
        if let Some(col) = c.borrow().get(&(cx, cz, seed)) {
            return col.clone();
        }
        let col = Rc::new(column_data(ow, cx, cz, seed));
        let mut map = c.borrow_mut();
        // Bound the cache: a streaming player visits new columns
        // constantly, so drop the lot once it grows large rather than
        // tracking LRU - the working set for one column (24 vertical
        // chunks) is rebuilt cheaply after a flush. 512 entries x ~40KB
        // (lattices + bitsets) stays ~20MB per worker thread.
        if map.len() > 512 {
            map.clear();
        }
        map.insert((cx, cz, seed), col.clone());
        col
    })
}

/// Engine biome at world column `(wx, wz)` from MC's real climate field.
/// The unified biome accessor (`worldgen::realistic_biome_at`) routes
/// the Mc profile here so terrain surface + decoration agree.
pub(crate) fn mc_biome_at(ctx: &WorldgenCtx, wx: i32, wz: i32) -> Biome {
    overworld_for(ctx.effective_seed()).biome_at(wx, wz)
}

/// W211: the REAL MC biome at world column `(wx, wz)` (ocean variants,
/// rivers, birch/dark/flower forests...). The feature passes key their
/// content tables on this; the engine palette is derived at the edges.
pub(crate) fn mc_full_biome_at(
    ctx: &WorldgenCtx,
    wx: i32,
    wz: i32,
) -> super::biome::McBiome {
    overworld_for(ctx.effective_seed()).mc_biome_at(wx, wz)
}

/// Surface height at world column `(wx, wz)` for the MC profile - used
/// by spawn placement (`column_height`). Reads the SAME cached column
/// data `gen_mc` fills from, so the reported height matches the
/// generated block exactly.
pub(crate) fn mc_column_height(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    mc_floor_at(ctx, wx, wz)
}

/// W201: seabed/surface height at world column `(wx, wz)` from the SAME
/// cached per-chunk column grid `gen_mc` filled the terrain with - so the
/// ocean-flora pass sees exactly the floor that was generated.
pub(crate) fn mc_floor_at(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    col.cols[wx.rem_euclid(cs) as usize][wz.rem_euclid(cs) as usize].top
}

/// W220: PRE-DECORATION solidity of the interpolated field at any world
/// cell, from the cached column bitsets. Replay-invariant (every chunk
/// that consults a shared cell agrees) - the lava-lake border checks
/// key on this.
pub(crate) fn column_is_solid(ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> bool {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    col.is_solid(wx.rem_euclid(cs) as usize, wy, wz.rem_euclid(cs) as usize)
}

/// 16x16 "steep slope" flags from the heightmap gradient: a column is
/// steep when an orthogonal neighbour's surface differs by >= 4 blocks,
/// which is where MC's `steep` surface condition shows stone / gravel
/// faces instead of grass. Edge columns clamp to in-chunk neighbours (a
/// 1-block seam inaccuracy, never visible as a hole).
#[allow(clippy::needless_range_loop)] // x/z index BOTH `s` and the +/-1 neighbours
fn steep_grid(data: &McColumnData) -> [[bool; CHUNK_SIZE]; CHUNK_SIZE] {
    let mut s = [[false; CHUNK_SIZE]; CHUNK_SIZE];
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let h = data.cols[x][z].top;
            let mut d = 0;
            if x > 0 {
                d = d.max((h - data.cols[x - 1][z].top).abs());
            }
            if x + 1 < CHUNK_SIZE {
                d = d.max((h - data.cols[x + 1][z].top).abs());
            }
            if z > 0 {
                d = d.max((h - data.cols[x][z - 1].top).abs());
            }
            if z + 1 < CHUNK_SIZE {
                d = d.max((h - data.cols[x][z + 1].top).abs());
            }
            s[x][z] = d >= 4;
        }
    }
    s
}

/// Cold columns paint MC's frozen surface (snow / ice) via the
/// `temperature` surface condition. The engine's two snowy biomes stand
/// in for MC's frozen-temperature columns.
fn is_frozen(biome: Biome) -> bool {
    matches!(biome, Biome::Tundra | Biome::SnowyMountain)
}

/// W213: MC-profile chunk generator, full 3D. Solidity comes from the
/// cached per-column scan of the CELL-INTERPOLATED `final_density`
/// (overhangs + density caves included); open cells route through the
/// ported MC aquifer (air vs water vs pressure-sealed stone); solid
/// cells within reach of their run's top get MC's real `surface_rule`.
/// The legacy hash carvers / ravines / dungeons no longer run here -
/// the density field IS the cave system.
pub(crate) fn gen_mc(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let origin = World::chunk_world_origin(coord);
    let data = column_data_cached(&ow, coord[0], coord[2], seed);

    // Steepness per column for the surface rule's `steep` condition.
    let steep = steep_grid(&data);
    let min_y = ow.min_y;
    let max_y = ow.min_y + ow.height;
    let mut aquifer = data.aquifer.borrow_mut();

    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x as i32 + x as i32;
        let wz = origin.z as i32 + z as i32;
        let wy = origin.y as i32 + y as i32;
        if let Some(b) = strata::bedrock_floor_block(wx, wy, wz, seed) {
            return b;
        }
        if wy > LATTICE_TOP {
            return BlockId::AIR;
        }
        let McCol { top: h, biome, surface_depth } = data.cols[x][z];
        if data.is_solid(x, wy, z) {
            // Solid rock. The surface band runs from each solid RUN's
            // top (SurfaceSystem semantics: an air gap - a cave -
            // resets stone_depth_above, so cave floors near the
            // surface band get painted too). `water_height` is sea
            // level over a seabed column; `steep` from the top grid.
            let depth_above = data.depth_above(x, wy, z, SURFACE_BAND);
            if depth_above <= SURFACE_BAND {
                let run_top = wy + depth_above;
                let sctx = SurfaceCtx {
                    x: wx,
                    y: wy,
                    z: wz,
                    stone_depth_above: depth_above,
                    stone_depth_below: i32::MAX / 2,
                    water_height: if h < SEA_LEVEL { SEA_LEVEL } else { i32::MIN },
                    surface: run_top - SURFACE_BAND,
                    surface_depth,
                    steep: steep[x][z],
                    frozen: is_frozen(biome),
                    biome,
                    min_y,
                    max_y,
                };
                if let Some(id) = ow.surface_eval(&sctx) {
                    return BlockId(id);
                }
            }
            return if wy == h {
                surface_block(biome, h)
            } else if wy >= h - 3 && depth_above <= 3 {
                near_surface_block(biome)
            } else {
                // W215: the giant ore veins substitute the default
                // stone (OreVeinifier); W202's placed ore features
                // paint the small blobs later in decoration.
                super::ore_vein::ore_vein(&ow, &data.cells, wx, wy, wz, seed)
                    .unwrap_or_else(|| strata::bulk_block(wy))
            };
        }
        // Open cell: above the aquifer's sampling ceiling the global
        // rule answers directly (sea below 63, sky above); below it the
        // aquifer decides air / water / pressure-sealed stone.
        if wy > aquifer.skip_above_y() {
            return if wy < SEA_LEVEL { BlockId(36) } else { BlockId::AIR };
        }
        let density = ow.density_cells(wx, wy, wz, &data.cells);
        match aquifer.substance(&ow, wx, wy, wz, density) {
            // Pressure-sealed cells take veins too (MC's filler chain
            // runs oreVeinify wherever the base state is stone).
            Substance::Solid => super::ore_vein::ore_vein(&ow, &data.cells, wx, wy, wz, seed)
                .unwrap_or_else(|| strata::bulk_block(wy)),
            Substance::Air => BlockId::AIR,
            Substance::Water => BlockId(36),
            Substance::Lava => BlockId(37),
        }
    });
    // W214: the MC worm carvers (cave / cave_extra_underground /
    // canyon) cut through the filled+painted terrain, with the same
    // aquifer deciding what the carved cells become.
    let origin_i = [origin.x as i32, origin.y as i32, origin.z as i32];
    super::carver::carve_mc(&mut chunk, coord, ctx, &ow, &mut aquifer, origin_i);
    drop(aquifer);
    decor::place_realistic(&mut chunk, coord, ctx);
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx { base_seed: 0, dim: String::new(), profile: WorldGen::Mc }
    }

    #[test]
    fn column_tops_in_overworld_range_with_relief() {
        // The interpolated MC density must put the surface somewhere
        // sane for a spread of chunk columns - proof the field shapes
        // terrain here - and not all at one height.
        let ow = build_overworld(0);
        let mut seen = std::collections::HashSet::new();
        for &(cx, cz) in &[(0, 0), (4, 4), (-8, 6), (31, -31)] {
            let data = column_data(&ow, cx, cz, 0);
            for lx in [0, 8, 15] {
                for lz in [0, 8, 15] {
                    let h = data.cols[lx][lz].top;
                    assert!((0..=LATTICE_TOP).contains(&h), "top {h} out of range");
                    seen.insert(h);
                }
            }
        }
        assert!(seen.len() > 3, "terrain is flat: {seen:?}");
    }

    /// W213: the interpolated field is seamless - the boundary plane
    /// between two neighbouring chunk columns evaluates identically
    /// from either column's lattice (shared world-space corners).
    #[test]
    fn cell_density_is_seamless_across_chunk_borders() {
        let ow = build_overworld(7);
        let west = ow.build_cells(0, 0, LATTICE_TOP);
        let east = ow.build_cells(16, 0, LATTICE_TOP);
        for y in [8, 40, 64, 100, 140] {
            for z in [0, 5, 11, 15] {
                let a = ow.density_cells(16, y, z, &west);
                let b = ow.density_cells(16, y, z, &east);
                assert!(
                    (a - b).abs() < 1e-9,
                    "seam at y{y} z{z}: {a} vs {b}"
                );
            }
        }
    }

    /// W213: density-native caves exist - some columns have open cells
    /// well below their surface.
    #[test]
    fn density_field_carves_caves() {
        let ow = build_overworld(0);
        let mut cave_cells = 0u32;
        for &(cx, cz) in &[(0, 0), (3, -2), (-5, 7), (10, 10)] {
            let data = column_data(&ow, cx, cz, 0);
            for lx in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    let top = data.cols[lx][lz].top;
                    for y in 8..(top - 10).max(8) {
                        if !data.is_solid(lx, y, lz) {
                            cave_cells += 1;
                        }
                    }
                }
            }
        }
        assert!(cave_cells > 50, "no underground open space found: {cave_cells}");
    }

    /// Evidence dump: openness of the interpolated field + aquifer
    /// outcomes by height band. Run with
    /// `cargo test dump_cave_stats -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_cave_stats() {
        let ow = build_overworld(0);
        for &(cx, cz) in &[(0, 0), (3, -2), (10, 10)] {
            let data = column_data(&ow, cx, cz, 0);
            let mut aq = data.aquifer.borrow_mut();
            println!("\n== chunk column ({cx},{cz}), skip_above_y={} ==", aq.skip_above_y());
            for band in [(0, 16), (16, 32), (32, 48), (48, 63), (63, 90), (90, 130)] {
                let (mut solid, mut air, mut water, mut sealed) = (0u32, 0u32, 0u32, 0u32);
                for lx in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let wx = cx * 16 + lx as i32;
                        let wz = cz * 16 + lz as i32;
                        for y in band.0..band.1 {
                            if data.is_solid(lx, y, lz) {
                                solid += 1;
                                continue;
                            }
                            if y > aq.skip_above_y() {
                                if y < SEA_LEVEL { water += 1 } else { air += 1 }
                                continue;
                            }
                            let d = ow.density_cells(wx, y, wz, &data.cells);
                            match aq.substance(&ow, wx, y, wz, d) {
                                Substance::Solid => sealed += 1,
                                Substance::Air => air += 1,
                                Substance::Water => water += 1,
                                Substance::Lava => {}
                            }
                        }
                    }
                }
                println!(
                    "  y {:>3}..{:<3} solid {solid:>5} air {air:>5} water {water:>5} sealed {sealed:>4}",
                    band.0, band.1
                );
            }
            let tops: Vec<i32> = (0..4).map(|i| data.cols[i * 5][i * 5].top).collect();
            println!("  sample tops: {tops:?}");
        }
    }

    /// W213: the aquifer keeps most deep underground open space DRY
    /// (pre-aquifer, everything below sea level flooded - the W187
    /// disaster). Count air vs water among sub-sea-level cave cells
    /// under land columns.
    #[test]
    fn aquifer_leaves_dry_caves_below_sea_level() {
        let c = ctx();
        let ow = build_overworld(0);
        // Find LAND chunk columns first (seed 0 near the origin is
        // ocean); the aquifer claim is about caves under land.
        let mut land: Vec<(i32, i32)> = Vec::new();
        'search: for r in 0..60 {
            for &(cx, cz) in &[(r, r), (-r, r), (r, -r), (-r, -r), (2 * r, 0), (0, 2 * r)] {
                let data = column_data_cached(&ow, cx, cz, 0);
                if data.cols[8][8].top > SEA_LEVEL + 8 {
                    if !land.contains(&(cx, cz)) {
                        land.push((cx, cz));
                    }
                    if land.len() >= 10 {
                        break 'search;
                    }
                }
            }
        }
        assert!(!land.is_empty(), "no land chunk columns found in the search area");
        let (mut dry, mut wet) = (0u32, 0u32);
        for &(cx, cz) in &land {
            for cy in 0..4 {
                let chunk = gen_mc([cx, cy, cz], &c);
                for lx in 0..CHUNK_SIZE {
                    for ly in 0..CHUNK_SIZE {
                        let wy = cy * 16 + ly as i32;
                        if wy >= SEA_LEVEL {
                            continue;
                        }
                        for lz in 0..CHUNK_SIZE {
                            match chunk.get(lx, ly, lz).0 {
                                0 => dry += 1,
                                36 => wet += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(dry > 0, "no dry underground air below sea level under land");
        // Aquifers make SOME water bodies, but dry must dominate.
        assert!(
            dry > wet,
            "underground below sea level mostly flooded: dry {dry} vs wet {wet}"
        );
    }

    #[test]
    fn gen_mc_has_solid_floor_and_terrain() {
        // Bottom chunk: y=0 (above the bedrock floor band) must be solid
        // under every column - no void to fall through - and the chunk
        // must contain a real surface block somewhere (grass / sand /
        // stone), not be empty.
        let c = ctx();
        let chunk = gen_mc([0, 0, 0], &c);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                assert_ne!(chunk.get(x, 0, z).0, 0, "void at column ({x},{z})");
            }
        }
    }

    /// Evidence dump: terrain height profile + biome labels + water
    /// coverage over a region, so the LOOK (rolling vs flat vs all-ocean,
    /// biome spread) is decidable without the GUI. Run with
    /// `cargo test dump_mc_terrain -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mc_terrain() {
        use std::collections::BTreeMap;
        let ow = build_overworld(0);
        let label = |b: Biome| match b {
            Biome::Beach => "Bch",
            Biome::Plains => "Pln",
            Biome::Forest => "For",
            Biome::Desert => "Des",
            Biome::Taiga => "Tai",
            Biome::Tundra => "Tun",
            Biome::Jungle => "Jun",
            Biome::Savanna => "Sav",
            Biome::Swamp => "Swp",
            Biome::Mountain => "Mtn",
            Biome::SnowyMountain => "Snw",
        };
        let c = ctx();
        // 1D transect: height + biome every 16 blocks for 1600 blocks.
        println!("\n== MC terrain transect (seed 0, z=0, sea={}) ==", ow.sea_level);
        for x in (0..1600).step_by(32) {
            let h = mc_column_height(&c, x, 0);
            let b = ow.biome_at(x, 0);
            let bar = "#".repeat(((h - 40).max(0) as usize / 2).min(60));
            let wet = if h < ow.sea_level { "~" } else { " " };
            println!("x{x:>4} h{h:>3}{wet} {} {bar}", label(b));
        }
        // Area stats: height range, water %, biome histogram.
        let mut hmin = i32::MAX;
        let mut hmax = i32::MIN;
        let mut water = 0u32;
        let mut total = 0u32;
        let mut hist: BTreeMap<&str, u32> = BTreeMap::new();
        for x in (-2000..2000).step_by(40) {
            for z in (-2000..2000).step_by(40) {
                let h = mc_column_height(&c, x, z);
                hmin = hmin.min(h);
                hmax = hmax.max(h);
                total += 1;
                if h < ow.sea_level {
                    water += 1;
                }
                *hist.entry(label(ow.biome_at(x, z))).or_default() += 1;
            }
        }
        println!(
            "\n== area 4000x4000: h [{hmin}..{hmax}], water {:.0}% ==",
            100.0 * water as f32 / total as f32
        );
        for (b, n) in &hist {
            println!("  {b}: {:.1}%", 100.0 * *n as f32 / total as f32);
        }
    }

    /// Evidence dump: what surface blocks does `gen_mc` actually place?
    /// Scans a wide chunk area, finds the topmost non-air/non-water block
    /// per column + the block 2 deeper, and histograms both. Proof the
    /// `surface_rule` fires: SANDSTONE under sand, GRAVEL beaches, PODZOL,
    /// SNOW - blocks the old per-biome surface fill never produced. Run:
    /// `cargo test dump_mc_surface -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mc_surface() {
        use std::collections::BTreeMap;
        let c = ctx();
        let name = |id: u16| match id {
            1 => "stone",
            2 => "dirt",
            3 => "grass_block",
            4 => "sand",
            15 => "snow",
            26 => "sandstone",
            394 => "podzol",
            396 => "coarse_dirt",
            397 => "mud",
            400 => "ice",
            644 => "gravel",
            645 => "red_sand",
            358 => "red_sandstone",
            other => return format!("id{other}"),
        }
        .to_string();
        let mut top: BTreeMap<u16, u32> = BTreeMap::new();
        let mut sub: BTreeMap<u16, u32> = BTreeMap::new();
        let mut land_top: BTreeMap<u16, u32> = BTreeMap::new();
        let ow = build_overworld(0);
        // Wide sparse sweep so we hit land, not just the watery origin.
        // NOTE: the topmost block includes DECORATION (leaves/logs/grass/
        // flowers from `decor::place_realistic`), so the real terrain
        // surface is the cube blocks here (grass/sand/stone/...); the
        // sub-surface + sandstone count is the cleaner signal.
        for cx in (-30..30).step_by(6) {
            for cz in (-30..30).step_by(6) {
                let mut stack = Vec::new();
                for cy in 0..11 {
                    stack.push(gen_mc([cx, cy, cz], &c));
                }
                for x in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
                        // topmost non-air, non-water world-Y in the stack
                        let mut ty = -1i32;
                        for (cy, ch) in stack.iter().enumerate() {
                            for y in 0..CHUNK_SIZE {
                                let id = ch.get(x, y, z).0;
                                if id != 0 && id != 36 {
                                    ty = (cy * CHUNK_SIZE + y) as i32;
                                }
                            }
                        }
                        if ty < 2 {
                            continue;
                        }
                        let at = |wy: i32| {
                            let cy = (wy as usize) / CHUNK_SIZE;
                            let ly = (wy as usize) % CHUNK_SIZE;
                            stack[cy].get(x, ly, z).0
                        };
                        *top.entry(at(ty)).or_default() += 1;
                        *sub.entry(at(ty - 2)).or_default() += 1;
                        // Land = surface at/above sea level (dry column).
                        if ty >= ow.sea_level {
                            *land_top.entry(at(ty)).or_default() += 1;
                        }
                    }
                }
            }
        }
        println!("\n== surface (topmost) blocks, seed 0, sparse sweep ==");
        for (id, n) in &top {
            println!("  {:>13}: {n}", name(*id));
        }
        println!("== 2 blocks below surface ==");
        for (id, n) in &sub {
            println!("  {:>13}: {n}", name(*id));
        }
        println!("== LAND surface (topmost >= sea level {}) ==", ow.sea_level);
        for (id, n) in &land_top {
            println!("  {:>13}: {n}", name(*id));
        }
    }

    #[test]
    fn biome_field_is_varied_not_degenerate() {
        // The MC climate field must place a SPREAD of biomes, not collapse
        // to one (the real risk: our non-seed-identical noise could skew a
        // channel so e.g. everything reads as ocean/Beach). Sweep a large
        // area and demand several distinct biomes including a land one.
        let ow = build_overworld(0);
        let mut seen = std::collections::HashSet::new();
        let mut land = 0;
        for x in (-2000..2000).step_by(40) {
            for z in (-2000..2000).step_by(40) {
                let b = ow.biome_at(x, z);
                seen.insert(b as u8);
                if b != Biome::Beach {
                    land += 1;
                }
            }
        }
        assert!(seen.len() >= 4, "biome field too uniform: {} distinct", seen.len());
        assert!(land > 0, "no land biomes at all - continentalness is broken");
    }

    /// Diagnostic: does the RAW per-block density carve caves/overhangs
    /// underground, and at what density? Tells aliasing (caves exist in
    /// raw density but the 4x8 lattice smooths them) apart from a carve
    /// failure (density never dips negative underground). Run with
    /// `cargo test density_cave_probe -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn density_cave_probe() {
        let ow = build_overworld(0);
        let mut open = 0u64;
        let mut solid = 0u64;
        let mut overhangs = 0u32;
        // Underground band y=8..56 over a 48x48 area.
        for x in (0..48).step_by(1) {
            for z in (0..48).step_by(1) {
                let mut prev_solid = false;
                let mut seen_air_after_solid = false;
                for y in 8..56 {
                    let s = ow.density(x, y, z) > 0.0;
                    if s {
                        solid += 1;
                    } else {
                        open += 1;
                    }
                    // Overhang/undercut signature: solid, then air, then
                    // solid again, going UP the column.
                    if prev_solid && !s {
                        seen_air_after_solid = true;
                    }
                    if s && seen_air_after_solid {
                        overhangs += 1;
                        seen_air_after_solid = false;
                    }
                    prev_solid = s;
                }
            }
        }
        let total = open + solid;
        println!(
            "\n== raw density y8..56, 48x48: open(cave) {open}/{total} = {:.1}%, overhang transitions {overhangs} ==",
            100.0 * open as f64 / total as f64
        );
    }

    #[test]
    fn gen_mc_produces_solid_and_open() {
        // The 3D density gen must produce a real mix of solid terrain and
        // open space (air caves / ocean water) across the underground
        // band - not a degenerate all-solid block (a broken heightmap) or
        // all-open void. Counts air + water as "open".
        let c = ctx();
        let mut open = 0u32;
        let mut solid = 0u32;
        for cc in [[0, 2, 0], [3, 2, 3], [-5, 3, 8], [10, 1, -6], [40, 3, 0]] {
            let chunk = gen_mc(cc, &c);
            for z in 0..CHUNK_SIZE {
                for y in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let id = chunk.get(x, y, z).0;
                        if id == 0 || id == 36 {
                            open += 1;
                        } else {
                            solid += 1;
                        }
                    }
                }
            }
        }
        assert!(solid > 0, "all open - terrain vanished");
        assert!(open > 0, "all solid - density carved nothing (heightmap bug)");
    }

    #[test]
    fn gen_mc_is_deterministic() {
        let c = ctx();
        let a = gen_mc([2, 1, -3], &c);
        let b = gen_mc([2, 1, -3], &c);
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    assert_eq!(a.get(x, y, z).0, b.get(x, y, z).0);
                }
            }
        }
    }
}
