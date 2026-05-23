//! W47.9 - sun + moon + sun-intensity render, relocated from main.rs.
//!
//! The world's day clock (`Voxel.day_time`, persisted via W46.31) drives
//! both the directional sun-shadow intensity AND the position of the
//! sun + moon mesh discs. Renders the discs through the mesh3d PBR
//! path so they get depth-tested against the world geometry (terrain
//! occludes the sky in caves / under overhangs).

use glam::{Mat4, Vec3};

use infengine_render::{MeshHandle, Renderer};

use crate::sky;
use crate::DAY_LENGTH_S;

const CELESTIAL_DIST: f32 = 140.0;

/// Render the sun + moon meshes and update the sun intensity uniform.
/// `day_time` is the world clock in seconds since dawn; `sun_dir` is
/// the active sun direction (already clamped to the floor when below
/// the horizon - the raw elevation is recomputed here from `day_time`
/// for the intensity curve so dimming through the horizon is smooth).
pub(crate) fn draw(
    renderer: &mut Renderer,
    player_cube: MeshHandle,
    player_pos: Vec3,
    sun_dir: Vec3,
    day_time: f32,
    session_secs_elapsed: f32,
) {
    // Tie sun intensity to the real (un-clamped) elevation so night
    // is actually dim. We recompute the raw direction here because
    // `day_night` clamps the dir vector when the sun is below the
    // horizon (to keep shadows sane), losing the sign we need for
    // the intensity curve.
    let phase = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
    let theta = phase * std::f32::consts::TAU;
    let raw_elev = theta.sin();
    // 3.0 at noon, ~0.4 at midnight (soft moonlight), smooth
    // transition through the horizon. Clamp avoids negatives.
    let sun_intensity = (1.5 + raw_elev * 1.5).clamp(0.10, 3.0);
    renderer.set_sun_intensity(sun_intensity);

    // Sun + moon as 3D meshes far away. Going through the mesh3d
    // pass means the depth buffer occludes them when terrain stands
    // between the player + the sky (rooms, mountains, etc.). We
    // over-saturate the albedo (>1.0) so the PBR shader's
    // `direct + ambient` output clamps to bright on every face -
    // no need for a dedicated unlit pipeline yet.
    let sun_elev = sun_dir.y;
    if sun_elev > -0.05 {
        let sun_world = Vec3::new(
            player_pos.x + sun_dir.x * CELESTIAL_DIST,
            player_pos.y + sun_dir.y * CELESTIAL_DIST,
            player_pos.z + sun_dir.z * CELESTIAL_DIST,
        );
        let m = Mat4::from_scale_rotation_translation(
            Vec3::splat(14.0),
            glam::Quat::IDENTITY,
            sun_world,
        );
        renderer.draw_mesh_pbr(
            player_cube,
            m.to_cols_array(),
            [5.0, 4.5, 2.2, 1.0],
            0.0,
            1.0,
        );
    }
    // Moon: opposite of sun, slightly smaller + cooler tint.
    // W12.4: moon brightness + size dimmed by the current phase so a
    // new moon nearly vanishes + a full moon stays bright. Phase
    // index walks 0..7 over a `MOON_PHASE_CYCLE_DAYS` cycle
    // (sky::moon_phase_for_day). Full = 0, new = 4.
    let moon_dir = -sun_dir;
    if moon_dir.y > -0.05 {
        // Pick the moon phase from the current in-game day count.
        // `day_time` cycles 0..DAY_LENGTH_S; integer divide of
        // wall-elapsed seconds by DAY_LENGTH_S gives the day idx.
        let day_idx = (session_secs_elapsed / DAY_LENGTH_S) as u32;
        let phase_idx = sky::moon_phase_for_day(day_idx);
        // Phase brightness: full moon (0) = 1.0, new moon (4) = 0.15,
        // others scale linearly. Symmetric so waxing + waning look the
        // same.
        let dist_from_full =
            (phase_idx as i32).abs().min(8 - phase_idx as i32) as f32;
        let brightness = (1.0 - dist_from_full / 4.0).max(0.15);
        // Disc size shrinks slightly with the phase as a soft visual
        // cue (MC's moon stays 1 size; ours fades + shrinks because we
        // don't have phase atlas sampling yet).
        let scale = 7.0 + 3.0 * brightness;
        let moon_world = Vec3::new(
            player_pos.x + moon_dir.x * CELESTIAL_DIST,
            player_pos.y + moon_dir.y * CELESTIAL_DIST,
            player_pos.z + moon_dir.z * CELESTIAL_DIST,
        );
        let m = Mat4::from_scale_rotation_translation(
            Vec3::splat(scale),
            glam::Quat::IDENTITY,
            moon_world,
        );
        let albedo = [
            2.8 * brightness,
            3.0 * brightness,
            3.8 * brightness,
            1.0,
        ];
        renderer.draw_mesh_pbr(
            player_cube,
            m.to_cols_array(),
            albedo,
            0.0,
            1.0,
        );
    }
}
