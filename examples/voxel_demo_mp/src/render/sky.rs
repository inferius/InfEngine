//! W47.15 - sky tint + fog upload, relocated from on_update.
//!
//! Computes the day/night clear colour, mixes in precipitation tints
//! (rain = stormy grey, snow = pale overcast) + lightning flash
//! (toward white), then pushes the sun direction + clear colour +
//! distance-fog band into the renderer.

use glam::Vec3;
use infengine_render::Renderer;

/// Apply the per-frame sky / weather / fog tint to `renderer`.
///
/// Returns the final tinted sky colour (used by the caller to feed
/// the distance-fog tint - kept on the public API so the caller can
/// still pass it into other render submodules without recomputing).
///
/// Inputs:
/// - `day_time`: seconds along the current day clock (0..DAY_LENGTH_S)
/// - `weather`: 0=clear, 1=rain, 2=snow
/// - `lightning_flash`: seconds remaining on an active flash (0 = none)
/// - `view_distance_chunks`: render distance setting (chunks)
///
/// The caller still owns `day_night(day_time)`; this helper takes the
/// sky colour from there and only applies the tint mixes + the fog
/// band setup.
pub(crate) fn apply_tint_and_fog(
    renderer: &mut Renderer,
    sun_dir: Vec3,
    base_sky: [f32; 3],
    weather: u8,
    lightning_flash: f32,
    view_distance_chunks: u8,
    underwater: bool,
) -> [f32; 3] {
    let mut sky = base_sky;
    // Rain: dark stormy grey.
    if weather == 1 {
        let target = [0.42, 0.46, 0.52];
        let t = 0.6;
        sky[0] = sky[0] * (1.0 - t) + target[0] * t;
        sky[1] = sky[1] * (1.0 - t) + target[1] * t;
        sky[2] = sky[2] * (1.0 - t) + target[2] * t;
    } else if weather == 2 {
        // Snow: lighter, blue-tinged overcast.
        let target = [0.70, 0.74, 0.80];
        let t = 0.55;
        sky[0] = sky[0] * (1.0 - t) + target[0] * t;
        sky[1] = sky[1] * (1.0 - t) + target[1] * t;
        sky[2] = sky[2] * (1.0 - t) + target[2] * t;
    }
    // Active lightning flash washes sky toward white.
    if lightning_flash > 0.0 {
        let t = (lightning_flash / 0.18).clamp(0.0, 1.0);
        let target = [1.0, 1.0, 1.0];
        sky[0] = sky[0] * (1.0 - t) + target[0] * t;
        sky[1] = sky[1] * (1.0 - t) + target[1] * t;
        sky[2] = sky[2] * (1.0 - t) + target[2] * t;
    }
    renderer.set_sun_dir(sun_dir);
    // W113: underwater post-process. When the camera (eye) voxel is water,
    // MC washes the whole view toward a deep blue-green and pulls the fog in
    // tight, so the world reads as "seen through water" rather than clear
    // air. Override the clear colour + fog band here (the sun dir stays so
    // lit surfaces still catch light). The fog starts right at the camera so
    // even near geometry takes the tint, reaching full murk by ~1.4 chunks -
    // roughly MC's underwater visibility. A dedicated fullscreen blue overlay
    // (for the immediate foreground) can layer on later if wanted.
    if underwater {
        let murk = [0.06_f32, 0.18, 0.36];
        renderer.set_clear_color(murk[0] as f64, murk[1] as f64, murk[2] as f64, 1.0);
        renderer.set_fog(0.5, 22.0, murk);
        return sky;
    }
    renderer.set_clear_color(sky[0] as f64, sky[1] as f64, sky[2] as f64, 1.0);
    // W30.5: distance fog. Start fading 2 chunks before the view edge,
    // reach full sky colour 0.5 chunks before the load boundary. Hides
    // the hard pop-clip at view_distance edges.
    let cs = infengine_voxel::CHUNK_SIZE as f32;
    let vd = view_distance_chunks as f32;
    let fog_start = (vd - 2.0).max(0.0) * cs;
    let fog_end = vd * cs * 0.95;
    renderer.set_fog(fog_start, fog_end, [sky[0], sky[1], sky[2]]);
    sky
}
