//! W213 - MC aquifers, ported from the decompiled 26.1.2
//! `Aquifer.NoiseBasedAquifer`.
//!
//! An aquifer decides what fills each OPEN (density <= 0) cell:
//! air, water (at a local level), or lava. The algorithm: random
//! "aquifer points" on a 16x12x16 grid; per block the 12 candidate
//! points of the surrounding 2x3x2 cell neighbourhood are ranked by
//! distance; each point owns a lazily computed [`FluidStatus`] (fluid
//! surface level + type) keyed off the PRELIMINARY surface and the
//! floodedness / spread / lava noises; between two nearly-equidistant
//! points with DIFFERENT statuses, the barrier noise + a fluid-level
//! pressure gradient can force stone (`density + pressure > 0`), which
//! is what seals cave lakes apart.
//!
//! Faithful with the engine's world band: the engine floor is y=0
//! (MC -64..0 is not realized - the established Anchor convention), so
//! the global LAVA band (y < -54) and the deep `fluidSurfaceLevel <=
//! -10` lava pockets never fire here; aquifers above y=0 behave like
//! MC's. Randomness is the engine's column-keyed SplitMix64 streams
//! (documented divergence: rules, not seed identity).

use std::collections::HashMap;

use super::noise::SplitMix64;
use super::overworld::Overworld;
use crate::worldgen::climate::SEA_LEVEL;

/// `Aquifer.FluidStatus` - the fluid column a point owns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FluidStatus {
    pub(crate) fluid_level: i32,
    pub(crate) lava: bool,
}

impl FluidStatus {
    /// `FluidStatus.at`: the block at `y` inside this status.
    fn at(&self, y: i32) -> Substance {
        if y < self.fluid_level {
            if self.lava { Substance::Lava } else { Substance::Water }
        } else {
            Substance::Air
        }
    }
}

/// What an open cell becomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Substance {
    /// Aquifer pressure seals the cell: place stone after all.
    Solid,
    Air,
    Water,
    Lava,
}

/// `DimensionType.WAY_BELOW_MIN_Y` stand-in ("no fluid here").
const WAY_BELOW: i32 = i32::MIN / 2;

/// The global fluid rule (`NoiseBasedChunkGenerator.createFluidPicker`):
/// lava below min(-54, sea), else water at sea level. The engine floor
/// is 0, so only the water branch is reachable.
fn global_fluid(y: i32) -> FluidStatus {
    if y < (-54).min(SEA_LEVEL) {
        FluidStatus { fluid_level: -54, lava: true }
    } else {
        FluidStatus { fluid_level: SEA_LEVEL, lava: false }
    }
}

/// `Aquifer.NoiseBasedAquifer.similarity` (threshold 25).
fn similarity(dist1: i32, dist2: i32) -> f64 {
    1.0 - (dist2 - dist1) as f64 / 25.0
}

// Grid mapping (16 x 12 x 16 cells).
fn grid_x(b: i32) -> i32 {
    b >> 4
}
fn grid_y(b: i32) -> i32 {
    b.div_euclid(12)
}
fn grid_z(b: i32) -> i32 {
    b >> 4
}

const SALT_AQUIFER: u64 = 0xA01F_E500;

/// Per-chunk-column aquifer state: lazy caches for the grid point
/// locations + statuses and the quart-quantized preliminary surface.
/// Every cached value is a pure function of (seed, position), so
/// neighbouring chunk columns recompute identical values - no seams.
pub(crate) struct Aquifer {
    seed: u32,
    /// Above this Y the global fluid applies directly (the max
    /// preliminary surface over the chunk's grid span + margin, exactly
    /// `skipSamplingAboveY`).
    skip_above_y: i32,
    locations: HashMap<(i32, i32, i32), (i32, i32, i32)>,
    statuses: HashMap<(i32, i32, i32), FluidStatus>,
    prelim: HashMap<(i32, i32), i32>,
}

impl Aquifer {
    /// Build for the chunk column at block origin `(x0, z0)`.
    pub(crate) fn new(ow: &Overworld, seed: u32, x0: i32, z0: i32) -> Aquifer {
        let mut a = Aquifer {
            seed,
            skip_above_y: 0,
            locations: HashMap::new(),
            statuses: HashMap::new(),
            prelim: HashMap::new(),
        };
        // skipSamplingAboveY: max preliminary surface over the grid span
        // covering the chunk (+8 adjust), then up to the top of the
        // covering grid row + margin (source lines 134-138).
        let min_gx = grid_x(x0 - 5);
        let max_gx = grid_x(x0 + 15 - 5) + 1;
        let min_gz = grid_z(z0 - 5);
        let max_gz = grid_z(z0 + 15 - 5) + 1;
        let mut max_prelim = i32::MIN;
        let (bx0, bx1) = (min_gx << 4, (max_gx << 4) + 9);
        let (bz0, bz1) = (min_gz << 4, (max_gz << 4) + 9);
        let mut bz = bz0;
        while bz <= bz1 {
            let mut bx = bx0;
            while bx <= bx1 {
                max_prelim = max_prelim.max(a.prelim_surface(ow, bx, bz));
                bx += 4;
            }
            bz += 4;
        }
        let max_adjusted = max_prelim + 8;
        let skip_grid_y = grid_y(max_adjusted + 12) + 1;
        a.skip_above_y = (skip_grid_y * 12 + 11) - 1;
        a
    }

    /// Above this Y open cells take the global fluid directly.
    pub(crate) fn skip_above_y(&self) -> i32 {
        self.skip_above_y
    }

    fn prelim_surface(&mut self, ow: &Overworld, x: i32, z: i32) -> i32 {
        let key = (x.div_euclid(4), z.div_euclid(4));
        if let Some(v) = self.prelim.get(&key) {
            return *v;
        }
        let v = ow.preliminary_surface(x, z);
        self.prelim.insert(key, v);
        v
    }

    /// The random aquifer point of grid cell `(gx, gy, gz)`.
    fn location(&mut self, gx: i32, gy: i32, gz: i32) -> (i32, i32, i32) {
        if let Some(l) = self.locations.get(&(gx, gy, gz)) {
            return *l;
        }
        // Positional random: fold the grid coords into the seed the
        // same SplitMix64 way the feature streams do.
        let mut h = SplitMix64::new(
            (self.seed as u64) ^ SALT_AQUIFER
                ^ (gx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (gy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ (gz as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
        );
        let l = (
            (gx << 4) + (h.next_u64() % 10) as i32,
            gy * 12 + (h.next_u64() % 9) as i32,
            (gz << 4) + (h.next_u64() % 10) as i32,
        );
        self.locations.insert((gx, gy, gz), l);
        l
    }

    /// `computeSubstance`: what fills the cell at `(x, y, z)` whose
    /// final density is `density` (<= 0 for open cells; callers pass
    /// solid cells straight through as rock).
    pub(crate) fn substance(&mut self, ow: &Overworld, x: i32, y: i32, z: i32, density: f64) -> Substance {
        if density > 0.0 {
            return Substance::Solid;
        }
        let global = global_fluid(y);
        if y > self.skip_above_y {
            return global.at(y);
        }
        if global.at(y) == Substance::Lava {
            return Substance::Lava;
        }

        // Rank the 12 candidate points of the 2x3x2 cell neighbourhood.
        let xa = grid_x(x - 5);
        let ya = grid_y(y + 1);
        let za = grid_z(z - 5);
        let mut d1 = i32::MAX;
        let mut d2 = i32::MAX;
        let mut d3 = i32::MAX;
        let mut c1 = (0, 0, 0);
        let mut c2 = (0, 0, 0);
        let mut c3 = (0, 0, 0);
        for dx in 0..=1 {
            for dy in -1..=1 {
                for dz in 0..=1 {
                    let cell = (xa + dx, ya + dy, za + dz);
                    let loc = self.location(cell.0, cell.1, cell.2);
                    let (lx, ly, lz) = (loc.0 - x, loc.1 - y, loc.2 - z);
                    let dist = lx * lx + ly * ly + lz * lz;
                    if d1 >= dist {
                        c3 = c2;
                        c2 = c1;
                        c1 = cell;
                        d3 = d2;
                        d2 = d1;
                        d1 = dist;
                    } else if d2 >= dist {
                        c3 = c2;
                        c2 = cell;
                        d3 = d2;
                        d2 = dist;
                    } else if d3 >= dist {
                        c3 = cell;
                        d3 = dist;
                    }
                }
            }
        }

        let status1 = self.status(ow, c1);
        let sim12 = similarity(d1, d2);
        let fluid_here = status1.at(y);
        if sim12 <= 0.0 {
            return fluid_here;
        }
        // (The MC water-over-lava fast path needs global lava below,
        // unreachable in the engine band - falls through naturally.)
        let mut barrier_cache = f64::NAN;
        let status2 = self.status(ow, c2);
        let p12 = sim12 * self.pressure(ow, [x, y, z], &mut barrier_cache, status1, status2);
        if density + p12 > 0.0 {
            return Substance::Solid;
        }
        let status3 = self.status(ow, c3);
        let sim13 = similarity(d1, d3);
        if sim13 > 0.0 {
            let p13 = sim12 * sim13 * self.pressure(ow, [x, y, z], &mut barrier_cache, status1, status3);
            if density + p13 > 0.0 {
                return Substance::Solid;
            }
        }
        let sim23 = similarity(d2, d3);
        if sim23 > 0.0 {
            let p23 = sim12 * sim23 * self.pressure(ow, [x, y, z], &mut barrier_cache, status2, status3);
            if density + p23 > 0.0 {
                return Substance::Solid;
            }
        }
        fluid_here
    }

    /// `calculatePressure`: the fluid-level gradient + barrier noise
    /// between two statuses at this block.
    fn pressure(
        &mut self,
        ow: &Overworld,
        pos: [i32; 3],
        barrier_cache: &mut f64,
        s1: FluidStatus,
        s2: FluidStatus,
    ) -> f64 {
        let [x, y, z] = pos;
        let t1 = s1.at(y);
        let t2 = s2.at(y);
        if (t1 == Substance::Lava && t2 == Substance::Water)
            || (t1 == Substance::Water && t2 == Substance::Lava)
        {
            return 2.0;
        }
        let level_diff = (s1.fluid_level - s2.fluid_level).abs();
        if level_diff == 0 {
            return 0.0;
        }
        let avg_level = 0.5 * (s1.fluid_level + s2.fluid_level) as f64;
        let above_avg = y as f64 + 0.5 - avg_level;
        let base = level_diff as f64 / 2.0;
        let toward_middle = base - above_avg.abs();
        let gradient = if above_avg > 0.0 {
            if toward_middle > 0.0 { toward_middle / 1.5 } else { toward_middle / 2.5 }
        } else {
            let v = 3.0 + toward_middle;
            if v > 0.0 { v / 3.0 } else { v / 10.0 }
        };
        let noise = if (-2.0..=2.0).contains(&gradient) {
            if barrier_cache.is_nan() {
                *barrier_cache = ow.barrier(x, y, z);
            }
            *barrier_cache
        } else {
            0.0
        };
        2.0 * (noise + gradient)
    }

    /// `getAquiferStatus` + `computeFluid`: the fluid status a grid
    /// point owns, computed at its random location.
    fn status(&mut self, ow: &Overworld, cell: (i32, i32, i32)) -> FluidStatus {
        if let Some(s) = self.statuses.get(&cell) {
            return *s;
        }
        let (x, y, z) = self.location(cell.0, cell.1, cell.2);
        let s = self.compute_fluid(ow, x, y, z);
        self.statuses.insert(cell, s);
        s
    }

    /// The 13-chunk surface sampling offsets (`SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`).
    const SURFACE_OFFSETS: [(i32, i32); 13] = [
        (0, 0),
        (-2, -1),
        (-1, -1),
        (0, -1),
        (1, -1),
        (-3, 0),
        (-2, 0),
        (-1, 0),
        (1, 0),
        (-2, 1),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];

    fn compute_fluid(&mut self, ow: &Overworld, x: i32, y: i32, z: i32) -> FluidStatus {
        let global = global_fluid(y);
        let mut lowest_prelim = i32::MAX;
        let cell_top = y + 12;
        let cell_bottom = y - 12;
        let mut center_under_global = false;
        for (ox, oz) in Self::SURFACE_OFFSETS {
            let sx = x + ox * 16;
            let sz = z + oz * 16;
            let prelim = self.prelim_surface(ow, sx, sz);
            let adjusted = prelim + 8;
            let start = ox == 0 && oz == 0;
            if start && cell_bottom > adjusted {
                return global;
            }
            let pokes_above = cell_top > adjusted;
            if pokes_above || start {
                let global_at_surface = global_fluid(adjusted);
                if global_at_surface.at(adjusted) != Substance::Air {
                    if start {
                        center_under_global = true;
                    }
                    if pokes_above {
                        return global_at_surface;
                    }
                }
            }
            lowest_prelim = lowest_prelim.min(prelim);
        }

        let level =
            self.compute_surface_level(ow, [x, y, z], global, lowest_prelim, center_under_global);
        FluidStatus {
            fluid_level: level,
            lava: self.compute_fluid_is_lava(ow, x, y, z, global, level),
        }
    }

    /// `computeSurfaceLevel`: floodedness noise against thresholds keyed
    /// by depth-below-surface decides no-fluid / spread-level / global.
    fn compute_surface_level(
        &mut self,
        ow: &Overworld,
        pos: [i32; 3],
        global: FluidStatus,
        lowest_prelim: i32,
        center_under_global: bool,
    ) -> i32 {
        let [x, y, z] = pos;
        let (partial, full) = if ow.is_deep_dark(x, y, z) {
            (-1.0, -1.0)
        } else {
            let below_surface = lowest_prelim + 8 - y;
            let factor = if center_under_global {
                (1.0 - below_surface as f64 / 64.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let noise = ow.floodedness(x, y, z).clamp(-1.0, 1.0);
            // Mth.map(factor, 1, 0, a, b) = a + (1 - factor) * (b - a).
            let full_thr = -0.3 + (1.0 - factor) * (0.8 - -0.3);
            let partial_thr = -0.8 + (1.0 - factor) * (0.4 - -0.8);
            (noise - partial_thr, noise - full_thr)
        };
        if full > 0.0 {
            global.fluid_level
        } else if partial > 0.0 {
            self.randomized_fluid_level(ow, x, y, z, lowest_prelim)
        } else {
            WAY_BELOW
        }
    }

    /// `computeRandomizedFluidSurfaceLevel`: 16x40x16 cells, spread
    /// noise (quantized to 3) around the cell middle.
    fn randomized_fluid_level(&mut self, ow: &Overworld, x: i32, y: i32, z: i32, lowest_prelim: i32) -> i32 {
        let cx = x.div_euclid(16);
        let cy = y.div_euclid(40);
        let cz = z.div_euclid(16);
        let cell_mid = cy * 40 + 20;
        let spread = ow.fluid_spread(cx, cy, cz) * 10.0;
        let quantized = (spread / 3.0).floor() as i32 * 3;
        (cell_mid + quantized).min(lowest_prelim)
    }

    /// `computeFluidType`: deep pockets (level <= -10) roll the lava
    /// noise on a 64x40x64 cell. Below the engine band in practice.
    fn compute_fluid_is_lava(
        &mut self,
        ow: &Overworld,
        x: i32,
        y: i32,
        z: i32,
        global: FluidStatus,
        level: i32,
    ) -> bool {
        if level <= -10 && level != WAY_BELOW && !global.lava {
            let cx = x.div_euclid(64);
            let cy = y.div_euclid(40);
            let cz = z.div_euclid(64);
            if ow.lava_noise(cx, cy, cz).abs() > 0.3 {
                return true;
            }
        }
        global.lava
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::mcgen::overworld::build_overworld;

    #[test]
    fn global_fluid_is_water_at_sea() {
        assert_eq!(global_fluid(40).at(40), Substance::Water);
        assert_eq!(global_fluid(63).at(63), Substance::Air);
        assert_eq!(global_fluid(100).at(100), Substance::Air);
    }

    #[test]
    fn similarity_matches_mc_threshold() {
        assert!((similarity(0, 25) - 0.0).abs() < 1e-9);
        assert!((similarity(10, 10) - 1.0).abs() < 1e-9);
        assert!(similarity(0, 50) < 0.0);
    }

    /// Solid cells pass through; far above the surface the global rule
    /// answers directly (air above sea, water below).
    #[test]
    fn substance_respects_density_and_sky() {
        let ow = build_overworld(7);
        let mut a = Aquifer::new(&ow, 7, 0, 0);
        assert_eq!(a.substance(&ow, 8, 90, 8, 0.5), Substance::Solid);
        let sky = a.skip_above_y + 5;
        assert_eq!(a.substance(&ow, 8, sky, 8, -0.5), Substance::Air);
    }

    /// Determinism across instances: two aquifers for the same column
    /// agree cell by cell (the caches are pure functions of seed+pos).
    #[test]
    fn deterministic_per_seed() {
        let ow = build_overworld(11);
        let mut a = Aquifer::new(&ow, 11, 16, 32);
        let mut b = Aquifer::new(&ow, 11, 16, 32);
        for y in (5..60).step_by(7) {
            assert_eq!(
                a.substance(&ow, 20, y, 40, -0.2),
                b.substance(&ow, 20, y, 40, -0.2)
            );
        }
    }
}
