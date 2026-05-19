//! Wave 12.4 - Sun + moon position math.
//!
//! The engine's existing day cycle (`lighting::compute_day_factor`)
//! drives the brightness curve; this module gives the sky-renderer
//! the world-space direction vectors for the sun + moon billboards.
//!
//! The two bodies are antipodal: the moon sits exactly opposite the
//! sun across the player so they never overlap. Sun is straight
//! overhead at the day-time midpoint of the daytime window
//! (phase = 0.25 in our 0..1 phase mapping, with day = [0, 0.5]
//! and night = [0.5, 1)).
//!
//! Cite `J:\Temp\template-mod-template-26.1.2\sources\
//! minecraft-merged-52430b475d-26.1.2-sources\
//! net\minecraft\client\renderer\DimensionSpecialEffects.java` -
//! `getSunriseColor` + the day-arc rotation around the X axis.
//! MC rotates the sky by `(skyAngle * 360deg)` around X; our
//! approximation: sun follows a great circle in the X-Y plane.
//!
//! `LevelRenderer.renderSky` shows how MC draws sun + moon as
//! billboard quads on the celestial dome.

use std::f32::consts::TAU;

/// Sun + moon disc diameter in radians of sky. Spec value: 5 deg.
pub(crate) const BODY_ANGULAR_DIAMETER_RAD: f32 = 5.0 * (std::f32::consts::PI / 180.0);

/// How many in-game days the moon takes to walk through all 8
/// phases. Spec value: 8 days -> one phase per day.
pub(crate) const MOON_PHASE_CYCLE_DAYS: u32 = 8;

/// Number of moon phase tiles in MC's moon_phases.png (4x2 atlas).
pub(crate) const MOON_PHASE_COUNT: u32 = 8;

/// Map a day_time fraction in `[0, 1)` to the sun's world-space
/// direction vector. Sun rises at phase 0 (east-ish), is straight
/// overhead at phase 0.25, sets at phase 0.5, sits underfoot at
/// phase 0.75.
///
/// Returned vector is unit-length. World axes:
/// - +X = east, +Z = south, +Y = up (right-handed).
/// - The arc sits in the X-Y plane (sun travels east -> up -> west).
pub(crate) fn sun_direction(phase: f32) -> [f32; 3] {
    let p = phase.rem_euclid(1.0);
    // Map phase 0 -> east, 0.25 -> overhead, 0.5 -> west, 0.75 ->
    // straight down. Angle 0 = +X direction; sweep counter-clockwise
    // around Z so at p = 0.25 the vector is (0, 1, 0).
    let angle = p * TAU;
    let x = angle.cos();
    let y = angle.sin();
    [x, y, 0.0]
}

/// Moon is antipodal to the sun.
pub(crate) fn moon_direction(phase: f32) -> [f32; 3] {
    let s = sun_direction(phase);
    [-s[0], -s[1], -s[2]]
}

/// Atlas tile index `0..8` for the moon phases sheet. Day count
/// advances the phase by one each in-game day so the moon walks
/// through full -> waning -> new -> waxing over 8 days.
pub(crate) fn moon_phase_for_day(day_count: u32) -> u32 {
    day_count % MOON_PHASE_COUNT
}

/// Convenience: pixel-space UV rect inside `moon_phases.png` for
/// a given phase index. The MC sheet is a 4x2 atlas; tile size is
/// `(width / 4, height / 2)` in source pixels.
pub(crate) fn moon_phase_uv(phase_idx: u32) -> [f32; 4] {
    let idx = phase_idx.min(MOON_PHASE_COUNT - 1);
    let col = idx % 4;
    let row = idx / 4;
    let u0 = col as f32 / 4.0;
    let v0 = row as f32 / 2.0;
    [u0, v0, u0 + 0.25, v0 + 0.5]
}

/// True when the sun direction is above the horizon (y > 0). The
/// renderer uses this to decide which sky-tint mix (day vs night)
/// applies.
pub(crate) fn sun_is_above_horizon(phase: f32) -> bool {
    sun_direction(phase)[1] > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    fn vec_approx(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        approx_eq(a[0], b[0], eps)
            && approx_eq(a[1], b[1], eps)
            && approx_eq(a[2], b[2], eps)
    }

    #[test]
    fn sun_position_at_noon_is_overhead() {
        // phase = 0.25 -> sun at zenith.
        let d = sun_direction(0.25);
        assert!(
            vec_approx(d, [0.0, 1.0, 0.0], 1e-3),
            "sun at phase 0.25 should be straight overhead, got {d:?}",
        );
    }

    #[test]
    fn moon_position_at_midnight_is_overhead() {
        // phase = 0.75 -> sun underfoot, moon overhead.
        let d = moon_direction(0.75);
        assert!(
            vec_approx(d, [0.0, 1.0, 0.0], 1e-3),
            "moon at phase 0.75 should be straight overhead, got {d:?}",
        );
    }

    #[test]
    fn sun_at_dawn_is_east() {
        let d = sun_direction(0.0);
        assert!(
            vec_approx(d, [1.0, 0.0, 0.0], 1e-3),
            "sun at phase 0 should be due east, got {d:?}",
        );
    }

    #[test]
    fn sun_at_dusk_is_west() {
        let d = sun_direction(0.5);
        assert!(
            vec_approx(d, [-1.0, 0.0, 0.0], 1e-3),
            "sun at phase 0.5 should be due west, got {d:?}",
        );
    }

    #[test]
    fn moon_phase_cycles_with_day_count() {
        // Day 0 = full moon (phase idx 0). Each in-game day bumps
        // the phase index by one, wrapping at 8.
        assert_eq!(moon_phase_for_day(0), 0);
        assert_eq!(moon_phase_for_day(1), 1);
        assert_eq!(moon_phase_for_day(7), 7);
        assert_eq!(moon_phase_for_day(8), 0); // wrap
        assert_eq!(moon_phase_for_day(9), 1);
    }

    #[test]
    fn moon_phase_uv_covers_4x2_atlas() {
        // Top-left tile = (0,0,0.25,0.5).
        let uv = moon_phase_uv(0);
        assert!(approx_eq(uv[0], 0.0, 1e-3));
        assert!(approx_eq(uv[1], 0.0, 1e-3));
        assert!(approx_eq(uv[2], 0.25, 1e-3));
        assert!(approx_eq(uv[3], 0.5, 1e-3));
        // Bottom-right tile (idx 7 = col 3, row 1).
        let uv = moon_phase_uv(7);
        assert!(approx_eq(uv[0], 0.75, 1e-3));
        assert!(approx_eq(uv[1], 0.5, 1e-3));
        assert!(approx_eq(uv[2], 1.0, 1e-3));
        assert!(approx_eq(uv[3], 1.0, 1e-3));
    }

    #[test]
    fn moon_phase_uv_oob_clamps_to_last_tile() {
        let uv = moon_phase_uv(99);
        let last = moon_phase_uv(7);
        assert_eq!(uv, last);
    }

    #[test]
    fn sun_above_horizon_during_day_window() {
        // Day window in our phase mapping: (0, 0.5). At the
        // boundary it dips to zero - we treat that as below.
        assert!(sun_is_above_horizon(0.1));
        assert!(sun_is_above_horizon(0.25));
        assert!(sun_is_above_horizon(0.4));
        assert!(!sun_is_above_horizon(0.5));
        assert!(!sun_is_above_horizon(0.6));
        assert!(!sun_is_above_horizon(0.75));
        assert!(!sun_is_above_horizon(0.0));
    }

    #[test]
    fn sun_and_moon_are_antipodal() {
        for p in [0.0, 0.1, 0.25, 0.5, 0.7, 0.99] {
            let s = sun_direction(p);
            let m = moon_direction(p);
            // m = -s. The sum should be zero.
            assert!(
                vec_approx([s[0] + m[0], s[1] + m[1], s[2] + m[2]], [0.0; 3], 1e-3),
                "sun + moon not antipodal at phase {p}",
            );
        }
    }

    #[test]
    fn body_angular_diameter_is_5_degrees() {
        // Sanity: 5 degrees in radians.
        let expected = 5.0_f32.to_radians();
        assert!(approx_eq(BODY_ANGULAR_DIAMETER_RAD, expected, 1e-4));
    }

    #[test]
    fn moon_phase_cycle_is_8_days() {
        assert_eq!(MOON_PHASE_CYCLE_DAYS, 8);
        assert_eq!(MOON_PHASE_COUNT, 8);
    }

    #[test]
    fn sun_direction_is_unit_length() {
        for p in [0.0, 0.1, 0.25, 0.5, 0.75, 0.99] {
            let s = sun_direction(p);
            let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-3, "sun len = {len} at phase {p}");
        }
    }

    #[test]
    fn phase_wraps_via_rem_euclid() {
        // Negative phase + phase > 1 should wrap into [0, 1).
        let a = sun_direction(1.25);
        let b = sun_direction(0.25);
        assert!(vec_approx(a, b, 1e-3));
        let c = sun_direction(-0.75);
        let d = sun_direction(0.25);
        assert!(vec_approx(c, d, 1e-3));
    }
}
