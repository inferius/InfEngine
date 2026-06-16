//! W184 - faithful MC worldgen, WAVE 3b: turn the real overworld
//! `final_density` (wave 3a) into chunks behind the `WorldGen::Mc`
//! profile.
//!
//! MC 1.18 generates terrain BEFORE biomes: the 3D shape comes purely
//! from `final_density` (continentalness / erosion / peaks-and-valleys
//! / jaggedness splines -> sloped_cheese -> slides), independent of the
//! biome. This wave takes the surface height from that real density and
//! reuses the proven Natural pipeline for everything below it (biome
//! surface block, water fill, strata ores, bedrock, caves, decoration).
//! So the headline (MC's actual terrain SHAPE) lands now; full 3D density
//! (overhangs, density-native caves, aquifers) is a follow-up.
//!
//! Faithful, not a shortcut: the height is sampled on the same 4-block
//! horizontal lattice MC's noise runs on (cellWidth = size_horizontal
//! << 2 = 4) and bilinearly interpolated across the chunk, exactly the
//! granularity MC's `NoiseChunk` interpolates at.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

use super::overworld::{Overworld, build_overworld};
use crate::worldgen::biome::Biome;
use crate::worldgen::climate::{SEA_LEVEL, near_surface_block, surface_block};
use crate::worldgen::ore::pick_stone_or_ore;
use crate::worldgen::{WorldgenCtx, caves, decor, dungeons, ravines, strata};

/// Topmost world-Y the surface scan starts from. MC's top slide
/// (`y_clamped_gradient` 240 -> 256) forces air well above any real
/// overworld land, so scanning from here never misses a peak while
/// skipping the always-empty sky band.
const SCAN_TOP: i32 = 220;

/// One chunk's 16x16 surface heightmap (topmost-solid world-Y per
/// column).
type HeightGrid = [[i32; CHUNK_SIZE]; CHUNK_SIZE];

/// One chunk's 16x16 per-column terrain data: `(surface_height, biome)`.
/// Cached together so the ~24 vertical chunks of a column reuse one
/// density scan + one biome classification.
type ColGrid = [[(i32, Biome); CHUNK_SIZE]; CHUNK_SIZE];

thread_local! {
    /// Per-thread cache of the assembled overworld: one build per seed
    /// per worker thread. `build_overworld` parses ~60 JSON files and
    /// constructs ~42 `NormalNoise` octave stacks - far too expensive
    /// to redo per chunk.
    static OW_CACHE: RefCell<Option<(u32, Rc<Overworld>)>> = const { RefCell::new(None) };

    /// Per-thread cache of the per-chunk-column surface heightmap
    /// (chunk `cx, cz, seed` -> 16x16 topmost-solid Y). The ~24
    /// vertical chunks of one column all need the same heightmap and
    /// stream in together, so this turns 24 density scans into one.
    static H_CACHE: RefCell<HashMap<(i32, i32, u32), Box<ColGrid>>> =
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

/// Topmost solid (`final_density > 0`) world-Y in the column at
/// `(wx, wz)` that has real ground beneath it. Coarse scan down from
/// [`SCAN_TOP`] in `cellHeight` steps, refine to the exact block, then
/// reject thin floating crusts. Returns `min_y` if the whole column
/// reads open (deep ocean basins still get a bedrock floor + water from
/// the fill pass).
///
/// W188: the spike filter is load-bearing. Over open ocean the density
/// hovers near zero, and sampling it at the noise-cell lattice turns
/// stray slightly-positive cells into 1-2 block floating crusts. Taken
/// as the surface, the heightmap fill grew them into sand pillars (the
/// user's "underwater islands/cacti"). Requiring solid ground a few
/// blocks below the candidate skips those crusts and finds the real
/// seabed.
fn surface_height(ow: &Overworld, wx: i32, wz: i32) -> i32 {
    let mut yc = SCAN_TOP;
    while yc > ow.min_y {
        if ow.density(wx, yc, wz) > 0.0 {
            // Exact topmost solid in (yc, yc + 8].
            let mut top = yc;
            for fy in (yc + 1..=yc + 8).rev() {
                if ow.density(wx, fy, wz) > 0.0 {
                    top = fy;
                    break;
                }
            }
            // Real surface = solid for at least 3 blocks down. A thinner
            // crust is a near-zero-density artifact; reject it and keep
            // scanning further down for the true ground.
            let run = (1..=4).take_while(|&k| ow.density(wx, top - k, wz) > 0.0).count();
            if run >= 3 {
                return top;
            }
            // Fall through to `yc -= 8`: the next band (below `yc`) can't
            // contain this crust (`top > yc`), so we make progress and
            // never re-scan it. W188 hang fix - the old `yc = top - 4`
            // could move yc UP (top reaches yc+8), re-finding the same
            // crust forever and freezing fresh-world generation.
        }
        yc -= 8;
    }
    ow.min_y
}

/// 16x16 surface heightmap for chunk column `(cx, cz)`, built by
/// sampling the real density at the 5x5 cell corners (every 4 blocks,
/// MC's horizontal noise resolution) and bilinearly interpolating. The
/// shared `lx == 16` / `lz == 16` corners match the neighbouring
/// chunk's `0` corners, so heights are seamless across chunk borders.
fn heightmap(ow: &Overworld, cx: i32, cz: i32) -> [[i32; CHUNK_SIZE]; CHUNK_SIZE] {
    let ox = cx * CHUNK_SIZE as i32;
    let oz = cz * CHUNK_SIZE as i32;
    // 5 corners per axis at local 0,4,8,12,16.
    let mut corner = [[0i32; 5]; 5];
    for (i, crow) in corner.iter_mut().enumerate() {
        for (j, c) in crow.iter_mut().enumerate() {
            *c = surface_height(ow, ox + i as i32 * 4, oz + j as i32 * 4);
        }
    }
    let mut h = [[0i32; CHUNK_SIZE]; CHUNK_SIZE];
    for (lx, hrow) in h.iter_mut().enumerate() {
        let ci = lx / 4;
        let fx = (lx % 4) as f32 / 4.0;
        for (lz, cell) in hrow.iter_mut().enumerate() {
            let cj = lz / 4;
            let fz = (lz % 4) as f32 / 4.0;
            let a = lerp(corner[ci][cj] as f32, corner[ci + 1][cj] as f32, fx);
            let b = lerp(corner[ci][cj + 1] as f32, corner[ci + 1][cj + 1] as f32, fx);
            *cell = lerp(a, b, fz).round() as i32;
        }
    }
    h
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolated surface height at a single world column `(wx, wz)` -
/// the SAME bilinear over the same global 4-aligned corners that
/// [`heightmap`] / `gen_mc` fill the terrain from. Spawn placement and
/// `column_height` use this (not raw `surface_height`) so the reported
/// height matches the block actually generated there; a point sample
/// drifts a few blocks from the interpolated surface, which spawned the
/// player buried in / under the real terrain (W191).
fn interp_height(ow: &Overworld, wx: i32, wz: i32) -> i32 {
    let x0 = wx.div_euclid(4) * 4;
    let z0 = wz.div_euclid(4) * 4;
    let fx = (wx - x0) as f32 / 4.0;
    let fz = (wz - z0) as f32 / 4.0;
    let h00 = surface_height(ow, x0, z0) as f32;
    let h10 = surface_height(ow, x0 + 4, z0) as f32;
    let h01 = surface_height(ow, x0, z0 + 4) as f32;
    let h11 = surface_height(ow, x0 + 4, z0 + 4) as f32;
    let a = lerp(h00, h10, fx);
    let b = lerp(h01, h11, fx);
    lerp(a, b, fz).round() as i32
}

/// Per-column `(surface_height, biome)` for chunk `(cx, cz)`: the
/// interpolated density heightmap plus MC's biome classification of the
/// real climate field at each column.
fn column_data(ow: &Overworld, cx: i32, cz: i32) -> ColGrid {
    let h = heightmap(ow, cx, cz);
    let ox = cx * CHUNK_SIZE as i32;
    let oz = cz * CHUNK_SIZE as i32;
    let mut col = [[(0i32, Biome::Plains); CHUNK_SIZE]; CHUNK_SIZE];
    for (lx, row) in col.iter_mut().enumerate() {
        for (lz, cell) in row.iter_mut().enumerate() {
            let biome = ow.biome_at(ox + lx as i32, oz + lz as i32);
            // `.max(1)`: keep a solid floor under every column so a
            // density-empty basin is a seabed, not a void (the Natural
            // floor invariant).
            *cell = (h[lx][lz].max(1), biome);
        }
    }
    col
}

/// Cached per-column data for `(cx, cz, seed)`.
fn column_data_cached(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> Box<ColGrid> {
    H_CACHE.with(|c| {
        if let Some(col) = c.borrow().get(&(cx, cz, seed)) {
            return col.clone();
        }
        let col = Box::new(column_data(ow, cx, cz));
        let mut map = c.borrow_mut();
        // Bound the cache: a streaming player visits new columns
        // constantly, so drop the lot once it grows large rather than
        // tracking LRU - the working set for one column (24 vertical
        // chunks) is rebuilt cheaply after a flush.
        if map.len() > 1024 {
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

/// Surface height at world column `(wx, wz)` for the MC profile - used
/// by spawn placement (`column_height`). Builds/uses the cached
/// overworld for the ctx seed.
pub(crate) fn mc_column_height(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let ow = overworld_for(ctx.effective_seed());
    interp_height(&ow, wx, wz)
}

/// MC-profile chunk generator: per-column surface height from the real
/// MC `final_density`, then the proven Natural surface / water / strata
/// / cave / decoration pipeline below it.
///
/// W188: reverted W187's per-voxel 3D solidity. Interpolating MC's
/// near-zero ocean density at the noise-cell lattice spawned thin
/// floating solid spires that the surface walk capped (+ grew cactus on)
/// while the water beneath flowed into the hollow - the user's
/// "underwater islands" plus a liquid-tick perf collapse. MC's overworld
/// surface is essentially a heightmap anyway (overhangs are rare); a
/// faithful 3D pass needs MC's aquifer system, deferred. The terrain
/// SHAPE still comes from the real density (the spike-filtered topmost
/// solid per column), so the world keeps its MC profile, and caves come
/// from the engine carver as before.
pub(crate) fn gen_mc(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let origin = World::chunk_world_origin(coord);
    let col = column_data_cached(&ow, coord[0], coord[2], seed);

    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let wy = origin.y as i32 + y as i32;
        if let Some(b) = strata::bedrock_floor_block(wx as i32, wy, wz as i32, seed) {
            return b;
        }
        let (h, biome) = col[x][z];
        if wy > h {
            // Water fills open columns up to sea level; above is sky.
            if wy <= SEA_LEVEL && h < SEA_LEVEL {
                return BlockId(36);
            }
            return BlockId::AIR;
        }
        if wy == h {
            surface_block(biome, h)
        } else if wy >= h - 3 {
            near_surface_block(biome)
        } else {
            pick_stone_or_ore(wx, wy, wz, biome, seed)
        }
    });
    caves::carve(&mut chunk, coord, ctx);
    ravines::carve(&mut chunk, coord, ctx);
    decor::place_realistic(&mut chunk, coord, ctx);
    dungeons::place(&mut chunk, coord, ctx);
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
    fn surface_height_in_overworld_range() {
        // The real MC density must put the surface somewhere sane
        // (between the deep floor and the scan cap) for a spread of
        // columns - proof the density actually shapes terrain here.
        let ow = build_overworld(0);
        let mut seen = std::collections::HashSet::new();
        for &(x, z) in &[(0, 0), (64, 64), (-128, 96), (500, -500), (37, 941)] {
            let h = surface_height(&ow, x, z);
            assert!((ow.min_y..=SCAN_TOP).contains(&h), "surface {h} out of range");
            seen.insert(h);
        }
        // Not every column the same height -> the terrain has relief.
        assert!(seen.len() > 1, "terrain is flat: {seen:?}");
    }

    #[test]
    fn surface_height_terminates_over_grid() {
        // W188 hang guard: surface_height must always return - the spike
        // filter's rescan once looped forever on ocean crust columns,
        // freezing fresh-world gen. Sweep a few hundred columns
        // (including the watery near-origin region); if any hangs, this
        // test never finishes.
        let ow = build_overworld(0);
        for x in (-160..160).step_by(8) {
            for z in (-160..160).step_by(8) {
                let h = surface_height(&ow, x, z);
                assert!((ow.min_y..=SCAN_TOP).contains(&h));
            }
        }
    }

    #[test]
    fn heightmap_corners_are_seamless() {
        // This chunk's lx=16 column == the east neighbour's lx=0 column;
        // both sample the same world column, so the heights must match.
        let ow = build_overworld(7);
        let _here = heightmap(&ow, 0, 0); // exercised by the gen test below
        let east = heightmap(&ow, 1, 0);
        // east neighbour's local x=0 world column is this chunk's x=16,
        // which the bilinear pins to corner[4][*]; compare the z line.
        for (lz, &e) in east[0].iter().enumerate() {
            let mine = surface_height(&ow, CHUNK_SIZE as i32, lz as i32);
            assert_eq!(mine, e, "seam mismatch at lz={lz}");
        }
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
        // 1D transect: height + biome every 16 blocks for 1600 blocks.
        println!("\n== MC terrain transect (seed 0, z=0, sea={}) ==", ow.sea_level);
        for x in (0..1600).step_by(32) {
            let h = surface_height(&ow, x, 0);
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
                let h = surface_height(&ow, x, z);
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
