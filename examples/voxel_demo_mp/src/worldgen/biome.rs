//! Biome enum + classic 3-biome map noise.
//!
//! The original 3 biomes (Desert / Plains / Mountain) drive the
//! `Classic` + `Amplified` generators. Phase E adds extended
//! biomes (Tundra / Taiga / Forest / Jungle / Swamp / Savanna /
//! SnowyMountain / Beach) used only by the `Realistic` profile;
//! the new variants are inert under `Classic` so legacy saves
//! still produce byte-identical terrain.

use super::seed_offset_for;

/// Biome tag. The first three variants are the legacy Classic
/// biomes and MUST stay first in the enum so `biome_at` keeps its
/// discriminants stable. The new variants are produced only by
/// the climate-driven `Realistic` generator (added in Phase E.2 /
/// E.3 - they're inert under Classic).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Biome {
    // Legacy (Classic / Amplified):
    Desert,
    Plains,
    Mountain,
    // Extended (Realistic):
    Tundra,
    Taiga,
    Forest,
    Jungle,
    Swamp,
    Savanna,
    SnowyMountain,
    Beach,
}

impl Biome {
    /// Short human label for chat overlays + debug printouts.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Biome::Desert => "Desert",
            Biome::Plains => "Plains",
            Biome::Mountain => "Mountain",
            Biome::Tundra => "Tundra",
            Biome::Taiga => "Taiga",
            Biome::Forest => "Forest",
            Biome::Jungle => "Jungle",
            Biome::Swamp => "Swamp",
            Biome::Savanna => "Savanna",
            Biome::SnowyMountain => "Snowy Mountain",
            Biome::Beach => "Beach",
        }
    }
}

/// Classic 3-biome map. Two layered sin/cos waves keep the biome
/// map smooth and deterministic. Range maps roughly to `[-1, 1]`
/// so the cutoffs are easy to reason about. A seed offset shifts
/// the wave so distinct hosts get different biome maps. Cutoffs
/// are tuned so Plains dominates (~70%) with Mountain + Desert as
/// accents - looks more like a starter world than a wall of
/// stone.
///
/// IMPORTANT: this fn drives `Classic` + `Amplified` and MUST
/// stay byte-identical to the pre-Phase-E baseline. Realistic
/// uses a separate `climate_at` path.
///
/// F.2c-1: the seed is now an explicit parameter instead of an
/// implicit read from the `WORLD_SEED` atomic. Callers thread
/// `ctx.effective_seed()` through.
pub(crate) fn biome_at(wx: f32, wz: f32, seed: u32) -> Biome {
    let s = seed_offset_for(seed);
    let n = ((wx + s) * 0.012).sin() * 0.6
        + ((wz + s) * 0.009).cos() * 0.5
        + (((wx + wz) + s) * 0.004).sin() * 0.3;
    if n < -0.95 {
        Biome::Desert
    } else if n > 0.95 {
        Biome::Mountain
    } else {
        Biome::Plains
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // W5.1b: `biome_at` takes seed as an explicit parameter; no
    // globals to lock or seed.

    #[test]
    fn biome_at_is_deterministic_for_seed() {
        let a = biome_at(123.5, 456.5, 42);
        let b = biome_at(123.5, 456.5, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn classic_biomes_cover_all_three_under_seed_0() {
        let mut seen = [false; 3];
        for x in -200..200 {
            for z in -200..200 {
                match biome_at(x as f32, z as f32, 0) {
                    Biome::Desert => seen[0] = true,
                    Biome::Plains => seen[1] = true,
                    Biome::Mountain => seen[2] = true,
                    _ => panic!("classic biome_at returned an extended biome"),
                }
            }
        }
        assert!(seen[0] && seen[1] && seen[2], "missing classic biome");
    }

    #[test]
    fn biome_label_is_non_empty_for_every_variant() {
        for b in [
            Biome::Desert,
            Biome::Plains,
            Biome::Mountain,
            Biome::Tundra,
            Biome::Taiga,
            Biome::Forest,
            Biome::Jungle,
            Biome::Swamp,
            Biome::Savanna,
            Biome::SnowyMountain,
            Biome::Beach,
        ] {
            assert!(!b.label().is_empty());
        }
    }
}
