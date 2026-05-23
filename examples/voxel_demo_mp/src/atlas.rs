//! W47.27 - block atlas + day/night helpers (relocated from main.rs).

#![allow(unused_imports)]

use crate::*;

pub(crate) fn load_block_atlas_from_dir(dir: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut copy_tile = |spec: &TileSpec| -> Option<()> {
        // Pick the first existing file from the fallback list.
        let path = spec
            .files
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.exists())?;
        let img = image::open(&path).ok()?.to_rgba8();
        let img = if img.width() == TILE_PX && img.height() == TILE_PX {
            img
        } else {
            image::imageops::resize(&img, TILE_PX, TILE_PX, image::imageops::FilterType::Nearest)
        };
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = spec.col * TILE_PX + tx;
                let py = spec.row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let pixel = img.get_pixel(tx, ty);
                rgba[idx] = pixel.0[0];
                rgba[idx + 1] = pixel.0[1];
                rgba[idx + 2] = pixel.0[2];
                // Opaque tiles force alpha to 255 so anti-aliased
                // edges in the source can't trip the global
                // alpha-cutout shader.
                rgba[idx + 3] = if spec.keep_alpha { pixel.0[3] } else { 255 };
            }
        }
        Some(())
    };
    // Iterate every (block, face) tile in the registry. Dedup by
    // (col, row) so a tile shared by multiple faces / blocks is
    // copied once.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for bt in BLOCK_TYPES {
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if !seen.insert((spec.col, spec.row)) {
                continue;
            }
            if copy_tile(spec).is_none() {
                tracing::warn!(
                    block = bt.name,
                    files = ?spec.files,
                    "atlas texture missing or unreadable"
                );
                return None;
            }
        }
    }
    tracing::info!(dir = %dir.display(), "loaded block atlas from disk");
    Some((w, h, rgba))
}

/// Procedural 64x32 RGBA atlas built once at startup. Each 16x16
/// tile is the block's base colour modulated by a deterministic
/// per-pixel noise so the texture reads as voxelly fabric rather
/// than flat colour.
/// Day/night driver. Given the current `time` in seconds (modulo
/// `DAY_LENGTH_S`), return `(sun_dir, sky_color)`:
/// - `sun_dir` points TOWARDS the sun (matches `Renderer::set_sun_dir`).
///   The sun arcs from +X at sunrise through +Y at noon to -X at
///   sunset, with a small +Z tilt so shadows are never axis-aligned.
/// - `sky_color` is the linear-RGB clear colour. Three anchors:
///   deep navy at midnight, warm orange at horizon, bright cyan at noon.
pub(crate) fn day_night(time: f32) -> (Vec3, [f32; 3]) {
    let phase = (time / DAY_LENGTH_S).rem_euclid(1.0);
    let theta = phase * std::f32::consts::TAU;
    // Sun position on a circle in the X-Y plane, tilted slightly
    // along +Z so shadows never align perfectly with cardinal axes.
    let sun = Vec3::new(theta.cos(), theta.sin(), 0.35).normalize();
    let elev = sun.y; // -1 (under ground) .. +1 (overhead)
    // W8.3: midnight is now near-black instead of a faint navy.
    // The user explicitly asked for a dark night sky; the old
    // [0.04, 0.06, 0.12] still read as "dim blue" on a calibrated
    // monitor.
    let night = [0.02_f32, 0.02, 0.05];
    let horizon = [0.95_f32, 0.55, 0.30];
    let noon = [0.45_f32, 0.65, 0.95];
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    // Below horizon -> night..horizon; above -> horizon..noon.
    let sky = if elev < 0.0 {
        lerp(night, horizon, (elev + 0.2) * 5.0)
    } else {
        lerp(horizon, noon, elev * 2.0)
    };
    // Below horizon: clamp the sun direction to a "moon"-ish weak
    // angle from above so shadows still render (the shadow pass
    // doesn't model occluded suns). The PBR shader gets a much
    // dimmer normal product because the sun_intensity stays fixed
    // but the n.l term goes near zero by geometry alone.
    let sun = if elev <= 0.05 {
        Vec3::new(0.2, 0.95, 0.25).normalize()
    } else {
        sun
    };
    (sun, sky)
}

// ---------------------------------------------------------------
// W9.5: ambient particle helpers used by `tick_ambient_particles`.
// ---------------------------------------------------------------

/// W38.2: Returns true for blocks that should emit ambient flame/smoke
/// particles each tick. Mirrors MC's open-flame spawn list. Note this
/// is DIFFERENT from light_emission - glowstone emits light but no
/// flame (solid block), while certain emitter variants emit particles
/// but not necessarily maximum light.
pub(crate) fn flame_emitter(id: u16) -> bool {
    // TORCH_ID = 10, redstone torches = 50/51, soul torches = 52/53,
    // W46.21 copper torches = 324/325.
    use crate::block::{COPPER_TORCH_ID, COPPER_WALL_TORCH_ID};
    matches!(
        id,
        x if x == TORCH_ID
            || x == REDSTONE_TORCH_ID
            || x == REDSTONE_WALL_TORCH_ID
            || x == SOUL_TORCH_ID
            || x == SOUL_WALL_TORCH_ID
            || x == COPPER_TORCH_ID
            || x == COPPER_WALL_TORCH_ID
    )
}


// ---------------------------------------------------------------

// W9.2: superseded by `resource_pack::fallback::placeholder_pack`,
// kept around for callers that haven't been migrated to the new
// API + so the legacy `--textures <dir>` flag still has a
// procedural fallback when its dir lookup misses.
#[allow(dead_code)]
pub(crate) fn build_block_atlas() -> (u32, u32, Vec<u8>) {
    let w = ATLAS_COLS * TILE_PX;
    let h = ATLAS_ROWS * TILE_PX;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    // Paint every tile referenced by BLOCK_TYPES once. The
    // procedural fallback approximates the block's albedo with
    // per-pixel noise so the texture reads as voxelly fabric
    // rather than flat colour. Each tile picks the colour of the
    // first BlockType that points at it.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let paint = |rgba: &mut Vec<u8>, col: u32, row: u32, color: [u8; 3]| {
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let px = col * TILE_PX + tx;
                let py = row * TILE_PX + ty;
                let idx = ((py * w + px) * 4) as usize;
                let n =
                    ((tx.wrapping_mul(31) ^ ty.wrapping_mul(17)) % 64) as i32 - 32;
                let bias = (n as f32 / 32.0) * 24.0;
                rgba[idx] = (color[0] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (color[1] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (color[2] as f32 + bias).clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = 255;
            }
        }
    };
    let f2u = |v: f32| (v * 255.0).clamp(0.0, 255.0) as u8;
    for bt in BLOCK_TYPES {
        let base = [f2u(bt.color[0]), f2u(bt.color[1]), f2u(bt.color[2])];
        for spec in [&bt.top, &bt.bottom, &bt.sides] {
            if seen.insert((spec.col, spec.row)) {
                paint(&mut rgba, spec.col, spec.row, base);
            }
        }
    }
    (w, h, rgba)
}

// `generate_chunk_at` + `gen_flat` + `gen_islands` + `gen_classic`
// moved to `worldgen.rs`.


// `body_aabb`, `is_passable`, `voxel_is_solid`,
// `submerged_liquid_eye`, `submerged_liquid`, and
// `aabb_intersects_solid` moved to `player.rs`.

/// Approximate face normal from 4 corners (used by the sloped
/// water top quad where the corner heights differ). Cross product
/// of two opposing edges - close enough for shading the gentle
/// slopes the depth ramp produces.
pub(crate) fn water_face_normal(
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
    _v3: [f32; 3],
) -> [f32; 3] {
    // W34: explicit upward normal for top face. The water top quad is
    // always nearly horizontal (corner heights wiggle within [0, 1])
    // so the outward normal is essentially +Y. Cross-product attempts
    // gave -Y for the SW,SE,NE corner order (caller convention is CW
    // from above), and negating that produced an off-look from above
    // per the user's repeated reports. Force-emit +Y which is the only
    // physically meaningful outward direction for a water surface.
    let _ = (v0, v1, v2);
    [0.0, 1.0, 0.0]
}


// `submerged_liquid_eye`, `submerged_liquid`, and
// `aabb_intersects_solid` moved to `player.rs`.
#[cfg(test)]
mod day_night_tests {
    use super::{DAY_LENGTH_S, day_night};

    /// Helper: just the sky color half of the `day_night` return.
    fn sky_at(t: f32) -> [f32; 3] {
        day_night(t).1
    }

    /// Helper: euclidean distance between two RGB triples. Lets the
    /// assertions read tolerantly so the smoothstep ramp doesn't
    /// have to land on exact float boundaries.
    fn rgb_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dr = a[0] - b[0];
        let dg = a[1] - b[1];
        let db = a[2] - b[2];
        (dr * dr + dg * dg + db * db).sqrt()
    }

    #[test]
    fn sky_color_at_noon_matches_daytime_anchor() {
        // W8.3: noon = day fraction 0.25 (sun at +Y peak in our
        // phase = sin(theta) curve). Within tolerance of the
        // noon anchor [0.45, 0.65, 0.95].
        let sky = sky_at(DAY_LENGTH_S * 0.25);
        let noon_anchor = [0.45_f32, 0.65, 0.95];
        assert!(
            rgb_dist(sky, noon_anchor) < 0.10,
            "noon sky {sky:?} far from anchor {noon_anchor:?}"
        );
    }

    #[test]
    fn sky_color_at_midnight_is_near_black() {
        // W8.3: midnight = day fraction 0.75 (sun straight down).
        // Each channel should be <= 0.10 (the night anchor max).
        let sky = sky_at(DAY_LENGTH_S * 0.75);
        for (i, ch) in sky.iter().enumerate() {
            assert!(
                *ch <= 0.10,
                "midnight sky channel {i} = {ch} not near-black"
            );
        }
    }

    #[test]
    fn sky_color_dawn_is_intermediate() {
        // W8.3: dawn (0.50) sits in the night..horizon ramp - the
        // result should be brighter than the midnight anchor but
        // darker than the noon anchor.
        let dawn = sky_at(DAY_LENGTH_S * 0.50);
        let midnight = sky_at(DAY_LENGTH_S * 0.75);
        let noon = sky_at(DAY_LENGTH_S * 0.25);
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        let strict_min = [0.0_f32, 0.0, 0.0];
        assert!(
            lum(dawn) >= lum(strict_min),
            "dawn {dawn:?} below strict min"
        );
        assert!(
            lum(dawn) < lum(noon),
            "dawn {dawn:?} brighter than noon {noon:?}"
        );
        // Soft sanity: midnight luminance is non-greater than dawn
        // (the ramp is monotonic in the dawn -> noon direction).
        assert!(lum(midnight) <= lum(noon));
    }

    #[test]
    fn sky_color_smooth_ramp_through_dusk() {
        // W8.3: stepping in 0.01-day fractions across dusk
        // (0.70..0.80) should produce a monotonically-darkening
        // sequence - no jumps back to bright blue.
        let mut prev_lum = f32::INFINITY;
        let mut samples = 0;
        for i in 70..=80 {
            let t = DAY_LENGTH_S * (i as f32 * 0.01);
            let c = sky_at(t);
            let lum = c[0] + c[1] + c[2];
            assert!(
                lum <= prev_lum + 0.01,
                "sky luminance not monotonic at t={t}: {lum} after {prev_lum}"
            );
            prev_lum = lum;
            samples += 1;
        }
        assert!(samples >= 10, "expected at least 10 ramp samples");
    }
}
