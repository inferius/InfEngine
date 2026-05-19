//! Height curve + cave carve. The Classic generator reads
//! `height_at_classic` for each (wx, wz) in the chunk; the
//! Realistic generator brings its own table-driven height fn that
//! covers the extended biomes. `cave_at` is shared by both.

use super::biome::Biome;

/// Per-biome height for the Classic generator. `amp_scale = 2.5`
/// gives the Amplified profile its taller-mountain feel without a
/// separate generator path.
pub(crate) fn height_at_classic(wx: f32, wz: f32, biome: Biome, amp_scale: f32) -> i32 {
    let base = match biome {
        Biome::Desert => 12.0,
        Biome::Plains => 18.0,
        Biome::Mountain => 30.0,
        // The classic generator only invokes biome_at, which never
        // returns an extended variant, so these branches are dead
        // weight in practice. Provide reasonable fall-throughs so a
        // misuse from Realistic doesn't blow up.
        _ => 18.0,
    };
    let amp = (match biome {
        Biome::Desert => 3.0,
        Biome::Plains => 6.0,
        Biome::Mountain => 18.0,
        _ => 6.0,
    }) * amp_scale;
    let h = base
        + ((wx * 0.13).sin() * 0.5 + (wz * 0.17).cos() * 0.5) * amp
        + ((wx + wz) * 0.07).sin() * amp * 0.4;
    h.round() as i32
}

/// Cheap procedural cave check: 3D noise made of sin/cos
/// octaves. Returns true when (wx, wy, wz) sits inside a void
/// pocket. Tuned so caves only carve into stone (under the
/// grass / dirt cap) + leave the surface intact, and so two
/// octaves combine into a "blobby tunnel" shape rather than
/// single-block holes.
pub(crate) fn cave_at(wx: f32, wy: f32, wz: f32) -> bool {
    if wy < 1.0 {
        return false; // keep a bedrock floor at y=0
    }
    let a = (wx * 0.09).sin() * 0.5
        + (wz * 0.08).cos() * 0.5
        + (wy * 0.12).sin() * 0.5;
    let b = (wx * 0.20 + 17.0).sin() * (wy * 0.16 + 9.0).cos()
        + (wz * 0.21 + 31.0).sin() * 0.6;
    let n = a * 0.55 + b * 0.45;
    n > 0.78
}

#[cfg(test)]
mod tests {
    use super::*;
    // W5.1b: height functions are pure (no seed mixing); no
    // globals to lock or seed.

    #[test]
    fn classic_heights_in_sane_range() {
        for wx in -64..64 {
            for wz in -64..64 {
                let h = height_at_classic(wx as f32, wz as f32, Biome::Plains, 1.0);
                assert!((-20..=120).contains(&h), "h={h}");
            }
        }
    }

    #[test]
    fn cave_at_floor_is_solid() {
        for wy in -3..1 {
            assert!(!cave_at(10.0, wy as f32, 10.0));
        }
    }

    #[test]
    fn amplified_extends_range() {
        let h1 = height_at_classic(7.0, 11.0, Biome::Mountain, 1.0);
        let h25 = height_at_classic(7.0, 11.0, Biome::Mountain, 2.5);
        // For any non-zero offset around the base the amplified
        // path moves the height in the same direction with
        // greater magnitude. Allow equality only when the noise
        // sample happens to be exactly zero.
        assert!((h25 - 30).abs() >= (h1 - 30).abs());
    }
}
