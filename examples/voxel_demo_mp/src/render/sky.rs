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
/// W264: which sky a dimension renders - derived from the dimension
/// registry (`has_ceiling` -> the roofed nether haze, no skylight ->
/// the End void) so modded dims pick a look by their registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DimSky {
    /// Day/night cycle, celestial bodies, weather tints.
    Day,
    /// Roofed red murk (the nether).
    NetherHaze,
    /// The End: starless dark violet void, long dim fog.
    EndVoid,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_tint_and_fog(
    renderer: &mut Renderer,
    sun_dir: Vec3,
    base_sky: [f32; 3],
    weather: u8,
    lightning_flash: f32,
    view_distance_chunks: u8,
    underwater: bool,
    // W207: MC's waterVision multiplier in [0.25, 1.0] - underwater
    // visibility grows the longer the eye stays submerged (see
    // `water_vision_mult`). Ignored unless `underwater`.
    water_vision: f32,
    // W212: the biome's water_fog_color at the camera (Mc profile);
    // None = MC's default 0x050533. Ignored unless `underwater`.
    water_fog: Option<[f32; 3]>,
    // W221: smoothed sky exposure at the camera (0.04..1). Scales the
    // fog + clear colour so caves fade to darkness, not to a glowing
    // sky-coloured haze.
    sky_exposure: f32,
    // W155/W264: the dimension's sky kind (registry-derived).
    dim_sky: DimSky,
) -> [f32; 3] {
    // W229.2: MC's render-distance fog term - a narrow CYLINDRICAL
    // curtain right at the chunk-load edge, span = clamp(rd/10, 4, 64)
    // (FogRenderer.setupFog). It replaces the old spherical band as
    // the pop-in hider and applies in every environment; the set_fog
    // calls below carry the ENVIRONMENTAL (spherical) term.
    let rd_blocks = view_distance_chunks as f32 * infengine_voxel::CHUNK_SIZE as f32;
    let rd_span = (rd_blocks / 10.0).clamp(4.0, 64.0);
    renderer.set_fog_render_distance(rd_blocks - rd_span, rd_blocks);
    if dim_sky == DimSky::NetherHaze {
        let haze = [0.16_f32, 0.05, 0.05];
        renderer.set_sun_dir(sun_dir);
        renderer.set_clear_color(haze[0] as f64, haze[1] as f64, haze[2] as f64, 1.0);
        // MC nether visibility ~ a few chunks of reddish murk.
        renderer.set_fog(8.0, 96.0, haze);
        return haze;
    }
    if dim_sky == DimSky::EndVoid {
        // MC End fog: the_end's 0xA080A0 scaled by 0.15
        // (`DimensionSpecialEffects.EndEffects`).
        let void = [0.094_f32, 0.075, 0.094];
        renderer.set_sun_dir(sun_dir);
        renderer.set_clear_color(void[0] as f64, void[1] as f64, void[2] as f64, 1.0);
        renderer.set_fog(48.0, rd_blocks.max(192.0), void);
        return void;
    }
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
        // W207: MC 26.1.2 water fog, from the decompiled source
        // (EnvironmentAttributes + WaterFogEnvironment): start -8,
        // end 96 * waterVision. The old hardcoded 0.5..22 band capped
        // underwater visibility at ~1.4 chunks regardless of the view
        // distance - a quarter of MC's fully-adjusted 96.
        // W212: the fog colour is the biome's WATER_FOG_COLOR attribute
        // (warm-ocean teal, swamp brown-green); the default 0x050533
        // covers non-Mc profiles.
        let mut murk = water_fog.unwrap_or([
            0x05 as f32 / 255.0,
            0x05 as f32 / 255.0,
            0x33 as f32 / 255.0,
        ]);
        // W221: submerged in a dark cave lake the murk darkens too.
        for c in &mut murk {
            *c *= sky_exposure;
        }
        renderer.set_clear_color(murk[0] as f64, murk[1] as f64, murk[2] as f64, 1.0);
        renderer.set_fog(-8.0, 96.0 * water_vision.clamp(0.25, 1.0), murk);
        return sky;
    }
    // W221: underground the horizon is darkness, not glowing sky - the
    // fog + clear colour scale with the camera's sky exposure.
    let dark = [sky[0] * sky_exposure, sky[1] * sky_exposure, sky[2] * sky_exposure];
    renderer.set_clear_color(dark[0] as f64, dark[1] as f64, dark[2] as f64, 1.0);
    // W229.2: the ENVIRONMENTAL fog is MC's gentle atmospheric haze -
    // overworld attribute defaults FOG_START_DISTANCE 0 /
    // FOG_END_DISTANCE 1024 (EnvironmentAttributes). At any practical
    // view distance it contributes only a few percent; the pop-in
    // hiding is the cylindrical render-distance curtain set above.
    // The old (vd-2)*16 .. vd*16*0.95 spherical smoothstep wrapped the
    // whole world in a milk wall at the view edge - THE biggest
    // "doesn't look like MC" offender at low view distances.
    renderer.set_fog(0.0, 1024.0, dark);
    sky
}

/// W207: MC's `LocalPlayer.getWaterVision()` curve, ported from the
/// decompiled 26.1.2 source. `t` is ticks with the eye submerged
/// (0..=600, +1/tick underwater, -10/tick surfaced): returns 0 out of
/// water, else `a * 0.6 + b * 0.4` where `a = clamp(t/100)` ramps the
/// first 5 s and `b = clamp((t-100)/500)` the remaining 25 s, reaching
/// 1.0 at t >= 600. The water fog END multiplies by `max(0.25, this)` -
/// eyes "adjust" to the water the longer you stay under.
pub(crate) fn water_vision_mult(t: f32) -> f32 {
    if t >= 600.0 {
        return 1.0;
    }
    let a = (t / 100.0).clamp(0.0, 1.0);
    let b = if t < 100.0 {
        0.0
    } else {
        ((t - 100.0) / 500.0).clamp(0.0, 1.0)
    };
    a * 0.6 + b * 0.4
}
