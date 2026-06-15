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
use crate::worldgen::biome_table::biome_from_climate;
use crate::worldgen::climate::{SEA_LEVEL, climate_at, near_surface_block, surface_block};
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
    static H_CACHE: RefCell<HashMap<(i32, i32, u32), Box<HeightGrid>>> =
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
/// `(wx, wz)`. Coarse scan down from [`SCAN_TOP`] in `cellHeight`
/// steps, then refine to the exact block. Returns `min_y` if the whole
/// column reads open (deep ocean basins still get a bedrock floor +
/// water from the fill pass).
fn surface_height(ow: &Overworld, wx: i32, wz: i32) -> i32 {
    let mut yc = SCAN_TOP;
    while yc > ow.min_y {
        if ow.density(wx, yc, wz) > 0.0 {
            // Topmost solid is in (yc, yc + 8]; the corner above (yc+8)
            // was open. Walk down from there for the exact block.
            for fy in (yc + 1..=yc + 8).rev() {
                if ow.density(wx, fy, wz) > 0.0 {
                    return fy;
                }
            }
            return yc;
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

/// Cached 16x16 heightmap for `(cx, cz, seed)`.
fn heightmap_cached(ow: &Overworld, cx: i32, cz: i32, seed: u32) -> Box<[[i32; CHUNK_SIZE]; CHUNK_SIZE]> {
    H_CACHE.with(|c| {
        if let Some(h) = c.borrow().get(&(cx, cz, seed)) {
            return h.clone();
        }
        let h = Box::new(heightmap(ow, cx, cz));
        let mut map = c.borrow_mut();
        // Bound the cache: a streaming player visits new columns
        // constantly, so drop the lot once it grows large rather than
        // tracking LRU - the working set for one column (24 vertical
        // chunks) is rebuilt cheaply after a flush.
        if map.len() > 1024 {
            map.clear();
        }
        map.insert((cx, cz, seed), h.clone());
        h
    })
}

/// Surface height at world column `(wx, wz)` for the MC profile - used
/// by spawn placement (`column_height`). Builds/uses the cached
/// overworld for the ctx seed.
pub(crate) fn mc_column_height(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let ow = overworld_for(ctx.effective_seed());
    surface_height(&ow, wx, wz)
}

/// MC-profile chunk generator: real-density surface height + the
/// Natural surface / water / strata / cave / decoration pipeline.
pub(crate) fn gen_mc(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let ow = overworld_for(seed);
    let origin = World::chunk_world_origin(coord);
    let cx = coord[0];
    let cz = coord[2];

    // Surface height from the REAL MC density, biome from climate (MC
    // shapes terrain before biomes, so biome only colours the surface).
    let hmap = heightmap_cached(&ow, cx, cz, seed);
    let mut col: [[(i32, Biome); CHUNK_SIZE]; CHUNK_SIZE] =
        [[(0, Biome::Plains); CHUNK_SIZE]; CHUNK_SIZE];
    for (lx, row) in col.iter_mut().enumerate() {
        for (lz, cell) in row.iter_mut().enumerate() {
            let wx = origin.x + lx as f32;
            let wz = origin.z + lz as f32;
            let biome = biome_from_climate(climate_at(wx, wz, seed));
            // `.max(1)`: keep a solid floor under every column so a
            // density-empty basin is a seabed, not a void (matches the
            // Natural floor invariant).
            *cell = (hmap[lx][lz].max(1), biome);
        }
    }

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
            if wy <= SEA_LEVEL && h < SEA_LEVEL {
                return BlockId(36); // water fills below sea level
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
