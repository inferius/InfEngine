//! Deterministic 3D ore hashing + per-biome ore picker. Every
//! sub-surface stone voxel runs through `pick_stone_or_ore`, which
//! (W121) delegates to the sea-relative `strata` depth-band model:
//! gold / diamond / redstone / lapis / emerald / copper / iron / coal
//! placed with MC-1.18-style triangular depth distributions, and the
//! deepslate-ore variants swapped in below the deepslate threshold.
//!
//! This module now owns only the shared 3D hash (`ore_hash`); the band
//! table + depth math live in `strata.rs` so they can be reused by the
//! bedrock-floor / deepslate fill in `natural.rs` + `climate.rs`.

use infengine_voxel::BlockId;

use super::biome::Biome;
use super::strata;

/// Cheap, deterministic 3D hash used by the ore generator. Mixes
/// integer voxel coordinates + a per-ore-kind salt into a [0, 1]
/// float. Two octaves of bit-rotation give a noticeably "veiny"
/// distribution rather than salt-and-pepper.
///
/// F.2c-2: takes an explicit seed instead of reading the
/// `WORLD_SEED` atomic; callers pass `ctx.effective_seed()`.
pub(crate) fn ore_hash(x: i32, y: i32, z: i32, salt: u32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (y as u32).wrapping_mul(0x9E37_79B1);
    h ^= (z as u32).wrapping_mul(0x1656_67B1);
    h ^= salt ^ seed;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    // Blob softening: average with a neighbour cell so the result
    // forms small pockets instead of isolated voxels.
    let mut g = (x as u32 ^ y as u32 ^ z as u32).wrapping_mul(salt | 1);
    g ^= g >> 17;
    g = g.wrapping_mul(0xC2B2_AE35);
    let a = h as f32 / u32::MAX as f32;
    let b = g as f32 / u32::MAX as f32;
    (a * 0.7 + b * 0.3).clamp(0.0, 1.0)
}

/// Resolve the bulk/ore block for one sub-surface stone voxel.
///
/// W121: replaces the old 2-ore (coal + iron) hardcode with the full
/// MC-1.18 roster placed by depth band. The actual table + triangular
/// depth distribution live in `strata::pick_block`; this wrapper keeps
/// the long-standing `(f32, i32, f32, Biome, seed)` signature that the
/// three column-fill paths (`gen_classic`, `gen_realistic`,
/// `gen_natural`) call, converting the float world coords to the integer
/// voxel coords the band model hashes on.
pub(crate) fn pick_stone_or_ore(wx: f32, wy: i32, wz: f32, biome: Biome, seed: u32) -> BlockId {
    strata::pick_block(wx as i32, wy, wz as i32, biome, seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    // W5.1b: ore_hash takes seed as a parameter; no globals to
    // lock or seed.

    #[test]
    fn ore_hash_in_unit_range() {
        for x in -8..8 {
            for y in -8..8 {
                for z in -8..8 {
                    let v = ore_hash(x, y, z, 0xABCD, 7);
                    assert!((0.0..=1.0).contains(&v));
                }
            }
        }
    }

    #[test]
    fn ore_hash_deterministic_across_calls() {
        let a = ore_hash(3, 5, 7, 0xCAFE, 11);
        let b = ore_hash(3, 5, 7, 0xCAFE, 11);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn ore_hash_changes_with_seed() {
        let a = ore_hash(0, 1, 0, 0xCAFE, 1);
        let b = ore_hash(0, 1, 0, 0xCAFE, 2);
        assert_ne!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn pick_stone_or_ore_returns_block() {
        // W121: the picker now returns the full ore roster + bulk
        // stone/deepslate. Just assert every result is a real block id
        // (the per-band depth + deepslate behaviour is tested in
        // `strata`). y span covers shallow stone -> deep deepslate.
        let known = [
            1u16, 96, 13, 14, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 246, 247, 248,
            249,
        ];
        for y in 0..63 {
            let b = pick_stone_or_ore(0.0, y, 0.0, Biome::Plains, 0);
            assert!(known.contains(&b.0), "unexpected ore-pick id {}", b.0);
        }
    }
}
