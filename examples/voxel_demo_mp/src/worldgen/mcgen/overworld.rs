//! W183 - faithful MC worldgen, WAVE 3a: assemble the real overworld
//! `final_density` from the embedded MC data + the wave-1/2 engine.
//!
//! Builds the noise resolver (each referenced noise -> a seeded
//! `NormalNoise` from its real MC params) + the density-function
//! resolver (over the embedded `density_function/*.json` closure), then
//! parses `noise_settings/overworld.json`'s `noise_router.final_density`
//! into one [`Df`] tree. Sampling that tree at a world point gives MC's
//! terrain density: > 0 solid, < 0 air. Wave 3b turns it into chunks.

use std::sync::Arc;

use serde_json::Value;

use super::data;
use super::density::{Df, NoiseResolver, parse};
use super::noise::{NoiseParams, NormalNoise};

/// The assembled overworld terrain sampler + the vertical config read
/// from the MC noise settings.
pub(crate) struct Overworld {
    final_density: Df,
    pub(crate) sea_level: i32,
    pub(crate) min_y: i32,
    pub(crate) height: i32,
}

impl Overworld {
    /// MC terrain density at a world block. `> 0` = solid (stone),
    /// `<= 0` = open (air, or water below sea level - resolved by the
    /// terrain stage).
    pub(crate) fn density(&self, x: i32, y: i32, z: i32) -> f64 {
        self.final_density.eval(x as f64, y as f64, z as f64)
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

    let fd = &settings["noise_router"]["final_density"];
    let final_density = parse(fd, &df_res, &noise_res as &NoiseResolver<'_>);

    let noise = &settings["noise"];
    let min_y = noise["min_y"].as_i64().unwrap_or(-64) as i32;
    let height = noise["height"].as_i64().unwrap_or(384) as i32;
    // sea_level lives at the top level (MC overworld = 63).
    let sea_level = settings["sea_level"].as_i64().unwrap_or(63) as i32;

    Overworld { final_density, sea_level, min_y, height }
}

#[cfg(test)]
mod tests {
    use super::*;

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
