//! Climate-driven biome map for the `Realistic` generator.
//!
//! Three independent slow 2D noises drive temperature, humidity,
//! and elevation. Each one uses the same sin/cos stack shape as
//! the legacy `biome_at` (so the world reads at the same broad
//! scale), but with a distinct seed-derived phase offset per
//! channel so the three components don't correlate. The output
//! of `climate_at` feeds the biome lookup table in `biome_table`.
//!
//! All three channels are normalised to roughly `[-1, 1]`. The
//! Climate struct is `Copy` so the chunk-fill closure can re-read
//! a per-(wx,wz) sample for the height curve + surface block
//! pick without re-running the noise.

use infengine_voxel::{BlockId, Chunk, ChunkCoord, World};

use super::biome::Biome;
use super::biome_table::biome_from_climate;
use super::ore::pick_stone_or_ore;
use super::{WorldgenCtx, caves, decor, dungeons, ravines};

/// Per-(wx, wz) climate sample. Each component is normalised to
/// roughly `[-1, 1]`. Re-derived deterministically from the world
/// seed; never persisted.
#[derive(Debug, Copy, Clone)]
pub(crate) struct Climate {
    /// -1.0 = frigid, +1.0 = scorching.
    pub temperature: f32,
    /// -1.0 = arid, +1.0 = lush.
    pub humidity: f32,
    /// -1.0 = deep ocean, +1.0 = peak. Normalised height
    /// pre-multiplier; feeds the height curve in `realistic`.
    pub elevation: f32,
}

/// Sea level used by the Realistic generator. Constant across
/// biomes so the shoreline is predictable; pinned a notch above
/// the Desert base height so deserts sit above water and any
/// "Beach" tile reads as a thin sandy collar.
pub(crate) const SEA_LEVEL: i32 = 12;

/// Channel phase offsets. Each one xor-mixes a fixed magic number
/// into the seed so distinct hosts get distinct temperature /
/// humidity / elevation maps without burning more atomic state.
///
/// F.2c-1: takes the effective seed as an explicit parameter
/// instead of reading the global atomic. Callers pass
/// `ctx.effective_seed()`.
fn channel_offset(channel: u32, seed: u32) -> f32 {
    let s = seed ^ channel;
    (s.wrapping_mul(0x9E37_79B1) as f32 / u32::MAX as f32) * 1000.0
}

/// 2D sin/cos stack. Returns a value in roughly `[-1.4, 1.4]`;
/// the caller clamps when it cares about strict bounds. Layered
/// frequencies + offsets shake out into a "broad gradient with
/// a touch of wobble" pattern, same shape as `biome_at` so the
/// regions stay biome-scale.
fn noise2(wx: f32, wz: f32, phase: f32, scale: f32) -> f32 {
    let a = ((wx + phase) * scale).sin() * 0.6;
    let b = ((wz + phase) * scale * 0.75).cos() * 0.5;
    let c = (((wx + wz) + phase) * scale * 0.33).sin() * 0.3;
    a + b + c
}

/// Climate sample at a world-space (wx, wz). Uses three distinct
/// channel offsets so temperature, humidity, and elevation don't
/// share a phase.
///
/// F.2c-1: takes the effective seed as an explicit parameter
/// instead of reading the global atomic. Callers pass
/// `ctx.effective_seed()`.
pub(crate) fn climate_at(wx: f32, wz: f32, seed: u32) -> Climate {
    // Channel constants are arbitrary but stable. Don't reuse the
    // legacy `biome_at` offset (0x9E37_79B1 via seed_offset()) so
    // the Classic + Realistic maps look distinct under the same
    // seed.
    let p_temp = channel_offset(0x4E15_77B7, seed);
    let p_hum = channel_offset(0xC2B2_AE35, seed);
    let p_elev = channel_offset(0x1656_67B1, seed);

    // Elevation rides a slightly higher frequency so coastlines
    // wrinkle inside biome regions, and humidity a slightly
    // lower one so swamps + jungles read as broad bands.
    let temperature = noise2(wx, wz, p_temp, 0.011).clamp(-1.0, 1.0);
    let humidity = noise2(wx, wz, p_hum, 0.008).clamp(-1.0, 1.0);
    let elevation = noise2(wx, wz, p_elev, 0.014).clamp(-1.0, 1.0);

    Climate {
        temperature,
        humidity,
        elevation,
    }
}

/// Map an elevation sample to an integer world-Y height. Negative
/// elevation goes below sea level (becomes water); positive
/// elevation climbs toward peak. Biome-specific bonuses are
/// applied on top so mountain biomes outpace forest, etc.
pub(crate) fn realistic_height(climate: Climate, biome: Biome) -> i32 {
    // Base elevation maps [-1, 1] to roughly [SEA_LEVEL - 8,
    // SEA_LEVEL + 16]. Peaks need extra amplitude on top via the
    // biome bias below.
    let base = SEA_LEVEL as f32 + climate.elevation * 12.0;
    let (bias, amp) = match biome {
        Biome::Mountain | Biome::SnowyMountain => (10.0, 16.0),
        Biome::Beach => (-1.0, 1.5),
        Biome::Swamp => (-3.0, 2.0),
        Biome::Jungle => (4.0, 6.0),
        Biome::Forest | Biome::Taiga => (3.0, 4.0),
        Biome::Plains | Biome::Savanna => (2.0, 3.0),
        Biome::Desert => (1.5, 2.5),
        Biome::Tundra => (1.0, 3.0),
    };
    // Add a fine-grain wiggle so flat-ish biomes still get
    // visible elevation variance without hand-rolling a second
    // noise per biome.
    let wiggle = climate.elevation.abs() * amp;
    let h = base + bias + wiggle * climate.humidity.signum();
    h.round() as i32
}

/// Surface block id for the given biome. The Realistic generator
/// uses these for the y==h cap; sub-surface blocks come from
/// `near_surface_block`.
///
/// W17.4 fix: Mountain biome below the snow line previously returned
/// stone (id 1), making the terrain look uniformly grey. Now:
/// - h < 22  -> grass (lower mountain slopes, same as forest)
/// - 22 <= h < 34 -> stone (bare rocky mid-slope)
/// - h >= 34 -> snow cap
pub(crate) fn surface_block(biome: Biome, h: i32) -> BlockId {
    match biome {
        Biome::Desert | Biome::Beach => BlockId(4), // sand
        Biome::Plains | Biome::Forest | Biome::Jungle | Biome::Savanna | Biome::Swamp => {
            BlockId(3) // grass
        }
        Biome::Taiga | Biome::Tundra => BlockId(15), // snow cap
        Biome::SnowyMountain => BlockId(15),
        Biome::Mountain => {
            if h >= 34 {
                BlockId(15) // snow peak
            } else if h >= 22 {
                BlockId(1) // bare stone mid-slope
            } else {
                BlockId(3) // grass lower slope
            }
        }
    }
}

pub(crate) fn near_surface_block(biome: Biome) -> BlockId {
    match biome {
        Biome::Desert | Biome::Beach => BlockId(26), // sandstone
        Biome::Mountain | Biome::SnowyMountain => BlockId(1),
        _ => BlockId(2), // dirt
    }
}

/// Realistic chunk generator. Climate -> biome -> per-cell
/// height + surface + ore. Negative-elevation cells under sea
/// level fill with water. W17 adds 3D-noise cave carving,
/// ravines, and dungeon rooms.
pub(crate) fn gen_realistic(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let climate = climate_at(wx, wz, seed);
        let biome = biome_from_climate(climate);
        let h = realistic_height(climate, biome);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            // Fill the gap up to sea level with water so oceans
            // + lake floors look right. Only Beach + Swamp +
            // negative-elevation cells produce h < SEA_LEVEL so
            // the rest of the world is unaffected.
            if wy <= SEA_LEVEL && h < SEA_LEVEL {
                return BlockId(36);
            }
            return BlockId::AIR;
        }
        // W17.1: 3D-noise cave carver replaces the old cave_at call.
        // The per-chunk pass below handles carving after terrain fill.
        if wy == h {
            surface_block(biome, h)
        } else if wy >= h - 3 {
            near_surface_block(biome)
        } else {
            pick_stone_or_ore(wx, wy, wz, biome, seed)
        }
    });
    // W17.1: 3D-noise cave carver pass. Runs before decorations so
    // trees/flowers are never placed on caved-out ground.
    caves::carve(&mut chunk, coord, ctx);
    // W17.2: ravine carver pass.
    ravines::carve(&mut chunk, coord, ctx);
    // Decoration passes (W79: unified driver).
    decor::place_realistic(&mut chunk, coord, ctx);
    // W17.3: dungeon room pass (runs last, after decorations, so it
    // can write cobblestone walls over any mis-placed decoration).
    dungeons::place(&mut chunk, coord, ctx);
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    // W5.1b: every helper takes seed as an explicit parameter;
    // no globals to lock or seed.

    #[test]
    fn climate_at_is_deterministic_per_seed() {
        let a = climate_at(123.0, 456.0, 7);
        let b = climate_at(123.0, 456.0, 7);
        assert_eq!(a.temperature.to_bits(), b.temperature.to_bits());
        assert_eq!(a.humidity.to_bits(), b.humidity.to_bits());
        assert_eq!(a.elevation.to_bits(), b.elevation.to_bits());
    }

    #[test]
    fn climate_channels_are_decorrelated() {
// Sample 4096 cells. The three channels share the same
        // noise shape but with distinct phases, so two channels
        // should never read as a constant scalar of each other.
        let mut equal_temp_hum = 0;
        let mut equal_temp_elev = 0;
        let mut equal_hum_elev = 0;
        for x in 0..64 {
            for z in 0..64 {
                let c = climate_at(x as f32 * 4.0, z as f32 * 4.0, 0);
                if (c.temperature - c.humidity).abs() < 0.001 {
                    equal_temp_hum += 1;
                }
                if (c.temperature - c.elevation).abs() < 0.001 {
                    equal_temp_elev += 1;
                }
                if (c.humidity - c.elevation).abs() < 0.001 {
                    equal_hum_elev += 1;
                }
            }
        }
        // No two channels should agree on more than ~1% of cells
        // (chance crossings near zero are fine).
        assert!(equal_temp_hum < 64, "temp~=hum on {equal_temp_hum}/4096");
        assert!(equal_temp_elev < 64, "temp~=elev on {equal_temp_elev}/4096");
        assert!(equal_hum_elev < 64, "hum~=elev on {equal_hum_elev}/4096");
    }

    #[test]
    fn climate_channels_span_full_range() {
let mut t_min = f32::INFINITY;
        let mut t_max = f32::NEG_INFINITY;
        let mut h_min = f32::INFINITY;
        let mut h_max = f32::NEG_INFINITY;
        let mut e_min = f32::INFINITY;
        let mut e_max = f32::NEG_INFINITY;
        // Sweep 1024x1024 cells in 4-cell steps so the lowest-
        // frequency humidity channel (scale=0.008) spans several
        // full periods. A tighter grid leaves the min/max
        // sampling phase-locked to a fixed offset.
        for x in (-512..512).step_by(4) {
            for z in (-512..512).step_by(4) {
                let c = climate_at(x as f32, z as f32, 0);
                t_min = t_min.min(c.temperature);
                t_max = t_max.max(c.temperature);
                h_min = h_min.min(c.humidity);
                h_max = h_max.max(c.humidity);
                e_min = e_min.min(c.elevation);
                e_max = e_max.max(c.elevation);
            }
        }
        assert!(t_min < -0.5 && t_max > 0.5, "temp range [{t_min}, {t_max}]");
        assert!(h_min < -0.5 && h_max > 0.5, "hum range [{h_min}, {h_max}]");
        assert!(e_min < -0.5 && e_max > 0.5, "elev range [{e_min}, {e_max}]");
    }

    #[test]
    fn climate_changes_with_seed() {
        let a = climate_at(100.0, 100.0, 1);
        let b = climate_at(100.0, 100.0, 99);
        assert!(
            (a.temperature - b.temperature).abs() > 0.01
                || (a.humidity - b.humidity).abs() > 0.01
                || (a.elevation - b.elevation).abs() > 0.01,
        );
    }

    #[test]
    fn realistic_height_under_sea_for_deep_elevation() {
let c = Climate {
            temperature: 0.0,
            humidity: 0.5,
            elevation: -1.0,
        };
        // Beach is a low-amp biome so even at min elevation it
        // sits just below sea level.
        let h = realistic_height(c, Biome::Beach);
        assert!(h < SEA_LEVEL, "beach h={h}, sea={SEA_LEVEL}");
    }

    #[test]
    fn realistic_height_for_mountain_climbs() {
let c = Climate {
            temperature: -0.5,
            humidity: 0.5,
            elevation: 1.0,
        };
        let h = realistic_height(c, Biome::Mountain);
        assert!(h > 30, "mountain h={h}");
    }

    // Avoid "unused fn" warnings until E.3b wires the chunk gen.
    #[test]
    fn gen_realistic_smoke_runs() {
let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 0,
            dim: String::new(),
            profile: crate::worldgen::WorldGen::Realistic,
        };
        let _ = gen_realistic([0, 0, 0], &ctx);
    }

    // Sanity: SEA_LEVEL sits below Mountain base + above the
    // typical Desert base. Pre-validates the constant before any
    // shoreline test depends on it.
    #[test]
    fn sea_level_in_sane_range() {
        const { assert!(SEA_LEVEL > 8 && SEA_LEVEL < 30) };
    }
}
