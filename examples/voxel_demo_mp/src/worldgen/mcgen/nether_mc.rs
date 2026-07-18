//! W266.2 - the faithful Nether, ported data-driven from
//! `noise_settings/nether.json` (26.1.2) - the same pipeline shape
//! as the overworld (W181+) and the End (W264):
//!
//! - final_density: squeeze(0.64 * interpolated(...slides... +
//!   base_3d_noise)) over the 0..128 band; solid where positive.
//! - Open cells: lava below sea level 32 (`default_fluid`), air
//!   above - no aquifer (`aquifers_enabled: false`).
//! - Biomes: `MultiNoiseBiomeSourceParameterList.Preset.NETHER` -
//!   FIVE fixed climate points over (temperature, humidity) +
//!   a per-biome `offset` handicap; nearest squared distance wins
//!   (nether_wastes t0/h0, soul_sand_valley h-0.5, crimson_forest
//!   t0.4, warped_forest h0.5 off0.375, basalt_deltas t-0.5
//!   off0.175). The router's temperature/vegetation channels feed
//!   the sample; the other channels are constant 0.
//! - Surface: the real `surface_rule` chain per solid run
//!   (netherrack / soul sand + soul soil / gravel / basalt +
//!   blackstone / nyliums + wart blocks, noise-gated bands), with
//!   the bedrock floor gradient + the `bedrock_roof` NOT-gradient
//!   (always bedrock at y 127) special-cased for the deep bulk
//!   exactly like the overworld's floor.
//!
//! Replaces the legacy hand-rolled `worldgen/nether.rs` behind the
//! same `WorldGen::Nether` profile (the legacy baseline test retired
//! with it). W267 adds the nether_cave carvers + decoration
//! (glowstone, fire, fungi, deltas); fortress/bastion follow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};
use serde_json::Value;

use super::data;
use super::density::{Df, parse};
use super::noise::{NoiseParams, NormalNoise, SplitMix64};
use super::surface_rule::{SurfaceCtx, SurfaceRules};
use crate::worldgen::WorldgenCtx;

/// Engine ids the fill places directly.
pub(crate) const LAVA_ID: u16 = 37;
/// The nether world band (noise: min_y 0, height 128).
pub(crate) const TOP_Y: i32 = 127;
/// `sea_level` (the lava ocean surface).
pub(crate) const SEA_LEVEL: i32 = 32;
/// Surface-band depth, like the overworld's (the deepest nether rule
/// reaches ~5 down; 16 keeps the SurfaceSystem semantics shared).
const SURFACE_BAND: i32 = 16;

/// The five `Preset.NETHER` climate points:
/// (temperature, humidity, offset, biome name).
const BIOMES: [(f64, f64, f64, &str); 5] = [
    (0.0, 0.0, 0.0, "nether_wastes"),
    (0.0, -0.5, 0.0, "soul_sand_valley"),
    (0.4, 0.0, 0.0, "crimson_forest"),
    (0.0, 0.5, 0.375, "warped_forest"),
    (-0.5, 0.0, 0.175, "basalt_deltas"),
];

pub(crate) struct NetherDim {
    final_density: Df,
    temperature: Df,
    vegetation: Df,
    surface: SurfaceRules,
    surface_noise: Arc<NormalNoise>,
}

fn noise_seed(seed: u32, name: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (seed as u64);
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

/// Parse the Nether router + surface rules for `seed`.
pub(crate) fn build_nether(seed: u32) -> NetherDim {
    let settings: Value =
        serde_json::from_str(data::NETHER_SETTINGS).expect("nether settings JSON");
    let df_res = |name: &str| -> Option<Value> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        data::df_json(n).and_then(|j| serde_json::from_str(j).ok())
    };
    let noise_res = move |name: &str| -> Option<Arc<NormalNoise>> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        let params: NoiseParams = serde_json::from_str(data::noise_json(n)?).ok()?;
        Some(Arc::new(NormalNoise::new(noise_seed(seed, n), &params)))
    };
    let nr = &settings["noise_router"];
    let blended_seed = noise_seed(seed, "nether_blended_noise");
    let p = |key: &str| {
        parse(
            &nr[key],
            &df_res,
            &noise_res as &super::density::NoiseResolver<'_>,
            blended_seed,
        )
    };
    // W299: number the `interpolated` wrappers so the terrain fill
    // can run them at MC's cell resolution (4x8 trilinear lattice).
    // Without this the fill sampled the blended noise per BLOCK -
    // the nether's high-frequency base_3d_noise (xz_factor 80 vs the
    // overworld's 684) turned into noise-grain pillar chaos instead
    // of MC's smooth caverns.
    let mut final_density = p("final_density");
    let mut next_interp = 0usize;
    final_density.assign_interp_ids(&mut next_interp);
    NetherDim {
        final_density,
        temperature: p("temperature"),
        vegetation: p("vegetation"),
        surface: SurfaceRules::parse(&settings["surface_rule"], &noise_res, seed),
        surface_noise: noise_res("surface").expect("surface noise params embedded"),
    }
}

impl NetherDim {
    /// The nearest of the five fixed climate points at a column
    /// (sampled mid-band; the nether channels are xz-driven).
    pub(crate) fn biome_at(&self, x: i32, z: i32) -> &'static str {
        let t = self.temperature.eval(x as f64, 64.0, z as f64);
        let h = self.vegetation.eval(x as f64, 64.0, z as f64);
        let mut best = BIOMES[0].3;
        let mut best_d = f64::MAX;
        for (bt, bh, off, name) in BIOMES {
            let d = (t - bt) * (t - bt) + (h - bh) * (h - bh) + off * off;
            if d < best_d {
                best_d = d;
                best = name;
            }
        }
        best
    }

    /// `SurfaceSystem.getSurfaceDepth` (shared with the overworld).
    fn surface_depth(&self, x: i32, z: i32, seed: u32) -> i32 {
        let noise = self.surface_noise.value(x as f64, 0.0, z as f64);
        let mut r = SplitMix64::new(
            (seed as u64) ^ 0x5FCE_DEA7
                ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
        );
        (noise * 2.75 + 3.0 + r.next_f64() * 0.25) as i32
    }
}

thread_local! {
    static NETHER_CACHE: RefCell<Option<(u32, Rc<NetherDim>)>> =
        const { RefCell::new(None) };
    /// Per-column solidity bitsets + biome names (the 16 vertical
    /// chunks of one column stream together).
    static COL_CACHE: RefCell<HashMap<(i32, i32, u32), Rc<NetherCol>>> =
        RefCell::new(HashMap::new());
}

fn nether_for(seed: u32) -> Rc<NetherDim> {
    NETHER_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        match slot.as_ref() {
            Some((s, n)) if *s == seed => return n.clone(),
            _ => {}
        }
        let n = Rc::new(build_nether(seed));
        *slot = Some((seed, n.clone()));
        n
    })
}

/// One chunk column: 128-bit solidity per cell + biome + surface
/// depth per column. W269: `carved` = `solid` minus the nether_cave
/// carver's cells (the dry-run mask) - the feature stage's floor and
/// layer scans read it, like MC's live post-carve heightmaps.
pub(crate) struct NetherCol {
    solid: Box<[[[u64; 2]; CHUNK_SIZE]; CHUNK_SIZE]>,
    carved: Box<[[[u64; 2]; CHUNK_SIZE]; CHUNK_SIZE]>,
    pub(crate) biome: [[&'static str; CHUNK_SIZE]; CHUNK_SIZE],
    surface_depth: [[i32; CHUNK_SIZE]; CHUNK_SIZE],
}

impl NetherCol {
    /// The FILL solidity (pre-carve; the terrain paint reads this).
    pub(crate) fn is_solid(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !(0..=TOP_Y).contains(&y) {
            return false;
        }
        (self.solid[lx][lz][(y >> 6) as usize] >> (y & 63)) & 1 == 1
    }

    /// W269: post-carve solidity (the feature floors).
    pub(crate) fn is_solid_carved(&self, lx: usize, y: i32, lz: usize) -> bool {
        if !(0..=TOP_Y).contains(&y) {
            return false;
        }
        (self.carved[lx][lz][(y >> 6) as usize] >> (y & 63)) & 1 == 1
    }

    /// Solid cells between `y` and its run top (SurfaceSystem's
    /// `stoneDepthAbove - 1`), capped at the surface band.
    fn depth_above(&self, lx: usize, y: i32, lz: usize) -> i32 {
        let mut d = 0;
        while d <= SURFACE_BAND && self.is_solid(lx, y + d + 1, lz) {
            d += 1;
        }
        d
    }
}

fn column_data(nd: &NetherDim, cx: i32, cz: i32, seed: u32) -> NetherCol {
    let cs = CHUNK_SIZE as i32;
    let (ox, oz) = (cx * cs, cz * cs);
    let mut solid = Box::new([[[0u64; 2]; CHUNK_SIZE]; CHUNK_SIZE]);
    let mut biome = [[""; CHUNK_SIZE]; CHUNK_SIZE];
    let mut surface_depth = [[3i32; CHUNK_SIZE]; CHUNK_SIZE];
    // W299: MC cell semantics - `interpolated` subtrees sampled at
    // the 4x8 corner lattice + trilinear, everything else per block
    // (same as the overworld fill since W213).
    let ny = (TOP_Y / super::density::CELL_H) as usize + 2;
    let cells =
        super::density::CellLattices::build(&[&nd.final_density], ox, oz, 0, ny);
    for lx in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            let (wx, wz) = (ox + lx as i32, oz + lz as i32);
            for y in 0..=TOP_Y {
                if nd
                    .final_density
                    .eval_cells(wx as f64, y as f64, wz as f64, &cells)
                    > 0.0
                {
                    solid[lx][lz][(y >> 6) as usize] |= 1u64 << (y & 63);
                }
            }
            biome[lx][lz] = nd.biome_at(wx, wz);
            surface_depth[lx][lz] = nd.surface_depth(wx, wz, seed);
        }
    }
    // W269: the carver dry-run over a copy (same walks as the chunk
    // carve; the features anchor to the carved floors).
    let mut carved = solid.clone();
    super::carver::carve_nether_mask(&mut carved, cx, cz, seed);
    NetherCol { solid, carved, biome, surface_depth }
}

fn column_cached(nd: &NetherDim, cx: i32, cz: i32, seed: u32) -> Rc<NetherCol> {
    COL_CACHE.with(|c| {
        if let Some(col) = c.borrow().get(&(cx, cz, seed)) {
            return col.clone();
        }
        let col = Rc::new(column_data(nd, cx, cz, seed));
        let mut map = c.borrow_mut();
        if map.len() > 512 {
            map.clear();
        }
        map.insert((cx, cz, seed), col.clone());
        col
    })
}

/// W268: the cached column for a chunk column, for the feature passes
/// (solidity bitsets + biomes without generating the chunk).
pub(crate) fn nether_col(ctx: &WorldgenCtx, cx: i32, cz: i32) -> Rc<NetherCol> {
    let seed = ctx.effective_seed();
    let nd = nether_for(seed);
    column_cached(&nd, cx, cz, seed)
}

/// The TERRAIN block at one cell (pre-features): bedrock shell /
/// surface-rule block / netherrack for solid cells, lava-or-air for
/// open ones. W268: factored out of the chunk fill so the feature
/// passes can read the pre-feature world across chunk borders (the
/// same read-the-terrain doctrine as the sculk pass).
#[allow(clippy::too_many_arguments)]
fn terrain_block(
    nd: &NetherDim,
    col: &NetherCol,
    lx: usize,
    lz: usize,
    wx: i32,
    wy: i32,
    wz: i32,
    bedrock: u16,
    netherrack: u16,
) -> u16 {
    if !(0..=TOP_Y).contains(&wy) {
        return 0;
    }
    // The bedrock gradients paint SOLID cells only (the surface
    // stage never fills open space) - roof first, floor after,
    // both shared with the in-band rule via the same rolls.
    if col.is_solid(lx, wy, lz) {
        if super::surface_rule::is_bedrock_roof(wx, wy, wz, TOP_Y) {
            return bedrock;
        }
        if super::surface_rule::is_bedrock_floor(wx, wy, wz, 0) {
            return bedrock;
        }
        let depth_above = col.depth_above(lx, wy, lz);
        if depth_above <= SURFACE_BAND {
            let run_top = wy + depth_above;
            let sctx = SurfaceCtx {
                x: wx,
                y: wy,
                z: wz,
                stone_depth_above: depth_above,
                stone_depth_below: i32::MAX / 2,
                water_height: if run_top < SEA_LEVEL { SEA_LEVEL } else { i32::MIN },
                surface: run_top - SURFACE_BAND,
                surface_depth: col.surface_depth[lx][lz],
                steep: false,
                frozen: false,
                mc_name: col.biome[lx][lz],
                min_y: 0,
                // The INCLUSIVE top: the roof gradient's
                // below_top anchors resolve against it.
                max_y: TOP_Y,
            };
            if let Some(id) = nd.surface.eval(&sctx) {
                return id;
            }
        }
        return netherrack;
    }
    // Open cells: the lava ocean below sea level 32.
    if wy < SEA_LEVEL { LAVA_ID } else { 0 }
}

/// W268: the pre-feature terrain block at a world position (feature
/// passes' cross-chunk read). W269: carver-aware - a carved cell
/// reads as the carver's lava / cave air, so features see the same
/// world the chunk carve produces.
pub(crate) fn nether_block_at(ctx: &WorldgenCtx, wx: i32, wy: i32, wz: i32) -> u16 {
    let seed = ctx.effective_seed();
    let nd = nether_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_cached(&nd, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    let (lx, lz) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
    let bedrock = super::features::block_id_by_name("bedrock").unwrap_or(45);
    let netherrack = super::features::block_id_by_name("netherrack").unwrap_or(1);
    let terrain = terrain_block(&nd, &col, lx, lz, wx, wy, wz, bedrock, netherrack);
    if (0..=TOP_Y).contains(&wy)
        && col.is_solid(lx, wy, lz)
        && !col.is_solid_carved(lx, wy, lz)
        // The chunk carve honors the replaceables tag (a gravel band
        // keeps its shelf); the read must mirror it exactly.
        && super::carver::nether_carvable(terrain)
    {
        // NetherWorldCarver.carveBlock's states.
        return if wy <= 31 { LAVA_ID } else { 0 };
    }
    terrain
}

/// The faithful Nether chunk generator (behind `WorldGen::Nether`).
pub(crate) fn gen_nether_mc(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let nd = nether_for(seed);
    let origin = World::chunk_world_origin(coord);
    let (ox, oy, oz) = (origin.x as i32, origin.y as i32, origin.z as i32);
    let mut chunk = Chunk::empty();
    if oy > TOP_Y {
        return chunk;
    }
    let col = column_cached(&nd, coord[0], coord[2], seed);
    let bedrock = super::features::block_id_by_name("bedrock").unwrap_or(45);
    let netherrack = super::features::block_id_by_name("netherrack").unwrap_or(1);
    chunk.fill_with(|x, y, z| {
        let wy = oy + y as i32;
        let (wx, wz) = (ox + x as i32, oz + z as i32);
        BlockId(terrain_block(&nd, &col, x, z, wx, wy, wz, bedrock, netherrack))
    });
    // W269: the nether_cave carver cuts the painted fill (MC's
    // fill -> surface -> carvers order; carved walls expose raw
    // blocks, lava floods below 32).
    super::carver::carve_nether(&mut chunk, coord, seed, [ox, oy, oz]);
    // W268: the biome decoration (glowstone, fire, deltas, fungi, the
    // nether ore set) - after the carve, like MC's decoration stage.
    super::nether_features::place_from_coord_nether(&mut chunk, coord, ctx);
    chunk
}

/// Post-carve floor for spawn helpers: the topmost solid below the
/// lava-proof band (a simple probe over the cached column).
pub(crate) fn nether_floor_at(ctx: &WorldgenCtx, wx: i32, wz: i32) -> i32 {
    let seed = ctx.effective_seed();
    let nd = nether_for(seed);
    let cs = CHUNK_SIZE as i32;
    let col = column_cached(&nd, wx.div_euclid(cs), wz.div_euclid(cs), seed);
    let (lx, lz) = (wx.rem_euclid(cs) as usize, wz.rem_euclid(cs) as usize);
    // Scan down from just under the roof band for the first solid
    // with open space above (a walkable nether floor). W269:
    // post-carve bits, so spawns don't land on carved-away ledges.
    for y in (SEA_LEVEL..=TOP_Y - 8).rev() {
        if col.is_solid_carved(lx, y, lz)
            && !col.is_solid_carved(lx, y + 1, lz)
            && !col.is_solid_carved(lx, y + 2, lz)
        {
            return y;
        }
    }
    SEA_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx {
            base_seed: 0,
            dim: "nether".into(),
            profile: WorldGen::Nether,
        }
    }

    /// Bedrock seals the floor row and the roof row; the lava sea
    /// fills open space below 32; netherrack dominates the bulk.
    #[test]
    fn nether_chunks_have_bedrock_shell_lava_sea_and_netherrack() {
        let c = ctx();
        let bedrock = super::super::features::block_id_by_name("bedrock").unwrap();
        let netherrack =
            super::super::features::block_id_by_name("netherrack").unwrap();
        let (mut rack, mut lava, mut air) = (0u32, 0u32, 0u32);
        for &(cx, cz) in &[(0, 0), (5, -3)] {
            // Floor chunk: y0 all bedrock.
            let floor = gen_nether_mc([cx, 0, cz], &c);
            for x in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        floor.get(x, 0, z).0,
                        bedrock,
                        "nether floor row must be bedrock"
                    );
                }
            }
            // Roof chunk: y127 all bedrock (local y 15 of cy 7).
            let roof = gen_nether_mc([cx, 7, cz], &c);
            for x in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(
                        roof.get(x, 15, z).0,
                        bedrock,
                        "nether roof row must be bedrock"
                    );
                }
            }
            for cy in 0..8 {
                let ch = gen_nether_mc([cx, cy, cz], &c);
                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        for z in 0..CHUNK_SIZE {
                            let id = ch.get(x, y, z).0;
                            if id == netherrack {
                                rack += 1;
                            } else if id == super::LAVA_ID {
                                lava += 1;
                            } else if id == 0 {
                                air += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(rack > 20_000, "netherrack bulk: {rack}");
        assert!(lava > 100, "the lava sea is missing: {lava}");
        assert!(air > 1_000, "no open caverns: {air}");
    }

    /// W299: the fill must run at MC's cell resolution. Full-res
    /// sampling of the nether blend noise (xz_factor 80) shreds the
    /// terrain into noise grain - a column then flips solid<->open
    /// many more times than the trilinear-interpolated fill. Pin
    /// both: the live fill stays smooth AND stays visibly smoother
    /// than a full-res evaluation of the same columns.
    #[test]
    fn nether_fill_uses_cell_interpolation() {
        let seed = ctx().effective_seed();
        let nd = nether_for(seed);
        let mut cell_flips = 0u32;
        let mut full_flips = 0u32;
        let mut cols = 0u32;
        for &(cx, cz) in &[(0, 0), (3, -2)] {
            let col = column_cached(&nd, cx, cz, seed);
            let (ox, oz) = (cx * CHUNK_SIZE as i32, cz * CHUNK_SIZE as i32);
            for lx in 0..CHUNK_SIZE {
                for lz in 0..CHUNK_SIZE {
                    cols += 1;
                    let mut prev_cell = col.is_solid(lx, 8, lz);
                    let mut prev_full = nd
                        .final_density
                        .eval((ox + lx as i32) as f64, 8.0, (oz + lz as i32) as f64)
                        > 0.0;
                    for y in 9..=120 {
                        let c = col.is_solid(lx, y, lz);
                        if c != prev_cell {
                            cell_flips += 1;
                            prev_cell = c;
                        }
                        let f = nd
                            .final_density
                            .eval(
                                (ox + lx as i32) as f64,
                                y as f64,
                                (oz + lz as i32) as f64,
                            )
                            > 0.0;
                        if f != prev_full {
                            full_flips += 1;
                            prev_full = f;
                        }
                    }
                }
            }
        }
        let cell_avg = cell_flips as f32 / cols as f32;
        let full_avg = full_flips as f32 / cols as f32;
        assert!(
            cell_avg < full_avg,
            "cell fill must be smoother than full-res: cell {cell_avg} vs full {full_avg}"
        );
        assert!(
            cell_avg < 8.0,
            "nether columns too fragmented (lattice inactive?): {cell_avg}"
        );
    }

    /// All five biomes materialize somewhere in a wide scan (the
    /// climate channels are smooth - basalt deltas need a deep
    /// temperature dip and can sit thousands of blocks out).
    #[test]
    fn all_five_nether_biomes_exist() {
        let nd = build_nether(0);
        let mut seen = std::collections::HashSet::new();
        'scan: for cx in (-300..300).step_by(3) {
            for cz in (-300..300).step_by(3) {
                seen.insert(nd.biome_at(cx * 16 + 8, cz * 16 + 8));
                if seen.len() == 5 {
                    break 'scan;
                }
            }
        }
        assert_eq!(seen.len(), 5, "biomes found: {seen:?}");
    }
}
