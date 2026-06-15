//! W185 - faithful MC worldgen, WAVE 4: overworld biome classification.
//!
//! MC 1.18 picks biomes from a 6-parameter climate point - temperature,
//! humidity (vegetation), continentalness, erosion, depth, weirdness -
//! sampled from real noise and matched against `OverworldBiomeBuilder`'s
//! parameter hypercubes (66 biomes). This module reproduces the LOOK by
//! sampling MC's real climate noise FIELD (see `Overworld::biome_at`)
//! and classifying it with MC's actual parameter band cutoffs.
//!
//! Honest scope: the climate field is faithful (the same continentalness
//! / erosion / temperature / vegetation / weirdness noises MC uses, with
//! the same domain warp), so biome REGIONS land where MC would put them.
//! The mapping then collapses MC's 66 biomes onto the engine's 11-biome
//! palette - that collapse is lossy, but the placement logic and region
//! shapes are MC's. Depth is omitted (it separates surface vs cave
//! biomes; the engine only needs the surface set).

use crate::worldgen::biome::Biome;

/// MC `TerrainShaper.peaksAndValleys(weirdness)`: folds the weirdness
/// noise into a peaks-and-valleys parameter (ridges -> peaks, the flat
/// stretches between -> valleys).
pub(crate) fn peaks_valleys(w: f64) -> f64 {
    -((w.abs() - 0.666_666_7).abs() - 0.333_333_34) * 3.0
}

/// Temperature band, 0 (frozen) .. 4 (hot). MC `Climate` temperature
/// parameter cutoffs.
fn temp_level(t: f64) -> u8 {
    match t {
        x if x < -0.45 => 0,
        x if x < -0.15 => 1,
        x if x < 0.2 => 2,
        x if x < 0.55 => 3,
        _ => 4,
    }
}

/// Humidity (vegetation) band, 0 (arid) .. 4 (humid). MC cutoffs.
fn hum_level(v: f64) -> u8 {
    match v {
        x if x < -0.35 => 0,
        x if x < -0.1 => 1,
        x if x < 0.1 => 2,
        x if x < 0.3 => 3,
        _ => 4,
    }
}

/// Classify an MC climate point into an engine biome, following the
/// structure of `OverworldBiomeBuilder`: a continentalness gate (ocean /
/// shore) first, then peaks-and-valleys + erosion for mountains, then a
/// temperature-major land table refined by humidity.
pub(crate) fn mc_biome(temperature: f64, vegetation: f64, continentalness: f64, erosion: f64, pv: f64) -> Biome {
    // Continentalness below 'coast' (~-0.11) is ocean / shoreline. The
    // engine has no ocean biome; a sandy seabed (Beach) reads right once
    // the terrain pass fills the column with water.
    if continentalness < -0.11 {
        return Biome::Beach;
    }
    let temp = temp_level(temperature);
    let hum = hum_level(vegetation);
    // Mountains: high peaks-and-valleys with low erosion (jagged /
    // windswept ridges). Higher erosion flattens the terrain, so it
    // stops reading as a mountain biome.
    if pv > 0.55 && erosion < 0.05 {
        return if temp <= 1 { Biome::SnowyMountain } else { Biome::Mountain };
    }
    // Temperature-major land table, refined by humidity (and weirdness
    // for the temperate swamp).
    match temp {
        0 => {
            // Frozen: snowy taiga where vegetated, else snowy plains.
            if hum >= 3 { Biome::Taiga } else { Biome::Tundra }
        }
        1 => Biome::Taiga, // cold -> conifer forest
        2 => {
            // Temperate: wet low ground -> swamp; humid -> forest; else plains.
            if hum >= 4 && pv < -0.6 {
                Biome::Swamp
            } else if hum >= 2 {
                Biome::Forest
            } else {
                Biome::Plains
            }
        }
        3 => {
            // Warm: humid -> forest, dry -> savanna, mid -> plains.
            if hum >= 3 {
                Biome::Forest
            } else if hum <= 1 {
                Biome::Savanna
            } else {
                Biome::Plains
            }
        }
        _ => {
            // Hot: arid -> desert, very humid -> jungle, else savanna.
            if hum <= 1 {
                Biome::Desert
            } else if hum >= 4 {
                Biome::Jungle
            } else {
                Biome::Savanna
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_valleys_fold() {
        // Ridge centre (|weirdness| ~ 0.6667) folds to a PEAK (~ +1);
        // the flat stretch (weirdness ~ 0) folds to a VALLEY (~ -1).
        assert!(peaks_valleys(0.6667) > 0.9);
        assert!(peaks_valleys(0.0) < -0.9);
    }

    #[test]
    fn ocean_when_continental_low() {
        assert_eq!(mc_biome(0.0, 0.0, -0.5, 0.0, 0.0), Biome::Beach);
    }

    #[test]
    fn desert_when_hot_arid_inland() {
        assert_eq!(mc_biome(0.8, -0.6, 0.4, 0.3, 0.0), Biome::Desert);
    }

    #[test]
    fn jungle_when_hot_humid_inland() {
        assert_eq!(mc_biome(0.8, 0.6, 0.4, 0.3, 0.0), Biome::Jungle);
    }

    #[test]
    fn tundra_when_frozen_arid() {
        assert_eq!(mc_biome(-0.8, -0.5, 0.4, 0.3, 0.0), Biome::Tundra);
    }

    #[test]
    fn mountain_on_jagged_peak() {
        // High PV + low erosion + warm -> Mountain; cold -> SnowyMountain.
        assert_eq!(mc_biome(0.3, 0.0, 0.4, -0.5, 0.9), Biome::Mountain);
        assert_eq!(mc_biome(-0.5, 0.0, 0.4, -0.5, 0.9), Biome::SnowyMountain);
    }

    #[test]
    fn plains_when_temperate_dry() {
        assert_eq!(mc_biome(0.0, -0.2, 0.4, 0.3, 0.0), Biome::Plains);
    }
}
