//! Climate-to-biome lookup table for the `Realistic` generator.
//!
//! Pure decision tree over `(temperature, humidity, elevation)`.
//! Order matters: each check filters out a slice of the climate
//! space before falling through to the next. The table covers
//! eleven biomes (the legacy 3 plus 8 extended ones) so the
//! Realistic profile produces every shipped variant at some
//! (wx, wz) under any non-pathological seed.
//!
//! Tuning rules of thumb:
//! - Beach is a thin shoreline collar around sea-level cells.
//! - Mountain biomes win on high elevation regardless of climate.
//! - Cold + dry -> Tundra; cold + moist -> Taiga; cold + high ->
//!   SnowyMountain.
//! - Hot + dry -> Desert / Savanna depending on humidity slope.
//! - Hot + very moist -> Jungle.
//! - Temperate moist + low -> Swamp.
//! - Everything else falls back to Plains / Forest depending on
//!   humidity.

use super::biome::Biome;
use super::climate::Climate;

/// Decision tree mapping a Climate sample to a Biome. Total fn
/// over the full `[-1, 1]^3` cube; every input picks exactly one
/// biome.
pub(crate) fn biome_from_climate(c: Climate) -> Biome {
    // 1) Sea-floor / shoreline first: very low elevation always
    //    reads as Beach so coastlines look consistent across the
    //    climate space. (Below-sea cells become water in
    //    gen_realistic; Beach sits a hair above the waterline.)
    if c.elevation < -0.55 {
        return Biome::Beach;
    }
    // 2) Mountains take over at high elevation. Cold high becomes
    //    a snow cap; warm-but-high stays as bare-stone Mountain.
    if c.elevation > 0.5 {
        return if c.temperature < -0.1 {
            Biome::SnowyMountain
        } else {
            Biome::Mountain
        };
    }
    // 3) Cold climates branch on humidity. Frigid + dry = Tundra,
    //    frigid + moist = Taiga (conifer forest).
    if c.temperature < -0.35 {
        return if c.humidity < -0.05 {
            Biome::Tundra
        } else {
            Biome::Taiga
        };
    }
    // 4) Hot climates branch the other way. Bone-dry hot = Desert,
    //    bone-dry-but-not-quite = Savanna, very moist hot = Jungle.
    if c.temperature > 0.35 {
        if c.humidity < -0.3 {
            return Biome::Desert;
        }
        if c.humidity > 0.45 {
            return Biome::Jungle;
        }
        return Biome::Savanna;
    }
    // 5) Temperate band: very wet + low = Swamp; otherwise pick
    //    Forest if humid, Plains if dry.
    if c.humidity > 0.55 && c.elevation < 0.0 {
        return Biome::Swamp;
    }
    if c.humidity > 0.05 {
        Biome::Forest
    } else {
        Biome::Plains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn climate(t: f32, h: f32, e: f32) -> Climate {
        Climate {
            temperature: t,
            humidity: h,
            elevation: e,
        }
    }

    #[test]
    fn beach_at_deep_elevation() {
        assert_eq!(biome_from_climate(climate(0.0, 0.0, -0.8)), Biome::Beach);
        assert_eq!(biome_from_climate(climate(0.5, -0.5, -1.0)), Biome::Beach);
    }

    #[test]
    fn snowy_mountain_at_cold_peak() {
        assert_eq!(
            biome_from_climate(climate(-0.5, 0.0, 0.9)),
            Biome::SnowyMountain
        );
    }

    #[test]
    fn mountain_at_warm_peak() {
        assert_eq!(biome_from_climate(climate(0.4, 0.0, 0.9)), Biome::Mountain);
    }

    #[test]
    fn tundra_when_frigid_arid() {
        assert_eq!(
            biome_from_climate(climate(-0.8, -0.5, 0.0)),
            Biome::Tundra,
        );
    }

    #[test]
    fn taiga_when_frigid_moist() {
        assert_eq!(
            biome_from_climate(climate(-0.8, 0.5, 0.0)),
            Biome::Taiga,
        );
    }

    #[test]
    fn desert_when_hot_arid() {
        assert_eq!(
            biome_from_climate(climate(0.8, -0.7, 0.0)),
            Biome::Desert,
        );
    }

    #[test]
    fn jungle_when_hot_wet() {
        assert_eq!(
            biome_from_climate(climate(0.7, 0.8, 0.0)),
            Biome::Jungle,
        );
    }

    #[test]
    fn savanna_when_hot_mid() {
        assert_eq!(
            biome_from_climate(climate(0.7, 0.0, 0.0)),
            Biome::Savanna,
        );
    }

    #[test]
    fn swamp_when_temperate_wet_low() {
        assert_eq!(
            biome_from_climate(climate(0.0, 0.8, -0.3)),
            Biome::Swamp,
        );
    }

    #[test]
    fn forest_when_temperate_humid() {
        assert_eq!(
            biome_from_climate(climate(0.0, 0.3, 0.2)),
            Biome::Forest,
        );
    }

    #[test]
    fn plains_when_temperate_dry() {
        assert_eq!(
            biome_from_climate(climate(0.0, -0.3, 0.0)),
            Biome::Plains,
        );
    }

    #[test]
    fn lookup_is_total() {
        // Sweep the climate cube; every input must produce some
        // valid biome (no panic, no fall-through).
        for ti in -10..=10 {
            for hi in -10..=10 {
                for ei in -10..=10 {
                    let t = ti as f32 / 10.0;
                    let h = hi as f32 / 10.0;
                    let e = ei as f32 / 10.0;
                    let _ = biome_from_climate(climate(t, h, e));
                }
            }
        }
    }
}
