//! Deterministic 3D ore hashing + per-biome ore picker. The
//! classic generator falls through to `pick_stone_or_ore` for
//! every below-cap stone voxel; under Mountain biome the
//! probabilities lean richer + iron's ceiling rises so peaks
//! become genuinely worth digging into.

use infengine_voxel::BlockId;

use super::biome::Biome;

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

/// Roll for ore veins under the surface cap. Two deterministic 3D
/// hashes seed coal + iron pockets; each is a noise value that
/// exceeds a threshold rarely. Coal is more common + can live
/// high; iron is rarer + only deep. Mountain biome boosts both
/// probabilities (lower thresholds) and raises the iron Y ceiling
/// so peaks become genuinely worth digging into.
pub(crate) fn pick_stone_or_ore(wx: f32, wy: i32, wz: f32, biome: Biome, seed: u32) -> BlockId {
    let ox = wx as i32;
    let oy = wy;
    let oz = wz as i32;
    let h_coal = ore_hash(ox, oy, oz, 0x9E37_79B9, seed);
    let h_iron = ore_hash(ox, oy, oz, 0x85EB_CA6B, seed);
    let (iron_thr, iron_max_y, coal_thr) = match biome {
        Biome::Mountain | Biome::SnowyMountain => (0.965, 24, 0.90),
        _ => (0.985, 14, 0.94),
    };
    if h_iron > iron_thr && oy < iron_max_y {
        BlockId(14)
    } else if h_coal > coal_thr {
        BlockId(13)
    } else {
        BlockId(1)
    }
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
        for y in 0..20 {
            let b = pick_stone_or_ore(0.0, y, 0.0, Biome::Plains, 0);
            assert!(matches!(b.0, 1 | 13 | 14));
        }
    }
}
