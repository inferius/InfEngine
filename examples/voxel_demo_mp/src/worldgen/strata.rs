//! W121 - underground strata model: sea-relative depth bands for ores,
//! the deepslate transition, and the bedrock floor.
//!
//! # Why this module exists
//!
//! Before W121 the overworld underground only ever placed coal + iron,
//! both gated by **absolute** Y constants (`iron_max_y = 14/24`) pinned
//! to the long-dead sea-12 world. With `SEA_LEVEL = 63` those caps left
//! every ore + cave crammed into the bottom ~1/5 of the 256-tall world,
//! entirely below the waterline, so a player digging down from a hill
//! hit 50+ blocks of empty stone first. There was also no deepslate
//! stratum and no bedrock floor at all.
//!
//! # MC-faithful, data-driven (per the project mandate)
//!
//! Minecraft 1.18 places ores with **depth-banded triangular / uniform
//! distributions** on a -64..320 world (floor y=-64, sea y=63). InfEngine's
//! world is shorter (floor y=0, sea y=63 -> only ~63 blocks of underground
//! vs MC's 127). Rather than invent fresh magic Y numbers, this module
//! rescales MC's real bands onto our underground via a single normalised
//! **depth fraction**:
//!
//! ```text
//! depth = (SEA_LEVEL - wy) / (SEA_LEVEL - WORLD_FLOOR)   // 0..1
//! ```
//!
//! `depth = 0.0` is the waterline (shallowest stone), `depth = 1.0` is the
//! world floor (deepest). Each ore's MC band (min..max, peak) is converted
//! to the same fraction so the *shape* of MC's distribution is preserved -
//! diamond/gold densest at the floor, redstone/lapis low, copper mid, coal
//! high - while the single `SEA_LEVEL` / `WORLD_FLOOR` edit (and any future
//! taller-world edit) moves them all together. No scattered absolute Y.
//!
//! The deepslate transition + bedrock floor are expressed the same way.

use infengine_voxel::BlockId;

use super::biome::Biome;
use super::climate::SEA_LEVEL;
use super::ore::ore_hash;

/// World floor Y. Everything at or below this is solid; `gen_natural` /
/// `gen_realistic` lay bedrock here. Kept as a named const so the cave /
/// ravine / dungeon carvers stop hard-coding `0` / `2` and so a future
/// taller world (negative floor) is a one-line change.
pub(crate) const WORLD_FLOOR: i32 = 0;

/// Number of bedrock rows at the very bottom (y in `WORLD_FLOOR
/// ..WORLD_FLOOR + BEDROCK_ROWS`). MC uses a jagged 0..4 band; we keep a
/// thin solid cap (the world is only 256 tall) and jitter the top rows so
/// the floor is not a dead-flat plane.
pub(crate) const BEDROCK_ROWS: i32 = 4;

/// Depth fraction (0..1) below which stone becomes **deepslate**. MC's
/// deepslate blends in around y0..y8 on a y-64 floor, i.e. the bottom
/// ~1/8 of the underground. On our compressed column that lands at the
/// lower quarter (~y16 and below), which is where the report asked for
/// the transition. Expressed as a fraction so it tracks `SEA_LEVEL`.
pub(crate) const DEEPSLATE_DEPTH: f32 = 0.75;

/// Stone block id (overworld bulk).
pub(crate) const STONE_ID: u16 = 1;
/// Deepslate block id (deep bulk).
pub(crate) const DEEPSLATE_ID: u16 = 96;
/// Bedrock block id (world floor).
pub(crate) const BEDROCK_ID: u16 = 45;

/// Normalised depth at world-Y `wy`: `0.0` at sea level, `1.0` at the
/// world floor, clamped. Values above sea level clamp to `0.0` (the
/// near-surface band is handled by the height curve, not here).
#[inline]
pub(crate) fn depth_fraction(wy: i32) -> f32 {
    let span = (SEA_LEVEL - WORLD_FLOOR) as f32;
    ((SEA_LEVEL - wy) as f32 / span).clamp(0.0, 1.0)
}

/// True when world-Y `wy` sits in the deepslate stratum (deep enough that
/// the bulk stone + every stone-ore swaps to its deepslate variant).
#[inline]
pub(crate) fn is_deepslate(wy: i32) -> bool {
    depth_fraction(wy) >= DEEPSLATE_DEPTH
}

/// Bulk fill block for a sub-surface, non-ore voxel at `wy`: deepslate in
/// the deep stratum, plain stone above it.
#[inline]
pub(crate) fn bulk_block(wy: i32) -> BlockId {
    if is_deepslate(wy) {
        BlockId(DEEPSLATE_ID)
    } else {
        BlockId(STONE_ID)
    }
}

/// Jagged bedrock floor block at `(wx, wy, wz)`, or `None` if this voxel
/// is above the bedrock band. Row `WORLD_FLOOR` is always solid bedrock;
/// the rows above it are bedrock only on a deterministic hash roll so the
/// top of the floor reads jagged (MC's 1..4 noisy bedrock), the rest of
/// the band falling through to normal stone/deepslate fill.
///
/// `seed` is mixed in so the jitter pattern differs per world.
#[inline]
pub(crate) fn bedrock_floor_block(wx: i32, wy: i32, wz: i32, seed: u32) -> Option<BlockId> {
    if wy < WORLD_FLOOR {
        // Below the playable floor: keep it solid (proxy void plug).
        return Some(BlockId(BEDROCK_ID));
    }
    if wy == WORLD_FLOOR {
        return Some(BlockId(BEDROCK_ID));
    }
    if wy < WORLD_FLOOR + BEDROCK_ROWS {
        // Jagged upper rows: higher rows get rarer bedrock. Row offset
        // 1..BEDROCK_ROWS-1; probability ramps from ~3/4 down to ~1/4.
        let off = wy - WORLD_FLOOR; // 1..BEDROCK_ROWS-1
        let keep = (BEDROCK_ROWS - off) as f32 / BEDROCK_ROWS as f32;
        let h = ore_hash(wx, wy, wz, 0xBED2_0C12, seed);
        if h < keep {
            return Some(BlockId(BEDROCK_ID));
        }
    }
    None
}

// ---------------------------------------------------------------
// Ore depth-band table (MC 1.18 shape, rescaled to our underground)
// ---------------------------------------------------------------

/// One ore's placement rule. `stone_id` is the normal-stratum form,
/// `deepslate_id` the deep-stratum swap. The vein is rolled when a 3D
/// hash exceeds `1.0 - rarity` *and* the local depth is inside the band,
/// scaled by a triangular weight that peaks at `peak`.
struct OreBand {
    /// Stone-stratum ore block id.
    stone_id: u16,
    /// Deepslate-stratum ore block id (used when `is_deepslate(wy)`).
    deepslate_id: u16,
    /// Shallow edge of the band as a depth fraction (0 = sea level).
    depth_lo: f32,
    /// Deep edge of the band as a depth fraction (1 = world floor).
    depth_hi: f32,
    /// Depth fraction of peak density (triangular falloff to the edges).
    peak: f32,
    /// Base rarity at the peak: fraction of eligible voxels that become
    /// this ore right at `peak`. Tapers to 0 at `depth_lo` / `depth_hi`.
    rarity: f32,
    /// Per-ore hash salt (decorrelates the veins).
    salt: u32,
    /// `true` -> only generate under mountain biomes (emerald).
    mountain_only: bool,
}

/// The ore roster, ordered **rarest / most-valuable first** so a voxel
/// that qualifies for several bands resolves to the scarcer ore (diamond
/// wins over coal). Depth fractions are MC's 1.18 bands mapped onto our
/// 0..63 underground:
///
/// | Ore      | MC band (y, floor -64) | -> depth fraction | density |
/// |----------|------------------------|-------------------|---------|
/// | Diamond  | -64..16, peak deep     | 0.75..1.0 p1.0    | low     |
/// | Gold     | -64..32, peak -16      | 0.6..1.0  p0.9    | low     |
/// | Emerald  | mountains, high        | 0.0..0.5  p0.1 mt | v.low   |
/// | Redstone | -64..15                | 0.78..1.0 p1.0    | med     |
/// | Lapis    | -64..64, peak 0        | 0.4..1.0  p0.65   | low     |
/// | Copper   | -16..112, peak 48      | 0.0..0.55 p0.2    | med     |
/// | Iron     | -24..56, peak 16       | 0.2..0.95 p0.6    | high    |
/// | Coal     | 0..256, peak ~96       | 0.0..0.7  p0.0    | high    |
///
/// The peak / range numbers are the MC distribution rescaled by the depth
/// fraction, NOT fresh magic constants (see module docs).
const ORE_BANDS: &[OreBand] = &[
    // Diamond: deepest, denser the lower you go. None above the deep band.
    OreBand {
        stone_id: 118,
        deepslate_id: 125,
        depth_lo: 0.72,
        depth_hi: 1.0,
        peak: 1.0,
        rarity: 0.020,
        salt: 0xD1A3_0000,
        mountain_only: false,
    },
    // Gold: deep, peak just above the floor.
    OreBand {
        stone_id: 117,
        deepslate_id: 126,
        depth_lo: 0.58,
        depth_hi: 1.0,
        peak: 0.88,
        rarity: 0.030,
        salt: 0x90D0_0000,
        mountain_only: false,
    },
    // Emerald: mountain biomes only, shallow (MC: high-altitude, peak
    // ~y232). On our column that is the near-surface underground band.
    OreBand {
        stone_id: 121,
        deepslate_id: 248,
        depth_lo: 0.0,
        depth_hi: 0.55,
        peak: 0.1,
        rarity: 0.015,
        salt: 0xE3E3_0000,
        mountain_only: true,
    },
    // Redstone: low, uniform-ish toward the floor.
    OreBand {
        stone_id: 119,
        deepslate_id: 246,
        depth_lo: 0.78,
        depth_hi: 1.0,
        peak: 1.0,
        rarity: 0.055,
        salt: 0x9ED5_0000,
        mountain_only: false,
    },
    // Lapis: triangle peaking around mid-deep (MC peak y0 on a -64 floor).
    OreBand {
        stone_id: 120,
        deepslate_id: 247,
        depth_lo: 0.40,
        depth_hi: 1.0,
        peak: 0.78,
        rarity: 0.030,
        salt: 0x1A95_0000,
        mountain_only: false,
    },
    // Copper: mid band, peak around the middle of the underground.
    OreBand {
        stone_id: 122,
        deepslate_id: 249,
        depth_lo: 0.0,
        depth_hi: 0.55,
        peak: 0.30,
        rarity: 0.050,
        salt: 0xC0BB_0000,
        mountain_only: false,
    },
    // Iron: broad, peak low-mid (MC peak y16 on a -64 floor).
    OreBand {
        stone_id: 14,
        deepslate_id: 124,
        depth_lo: 0.18,
        depth_hi: 0.96,
        peak: 0.62,
        rarity: 0.075,
        salt: 0x1804_0000,
        mountain_only: false,
    },
    // Coal: shallow-to-mid, common; the high-up fallback ore.
    OreBand {
        stone_id: 13,
        deepslate_id: 123,
        depth_lo: 0.0,
        depth_hi: 0.72,
        peak: 0.18,
        rarity: 0.085,
        salt: 0x60A1_0000,
        mountain_only: false,
    },
];

/// Triangular weight in `[0, 1]`: 1 at `peak`, linearly down to 0 at the
/// band edges `lo` / `hi`. Outside `[lo, hi]` returns 0.
#[inline]
fn triangle_weight(d: f32, lo: f32, hi: f32, peak: f32) -> f32 {
    if d < lo || d > hi {
        return 0.0;
    }
    if d <= peak {
        // Rising edge lo..peak.
        let span = (peak - lo).max(1e-4);
        (d - lo) / span
    } else {
        // Falling edge peak..hi.
        let span = (hi - peak).max(1e-4);
        (hi - d) / span
    }
}

/// Resolve the bulk/ore block for one sub-surface voxel. Walks the ore
/// table rarest-first; the first band whose depth window contains `wy`
/// and whose 3D hash clears the depth-weighted threshold wins. Falls
/// through to bulk stone / deepslate.
///
/// Mountain biomes also widen iron + coal slightly (peaks are worth
/// digging) - mirrors the old `pick_stone_or_ore` mountain boost without
/// the absolute Y cap.
pub(crate) fn pick_block(wx: i32, wy: i32, wz: i32, biome: Biome, seed: u32) -> BlockId {
    let d = depth_fraction(wy);
    let mountain = matches!(biome, Biome::Mountain | Biome::SnowyMountain);
    let deep = is_deepslate(wy);

    for band in ORE_BANDS {
        if band.mountain_only && !mountain {
            continue;
        }
        let w = triangle_weight(d, band.depth_lo, band.depth_hi, band.peak);
        if w <= 0.0 {
            continue;
        }
        // Mountain biomes get a modest richness bump on the common metals
        // so peaks read as worth tunnelling (legacy behaviour, now
        // depth-relative instead of a Y cap).
        let rich = if mountain && (band.stone_id == 13 || band.stone_id == 14) {
            1.4
        } else {
            1.0
        };
        let threshold = 1.0 - (band.rarity * w * rich);
        let h = ore_hash(wx, wy, wz, band.salt, seed);
        if h > threshold {
            return BlockId(if deep { band.deepslate_id } else { band.stone_id });
        }
    }
    bulk_block(wy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_fraction_endpoints() {
        assert!((depth_fraction(SEA_LEVEL) - 0.0).abs() < 1e-6);
        assert!((depth_fraction(WORLD_FLOOR) - 1.0).abs() < 1e-6);
        // Above sea clamps to 0.
        assert_eq!(depth_fraction(SEA_LEVEL + 20), 0.0);
    }

    #[test]
    fn deepslate_only_deep() {
        assert!(!is_deepslate(SEA_LEVEL));
        assert!(!is_deepslate(SEA_LEVEL - 10));
        assert!(is_deepslate(WORLD_FLOOR + 1));
        // Bulk block switches accordingly.
        assert_eq!(bulk_block(SEA_LEVEL - 5).0, STONE_ID);
        assert_eq!(bulk_block(WORLD_FLOOR + 2).0, DEEPSLATE_ID);
    }

    #[test]
    fn bedrock_floor_solid_at_zero() {
        // y=0 always bedrock regardless of x/z.
        for x in 0..8 {
            for z in 0..8 {
                assert_eq!(bedrock_floor_block(x, WORLD_FLOOR, z, 0).map(|b| b.0), Some(BEDROCK_ID));
            }
        }
        // Above the band -> None.
        assert!(bedrock_floor_block(0, WORLD_FLOOR + BEDROCK_ROWS, 0, 0).is_none());
    }

    #[test]
    fn triangle_weight_shape() {
        // Peak in the middle.
        assert!((triangle_weight(0.5, 0.0, 1.0, 0.5) - 1.0).abs() < 1e-6);
        // Edges are zero.
        assert!(triangle_weight(0.0, 0.0, 1.0, 0.5) < 1e-6);
        assert!(triangle_weight(1.0, 0.0, 1.0, 0.5) < 1e-6);
        // Outside band is zero.
        assert_eq!(triangle_weight(0.2, 0.4, 0.9, 0.6), 0.0);
    }

    #[test]
    fn deep_voxels_produce_deepslate_variants() {
        // Sweep the deep stratum; any ore found there must be a deepslate
        // id (123-126 or 246-249), never a stone-form ore.
        let stone_ores = [13u16, 14, 117, 118, 119, 120, 121, 122];
        for wy in WORLD_FLOOR + 1..(SEA_LEVEL - (SEA_LEVEL as f32 * 0.3) as i32) {
            if !is_deepslate(wy) {
                continue;
            }
            for x in 0..16 {
                for z in 0..16 {
                    let b = pick_block(x, wy, z, Biome::Plains, 7).0;
                    assert!(
                        !stone_ores.contains(&b),
                        "stone-form ore {b} at deep wy={wy}"
                    );
                }
            }
        }
    }

    #[test]
    fn pick_block_only_returns_known_ids() {
        let known = [
            STONE_ID, DEEPSLATE_ID, 13, 14, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126,
            246, 247, 248, 249,
        ];
        for wy in WORLD_FLOOR..SEA_LEVEL {
            for x in 0..8 {
                for z in 0..8 {
                    for biome in [Biome::Plains, Biome::Mountain] {
                        let b = pick_block(x, wy, z, biome, 3).0;
                        assert!(known.contains(&b), "unexpected block {b} at wy={wy}");
                    }
                }
            }
        }
    }

    #[test]
    fn emerald_is_mountain_only() {
        // Emerald (121 stone / 248 deepslate) must never appear in plains.
        for wy in WORLD_FLOOR..SEA_LEVEL {
            for x in 0..24 {
                for z in 0..24 {
                    let b = pick_block(x, wy, z, Biome::Plains, 11).0;
                    assert_ne!(b, 121, "emerald in plains at wy={wy}");
                    assert_ne!(b, 248, "deepslate emerald in plains at wy={wy}");
                }
            }
        }
    }

    #[test]
    fn determinism() {
        let a = pick_block(5, 20, 7, Biome::Mountain, 99);
        let b = pick_block(5, 20, 7, Biome::Mountain, 99);
        assert_eq!(a, b);
    }
}
