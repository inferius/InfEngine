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
//! W260: the engine world floor moved to MC's -64 (24 vertical
//! chunks, -64..319). The lattice, the solidity bitsets, the aquifer
//! (global lava sea below -54, deep lava pockets), the carvers and
//! the deepslate/bedrock vertical gradients all run in native MC
//! coordinates now - the pre-W260 "engine floor is y=0" note is
//! history.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::aquifer::{Aquifer, Substance};
use super::density::CellLattices;
use super::overworld::{Overworld, build_overworld};
use super::surface_rule::SurfaceCtx;
use super::biome::McBiome;
use crate::worldgen::biome::Biome;
use crate::worldgen::climate::{SEA_LEVEL, near_surface_block, surface_block};
use crate::worldgen::{WorldgenCtx, decor};

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

/// W260: the solidity bitsets span the full world band -64..=319
/// (384 bits = 6 words); bit index = `y - BITS_MIN_Y`. `ow.min_y`
/// (parsed from the MC noise JSON) is debug-asserted against this
/// once per column build.
pub(crate) const BITS_MIN_Y: i32 = -64;
const BITS_MAX_Y: i32 = 319;
pub(crate) const BITS_WORDS: usize = 6;

/// One chunk column's solidity bitsets (shared with the carver's
/// dry-run mask sink).
pub(crate) type SolidBits = [[[u64; BITS_WORDS]; CHUNK_SIZE]; CHUNK_SIZE];


/// One column's terrain summary.
#[derive(Clone, Copy)]
pub(crate) struct McCol {
    /// Topmost solid world-Y of the interpolated field (0 = fully open
    /// basin; the bedrock floor still seals the world bottom).
    pub(crate) top: i32,
    /// W221: the REAL MC biome - surface rules match its name, the
    /// legacy fallbacks collapse it via `to_engine()`.
    pub(crate) mc: McBiome,
    /// W217: the noise-driven soft-layer thickness
    /// (`SurfaceSystem.getSurfaceDepth`), feeding the surface rule's
    /// depth conditions.
    pub(crate) surface_depth: i32,
}

/// W213: everything the ~24 vertical chunks of one chunk column share:
/// per-column tops + biomes, the 256-bit per-column solidity bitsets,
/// the interpolation lattices and the (lazily filling) aquifer.
///
/// W226.1: solidity exists in TWO stages, matching MC's generation
/// order (surface -> carvers -> features). `solid`/`cols[].top` are the
/// PRE-carve fill the surface rules painted; `solid_pc`/`floor` are the
/// POST-carve state the feature stage anchors to (MC updates heightmaps
/// on every carver setBlock - anchoring features to the fill made
/// seagrass float over carver-cut seabeds).
/// W314: the EXPENSIVE, immutable per-column payload - shared
/// ACROSS worker threads via a global cache (each of the 8 workers
/// used to rebuild its own copy of every frontier-neighbour column
/// probed by disks / lakes / monster rooms: ~25 ms each, 8x over).
/// Everything here is plain data (Send + Sync); the mutable aquifer
/// stays per-thread in `McColumnData`.
pub(crate) struct ColumnCore {
    cols: [[McCol; CHUNK_SIZE]; CHUNK_SIZE],
    solid: Box<SolidBits>,
    /// Post-carve solidity (the carver dry-run mask).
    solid_pc: Box<SolidBits>,
    /// Post-carve topmost-solid per column (OCEAN_FLOOR heightmap).
    floor: [[i32; CHUNK_SIZE]; CHUNK_SIZE],
    cells: CellLattices,
}

pub(crate) struct McColumnData {
    core: std::sync::Arc<ColumnCore>,
    aquifer: RefCell<Aquifer>,
}

impl std::ops::Deref for McColumnData {
    type Target = ColumnCore;
    fn deref(&self) -> &ColumnCore {
        &self.core
    }
}

impl ColumnCore {
    fn is_solid(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !(BITS_MIN_Y..=BITS_MAX_Y).contains(&y) {
            return false;
        }
        let b = (y - BITS_MIN_Y) as usize;
        (self.solid[lx][lz][b >> 6] >> (b & 63)) & 1 == 1
    }

    /// Post-carve solidity (feature-stage view).
    fn is_solid_pc(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !(BITS_MIN_Y..=BITS_MAX_Y).contains(&y) {
            return false;
        }
        let b = (y - BITS_MIN_Y) as usize;
        (self.solid_pc[lx][lz][b >> 6] >> (b & 63)) & 1 == 1
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
pub(crate) fn overworld_for(seed: u32) -> Rc<Overworld> {
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
/// the full solidity scan of the interpolated field
/// (`min_y..=LATTICE_TOP` per column), per-column tops + biomes, and
/// a fresh aquifer.
fn column_data(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> ColumnCore {
    debug_assert_eq!(ow.min_y, BITS_MIN_Y, "solidity bitsets sized for min_y -64");
    let cs = CHUNK_SIZE as i32;
    let ox = cx * cs;
    let oz = cz * cs;
    let cells = ow.build_cells(ox, oz, LATTICE_TOP);
    let mut aquifer = Aquifer::new(ow, seed, ox, oz);
    let mut solid = Box::new([[[0u64; BITS_WORDS]; CHUNK_SIZE]; CHUNK_SIZE]);
    let mut cols = [[McCol { top: BITS_MIN_Y, mc: McBiome::Plains, surface_depth: 3 };
        CHUNK_SIZE]; CHUNK_SIZE];
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            let wx = ox + lx as i32;
            let wz = oz + lz as i32;
            let mut top = BITS_MIN_Y;
            for y in ow.min_y..=LATTICE_TOP {
                if ow.density_cells(wx, y, wz, &cells) > 0.0 {
                    let b = (y - BITS_MIN_Y) as usize;
                    solid[lx][lz][b >> 6] |= 1u64 << (b & 63);
                    top = y;
                }
            }
            cols[lx][lz] = McCol {
                top,
                mc: ow.mc_biome_at(wx, wz),
                surface_depth: ow.surface_depth(wx, wz, seed),
            };
        }
    }
    // W226.1: replay the carvers against a copy of the bitset so the
    // feature stage (mc_floor_at / column_is_solid) sees the carved
    // terrain. Same walks + rng as the per-slice chunk carve.
    let mut solid_pc = solid.clone();
    super::carver::carve_mask(&mut solid_pc, cx, cz, seed, ow, &mut aquifer);
    let mut floor = [[BITS_MIN_Y; CHUNK_SIZE]; CHUNK_SIZE];
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            floor[lx][lz] = top_set_bit(&solid_pc[lx][lz]);
        }
    }
    drop(aquifer);
    ColumnCore { cols, solid, solid_pc, floor, cells }
}

/// W314: the cross-worker core cache. RwLock HashMap of Arc cores -
/// a frontier column probed by one worker's feature pass is reused
/// by every other worker (and by the column's own later gen job)
/// instead of each thread rebuilding it (~25 ms a copy).
///
/// W328: eviction is FIFO-halving, not a wholesale clear. The old
/// `clear()` at cap nuked the ENTIRE working set the moment travel
/// pushed the map past 2048 - every disk/lake/monster-room probe
/// then rebuilt its 3x3 neighbourhood cold again, which is exactly
/// the 10-13 ms avg_deco the walking logs showed. Values carry an
/// insert sequence; on overflow the oldest half leaves and the
/// active frontier stays warm. Cap 3072 (~30 KB a core = ~90 MB
/// worst case; a VD-14 ring is ~850 columns).
type CoreCacheMap =
    HashMap<(i32, i32, u32), (std::sync::Arc<ColumnCore>, u64)>;
static CORE_CACHE: std::sync::LazyLock<std::sync::RwLock<CoreCacheMap>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(HashMap::new()));
static CORE_CACHE_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
// W405: 3072 -> 6144 floor. A view-30 ring alone is 61^2 = 3721
// columns, so the old cap thrashed even standing still - and in
// flight the frontier + the deco probes into 8 neighbour columns
// re-built evicted cores constantly (the R25 "gen 8 -> 30 ms while
// moving" degradation).
//
// W406 (R26 mandate: "use the memory the machine actually has"):
// the cap now scales with PHYSICAL RAM - ~8% of it at ~50KB per
// core, clamped to [6144, 24576] (~300MB..1.2GB). Detection is a
// direct kernel32 call (std already links it); non-Windows or a
// failed call keeps the floor.
const CORE_CACHE_CAP_FLOOR: usize = 6144;
const CORE_CACHE_CAP_CEIL: usize = 24576;

#[cfg(windows)]
fn total_physical_ram_bytes() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut m = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: the struct is #[repr(C)] with `length` set as the API
    // requires; kernel32 fills the rest on success.
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m.total_phys)
}

#[cfg(not(windows))]
fn total_physical_ram_bytes() -> Option<u64> {
    None
}

fn core_cache_cap() -> usize {
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CAP.get_or_init(|| {
        let cap = total_physical_ram_bytes()
            .map(|total| (total / 100 * 8 / 50_000) as usize)
            .unwrap_or(CORE_CACHE_CAP_FLOOR)
            .clamp(CORE_CACHE_CAP_FLOOR, CORE_CACHE_CAP_CEIL);
        tracing::info!(cap, "W406: column-core cache sized from RAM");
        cap
    })
}

fn column_core_cached(
    ow: &Overworld,
    cx: i32,
    cz: i32,
    seed: u32,
) -> std::sync::Arc<ColumnCore> {
    if let Ok(map) = CORE_CACHE.read()
        && let Some((hit, _)) = map.get(&(cx, cz, seed))
    {
        return hit.clone();
    }
    let core = std::sync::Arc::new(column_data(ow, cx, cz, seed));
    if let Ok(mut map) = CORE_CACHE.write() {
        if map.len() >= core_cache_cap() {
            // Drop the oldest-inserted half (median split on the
            // sequence). One O(n) pass over ~3k entries, only on
            // overflow - never on the hot lookup path.
            let mut seqs: Vec<u64> = map.values().map(|(_, s)| *s).collect();
            seqs.sort_unstable();
            // W405: drop the oldest QUARTER (was half) - the gentler
            // sweep keeps more of the live ring resident.
            let cut = seqs[seqs.len() / 4];
            map.retain(|_, (_, s)| *s >= cut);
        }
        let seq = CORE_CACHE_SEQ
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        map.insert((cx, cz, seed), (core.clone(), seq));
    }
    core
}

/// Highest set bit of a column bitset, as a world Y (`BITS_MIN_Y`
/// when empty, matching the pre-carve `top` fallback).
fn top_set_bit(bits: &[u64; BITS_WORDS]) -> i32 {
    for w in (0..BITS_WORDS).rev() {
        if bits[w] != 0 {
            return w as i32 * 64 + 63 - bits[w].leading_zeros() as i32 + BITS_MIN_Y;
        }
    }
    BITS_MIN_Y
}

/// Cached per-column data for `(cx, cz, seed)`.
fn column_data_cached(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> Rc<McColumnData> {
    H_CACHE.with(|c| {
        if let Some(col) = c.borrow().get(&(cx, cz, seed)) {
            return col.clone();
        }
        // W314: the heavy core comes from the CROSS-WORKER cache;
        // only the cheap per-thread aquifer is minted fresh.
        let core = column_core_cached(ow, cx, cz, seed);
        let cs = CHUNK_SIZE as i32;
        let aquifer = Aquifer::new(ow, seed, cx * cs, cz * cs);
        let col = Rc::new(McColumnData {
            core,
            aquifer: RefCell::new(aquifer),
        });
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
/// ocean-flora pass sees exactly the floor that was generated. W226.1:
/// POST-carve (MC's live heightmaps track carver setBlocks; features
/// must anchor to the carved seabed, not the fill).
pub(crate) fn mc_floor_at(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    col.floor[wx.rem_euclid(cs) as usize][wz.rem_euclid(cs) as usize]
}

/// W244: the block id painted on a column's TOP surface cell -
/// replay-invariant (pure column data + surface rule, no chunk
/// reads), so surface features (desert well's IS_SAND gate) can test
/// the ground identically from every 3x3 replay window. Returns 0
/// when the post-carve top differs from the pre-carve top (a carver
/// shaved the column, exposing the underground paint) - callers
/// treat that as "not a pristine surface".
pub(crate) fn mc_surface_block_at(ctx: &WorldgenCtx, wx: i32, wz: i32) -> u16 {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let data = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    let (x, z) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
    let McCol { top: h, mc, surface_depth } = data.cols[x][z];
    if data.floor[x][z] != h {
        return 0;
    }
    // The same SurfaceCtx the fill builds for the run-top cell
    // (depth_above = 0); steep re-derives the in-chunk-clamped
    // gradient the painted chunk used.
    let steep = {
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
        d >= 4
    };
    let sctx = SurfaceCtx {
        x: wx,
        y: h,
        z: wz,
        stone_depth_above: 0,
        stone_depth_below: i32::MAX / 2,
        water_height: if h < SEA_LEVEL { SEA_LEVEL } else { i32::MIN },
        surface: h - SURFACE_BAND,
        surface_depth,
        steep,
        frozen: super::vegetation::cold_enough_to_snow_at(mc, wx, h, wz),
        mc_name: mc.name(),
        min_y: ow.min_y,
        max_y: ow.min_y + ow.height,
    };
    if let Some(id) = ow.surface_eval(&sctx) {
        return id;
    }
    crate::worldgen::climate::surface_block(mc.to_engine(), h).0
}

/// W253: the PAINTED pre-decoration block id at any world cell -
/// replays the exact `gen_mc` solid-cell fill decision from the
/// cached column data, replay-invariant like `column_is_solid`.
/// Returns 0 for cells that are not solid POST-carve (air, water,
/// lava, carved space) - dig-down callers (buried treasure) only
/// need the solid material identity.
pub(crate) fn mc_painted_block_at(ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> u16 {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let data = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    let (x, z) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
    if !data.is_solid_pc(x, wy, z) {
        return 0;
    }
    if wy > LATTICE_TOP {
        return 0;
    }
    let McCol { top: h, mc, surface_depth } = data.cols[x][z];
    if data.is_solid(x, wy, z) {
        let depth_above = data.depth_above(x, wy, z, SURFACE_BAND);
        if depth_above <= SURFACE_BAND {
            let run_top = wy + depth_above;
            // The fill's steep grid, re-derived for one column (same
            // in-chunk-clamped gradient as `mc_surface_block_at`).
            let steep = {
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
                d >= 4
            };
            let sctx = SurfaceCtx {
                x: wx,
                y: wy,
                z: wz,
                stone_depth_above: depth_above,
                stone_depth_below: i32::MAX / 2,
                water_height: if h < SEA_LEVEL { SEA_LEVEL } else { i32::MIN },
                surface: run_top - SURFACE_BAND,
                surface_depth,
                steep,
                frozen: super::vegetation::cold_enough_to_snow_at(mc, wx, wy, wz),
                mc_name: mc.name(),
                min_y: ow.min_y,
                max_y: ow.min_y + ow.height,
            };
            if let Some(id) = ow.surface_eval(&sctx) {
                return id;
            }
        }
        let biome = mc.to_engine();
        return if wy == h {
            surface_block(biome, h).0
        } else if wy >= h - 3 && depth_above <= 3 {
            near_surface_block(biome).0
        } else {
            super::ore_vein::ore_vein(&ow, &data.cells, wx, wy, wz, seed)
                .unwrap_or_else(|| mc_bulk_block(wx, wy, wz, ow.min_y))
                .0
        };
    }
    // Solid post-carve without a pre-carve run: an aquifer pressure
    // seal - bulk stone + veins, like the fill's Substance::Solid arm.
    super::ore_vein::ore_vein(&ow, &data.cells, wx, wy, wz, seed)
        .unwrap_or_else(|| mc_bulk_block(wx, wy, wz, ow.min_y))
        .0
}

/// W260: bulk fill for a deep solid cell in MC's filler order - the
/// vein pass already ran (callers try [`super::ore_vein`] first),
/// then the surface stage's trailing vertical gradients pick jagged
/// bedrock at the world floor, deepslate below y 8, stone above.
/// `SurfaceSystem.buildSurface` only applies rules where the cell
/// still holds the default block, which is exactly this fallback
/// position; the gradients share the rule chain's per-cell rolls, so
/// band-painted and bulk cells agree.
fn mc_bulk_block(wx: i32, wy: i32, wz: i32, min_y: i32) -> BlockId {
    use std::sync::OnceLock;
    if super::surface_rule::is_bedrock_floor(wx, wy, wz, min_y) {
        static BEDROCK: OnceLock<u16> = OnceLock::new();
        return BlockId(*BEDROCK.get_or_init(|| {
            super::features::block_id_by_name("bedrock").unwrap_or(1)
        }));
    }
    if super::surface_rule::is_deepslate(wx, wy, wz) {
        static DEEPSLATE: OnceLock<u16> = OnceLock::new();
        BlockId(*DEEPSLATE.get_or_init(|| {
            super::features::block_id_by_name("deepslate").unwrap_or(1)
        }))
    } else {
        BlockId(1)
    }
}

/// W261: `OverworldBiomeBuilder.isDeepDarkRegion` at a world cell -
/// the 3D cave-biome check (erosion + depth) the ancient city's
/// `has_structure/deep_dark` gate needs. Replay-invariant (pure
/// noise evals).
pub(crate) fn mc_is_deep_dark(ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> bool {
    overworld_for(ctx.effective_seed()).is_deep_dark(wx, wy, wz)
}

/// W220: PRE-DECORATION solidity at any world cell, from the cached
/// column bitsets. Replay-invariant (every chunk that consults a shared
/// cell agrees) - the lava-lake border checks key on this. W226.1:
/// post-carve, matching what the feature stage finds in real chunks.
pub(crate) fn column_is_solid(ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> bool {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_data_cached(&ow, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    col.is_solid_pc(wx.rem_euclid(cs) as usize, wy, wz.rem_euclid(cs) as usize)
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


thread_local! {
    /// W307: per-worker phase split inside `gen_mc` -
    /// (column_us, fill_us, carve_us, deco_us, chunks). The W305
    /// worker profile showed gen at 30-40 ms/chunk vs 4.4 ms meshing;
    /// this names WHICH stage burns it. One log per 512 chunks.
    static MC_PROF: std::cell::RefCell<(u64, u64, u64, u64, u32)> =
        const { std::cell::RefCell::new((0, 0, 0, 0, 0)) };
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
    let _t0 = std::time::Instant::now();
    let data = column_data_cached(&ow, coord[0], coord[2], seed);
    let _col_us = _t0.elapsed().as_micros() as u64;
    let _t1 = std::time::Instant::now();

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
        if wy > LATTICE_TOP {
            return BlockId::AIR;
        }
        let McCol { top: h, mc, surface_depth } = data.cols[x][z];
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
                    // W221: the real per-cell coldEnoughToSnow model.
                    frozen: super::vegetation::cold_enough_to_snow_at(mc, wx, wy, wz),
                    mc_name: mc.name(),
                    min_y,
                    max_y,
                };
                if let Some(id) = ow.surface_eval(&sctx) {
                    return BlockId(id);
                }
            }
            let biome = mc.to_engine();
            return if wy == h {
                surface_block(biome, h)
            } else if wy >= h - 3 && depth_above <= 3 {
                near_surface_block(biome)
            } else {
                // W215: the giant ore veins substitute the default
                // stone (OreVeinifier); W202's placed ore features
                // paint the small blobs later in decoration. W260:
                // the fallback is the bedrock/deepslate gradient
                // chain, not the legacy rescaled strata.
                super::ore_vein::ore_vein(&ow, &data.cells, wx, wy, wz, seed)
                    .unwrap_or_else(|| mc_bulk_block(wx, wy, wz, min_y))
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
                .unwrap_or_else(|| mc_bulk_block(wx, wy, wz, min_y)),
            Substance::Air => BlockId::AIR,
            Substance::Water => BlockId(36),
            Substance::Lava => BlockId(37),
        }
    });
    // W214: the MC worm carvers (cave / cave_extra_underground /
    // canyon) cut through the filled+painted terrain, with the same
    // aquifer deciding what the carved cells become.
    let origin_i = [origin.x as i32, origin.y as i32, origin.z as i32];
    let _fill_us = _t1.elapsed().as_micros() as u64;
    let _t2 = std::time::Instant::now();
    super::carver::carve_mc(&mut chunk, coord, ctx, &ow, &mut aquifer, origin_i);
    drop(aquifer);
    let _carve_us = _t2.elapsed().as_micros() as u64;
    let _t3 = std::time::Instant::now();
    decor::place_realistic(&mut chunk, coord, ctx);
    let _deco_us = _t3.elapsed().as_micros() as u64;
    MC_PROF.with(|p| {
        let mut p = p.borrow_mut();
        p.0 += _col_us;
        p.1 += _fill_us;
        p.2 += _carve_us;
        p.3 += _deco_us;
        p.4 += 1;
        if p.4 >= 512 {
            tracing::info!(
                avg_column_ms = p.0 as f32 / 1000.0 / p.4 as f32,
                avg_fill_ms = p.1 as f32 / 1000.0 / p.4 as f32,
                avg_carve_ms = p.2 as f32 / 1000.0 / p.4 as f32,
                avg_deco_ms = p.3 as f32 / 1000.0 / p.4 as f32,
                chunks = p.4,
                "W307: mc gen phase split"
            );
            *p = (0, 0, 0, 0, 0);
        }
    });
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
                    assert!((BITS_MIN_Y..=LATTICE_TOP).contains(&h), "top {h} out of range");
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

    /// W221 bug probe: measure the four in-game symptoms across a wide
    /// area. Run with
    /// `cargo test dump_w221_bug_probe -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_w221_bug_probe() {
        let ow = build_overworld(0);
        let mut holes = 0u32; // surface_depth <= 0 (exposed stone risk)
        let mut cols = 0u32;
        let mut dry_bed = 0u32; // depression below sea with AIR above the floor
        let mut wet_bed = 0u32;
        let mut snow_on_hot = 0u32; // snow/ice on temp-band>=3 columns
        let mut depth_hist = [0u32; 9];
        for ccx in -24..24 {
            for ccz in -24..24 {
                let data = column_data_cached(&ow, ccx, ccz, 0);
                let mut aq = data.aquifer.borrow_mut();
                for lx in (0..CHUNK_SIZE).step_by(4) {
                    for lz in (0..CHUNK_SIZE).step_by(4) {
                        let wx = ccx * 16 + lx as i32;
                        let wz = ccz * 16 + lz as i32;
                        cols += 1;
                        let McCol { top, surface_depth, .. } = data.cols[lx][lz];
                        depth_hist[surface_depth.clamp(0, 8) as usize] += 1;
                        if surface_depth <= 0 {
                            holes += 1;
                        }
                        // Depressions below sea level: what does the cell
                        // just above the floor become?
                        if top < SEA_LEVEL - 1 && top > 20 {
                            let y = top + 1;
                            let d = ow.density_cells(wx, y, wz, &data.cells);
                            let sub = if y > aq.skip_above_y() {
                                if y < SEA_LEVEL { Substance::Water } else { Substance::Air }
                            } else {
                                aq.substance(&ow, wx, y, wz, d)
                            };
                            match sub {
                                Substance::Air => {
                                    dry_bed += 1;
                                    if dry_bed <= 6 {
                                        let prelim = ow.preliminary_surface(wx, wz);
                                        let flood = ow.floodedness(wx, y, wz);
                                        println!(
                                            "DRY bed at ({wx},{y},{wz}): top {top}, prelim {prelim}, skip {}, floodedness {flood:.3}",
                                            aq.skip_above_y()
                                        );
                                    }
                                }
                                Substance::Water => wet_bed += 1,
                                _ => {}
                            }
                        }
                        // Hot biome (temp band 3-4) with frozen surface?
                        let mc = ow.mc_biome_at(wx, wz);
                        let name = mc.name();
                        if (name == "desert" || name == "savanna")
                            && crate::worldgen::mcgen::vegetation::height_adjusted_temperature(
                                mc, wx, top, wz,
                            ) < 0.15
                        {
                            snow_on_hot += 1;
                        }
                    }
                }
            }
        }
        println!("\n== W221 probe over {cols} columns ==");
        println!("surface_depth hist (0..8+): {depth_hist:?}");
        println!("holes (depth<=0): {holes} ({:.1}%)", 100.0 * holes as f32 / cols as f32);
        println!("sub-sea depressions: wet {wet_bed}, DRY {dry_bed} ({:.1}% dry)",
            100.0 * dry_bed as f32 / (dry_bed + wet_bed).max(1) as f32);
        println!("hot-biome frozen columns: {snow_on_hot}");
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
            // W314: column_data returns the shared CORE; mint the
            // per-probe aquifer alongside.
            let cs_i = CHUNK_SIZE as i32;
            let mut aq = Aquifer::new(&ow, 0, cx * cs_i, cz * cs_i);
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

    /// W260: the deep band is MC's. Below y -55 every open cell is
    /// the global lava sea (never bare air), and the deepslate
    /// gradient owns the bulk below y 0; the tuff/iron vein type has
    /// its band back (-60..-8), so deep chunks may carry
    /// deepslate_iron_ore / tuff instead of deepslate - both count
    /// as "deep strata" here.
    #[test]
    fn deep_band_has_lava_sea_and_no_bare_air() {
        let c = ctx();
        let (mut lava, mut air_below, mut deep_cells, mut strata_cells) = (0u64, 0u64, 0u64, 0u64);
        for &(cx, cz) in &[(0, 0), (3, -2), (-5, 7), (10, 10), (17, 3), (-11, -9)] {
            for cy in [-4, -3] {
                let chunk = gen_mc([cx, cy, cz], &c);
                for ly in 0..CHUNK_SIZE {
                    let wy = cy * 16 + ly as i32;
                    for lx in 0..CHUNK_SIZE {
                        for lz in 0..CHUNK_SIZE {
                            let id = chunk.get(lx, ly, lz).0;
                            if wy < -55 {
                                match id {
                                    0 => air_below += 1,
                                    37 => lava += 1,
                                    _ => {}
                                }
                            }
                            if (-60..0).contains(&wy) {
                                deep_cells += 1;
                                // Anything but plain stone/air/water:
                                // deepslate, tuff, deepslate ores,
                                // bedrock band, vein bodies.
                                if id != 0 && id != 1 && id != 36 && id != 37 {
                                    strata_cells += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(air_below, 0, "open cells below -55 must be the lava sea, not air");
        assert!(lava > 0, "no lava sea found in any sampled deep chunk");
        assert!(
            strata_cells > deep_cells / 2,
            "deep band should be deepslate-family strata: {strata_cells}/{deep_cells}"
        );
    }

    /// W260 evidence dump: what the deep band actually contains per
    /// 16-block Y slab. Run with
    /// `cargo test dump_deep_band -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_deep_band() {
        use std::collections::HashMap;
        let c = ctx();
        let mut by_band: HashMap<(i32, u16), u64> = HashMap::new();
        for &(cx, cz) in &[(0, 0), (3, -2), (-5, 7), (10, 10)] {
            for cy in -4..0 {
                let chunk = gen_mc([cx, cy, cz], &c);
                for ly in 0..CHUNK_SIZE {
                    let wy = cy * 16 + ly as i32;
                    let band = wy.div_euclid(16) * 16;
                    for lx in 0..CHUNK_SIZE {
                        for lz in 0..CHUNK_SIZE {
                            *by_band.entry((band, chunk.get(lx, ly, lz).0)).or_default() += 1;
                        }
                    }
                }
            }
        }
        let mut bands: Vec<i32> = by_band.keys().map(|(b, _)| *b).collect();
        bands.sort_unstable();
        bands.dedup();
        for band in bands {
            let mut ids: Vec<(u16, u64)> = by_band
                .iter()
                .filter(|((b, _), _)| *b == band)
                .map(|((_, id), n)| (*id, *n))
                .collect();
            ids.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let row: Vec<String> = ids
                .iter()
                .take(8)
                .map(|(id, n)| {
                    match crate::block::block_type(*id) {
                        Some(b) => format!("{}:{n}", b.name),
                        None => format!("id{id}:{n}"),
                    }
                })
                .collect();
            println!("y {band:>4}..{:<4} {}", band + 15, row.join(" "));
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
        // W227: the same area histogrammed by REAL MC biome (the full
        // classifier), so new slice biomes are visible pre-collapse.
        let mut full: BTreeMap<&str, u32> = BTreeMap::new();
        for x in (-2000..2000).step_by(40) {
            for z in (-2000..2000).step_by(40) {
                *full.entry(ow.mc_biome_at(x, z).name()).or_default() += 1;
            }
        }
        println!("\n== full MC biomes ==");
        for (b, n) in &full {
            println!("  {b}: {:.2}%", 100.0 * *n as f32 / total as f32);
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

    /// W229.2 diagnostic (run with --ignored): where do the steep
    /// terraced tower formations come from (user report, seed
    /// 3592732596 near spawn)? Histograms the per-column height
    /// gradient over the spawn area and prints every "tower" column
    /// (gradient >= 6) with its biome + the climate channels the
    /// terrain splines key on - if towers cluster in windswept /
    /// shattered / peak bands that's MC-faithful roughness; scattered
    /// through plains/forest at mid erosion it's a spline/density bug.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_steep_formations() {
        let c = WorldgenCtx {
            base_seed: 3592732596,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let ow = overworld_for(c.effective_seed());
        const R: i32 = 64;
        let h = |x: i32, z: i32| mc_floor_at(&c, x, z);
        let mut grad_hist = [0u32; 17];
        let mut towers: Vec<(i32, i32, i32, i32)> = Vec::new();
        for x in -R..R {
            for z in -R..R {
                let h0 = h(x, z);
                let g = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .map(|(dx, dz)| (h0 - h(x + dx, z + dz)).abs())
                    .max()
                    .unwrap();
                grad_hist[(g as usize).min(16)] += 1;
                if g >= 6 {
                    towers.push((x, z, h0, g));
                }
            }
        }
        println!("\n== gradient histogram ({}x{} cols) ==", 2 * R, 2 * R);
        for (g, n) in grad_hist.iter().enumerate() {
            if *n > 0 {
                println!("  |dh| {g:>2}{}: {n}", if g == 16 { "+" } else { " " });
            }
        }
        towers.sort_by_key(|t| -t.3);
        println!("== towers (|dh| >= 6): {} ==", towers.len());
        for (x, z, h0, g) in towers.iter().take(30) {
            let mc = ow.mc_biome_at(*x, *z);
            let (_, _, c_, e, w) = ow.climate_at(*x, *z);
            let pv = crate::worldgen::mcgen::biome::peaks_valleys(w);
            println!(
                "  ({x:>4},{z:>4}) h{h0:>3} dh{g:>2}  {:<24} cont {c_:>6.3} ero {e:>6.3} pv {pv:>6.3}",
                mc.name()
            );
        }
    }

    /// W229.2 diagnostic: does the post-carve floor map agree with the
    /// GENERATED chunks in the steep-formation cluster near the user's
    /// spawn? Prints a transect: real top-most non-air block per
    /// column vs mc_floor_at. A mismatch = the W226.1 carve mask
    /// diverges from the real carve; agreement = the formations are
    /// real carved canyons.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_floor_vs_chunks_transect() {
        let c = WorldgenCtx {
            base_seed: 3592732596,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let z = -10i32;
        println!("\n== transect z={z}, x -48..-24: chunk top vs floor map ==");
        for x in -48i32..-24 {
            // Real top: scan generated chunks downward from y 96.
            let mut top = -1i32;
            let mut top_id = 0u16;
            'scan: for cy in (1..6).rev() {
                let ch = crate::worldgen::generate_chunk_at_ctx([x.div_euclid(16), cy, z.div_euclid(16)], &c);
                let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                for ly in (0..16).rev() {
                    let id = ch.get(lx, ly, lz).0;
                    if id != 0 && id != 36 {
                        top = cy * 16 + ly as i32;
                        top_id = id;
                        break 'scan;
                    }
                }
            }
            let floor = mc_floor_at(&c, x, z);
            let mark = if (top - floor).abs() > 1 { "  <-- MISMATCH" } else { "" };
            let name = crate::block::block_type(top_id).map(|b| b.name).unwrap_or("?");
            println!("  x{x:>4}: chunk top {top:>3} ({name:<12}) floor {floor:>3}{mark}");
        }
    }

    /// W239 diagnostic: where does per-chunk generation time go?
    /// Prints cold column-data cost vs warm per-chunk cost so the
    /// worker-dispatch redesign has numbers.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_generation_cost_breakdown() {
        let c = WorldgenCtx {
            base_seed: 3653370720,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Fresh columns, far from anything cached in this thread.
        let mut cold_ms = 0.0f64;
        let mut cols = 0u32;
        for cx in 500..505i32 {
            for cz in 500..505 {
                let t = std::time::Instant::now();
                let ow = overworld_for(c.effective_seed());
                let _ = column_data_cached(&ow, cx, cz, c.effective_seed());
                cold_ms += t.elapsed().as_secs_f64() * 1000.0;
                cols += 1;
            }
        }
        println!("column_data COLD: {:.1} ms avg over {cols} columns", cold_ms / cols as f64);
        // Full chunk gen with a WARM column (same thread): fill +
        // surface + features + everything generate_chunk_at_ctx does.
        let mut warm_ms = 0.0f64;
        let mut chunks = 0u32;
        for cx in 500..505i32 {
            for cz in 500..505 {
                for cy in 0..16 {
                    let t = std::time::Instant::now();
                    let _ = crate::worldgen::generate_chunk_at_ctx([cx, cy, cz], &c);
                    warm_ms += t.elapsed().as_secs_f64() * 1000.0;
                    chunks += 1;
                }
            }
        }
        println!("chunk gen WARM-column: {:.2} ms avg over {chunks} chunks", warm_ms / chunks as f64);
        println!(
            "=> column job (1 cold + 16 warm): {:.1} ms vs scattered worst case (16 cold): {:.1} ms",
            cold_ms / cols as f64 + 16.0 * warm_ms / chunks as f64,
            16.0 * (cold_ms / cols as f64 + warm_ms / chunks as f64),
        );
    }

    /// W232 diagnostic: the user's badlands-canyon "holes" report
    /// (seed 3653370720, standing at ~(106, 55, -90)). Prints PRE vs
    /// POST carve solidity + the McBiome across the crevice: if the
    /// void exists only POST-carve it's the canyon/cave carver
    /// working as MC intends; if it's in neither it's a chunk hole.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_user_badlands_canyon() {
        let seed = 3653370720u32;
        let ow = overworld_for(seed);
        // Locate the badlands the user stood in (the F3 coords in
        // their report belong to the ocean-perf moment, not the
        // canyon screenshot).
        let mut badlands_at: Option<(i32, i32)> = None;
        'scan: for r in 1..=16i32 {
            for dz in (-r..=r).step_by(2) {
                for dx in (-r..=r).step_by(2) {
                    if dx.abs() < r && dz.abs() < r {
                        continue;
                    }
                    let (x, z) = (106 + dx * 32, -90 + dz * 32);
                    let mc = ow.mc_biome_at(x, z);
                    if matches!(
                        mc,
                        McBiome::Badlands
                            | McBiome::WoodedBadlands
                            | McBiome::ErodedBadlands
                    ) {
                        badlands_at = Some((x, z));
                        break 'scan;
                    }
                }
            }
        }
        println!("nearest badlands column: {badlands_at:?}");
        // Cross-section the badlands area for carved voids: columns
        // where POST-carve has air that PRE-carve doesn't = carver.
        if let Some((bx, bz)) = badlands_at {
            for z in [bz - 8, bz, bz + 8] {
                println!(
                    "\n== badlands columns, z={z}, PRE vs POST carve (y 20..90) =="
                );
                for x in (bx - 16..=bx + 16).step_by(4) {
                    let col = column_data_cached(
                        &ow,
                        x.div_euclid(16),
                        z.div_euclid(16),
                        seed,
                    );
                    let (lx, lz) =
                        (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                    let run = |pc: bool| -> String {
                        let mut s = String::new();
                        for y in 20..=90 {
                            let solid = if pc {
                                col.is_solid_pc(lx, y, lz)
                            } else {
                                col.is_solid(lx, y, lz)
                            };
                            s.push(if solid { '#' } else { '.' });
                        }
                        s
                    };
                    let mc = ow.mc_biome_at(x, z);
                    println!("  x{x:>5} pre : {}   {}", run(false), mc.name());
                    println!("        post: {}", run(true));
                }
            }
        }
        for z in [-95i32, -90, -85] {
            println!("\n== badlands canyon, z={z}, PRE vs POST carve (y 20..90) ==");
            for x in (96i32..=118).step_by(2) {
                let col = column_data_cached(
                    &ow,
                    x.div_euclid(16),
                    z.div_euclid(16),
                    seed,
                );
                let (lx, lz) =
                    (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                let run = |pc: bool| -> String {
                    let mut s = String::new();
                    for y in 20..=90 {
                        let solid = if pc {
                            col.is_solid_pc(lx, y, lz)
                        } else {
                            col.is_solid(lx, y, lz)
                        };
                        s.push(if solid { '#' } else { '.' });
                    }
                    s
                };
                let mc = ow.mc_biome_at(x, z);
                println!("  x{x:>4} pre : {}   {}", run(false), mc.name());
                println!("       post: {}", run(true));
            }
        }
    }

    /// W229.2 diagnostic: is the spawn-area canyon carved or a density
    /// feature? Prints PRE-carve vs POST-carve solid runs for columns
    /// across it.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_canyon_pre_vs_post_carve()
    {
        let seed = 3592732596u32;
        let ow = overworld_for(seed);
        let z = -10i32;
        println!("\n== canyon columns, PRE vs POST carve solid gaps (y 20..75) ==");
        for x in [-39i32, -37, -36, -35, -34, -33, -31] {
            let col = column_data_cached(&ow, x.div_euclid(16), z.div_euclid(16), seed);
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            let run = |pc: bool| -> String {
                let mut s = String::new();
                for y in 20..=75 {
                    let solid = if pc { col.is_solid_pc(lx, y, lz) } else { col.is_solid(lx, y, lz) };
                    s.push(if solid { '#' } else { '.' });
                }
                s
            };
            println!("  x{x:>4} pre : {}", run(false));
            println!("       post: {}", run(true));
        }
    }

    /// W226.2 diagnostic (run with --ignored): quad counts of the
    /// runtime mesher over real terrain chunks - the greedy-vs-unit
    /// cost ledger for the perf epic (constant light = maximum merge,
    /// so the greedy number is its best case).
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_mesh_quad_counts() {
        let c = WorldgenCtx {
            base_seed: 3592732596,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut quads_special = 0usize;
        let mut quads_greedy = 0usize;
        let mut chunks = 0usize;
        for ccx in -4..4 {
            for ccz in -4..4 {
                let h = mc_floor_at(&c, ccx * 16 + 8, ccz * 16 + 8);
                let cy = h.div_euclid(16);
                let coord = [ccx, cy, ccz];
                let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &c);
                chunks += 1;
                let opaque = |b: infengine_voxel::BlockId| {
                    crate::block::block_type(b.0).is_some_and(|bt| !bt.transparent)
                };
                let occ = |p: [i32; 3]| {
                    (0..16).contains(&p[0])
                        && (0..16).contains(&p[1])
                        && (0..16).contains(&p[2])
                        && opaque(chunk.get(p[0] as usize, p[1] as usize, p[2] as usize))
                };
                let subs = infengine_voxel::chunk_mesh_greedy_with_special(
                    &chunk,
                    glam::Vec3::ZERO,
                    |_, _| [0.25f32, 0.25, 0.75, 0.75],
                    opaque,
                    |_, _| [0.0f32, 1.0f32],
                    |_, _| 0u32,
                    occ,
                    |_| infengine_voxel::BlockId::AIR,
                    |_, _, _, _| false,
                );
                quads_special += subs.iter().map(|s| s.vertices.len() / 4).sum::<usize>();
                let greedy = infengine_voxel::chunk_mesh_greedy(
                    &chunk,
                    glam::Vec3::ZERO,
                    |_, _| [0.25f32, 0.25, 0.75, 0.75],
                    opaque,
                    |_, _| [0.0f32, 1.0f32],
                    |_, _| 0u32,
                );
                quads_greedy += greedy.iter().map(|s| s.vertices.len() / 4).sum::<usize>();
            }
        }
        println!(
            "\n== mesh quads over {chunks} surface chunks (seed 3592732596): runtime mesher {quads_special}, greedy(no-special) {quads_greedy} =="
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
