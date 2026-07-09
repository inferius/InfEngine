//! W185 - faithful MC worldgen, WAVE 4: overworld biome classification.
//!
//! MC 1.18 picks biomes from a 6-parameter climate point - temperature,
//! humidity (vegetation), continentalness, erosion, depth, weirdness -
//! sampled from real noise and matched against `OverworldBiomeBuilder`'s
//! parameter hypercubes (66 biomes). This module reproduces the LOOK by
//! sampling MC's real climate noise FIELD (see `Overworld::biome_at`)
//! and classifying it with MC's actual parameter band cutoffs.
//!
//! W227: the FULL inland slice decomposition from the source -
//! `addValleys` / `addLowSlice` / `addMidSlice` / `addHighSlice` /
//! `addPeaks` keyed on the peaks-and-valleys fold of weirdness, with
//! the PLATEAU / SHATTERED tables and the pick helpers (peak / slope /
//! badlands / windswept-savanna / stony-shore / beach). The source adds
//! non-overlapping hypercubes per slice, so a direct decision tree over
//! the band indices is equivalent to MC's nearest-point lookup.
//!
//! Honest scope: the climate field is faithful (the same noises with
//! the same domain warp), so biome REGIONS land where MC would put
//! them. Depth is omitted (no cave biomes). Unhosted content gaps:
//! pale_garden (no pale-oak blocks - hosts dark-forest-adjacent
//! content), mushroom_fields (no huge-mushroom blocks yet).

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

/// Erosion band, 0..6. MC `Climate` erosion parameter cutoffs.
fn erosion_level(e: f64) -> u8 {
    match e {
        x if x < -0.78 => 0,
        x if x < -0.375 => 1,
        x if x < -0.2225 => 2,
        x if x < 0.05 => 3,
        x if x < 0.45 => 4,
        x if x < 0.55 => 5,
        _ => 6,
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
    /// W227: the erosion-0..2 low/mid-slice coast.
    StonyShore,
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
    // W227: PLATEAU_BIOMES rows.
    Meadow,
    CherryGrove,
    /// No pale-oak blocks yet - hosted with dark-forest-like content.
    PaleGarden,
    SavannaPlateau,
    Badlands,
    WoodedBadlands,
    ErodedBadlands,
    // W227: SHATTERED_BIOMES rows + the windswept savanna pick.
    WindsweptGravellyHills,
    WindsweptHills,
    WindsweptForest,
    WindsweptSavanna,
    // W227: peak / slope picks.
    JaggedPeaks,
    FrozenPeaks,
    StonyPeaks,
    SnowySlopes,
    Grove,
    /// W227: continentalness < -1.05 island. No huge-mushroom blocks
    /// yet - mycelium surface + normal mushroom patches only.
    MushroomFields,
}

impl McBiome {
    /// Every variant, for table-coverage tests.
    pub(crate) const ALL: [McBiome; 51] = [
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
        McBiome::StonyShore,
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
        McBiome::Meadow,
        McBiome::CherryGrove,
        McBiome::PaleGarden,
        McBiome::SavannaPlateau,
        McBiome::Badlands,
        McBiome::WoodedBadlands,
        McBiome::ErodedBadlands,
        McBiome::WindsweptGravellyHills,
        McBiome::WindsweptHills,
        McBiome::WindsweptForest,
        McBiome::WindsweptSavanna,
        McBiome::JaggedPeaks,
        McBiome::FrozenPeaks,
        McBiome::StonyPeaks,
        McBiome::SnowySlopes,
        McBiome::Grove,
        McBiome::MushroomFields,
    ];

    /// The biome's MC registry name (biome JSON stem) - keys the
    /// temperature table, water colours, surface-rule biome conditions
    /// and diagnostics.
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
            McBiome::StonyShore => "stony_shore",
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
            McBiome::Meadow => "meadow",
            McBiome::CherryGrove => "cherry_grove",
            McBiome::PaleGarden => "pale_garden",
            McBiome::SavannaPlateau => "savanna_plateau",
            McBiome::Badlands => "badlands",
            McBiome::WoodedBadlands => "wooded_badlands",
            McBiome::ErodedBadlands => "eroded_badlands",
            McBiome::WindsweptGravellyHills => "windswept_gravelly_hills",
            McBiome::WindsweptHills => "windswept_hills",
            McBiome::WindsweptForest => "windswept_forest",
            McBiome::WindsweptSavanna => "windswept_savanna",
            McBiome::JaggedPeaks => "jagged_peaks",
            McBiome::FrozenPeaks => "frozen_peaks",
            McBiome::StonyPeaks => "stony_peaks",
            McBiome::SnowySlopes => "snowy_slopes",
            McBiome::Grove => "grove",
            McBiome::MushroomFields => "mushroom_fields",
        }
    }

    /// Collapse to the engine's 11-biome palette (HUD label, mob spawn
    /// pools, the legacy decor passes, surface-rule fallbacks).
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
            | McBiome::SnowyBeach
            | McBiome::StonyShore => Biome::Beach,
            McBiome::SnowyPlains | McBiome::IceSpikes => Biome::Tundra,
            McBiome::SnowyTaiga
            | McBiome::Taiga
            | McBiome::OldGrowthSpruceTaiga
            | McBiome::OldGrowthPineTaiga => Biome::Taiga,
            McBiome::Plains
            | McBiome::SunflowerPlains
            | McBiome::Meadow
            | McBiome::MushroomFields => Biome::Plains,
            McBiome::Forest
            | McBiome::FlowerForest
            | McBiome::BirchForest
            | McBiome::OldGrowthBirchForest
            | McBiome::DarkForest
            | McBiome::CherryGrove
            | McBiome::PaleGarden => Biome::Forest,
            McBiome::Savanna | McBiome::SavannaPlateau | McBiome::WindsweptSavanna => {
                Biome::Savanna
            }
            McBiome::Jungle | McBiome::SparseJungle | McBiome::BambooJungle => Biome::Jungle,
            McBiome::Desert
            | McBiome::Badlands
            | McBiome::WoodedBadlands
            | McBiome::ErodedBadlands => Biome::Desert,
            McBiome::Swamp | McBiome::MangroveSwamp => Biome::Swamp,
            McBiome::SnowySlopes
            | McBiome::Grove
            | McBiome::JaggedPeaks
            | McBiome::FrozenPeaks => Biome::SnowyMountain,
            McBiome::WindsweptHills
            | McBiome::WindsweptGravellyHills
            | McBiome::WindsweptForest
            | McBiome::StonyPeaks => Biome::Mountain,
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

/// W227 OverworldBiomeBuilder.PLATEAU_BIOMES: [temperature][humidity].
const PLATEAU_BIOMES: [[McBiome; 5]; 5] = [
    [McBiome::SnowyPlains, McBiome::SnowyPlains, McBiome::SnowyPlains, McBiome::SnowyTaiga, McBiome::SnowyTaiga],
    [McBiome::Meadow, McBiome::Meadow, McBiome::Forest, McBiome::Taiga, McBiome::OldGrowthSpruceTaiga],
    [McBiome::Meadow, McBiome::Meadow, McBiome::Meadow, McBiome::Meadow, McBiome::PaleGarden],
    [McBiome::SavannaPlateau, McBiome::SavannaPlateau, McBiome::Forest, McBiome::Forest, McBiome::Jungle],
    [McBiome::Badlands, McBiome::Badlands, McBiome::Badlands, McBiome::WoodedBadlands, McBiome::WoodedBadlands],
];

/// W227 OverworldBiomeBuilder.PLATEAU_BIOMES_VARIANT (weirdness > 0).
const PLATEAU_BIOMES_VARIANT: [[Option<McBiome>; 5]; 5] = [
    [Some(McBiome::IceSpikes), None, None, None, None],
    [Some(McBiome::CherryGrove), None, Some(McBiome::Meadow), Some(McBiome::Meadow), Some(McBiome::OldGrowthPineTaiga)],
    [Some(McBiome::CherryGrove), Some(McBiome::CherryGrove), Some(McBiome::Forest), Some(McBiome::BirchForest), None],
    [None, None, None, None, None],
    [Some(McBiome::ErodedBadlands), Some(McBiome::ErodedBadlands), None, None, None],
];

/// W227 OverworldBiomeBuilder.SHATTERED_BIOMES (erosion-5 windswept
/// band), None = fall through to the middle biome.
const SHATTERED_BIOMES: [[Option<McBiome>; 5]; 5] = [
    [Some(McBiome::WindsweptGravellyHills), Some(McBiome::WindsweptGravellyHills), Some(McBiome::WindsweptHills), Some(McBiome::WindsweptForest), Some(McBiome::WindsweptForest)],
    [Some(McBiome::WindsweptGravellyHills), Some(McBiome::WindsweptGravellyHills), Some(McBiome::WindsweptHills), Some(McBiome::WindsweptForest), Some(McBiome::WindsweptForest)],
    [Some(McBiome::WindsweptHills), Some(McBiome::WindsweptHills), Some(McBiome::WindsweptHills), Some(McBiome::WindsweptForest), Some(McBiome::WindsweptForest)],
    [None, None, None, None, None],
    [None, None, None, None, None],
];

// ------------------------------------------------------------------
// The pick helpers, 1:1 with the source. `wpos` = MC's
// `weirdness.max() >= 0` for the slice span the point falls into
// (variant tables apply on the positive-weirdness side).
// ------------------------------------------------------------------

/// MC `pickMiddleBiome`.
fn middle_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if wpos && let Some(variant) = MIDDLE_BIOMES_VARIANT[temp][hum] {
        return variant;
    }
    MIDDLE_BIOMES[temp][hum]
}

/// MC `pickBadlandsBiome`.
fn badlands_biome(hum: usize, wpos: bool) -> McBiome {
    if hum < 2 {
        if wpos { McBiome::ErodedBadlands } else { McBiome::Badlands }
    } else if hum < 3 {
        McBiome::Badlands
    } else {
        McBiome::WoodedBadlands
    }
}

/// MC `pickMiddleBiomeOrBadlandsIfHot`.
fn middle_or_badlands_if_hot(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if temp == 4 { badlands_biome(hum, wpos) } else { middle_biome(temp, hum, wpos) }
}

/// MC `pickPlateauBiome`.
fn plateau_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if wpos && let Some(variant) = PLATEAU_BIOMES_VARIANT[temp][hum] {
        return variant;
    }
    PLATEAU_BIOMES[temp][hum]
}

/// MC `pickSlopeBiome`.
fn slope_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if temp >= 3 {
        plateau_biome(temp, hum, wpos)
    } else if hum <= 1 {
        McBiome::SnowySlopes
    } else {
        McBiome::Grove
    }
}

/// MC `pickMiddleBiomeOrBadlandsIfHotOrSlopeIfCold`.
fn middle_or_badlands_if_hot_or_slope_if_cold(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if temp == 0 {
        slope_biome(temp, hum, wpos)
    } else {
        middle_or_badlands_if_hot(temp, hum, wpos)
    }
}

/// MC `pickPeakBiome`.
fn peak_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    if temp <= 2 {
        if wpos { McBiome::FrozenPeaks } else { McBiome::JaggedPeaks }
    } else if temp == 3 {
        McBiome::StonyPeaks
    } else {
        badlands_biome(hum, wpos)
    }
}

/// MC `pickShatteredBiome`.
fn shattered_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    SHATTERED_BIOMES[temp][hum].unwrap_or_else(|| middle_biome(temp, hum, wpos))
}

/// MC `maybePickWindsweptSavannaBiome`.
fn maybe_windswept_savanna(temp: usize, hum: usize, wpos: bool, under: McBiome) -> McBiome {
    if temp > 1 && hum < 4 && wpos { McBiome::WindsweptSavanna } else { under }
}

/// MC `pickBeachBiome`.
fn beach_biome(temp: usize) -> McBiome {
    match temp {
        0 => McBiome::SnowyBeach,
        4 => McBiome::Desert,
        _ => McBiome::Beach,
    }
}

/// MC `pickShatteredCoastBiome`.
fn shattered_coast_biome(temp: usize, hum: usize, wpos: bool) -> McBiome {
    let under = if wpos { middle_biome(temp, hum, wpos) } else { beach_biome(temp) };
    maybe_windswept_savanna(temp, hum, wpos, under)
}

// ------------------------------------------------------------------
// The inland slices. Continentalness bands past the coast check:
// coast [-0.19,-0.11), near [-0.11,0.03), mid [0.03,0.3), far [0.3,..].
// ------------------------------------------------------------------

/// MC `addValleys` (the single [-0.05, 0.05] weirdness span - its max
/// is >= 0, so the variant side of every pick applies).
fn valleys(temp: usize, hum: usize, c: f64, er: u8) -> McBiome {
    let river = if temp == 0 { McBiome::FrozenRiver } else { McBiome::River };
    match er {
        0 | 1 => {
            if c < 0.03 {
                river
            } else {
                // Mid/far-inland valley floors rise into middle biomes
                // (badlands in the hot row).
                middle_or_badlands_if_hot(temp, hum, true)
            }
        }
        2..=5 => river,
        _ => {
            // Erosion 6: rivers survive only on the coast band; inland
            // the valley floor IS the wetland (frozen row stays river).
            if c < -0.11 {
                river
            } else {
                match temp {
                    0 => McBiome::FrozenRiver,
                    1 | 2 => McBiome::Swamp,
                    _ => McBiome::MangroveSwamp,
                }
            }
        }
    }
}

/// MC `addLowSlice`.
fn low_slice(temp: usize, hum: usize, c: f64, er: u8, wpos: bool) -> McBiome {
    if c < -0.11 {
        // Coast column.
        return match er {
            0..=2 => McBiome::StonyShore,
            3 | 4 => beach_biome(temp),
            5 => shattered_coast_biome(temp, hum, wpos),
            _ => beach_biome(temp),
        };
    }
    if er == 6 {
        return match temp {
            0 => middle_biome(temp, hum, wpos),
            1 | 2 => McBiome::Swamp,
            _ => McBiome::MangroveSwamp,
        };
    }
    if c < 0.03 {
        // Near inland.
        match er {
            0 | 1 => middle_or_badlands_if_hot(temp, hum, wpos),
            2..=4 => middle_biome(temp, hum, wpos),
            _ => maybe_windswept_savanna(temp, hum, wpos, middle_biome(temp, hum, wpos)),
        }
    } else {
        // Mid + far inland.
        match er {
            0 | 1 => middle_or_badlands_if_hot_or_slope_if_cold(temp, hum, wpos),
            2 | 3 => middle_or_badlands_if_hot(temp, hum, wpos),
            _ => middle_biome(temp, hum, wpos),
        }
    }
}

/// MC `addMidSlice`.
fn mid_slice(temp: usize, hum: usize, c: f64, er: u8, wpos: bool) -> McBiome {
    if c < -0.11 {
        // Coast column.
        return match er {
            0..=2 => McBiome::StonyShore,
            3 => middle_biome(temp, hum, wpos),
            4 => {
                if wpos { middle_biome(temp, hum, wpos) } else { beach_biome(temp) }
            }
            5 => shattered_coast_biome(temp, hum, wpos),
            _ => {
                if wpos { middle_biome(temp, hum, wpos) } else { beach_biome(temp) }
            }
        };
    }
    if er == 6 {
        return match temp {
            0 => middle_biome(temp, hum, wpos),
            1 | 2 => McBiome::Swamp,
            _ => McBiome::MangroveSwamp,
        };
    }
    if c < 0.03 {
        // Near inland.
        match er {
            0 => slope_biome(temp, hum, wpos),
            1 => middle_or_badlands_if_hot_or_slope_if_cold(temp, hum, wpos),
            2..=4 => middle_biome(temp, hum, wpos),
            _ => maybe_windswept_savanna(temp, hum, wpos, middle_biome(temp, hum, wpos)),
        }
    } else if c < 0.3 {
        // Mid inland.
        match er {
            0 => slope_biome(temp, hum, wpos),
            1 => middle_or_badlands_if_hot_or_slope_if_cold(temp, hum, wpos),
            2 | 3 => middle_or_badlands_if_hot(temp, hum, wpos),
            4 => middle_biome(temp, hum, wpos),
            _ => shattered_biome(temp, hum, wpos),
        }
    } else {
        // Far inland.
        match er {
            0 => slope_biome(temp, hum, wpos),
            1 => {
                if temp == 0 {
                    slope_biome(temp, hum, wpos)
                } else {
                    plateau_biome(temp, hum, wpos)
                }
            }
            2 => plateau_biome(temp, hum, wpos),
            3 => middle_or_badlands_if_hot(temp, hum, wpos),
            4 => middle_biome(temp, hum, wpos),
            _ => shattered_biome(temp, hum, wpos),
        }
    }
}

/// MC `addHighSlice`.
fn high_slice(temp: usize, hum: usize, c: f64, er: u8, wpos: bool) -> McBiome {
    if c < -0.11 {
        // Coast column: middle everywhere except the shattered band.
        return match er {
            5 => maybe_windswept_savanna(temp, hum, wpos, middle_biome(temp, hum, wpos)),
            _ => middle_biome(temp, hum, wpos),
        };
    }
    if c < 0.03 {
        // Near inland.
        match er {
            0 => slope_biome(temp, hum, wpos),
            1 => middle_or_badlands_if_hot_or_slope_if_cold(temp, hum, wpos),
            2..=4 => middle_biome(temp, hum, wpos),
            5 => maybe_windswept_savanna(temp, hum, wpos, middle_biome(temp, hum, wpos)),
            _ => middle_biome(temp, hum, wpos),
        }
    } else if c < 0.3 {
        // Mid inland.
        match er {
            0 => peak_biome(temp, hum, wpos),
            1 => slope_biome(temp, hum, wpos),
            2 => plateau_biome(temp, hum, wpos),
            3 => middle_or_badlands_if_hot(temp, hum, wpos),
            4 => middle_biome(temp, hum, wpos),
            5 => shattered_biome(temp, hum, wpos),
            _ => middle_biome(temp, hum, wpos),
        }
    } else {
        // Far inland.
        match er {
            0 => peak_biome(temp, hum, wpos),
            1 => slope_biome(temp, hum, wpos),
            2 | 3 => plateau_biome(temp, hum, wpos),
            4 => middle_biome(temp, hum, wpos),
            5 => shattered_biome(temp, hum, wpos),
            _ => middle_biome(temp, hum, wpos),
        }
    }
}

/// MC `addPeaks`.
fn peaks(temp: usize, hum: usize, c: f64, er: u8, wpos: bool) -> McBiome {
    match er {
        0 => peak_biome(temp, hum, wpos),
        1 => {
            if c < 0.03 {
                middle_or_badlands_if_hot_or_slope_if_cold(temp, hum, wpos)
            } else {
                peak_biome(temp, hum, wpos)
            }
        }
        2 => {
            if c < 0.03 {
                middle_biome(temp, hum, wpos)
            } else {
                plateau_biome(temp, hum, wpos)
            }
        }
        3 => {
            if c < 0.03 {
                middle_biome(temp, hum, wpos)
            } else if c < 0.3 {
                middle_or_badlands_if_hot(temp, hum, wpos)
            } else {
                plateau_biome(temp, hum, wpos)
            }
        }
        4 => middle_biome(temp, hum, wpos),
        5 => {
            if c < 0.03 {
                maybe_windswept_savanna(temp, hum, wpos, shattered_biome(temp, hum, wpos))
            } else {
                shattered_biome(temp, hum, wpos)
            }
        }
        _ => middle_biome(temp, hum, wpos),
    }
}

/// W211/W227: classify an MC climate point into the REAL MC biome,
/// following `OverworldBiomeBuilder`: the off-coast continentalness
/// gates (mushroom fields / deep ocean / ocean), then the weirdness
/// slice (valley / low / mid / high / peak via the peaks-and-valleys
/// fold, the exact `getDebugStringForPeaksAndValleys` cutoffs) and the
/// slice's per-(continentalness x erosion) rules.
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
    let er = erosion_level(erosion);
    // Off-coast bands (OverworldBiomeBuilder: mushroom (-1.2,-1.05),
    // deepOcean (-1.05,-0.455), ocean (-0.455,-0.19)).
    if continentalness < -1.05 {
        return McBiome::MushroomFields;
    }
    if continentalness < -0.455 {
        return OCEANS[0][temp];
    }
    if continentalness < -0.19 {
        return OCEANS[1][temp];
    }
    // The variant side: MC keys picks on the slice span's max >= 0; for
    // a point that is simply its weirdness sign (the valley slice spans
    // zero and always picks variants).
    let wpos = weirdness >= 0.0;
    // Slice cutoffs = peaks_valleys(0.05 / 0.26666668 / 0.4 / 0.56666666).
    if pv < -0.85 {
        valleys(temp, hum, continentalness, er)
    } else if pv < -0.2 {
        low_slice(temp, hum, continentalness, er, wpos)
    } else if pv < 0.2 {
        mid_slice(temp, hum, continentalness, er, wpos)
    } else if pv < 0.7 {
        high_slice(temp, hum, continentalness, er, wpos)
    } else {
        peaks(temp, hum, continentalness, er, wpos)
    }
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
        // Peak slice, far inland, erosion 1: warm band 3 -> stony peaks
        // (Mountain); frozen -> frozen peaks (SnowyMountain; the legacy
        // pv fold recovers a positive weirdness).
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
        // Forest variants: MIDDLE_BIOMES[2][0] + the weirdness overlay
        // (low slice, near inland, erosion 4).
        assert_eq!(mc_biome_full(0.0, -0.5, 0.0, 0.3, -0.3), B::FlowerForest);
        assert_eq!(mc_biome_full(0.0, -0.5, 0.0, 0.3, 0.3), B::SunflowerPlains);
        assert_eq!(mc_biome_full(0.0, 0.25, 0.0, 0.3, -0.3), B::BirchForest);
        assert_eq!(mc_biome_full(0.0, 0.25, 0.0, 0.3, 0.3), B::OldGrowthBirchForest);
        // Coast: the LOW slice (pv -0.55 at |w| 0.15) keeps beaches on
        // erosion 3-4.
        assert_eq!(mc_biome_full(0.0, 0.0, -0.15, 0.0, -0.15), B::Beach);
        assert_eq!(mc_biome_full(-0.6, 0.0, -0.15, 0.0, -0.15), B::SnowyBeach);
    }

    /// W227: pins for the new slice decomposition - peaks, slopes,
    /// plateaus, shattered, badlands, stony shore, mushroom fields.
    #[test]
    fn w227_slice_pins() {
        use McBiome as B;
        // |w|=0.6667 folds to pv 1.0 = the peak slice; far inland e0.
        assert_eq!(mc_biome_full(0.0, 0.0, 0.5, -0.9, -0.6667), B::JaggedPeaks);
        // Positive-weirdness side of the same point freezes the peak.
        assert_eq!(mc_biome_full(0.0, 0.0, 0.5, -0.9, 0.6667), B::FrozenPeaks);
        // Warm band 3 peaks are stony, hot band 4 peaks are badlands
        // (arid rows erode on the positive-weirdness side, humid rows
        // are wooded - pickBadlandsBiome).
        assert_eq!(mc_biome_full(0.4, 0.0, 0.5, -0.9, -0.6667), B::StonyPeaks);
        assert_eq!(mc_biome_full(0.8, 0.0, 0.5, -0.9, -0.6667), B::Badlands);
        assert_eq!(mc_biome_full(0.8, -0.5, 0.5, -0.9, 0.6667), B::ErodedBadlands);
        assert_eq!(mc_biome_full(0.8, 0.5, 0.5, -0.9, -0.6667), B::WoodedBadlands);
        // High slice (|w|=0.5 -> pv 0.5): near-inland e0 = the slope
        // band; dry cold -> snowy slopes, wetter cold -> grove.
        assert_eq!(mc_biome_full(-0.6, -0.5, 0.0, -0.9, -0.5), B::SnowySlopes);
        assert_eq!(mc_biome_full(-0.6, 0.5, 0.0, -0.9, -0.5), B::Grove);
        // High slice far-inland e2 = plateau: temperate -> meadow,
        // its positive-weirdness variant -> cherry grove; hot ->
        // badlands plateau row; savanna row -> savanna plateau.
        assert_eq!(mc_biome_full(0.0, -0.2, 0.5, -0.3, -0.5), B::Meadow);
        assert_eq!(mc_biome_full(0.0, -0.2, 0.5, -0.3, 0.5), B::CherryGrove);
        assert_eq!(mc_biome_full(0.3, -0.5, 0.5, -0.3, -0.5), B::SavannaPlateau);
        assert_eq!(mc_biome_full(0.1, 0.5, 0.5, -0.3, -0.5), B::PaleGarden);
        // Mid slice, mid-inland e5 = the shattered band.
        assert_eq!(mc_biome_full(-0.6, -0.5, 0.2, 0.5, -0.35), B::WindsweptGravellyHills);
        assert_eq!(mc_biome_full(0.0, 0.0, 0.2, 0.5, -0.35), B::WindsweptHills);
        assert_eq!(mc_biome_full(0.0, 0.5, 0.2, 0.5, -0.35), B::WindsweptForest);
        // Windswept savanna: warm + not-humid + positive weirdness on
        // the near-inland shattered band.
        assert_eq!(mc_biome_full(0.4, 0.0, 0.0, 0.5, 0.35), B::WindsweptSavanna);
        // Stony shore: low/mid-slice coast with low erosion.
        assert_eq!(mc_biome_full(0.0, 0.0, -0.15, -0.9, -0.15), B::StonyShore);
        // Mushroom fields island far off-coast.
        assert_eq!(mc_biome_full(0.0, 0.0, -1.1, 0.0, 0.0), B::MushroomFields);
        // Mid slice far-inland e1 cold -> slope, else plateau.
        assert_eq!(mc_biome_full(-0.6, -0.5, 0.5, -0.5, -0.35), B::SnowySlopes);
        assert_eq!(mc_biome_full(0.0, -0.2, 0.5, -0.5, -0.35), B::Meadow);
    }
}
