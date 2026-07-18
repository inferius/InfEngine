//! W183 - faithful MC worldgen, WAVE 3a: assemble the real overworld
//! `final_density` from the embedded MC data + the wave-1/2 engine.
//!
//! Builds the noise resolver (each referenced noise -> a seeded
//! `NormalNoise` from its real MC params) + the density-function
//! resolver (over the embedded `density_function/*.json` closure), then
//! parses `noise_settings/overworld.json`'s `noise_router.final_density`
//! into one [`Df`] tree. Sampling that tree at a world point gives MC's
//! terrain density: > 0 solid, < 0 air. Wave 3b turns it into chunks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::biome::{McBiome, mc_biome_full};
use super::data;
use super::density::{CELL_H, CellLattices, Df, NoiseResolver, parse};
use super::noise::{NoiseParams, NormalNoise};
use super::surface_rule::{SurfaceCtx, SurfaceRules};
use crate::worldgen::biome::Biome;

/// The assembled overworld terrain sampler + the vertical config read
/// from the MC noise settings.
pub(crate) struct Overworld {
    final_density: Df,
    /// The five climate parameters MC matches biomes against (the
    /// `noise_router` entries). Temperature + vegetation are
    /// shifted-noise; continents / erosion / ridges are the named
    /// density functions. Sampled by [`Overworld::biome_at`].
    temperature: Df,
    vegetation: Df,
    continents: Df,
    erosion: Df,
    ridges: Df,
    /// W213: the aquifer router entries (`Aquifer.NoiseBasedAquifer`
    /// reads these off the NoiseRouter) + the preliminary surface and
    /// depth channels.
    barrier: Df,
    fluid_level_floodedness: Df,
    fluid_level_spread: Df,
    lava: Df,
    depth: Df,
    preliminary_surface: Df,
    /// W215: the giant-ore-vein router channels (OreVeinifier). Their
    /// `interpolated` wrappers share the chunk-column lattices with
    /// `final_density` (ids continue after its nodes).
    vein_toggle: Df,
    vein_ridged: Df,
    vein_gap: Df,
    /// MC's `surface_rule` (grass / sand+sandstone / gravel / podzol /
    /// snow), parsed from the same settings. Evaluated per surface column
    /// by the terrain stage.
    surface: SurfaceRules,
    /// W217: the `surface` noise driving `SurfaceSystem.getSurfaceDepth`
    /// (the wobbling grass/dirt layer thickness).
    surface_noise: Arc<NormalNoise>,
    pub(crate) sea_level: i32,
    pub(crate) min_y: i32,
    pub(crate) height: i32,
    /// W304: per-column biome memo. One `mc_biome_at` costs 5 density
    /// evals (dozens of octave samples each) and the surface stage +
    /// every feature pass + spawns re-ask the same columns. RefCell
    /// is safe: `Overworld` lives in a per-thread `Rc` cache (one
    /// instance per thread + seed, never shared across threads).
    biome_memo: RefCell<HashMap<(i32, i32), McBiome>>,
}

impl Overworld {
    /// MC terrain density at a world block, FULL RESOLUTION (the
    /// `interpolated` wrappers evaluate their subtree directly). Kept
    /// for tests/diagnostics; terrain generation uses
    /// [`Overworld::density_cells`], which is what MC actually renders.
    pub(crate) fn density(&self, x: i32, y: i32, z: i32) -> f64 {
        self.final_density.eval(x as f64, y as f64, z as f64)
    }

    /// W213: `final_density` with MC's cell semantics - `interpolated`
    /// subtrees sampled from the prebuilt per-chunk-column corner
    /// lattices, everything outside per block. `> 0` = solid.
    pub(crate) fn density_cells(&self, x: i32, y: i32, z: i32, cells: &CellLattices) -> f64 {
        self.final_density.eval_cells(x as f64, y as f64, z as f64, cells)
    }

    /// Build the interpolation lattices for the chunk column at block
    /// origin `(x0, z0)`, spanning `min_y..=min_y+(ny-1)*CELL_H`
    /// vertically (W260: the corner base is the dimension floor -64,
    /// not 0). Covers `final_density` AND the vein channels (id order).
    pub(crate) fn build_cells(&self, x0: i32, z0: i32, top_y: i32) -> CellLattices {
        let ny = ((top_y - self.min_y) / CELL_H) as usize + 2;
        CellLattices::build(
            &[&self.final_density, &self.vein_toggle, &self.vein_ridged, &self.vein_gap],
            x0,
            z0,
            self.min_y,
            ny,
        )
    }

    /// W215: the vein channels with cell semantics (toggle + ridged
    /// are `interpolated` in the JSON; gap is a plain block-res noise).
    pub(crate) fn vein_toggle_cells(&self, x: i32, y: i32, z: i32, cells: &CellLattices) -> f64 {
        self.vein_toggle.eval_cells(x as f64, y as f64, z as f64, cells)
    }
    pub(crate) fn vein_ridged_cells(&self, x: i32, y: i32, z: i32, cells: &CellLattices) -> f64 {
        self.vein_ridged.eval_cells(x as f64, y as f64, z as f64, cells)
    }
    pub(crate) fn vein_gap_cells(&self, x: i32, y: i32, z: i32, cells: &CellLattices) -> f64 {
        self.vein_gap.eval_cells(x as f64, y as f64, z as f64, cells)
    }

    /// W213: `preliminary_surface_level` router entry, exactly
    /// `NoiseChunk.computePreliminarySurfaceLevel`: floor of the Df at
    /// the QUART-quantized column, y = 0. The aquifer keys everything
    /// off this surface.
    pub(crate) fn preliminary_surface(&self, x: i32, z: i32) -> i32 {
        let qx = (x.div_euclid(4) * 4) as f64;
        let qz = (z.div_euclid(4) * 4) as f64;
        self.preliminary_surface.eval(qx, 0.0, qz).floor() as i32
    }

    /// Aquifer noise channels (block-resolution Df evals).
    pub(crate) fn barrier(&self, x: i32, y: i32, z: i32) -> f64 {
        self.barrier.eval(x as f64, y as f64, z as f64)
    }
    pub(crate) fn floodedness(&self, x: i32, y: i32, z: i32) -> f64 {
        self.fluid_level_floodedness.eval(x as f64, y as f64, z as f64)
    }
    pub(crate) fn fluid_spread(&self, x: i32, y: i32, z: i32) -> f64 {
        self.fluid_level_spread.eval(x as f64, y as f64, z as f64)
    }
    pub(crate) fn lava_noise(&self, x: i32, y: i32, z: i32) -> f64 {
        self.lava.eval(x as f64, y as f64, z as f64)
    }
    /// erosion + depth at a block - `OverworldBiomeBuilder.isDeepDarkRegion`.
    pub(crate) fn is_deep_dark(&self, x: i32, y: i32, z: i32) -> bool {
        let (fx, fy, fz) = (x as f64, y as f64, z as f64);
        self.erosion.eval(fx, fy, fz) < -0.225 && self.depth.eval(fx, fy, fz) > 0.9
    }

    /// Engine biome at world column `(x, z)` - the full classification
    /// collapsed to the legacy 11-biome palette.
    pub(crate) fn biome_at(&self, x: i32, z: i32) -> Biome {
        self.mc_biome_at(x, z).to_engine()
    }

    /// W211: the REAL MC biome at world column `(x, z)` - the full
    /// OverworldBiomeBuilder classification (ocean variants, rivers,
    /// birch/dark/flower forests, ...) before the engine collapse. The
    /// temperature / vegetation channels are xz-only and continents /
    /// erosion / ridges are 2D, so `y` doesn't matter.
    pub(crate) fn mc_biome_at(&self, x: i32, z: i32) -> McBiome {
        if let Some(b) = self.biome_memo.borrow().get(&(x, z)) {
            return *b;
        }
        let (t, v, c, e, w) = self.climate_at(x, z);
        let b = mc_biome_full(t, v, c, e, w);
        let mut memo = self.biome_memo.borrow_mut();
        // ~262k entries = a few MB; wholesale clear keeps the common
        // steady-state (a moving gen frontier) fully cached.
        if memo.len() >= (1 << 18) {
            memo.clear();
        }
        memo.insert((x, z), b);
        b
    }

    /// W229.2: the raw climate sample `(temperature, vegetation,
    /// continents, erosion, weirdness)` at a column - terrain-shape
    /// diagnostics read the channels the slices key on.
    pub(crate) fn climate_at(&self, x: i32, z: i32) -> (f64, f64, f64, f64, f64) {
        let (fx, fz) = (x as f64, z as f64);
        (
            self.temperature.eval(fx, 0.0, fz),
            self.vegetation.eval(fx, 0.0, fz),
            self.continents.eval(fx, 0.0, fz),
            self.erosion.eval(fx, 0.0, fz),
            self.ridges.eval(fx, 0.0, fz),
        )
    }

    /// MC surface block for a column cell, or `None` to leave whatever
    /// the terrain stage placed (deep stone). See [`SurfaceCtx`].
    pub(crate) fn surface_eval(&self, ctx: &SurfaceCtx) -> Option<u16> {
        self.surface.eval(ctx)
    }

    /// W217: `SurfaceSystem.getSurfaceDepth`, exactly: the surface
    /// noise at (x, 0, z) * 2.75 + 3.0 + a positional 0..0.25 jitter.
    pub(crate) fn surface_depth(&self, x: i32, z: i32, seed: u32) -> i32 {
        let noise = self.surface_noise.value(x as f64, 0.0, z as f64);
        let mut r = super::noise::SplitMix64::new(
            (seed as u64) ^ 0x5FCE_DEA7
                ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
        );
        (noise * 2.75 + 3.0 + r.next_f64() * 0.25) as i32
    }
}

/// Per-noise seed: fold the noise name into the world seed so the
/// channels (continentalness / erosion / ...) are decorrelated, the
/// way MC's positional seeding decorrelates them.
fn noise_seed(seed: u32, name: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (seed as u64);
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

/// Parse the real overworld `final_density` for `seed`.
pub(crate) fn build_overworld(seed: u32) -> Overworld {
    let settings: Value =
        serde_json::from_str(data::OVERWORLD_SETTINGS).expect("overworld settings JSON");

    // df resolver: "minecraft:overworld/depth" -> embedded JSON Value.
    let df_res = |name: &str| -> Option<Value> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        data::df_json(n).and_then(|j| serde_json::from_str(j).ok())
    };
    // noise resolver: build a seeded NormalNoise from the real params.
    let noise_res = move |name: &str| -> Option<Arc<NormalNoise>> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        let params: NoiseParams = serde_json::from_str(data::noise_json(n)?).ok()?;
        Some(Arc::new(NormalNoise::new(noise_seed(seed, n), &params)))
    };

    let nr = &settings["noise_router"];
    let noise_res_ref = &noise_res as &NoiseResolver<'_>;
    // W216: old_blended_noise's legacy octave stacks seed off the world
    // seed like every other channel.
    let blended_seed = noise_seed(seed, "old_blended_noise");
    let p = |key: &str| parse(&nr[key], &df_res, noise_res_ref, blended_seed);
    let mut final_density = p("final_density");
    // W213: number the `interpolated` subtrees so the terrain stage can
    // build one corner lattice per node and sample with MC's cell
    // semantics.
    let mut next_id = 0usize;
    final_density.assign_interp_ids(&mut next_id);
    // The five climate channels MC matches biomes against.
    let temperature = p("temperature");
    let vegetation = p("vegetation");
    let continents = p("continents");
    let erosion = p("erosion");
    let ridges = p("ridges");
    // W213: the aquifer channels + preliminary surface + depth.
    let barrier = p("barrier");
    let fluid_level_floodedness = p("fluid_level_floodedness");
    let fluid_level_spread = p("fluid_level_spread");
    let lava = p("lava");
    let depth = p("depth");
    let preliminary_surface = p("preliminary_surface_level");
    // W215: the ore-vein channels; their interpolated ids continue
    // after final_density's so all four share one CellLattices.
    let mut vein_toggle = p("vein_toggle");
    let mut vein_ridged = p("vein_ridged");
    let mut vein_gap = p("vein_gap");
    vein_toggle.assign_interp_ids(&mut next_id);
    vein_ridged.assign_interp_ids(&mut next_id);
    vein_gap.assign_interp_ids(&mut next_id);
    // The surface rule reuses the same seeded-noise resolver (it samples
    // the `surface` / `surface_swamp` / `gravel` / ... noises); the seed
    // also builds the badlands clay bands (W227.3).
    let surface = SurfaceRules::parse(&settings["surface_rule"], &noise_res, seed);
    // W217: the surface-depth noise is the SAME seeded "surface" noise.
    let surface_noise = noise_res("surface").expect("surface noise params embedded");

    let noise = &settings["noise"];
    let min_y = noise["min_y"].as_i64().unwrap_or(-64) as i32;
    let height = noise["height"].as_i64().unwrap_or(384) as i32;
    // sea_level lives at the top level (MC overworld = 63).
    let sea_level = settings["sea_level"].as_i64().unwrap_or(63) as i32;

    Overworld {
        final_density,
        temperature,
        vegetation,
        continents,
        erosion,
        ridges,
        barrier,
        fluid_level_floodedness,
        fluid_level_spread,
        lava,
        depth,
        preliminary_surface,
        vein_toggle,
        vein_ridged,
        vein_gap,
        surface,
        surface_noise,
        sea_level,
        min_y,
        height,
        biome_memo: RefCell::new(HashMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evidence dump: how open is the FULL-RES density underground, and
    /// what do the individual cave sub-functions evaluate to? Run with
    /// `cargo test dump_cave_terms -p voxel_demo_mp --release --lib -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_cave_terms() {
        let ow = build_overworld(0);
        for y in [15, 30, 45] {
            let mut open = 0u32;
            let mut total = 0u32;
            for x in (-512..512).step_by(8) {
                for z in (-512..512).step_by(8) {
                    total += 1;
                    if ow.density(x, y, z) <= 0.0 {
                        open += 1;
                    }
                }
            }
            println!("y={y}: open {open}/{total} ({:.1}%)", 100.0 * open as f32 / total as f32);
        }
        // Individual cave terms at a few underground probes.
        let df_res = |name: &str| -> Option<serde_json::Value> {
            let n = name.strip_prefix("minecraft:").unwrap_or(name);
            data::df_json(n).and_then(|j| serde_json::from_str(j).ok())
        };
        let noise_res = move |name: &str| -> Option<std::sync::Arc<NormalNoise>> {
            let n = name.strip_prefix("minecraft:").unwrap_or(name);
            let params: NoiseParams = serde_json::from_str(data::noise_json(n)?).ok()?;
            Some(std::sync::Arc::new(NormalNoise::new(12345, &params)))
        };
        let noise_ref = &noise_res as &NoiseResolver<'_>;
        for name in [
            "overworld/caves/entrances",
            "overworld/caves/spaghetti_2d",
            "overworld/caves/noodle",
            "overworld/caves/pillars",
            "overworld/sloped_cheese",
        ] {
            let v = df_res(name).expect("df json");
            let d = parse(&v, &df_res, noise_ref, 12345);
            let samples: Vec<f64> = [(0, 30, 0), (100, 20, -50), (-200, 40, 300), (37, 25, 941)]
                .iter()
                .map(|&(x, y, z)| d.eval(x as f64, y as f64, z as f64))
                .collect();
            println!("{name}: {samples:?}");
        }
        // Full-res vs cell-interpolated, same chunk column.
        for cxz in [(0, 0), (128, 128), (-256, 64)] {
            let cells = ow.build_cells(cxz.0, cxz.1, 224);
            let (mut full_open, mut cell_open, mut n) = (0u32, 0u32, 0u32);
            let mut diffs = 0.0f64;
            for y in [15, 25, 35, 45] {
                for lx in 0..16 {
                    for lz in 0..16 {
                        let (x, z) = (cxz.0 + lx, cxz.1 + lz);
                        let f = ow.density(x, y, z);
                        let c = ow.density_cells(x, y, z, &cells);
                        n += 1;
                        diffs += (f - c).abs();
                        if f <= 0.0 {
                            full_open += 1;
                        }
                        if c <= 0.0 {
                            cell_open += 1;
                        }
                    }
                }
            }
            println!(
                "col {cxz:?}: full_open {full_open} cell_open {cell_open} / {n}, mean |diff| {:.4}",
                diffs / n as f64
            );
        }
    }

    #[test]
    fn settings_parse_with_mc_vertical_config() {
        let ow = build_overworld(12345);
        assert_eq!(ow.min_y, -64);
        assert_eq!(ow.height, 384);
        assert_eq!(ow.sea_level, 63);
    }

    /// The whole engine end-to-end on REAL MC data: deep underground is
    /// solid (density > 0), high in the sky is open (density < 0), and
    /// the surface band straddles zero. This is the load-bearing test
    /// that the noise + density evaluator + spline + the embedded data
    /// all fit together.
    #[test]
    fn density_is_solid_deep_and_open_high() {
        let ow = build_overworld(2024);
        // Sample several columns so we're not unlucky on one spot.
        let mut deep_solid = 0;
        let mut high_open = 0;
        let cols = [(0, 0), (200, 200), (-300, 150), (1000, -800), (37, 941)];
        for &(x, z) in &cols {
            if ow.density(x, ow.min_y + 8, z) > 0.0 {
                deep_solid += 1;
            }
            if ow.density(x, 300, z) < 0.0 {
                high_open += 1;
            }
        }
        assert!(deep_solid >= 4, "deep should be mostly solid, got {deep_solid}/5");
        assert_eq!(high_open, 5, "y=300 (above terrain) must be open everywhere");
    }

    #[test]
    fn density_is_deterministic_per_seed() {
        let a = build_overworld(99);
        let b = build_overworld(99);
        for &(x, y, z) in &[(0, 64, 0), (123, 40, -77), (-50, 90, 50)] {
            assert_eq!(a.density(x, y, z), b.density(x, y, z));
        }
    }
}
