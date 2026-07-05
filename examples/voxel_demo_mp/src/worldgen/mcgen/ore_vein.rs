//! W215 - the giant ore veins, ported from the decompiled 26.1.2
//! `OreVeinifier`.
//!
//! Where the noise fill places bulk stone, the vein channels can
//! substitute a snaking granite-and-copper (or tuff-and-iron) body:
//! `vein_toggle`'s sign picks the type, its |value| (with an edge
//! roundoff near the band limits) gates against 0.4, a per-block
//! positional roll keeps 70% solidness, `vein_ridged >= 0` hollows the
//! twin-ridge shape, and richness (0.1..0.3 mapped from |toggle|
//! 0.4..0.6) + `vein_gap > -0.3` decide ore vs filler, with a 2% raw
//! ore block chance. The toggle/ridged channels are `interpolated` in
//! the data - they share the chunk-column cell lattices with
//! `final_density`, so the veins have MC's exact blobby cell look.
//!
//! Engine band note: the IRON vein band (MC y -60..-8) lies entirely
//! below the realized y>=0 world, so only COPPER veins (y 0..50)
//! materialize - the type logic stays complete for a future deeper
//! band. Randomness is the engine's positional SplitMix64 (rules, not
//! seed identity).

use std::sync::OnceLock;

use infengine_voxel::BlockId;

use super::density::CellLattices;
use super::features::block_id_by_name;
use super::noise::SplitMix64;
use super::overworld::Overworld;

const VEININESS_THRESHOLD: f64 = 0.4;
const EDGE_ROUNDOFF_BEGIN: f64 = 20.0;
const MAX_EDGE_ROUNDOFF: f64 = 0.2;
const VEIN_SOLIDNESS: f32 = 0.7;
const MIN_RICHNESS: f64 = 0.1;
const MAX_RICHNESS: f64 = 0.3;
const MAX_RICHNESS_THRESHOLD: f64 = 0.6;
const CHANCE_OF_RAW_ORE_BLOCK: f32 = 0.02;
const SKIP_ORE_IF_GAP_NOISE_IS_BELOW: f64 = -0.3;

const SALT_ORE_VEIN: u64 = 0x09E5_E11A;

/// `OreVeinifier.VeinType` resolved to engine ids.
struct VeinType {
    ore: u16,
    raw_block: u16,
    filler: u16,
    min_y: i32,
    max_y: i32,
}

fn vein_types() -> &'static (VeinType, VeinType) {
    static TYPES: OnceLock<(VeinType, VeinType)> = OnceLock::new();
    TYPES.get_or_init(|| {
        let id = |n: &str| block_id_by_name(n).unwrap_or(1);
        (
            // COPPER: copper_ore / raw_copper_block / granite, y 0..50.
            VeinType {
                ore: id("copper_ore"),
                raw_block: id("raw_copper_block"),
                filler: id("granite"),
                min_y: 0,
                max_y: 50,
            },
            // IRON: deepslate_iron_ore / raw_iron_block / tuff, y -60..-8.
            VeinType {
                ore: id("deepslate_iron_ore"),
                raw_block: id("raw_iron_block"),
                filler: id("tuff"),
                min_y: -60,
                max_y: -8,
            },
        )
    })
}

/// `Mth.clampedMap`.
fn clamped_map(v: f64, from0: f64, from1: f64, to0: f64, to1: f64) -> f64 {
    let t = ((v - from0) / (from1 - from0)).clamp(0.0, 1.0);
    to0 + t * (to1 - to0)
}

/// The per-block positional roll stream (`oreVeinsPositionalRandomFactory.at`).
fn positional(seed: u32, x: i32, y: i32, z: i32) -> SplitMix64 {
    SplitMix64::new(
        (seed as u64) ^ SALT_ORE_VEIN
            ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
    )
}

/// `OreVeinifier.create`'s filler, for one bulk-stone cell: `Some` =
/// the vein body replaces the default stone, `None` = keep it.
pub(crate) fn ore_vein(
    ow: &Overworld,
    cells: &CellLattices,
    wx: i32,
    wy: i32,
    wz: i32,
    seed: u32,
) -> Option<BlockId> {
    let toggle = ow.vein_toggle_cells(wx, wy, wz, cells);
    let (copper, iron) = vein_types();
    let vein = if toggle > 0.0 { copper } else { iron };
    let ridged_veininess = toggle.abs();
    let dist_top = vein.max_y - wy;
    let dist_bottom = wy - vein.min_y;
    if dist_top < 0 || dist_bottom < 0 {
        return None;
    }
    let dist_edge = dist_top.min(dist_bottom) as f64;
    let edge_roundoff = clamped_map(dist_edge, 0.0, EDGE_ROUNDOFF_BEGIN, -MAX_EDGE_ROUNDOFF, 0.0);
    if ridged_veininess + edge_roundoff < VEININESS_THRESHOLD {
        return None;
    }
    let mut r = positional(seed, wx, wy, wz);
    if r.next_f64() as f32 > VEIN_SOLIDNESS {
        return None;
    }
    if ow.vein_ridged_cells(wx, wy, wz, cells) >= 0.0 {
        return None;
    }
    let richness = clamped_map(
        ridged_veininess,
        VEININESS_THRESHOLD,
        MAX_RICHNESS_THRESHOLD,
        MIN_RICHNESS,
        MAX_RICHNESS,
    );
    if (r.next_f64() as f32) < richness as f32
        && ow.vein_gap_cells(wx, wy, wz, cells) > SKIP_ORE_IF_GAP_NOISE_IS_BELOW
    {
        Some(BlockId(if (r.next_f64() as f32) < CHANCE_OF_RAW_ORE_BLOCK {
            vein.raw_block
        } else {
            vein.ore
        }))
    } else {
        Some(BlockId(vein.filler))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::mcgen::overworld::build_overworld;

    /// Veins exist, stay in the copper band, and contain both ore and
    /// granite filler (the iron band is below the engine world).
    #[test]
    fn copper_veins_materialize_in_band() {
        let ow = build_overworld(0);
        let (copper, _) = vein_types();
        let (mut ore, mut filler, mut outside_band) = (0u32, 0u32, 0u32);
        for cx in -6..6 {
            for cz in -6..6 {
                let cells = ow.build_cells(cx * 16, cz * 16, 224);
                for y in (2..64).step_by(3) {
                    for lx in (0..16).step_by(5) {
                        for lz in (0..16).step_by(5) {
                            let (x, z) = (cx * 16 + lx, cz * 16 + lz);
                            let Some(b) = ore_vein(&ow, &cells, x, y, z, 0) else {
                                continue;
                            };
                            if !(0..=50).contains(&y) {
                                outside_band += 1;
                            }
                            if b.0 == copper.ore || b.0 == copper.raw_block {
                                ore += 1;
                            } else if b.0 == copper.filler {
                                filler += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(outside_band, 0, "vein cells outside the copper band");
        assert!(ore > 0, "no copper ore materialized");
        assert!(filler > ore, "granite filler must dominate: ore {ore} filler {filler}");
    }

    #[test]
    fn clamped_map_matches_mc() {
        assert!((clamped_map(0.0, 0.0, 20.0, -0.2, 0.0) - -0.2).abs() < 1e-9);
        assert!((clamped_map(20.0, 0.0, 20.0, -0.2, 0.0) - 0.0).abs() < 1e-9);
        assert!((clamped_map(50.0, 0.0, 20.0, -0.2, 0.0) - 0.0).abs() < 1e-9);
        assert!((clamped_map(0.5, 0.4, 0.6, 0.1, 0.3) - 0.2).abs() < 1e-9);
    }
}
