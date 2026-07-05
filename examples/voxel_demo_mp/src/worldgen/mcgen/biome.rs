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

/// W211: the REAL MC biome, carried through mcgen instead of collapsing
/// to the engine's 11 immediately. Only the variants the 5-channel
/// climate sample can distinguish (no depth channel -> no cave biomes).
/// The engine palette is derived at the edges via [`McBiome::to_engine`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum McBiome {
    // Oceans (OverworldBiomeBuilder.OCEANS rows: deep / shallow).
    DeepFrozenOcean,
    DeepColdOcean,
    DeepOcean,
    DeepLukewarmOcean,
    WarmOcean,
    FrozenOcean,
    ColdOcean,
    Ocean,
    LukewarmOcean,
    // Valleys.
    River,
    FrozenRiver,
    // Coast.
    Beach,
    SnowyBeach,
    // MIDDLE_BIOMES (+ the weirdness variants we can host).
    SnowyPlains,
    IceSpikes,
    SnowyTaiga,
    Taiga,
    OldGrowthSpruceTaiga,
    OldGrowthPineTaiga,
    Plains,
    SunflowerPlains,
    Forest,
    FlowerForest,
    BirchForest,
    OldGrowthBirchForest,
    DarkForest,
    Savanna,
    Jungle,
    SparseJungle,
    BambooJungle,
    Desert,
    Swamp,
    /// The hot erosion-6 wetland (temperatures 3-4). Hosted with swamp
    /// content until mangrove blocks + the mangrove placer exist.
    MangroveSwamp,
    // Mountain ridges (the slice the current classifier resolves).
    SnowySlopes,
    WindsweptHills,
}

impl McBiome {
    /// Every variant, for table-coverage tests.
    pub(crate) const ALL: [McBiome; 35] = [
        McBiome::DeepFrozenOcean,
        McBiome::DeepColdOcean,
        McBiome::DeepOcean,
        McBiome::DeepLukewarmOcean,
        McBiome::WarmOcean,
        McBiome::FrozenOcean,
        McBiome::ColdOcean,
        McBiome::Ocean,
        McBiome::LukewarmOcean,
        McBiome::River,
        McBiome::FrozenRiver,
        McBiome::Beach,
        McBiome::SnowyBeach,
        McBiome::SnowyPlains,
        McBiome::IceSpikes,
        McBiome::SnowyTaiga,
        McBiome::Taiga,
        McBiome::OldGrowthSpruceTaiga,
        McBiome::OldGrowthPineTaiga,
        McBiome::Plains,
        McBiome::SunflowerPlains,
        McBiome::Forest,
        McBiome::FlowerForest,
        McBiome::BirchForest,
        McBiome::OldGrowthBirchForest,
        McBiome::DarkForest,
        McBiome::Savanna,
        McBiome::Jungle,
        McBiome::SparseJungle,
        McBiome::BambooJungle,
        McBiome::Desert,
        McBiome::Swamp,
        McBiome::MangroveSwamp,
        McBiome::SnowySlopes,
        McBiome::WindsweptHills,
    ];

    /// The biome's MC registry name (biome JSON stem) - keys the
    /// temperature table, water colours, and diagnostics.
    pub(crate) fn name(self) -> &'static str {
        match self {
            McBiome::DeepFrozenOcean => "deep_frozen_ocean",
            McBiome::DeepColdOcean => "deep_cold_ocean",
            McBiome::DeepOcean => "deep_ocean",
            McBiome::DeepLukewarmOcean => "deep_lukewarm_ocean",
            McBiome::WarmOcean => "warm_ocean",
            McBiome::FrozenOcean => "frozen_ocean",
            McBiome::ColdOcean => "cold_ocean",
            McBiome::Ocean => "ocean",
            McBiome::LukewarmOcean => "lukewarm_ocean",
            McBiome::River => "river",
            McBiome::FrozenRiver => "frozen_river",
            McBiome::Beach => "beach",
            McBiome::SnowyBeach => "snowy_beach",
            McBiome::SnowyPlains => "snowy_plains",
            McBiome::IceSpikes => "ice_spikes",
            McBiome::SnowyTaiga => "snowy_taiga",
            McBiome::Taiga => "taiga",
            McBiome::OldGrowthSpruceTaiga => "old_growth_spruce_taiga",
            McBiome::OldGrowthPineTaiga => "old_growth_pine_taiga",
            McBiome::Plains => "plains",
            McBiome::SunflowerPlains => "sunflower_plains",
            McBiome::Forest => "forest",
            McBiome::FlowerForest => "flower_forest",
            McBiome::BirchForest => "birch_forest",
            McBiome::OldGrowthBirchForest => "old_growth_birch_forest",
            McBiome::DarkForest => "dark_forest",
            McBiome::Savanna => "savanna",
            McBiome::Jungle => "jungle",
            McBiome::SparseJungle => "sparse_jungle",
            McBiome::BambooJungle => "bamboo_jungle",
            McBiome::Desert => "desert",
            McBiome::Swamp => "swamp",
            McBiome::MangroveSwamp => "mangrove_swamp",
            McBiome::SnowySlopes => "snowy_slopes",
            McBiome::WindsweptHills => "windswept_hills",
        }
    }

    /// Collapse to the engine's 11-biome palette (HUD label, mob spawn
    /// pools, the legacy decor passes, surface-rule conditions).
    pub(crate) fn to_engine(self) -> Biome {
        match self {
            McBiome::DeepFrozenOcean
            | McBiome::DeepColdOcean
            | McBiome::DeepOcean
            | McBiome::DeepLukewarmOcean
            | McBiome::WarmOcean
            | McBiome::FrozenOcean
            | McBiome::ColdOcean
            | McBiome::Ocean
            | McBiome::LukewarmOcean
            | McBiome::River
            | McBiome::FrozenRiver
            | McBiome::Beach
            | McBiome::SnowyBeach => Biome::Beach,
            McBiome::SnowyPlains | McBiome::IceSpikes => Biome::Tundra,
            McBiome::SnowyTaiga
            | McBiome::Taiga
            | McBiome::OldGrowthSpruceTaiga
            | McBiome::OldGrowthPineTaiga => Biome::Taiga,
            McBiome::Plains | McBiome::SunflowerPlains => Biome::Plains,
            McBiome::Forest
            | McBiome::FlowerForest
            | McBiome::BirchForest
            | McBiome::OldGrowthBirchForest
            | McBiome::DarkForest => Biome::Forest,
            McBiome::Savanna => Biome::Savanna,
            McBiome::Jungle | McBiome::SparseJungle | McBiome::BambooJungle => Biome::Jungle,
            McBiome::Desert => Biome::Desert,
            McBiome::Swamp | McBiome::MangroveSwamp => Biome::Swamp,
            McBiome::SnowySlopes => Biome::SnowyMountain,
            McBiome::WindsweptHills => Biome::Mountain,
        }
    }
}

/// OverworldBiomeBuilder.OCEANS: [deep / shallow][temperature index].
const OCEANS: [[McBiome; 5]; 2] = [
    [
        McBiome::DeepFrozenOcean,
        McBiome::DeepColdOcean,
        McBiome::DeepOcean,
        McBiome::DeepLukewarmOcean,
        McBiome::WarmOcean,
    ],
    [
        McBiome::FrozenOcean,
        McBiome::ColdOcean,
        McBiome::Ocean,
        McBiome::LukewarmOcean,
        McBiome::WarmOcean,
    ],
];

/// OverworldBiomeBuilder.MIDDLE_BIOMES: [temperature][humidity].
const MIDDLE_BIOMES: [[McBiome; 5]; 5] = [
    [McBiome::SnowyPlains, McBiome::SnowyPlains, McBiome::SnowyPlains, McBiome::SnowyTaiga, McBiome::Taiga],
    [McBiome::Plains, McBiome::Plains, McBiome::Forest, McBiome::Taiga, McBiome::OldGrowthSpruceTaiga],
    [McBiome::FlowerForest, McBiome::Plains, McBiome::Forest, McBiome::BirchForest, McBiome::DarkForest],
    [McBiome::Savanna, McBiome::Savanna, McBiome::Forest, McBiome::Jungle, McBiome::Jungle],
    [McBiome::Desert, McBiome::Desert, McBiome::Desert, McBiome::Desert, McBiome::Desert],
];

/// OverworldBiomeBuilder.MIDDLE_BIOMES_VARIANT (weirdness > 0), None =
/// fall through to MIDDLE_BIOMES.
const MIDDLE_BIOMES_VARIANT: [[Option<McBiome>; 5]; 5] = [
    [Some(McBiome::IceSpikes), None, Some(McBiome::SnowyTaiga), None, None],
    [None, None, None, None, Some(McBiome::OldGrowthPineTaiga)],
    [Some(McBiome::SunflowerPlains), None, None, Some(McBiome::OldGrowthBirchForest), None],
    [None, None, Some(McBiome::Plains), Some(McBiome::SparseJungle), Some(McBiome::BambooJungle)],
    [None, None, None, None, None],
];

/// MIDDLE_BIOMES with the weirdness>0 variant overlay - MC's
/// `pickMiddleBiome`.
fn middle_biome(temp: usize, hum: usize, weirdness: f64) -> McBiome {
    if weirdness > 0.0
        && let Some(variant) = MIDDLE_BIOMES_VARIANT[temp][hum]
    {
        return variant;
    }
    MIDDLE_BIOMES[temp][hum]
}

/// W211: classify an MC climate point into the REAL MC biome, following
/// `OverworldBiomeBuilder`: continentalness gates (deep ocean / ocean /
/// coast), the valleys slice (`addValleys` - rivers, but erosion-6
/// inland valleys are the swamps and erosion-0/1 deep-inland valleys
/// are middle biomes), the mountain ridge slice, the erosion-6 wetland
/// band (swamp / mangrove swamp by temperature, exactly the source's
/// spans), then MIDDLE_BIOMES[temp][humidity] with the weirdness
/// variants. Unhosted in this slice pass: stony_shore on erosion-0/1
/// coast valleys, the badlands rows (collapse to desert), the
/// plateau/peak sub-biomes.
pub(crate) fn mc_biome_full(
    temperature: f64,
    vegetation: f64,
    continentalness: f64,
    erosion: f64,
    weirdness: f64,
) -> McBiome {
    let pv = peaks_valleys(weirdness);
    let temp = temp_level(temperature) as usize;
    let hum = hum_level(vegetation) as usize;
    // Ocean bands (OverworldBiomeBuilder: deepOcean (-1.05,-0.455),
    // ocean (-0.455,-0.19)).
    if continentalness < -0.455 {
        return OCEANS[0][temp];
    }
    if continentalness < -0.19 {
        return OCEANS[1][temp];
    }
    // Valleys slice (pv < -0.85), ported from `addValleys`.
    if pv < -0.85 {
        let river = if temp == 0 { McBiome::FrozenRiver } else { McBiome::River };
        if erosion > 0.55 {
            // Erosion 6: rivers survive only on the coast band; from
            // near-inland on the valley floor IS the wetland.
            if continentalness < -0.11 {
                return river;
            }
            return match temp {
                0 => McBiome::FrozenRiver,
                1 | 2 => McBiome::Swamp,
                _ => McBiome::MangroveSwamp,
            };
        }
        if erosion < -0.375 && continentalness >= 0.03 {
            // Erosion 0-1 deep inland: the valley floor rises into
            // middle biomes (badlands-if-hot collapses onto desert).
            return middle_biome(temp, hum, weirdness);
        }
        return river;
    }
    // Coast.
    if continentalness < -0.11 {
        return if temp == 0 { McBiome::SnowyBeach } else { McBiome::Beach };
    }
    // Mountain ridges: high peaks-and-valleys with low erosion.
    if pv > 0.55 && erosion < 0.05 {
        return if temp <= 1 { McBiome::SnowySlopes } else { McBiome::WindsweptHills };
    }
    // The erosion-6 wetland band (low + mid slices, pv < 0.2): swamp for
    // temperatures 1-2, mangrove swamp for 3-4 (the exact source spans;
    // temperature 0 falls through to the snowy middle row).
    if erosion > 0.55 && pv < 0.2 && (1..=4).contains(&temp) {
        return if temp <= 2 { McBiome::Swamp } else { McBiome::MangroveSwamp };
    }
    middle_biome(temp, hum, weirdness)
}

/// Engine-palette classification (the pre-W211 entry point): the full
/// classification collapsed to the legacy 11.
pub(crate) fn mc_biome(temperature: f64, vegetation: f64, continentalness: f64, erosion: f64, pv: f64) -> Biome {
    // Recover a representative weirdness for the fold the caller made:
    // sign information is lost, but the variant table only needs w > 0,
    // and pv is what the bands key on. Callers that have the raw
    // weirdness should use `mc_biome_full` directly.
    mc_biome_full(temperature, vegetation, continentalness, erosion, pv_to_weirdness(pv)).to_engine()
}

/// Invert peaks_valleys approximately: pick the positive-branch
/// weirdness that folds to `pv` (variant tables treat w>0 as variant;
/// using the positive branch keeps legacy callers' behaviour stable).
fn pv_to_weirdness(pv: f64) -> f64 {
    // peaks_valleys(w) for w in [0, 2/3] is monotone: -(|w - 2/3| - 1/3)*3
    // = (1/3 - |w-2/3|)*3. Solve on the w<2/3 branch: pv = (w - 1/3)*3.
    pv / 3.0 + 1.0 / 3.0
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
    fn jungle_when_warm_humid_inland() {
        // MC's MIDDLE_BIOMES puts jungles in temperature band 3
        // (0.2..0.55); band 4 is all desert.
        assert_eq!(mc_biome(0.4, 0.6, 0.4, 0.3, 0.0), Biome::Jungle);
        assert_eq!(mc_biome(0.8, 0.6, 0.4, 0.3, 0.0), Biome::Desert);
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

    /// W211: the full (pre-collapse) classification pins - ocean
    /// variants, rivers, the valleys sub-rules, forest variants.
    #[test]
    fn w211_full_classification_pins() {
        use McBiome as B;
        // Ocean temperature bands, deep (c < -0.455) vs shallow.
        assert_eq!(mc_biome_full(-0.6, 0.0, -0.6, 0.0, 0.0), B::DeepFrozenOcean);
        assert_eq!(mc_biome_full(-0.3, 0.0, -0.6, 0.0, 0.0), B::DeepColdOcean);
        assert_eq!(mc_biome_full(0.3, 0.0, -0.3, 0.0, 0.0), B::LukewarmOcean);
        assert_eq!(mc_biome_full(0.8, 0.0, -0.3, 0.0, 0.0), B::WarmOcean);
        // weirdness 0 folds to pv -1: the valleys slice. Mid erosion ->
        // rivers (frozen in temperature band 0).
        assert_eq!(mc_biome_full(0.0, 0.0, 0.2, 0.0, 0.0), B::River);
        assert_eq!(mc_biome_full(-0.6, 0.0, 0.2, 0.0, 0.0), B::FrozenRiver);
        // Erosion-6 inland valleys are the wetlands, not rivers; swamp
        // for temperature 1-2, mangrove for 3-4 (the source spans).
        assert_eq!(mc_biome_full(0.0, 0.0, 0.2, 0.7, 0.0), B::Swamp);
        assert_eq!(mc_biome_full(0.4, 0.0, 0.2, 0.7, 0.0), B::MangroveSwamp);
        // ...but rivers survive on the erosion-6 coast band.
        assert_eq!(mc_biome_full(0.0, 0.0, -0.15, 0.7, 0.0), B::River);
        // Erosion-0/1 deep-inland valleys rise into middle biomes.
        assert_eq!(mc_biome_full(0.0, 0.0, 0.5, -0.5, 0.0), B::Forest);
        // Forest variants: MIDDLE_BIOMES[2][0] + the weirdness overlay.
        assert_eq!(mc_biome_full(0.0, -0.5, 0.4, 0.3, -0.3), B::FlowerForest);
        assert_eq!(mc_biome_full(0.0, -0.5, 0.4, 0.3, 0.3), B::SunflowerPlains);
        assert_eq!(mc_biome_full(0.0, 0.25, 0.4, 0.3, -0.3), B::BirchForest);
        assert_eq!(mc_biome_full(0.0, 0.25, 0.4, 0.3, 0.3), B::OldGrowthBirchForest);
        // Coast.
        assert_eq!(mc_biome_full(0.0, 0.0, -0.15, 0.0, -0.3), B::Beach);
        assert_eq!(mc_biome_full(-0.6, 0.0, -0.15, 0.0, -0.3), B::SnowyBeach);
    }
}
